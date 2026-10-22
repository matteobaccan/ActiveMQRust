// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! The last hour of memory, and the charts the overview draws from it.
//!
//! One task started with the console reads the accounted message memory every second and the
//! process memory once per bucket; nothing here runs on the path that stores or delivers a
//! message. Readings are folded into 360 buckets of ten seconds, so the history costs a few
//! kilobytes whatever the broker does, and starts empty at every start.

use std::collections::VecDeque;
use std::fmt::Write;

use chrono::{DateTime, Local, TimeZone};
use parking_lot::Mutex;

use super::pages::esc;
use super::{fmt_bytes, process_memory};

/// Seconds in one bucket.
pub const INTERVAL_SECS: i64 = 10;
/// Closed buckets kept: one hour.
pub const BUCKETS: usize = 360;

/// Ten seconds of readings.
#[derive(Debug, Clone, Copy)]
pub struct Bucket {
    pub start: DateTime<Local>,
    /// Accounted message memory, in bytes.
    pub min: u64,
    pub max: u64,
    pub last: u64,
    /// Process memory read once in this bucket; 0 until it is read.
    pub working_set: u64,
    pub private_bytes: u64,
}

impl Bucket {
    fn new(start: DateTime<Local>, used: u64) -> Bucket {
        Bucket {
            start,
            min: used,
            max: used,
            last: used,
            working_set: 0,
            private_bytes: 0,
        }
    }
}

/// The ring of buckets: the closed ones and the one being filled.
#[derive(Default)]
struct Ring {
    closed: VecDeque<Bucket>,
    current: Option<Bucket>,
}

pub struct History {
    ring: Mutex<Ring>,
}

impl Default for History {
    fn default() -> Self {
        History::new()
    }
}

impl History {
    pub fn new() -> History {
        History {
            ring: Mutex::new(Ring::default()),
        }
    }

    /// Folds one reading in, closing the bucket in progress when its ten seconds are over.
    /// `process` carries the process memory when it was read for this bucket.
    pub fn sample(&self, now: DateTime<Local>, used: u64, process: Option<(u64, u64)>) {
        let slot = now.timestamp().div_euclid(INTERVAL_SECS);
        let start = Local.timestamp_opt(slot * INTERVAL_SECS, 0).earliest().unwrap_or(now);
        let mut ring = self.ring.lock();
        match &mut ring.current {
            Some(bucket) if bucket.start == start => {
                bucket.min = bucket.min.min(used);
                bucket.max = bucket.max.max(used);
                bucket.last = used;
            }
            _ => {
                if let Some(done) = ring.current.take() {
                    if ring.closed.len() == BUCKETS {
                        ring.closed.pop_front();
                    }
                    ring.closed.push_back(done);
                }
                ring.current = Some(Bucket::new(start, used));
            }
        }
        if let (Some(bucket), Some((ws, private))) = (&mut ring.current, process) {
            bucket.working_set = ws;
            bucket.private_bytes = private;
        }
    }

    /// Every bucket, oldest first, the one in progress last.
    pub fn buckets(&self) -> Vec<Bucket> {
        let ring = self.ring.lock();
        ring.closed.iter().copied().chain(ring.current).collect()
    }
}

/// Reads memory while the console runs: message memory every second, process memory once per
/// bucket. One task, outside the message path.
pub async fn sampler(
    broker: std::sync::Arc<crate::broker::Broker>,
    history: std::sync::Arc<History>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut since_process = 0i64;
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = shutdown.changed() => return,
        }
        // The first reading of a bucket also carries the process memory.
        let process = (since_process % INTERVAL_SECS == 0).then(process_memory);
        since_process += 1;
        history.sample(Local::now(), broker.memory.used(), process);
    }
}

// ---------------------------------------------------------------------------
// Charts
// ---------------------------------------------------------------------------

/// Width and height of the chart's coordinate system; the SVG scales to the page width.
const W: f64 = 600.0;
const H: f64 = 170.0;
/// The plot area inside it.
const LEFT: f64 = 52.0;
const RIGHT: f64 = 594.0;
const TOP: f64 = 8.0;
const BOTTOM: f64 = 138.0;

/// One line of a chart.
struct Series<'a> {
    label: &'a str,
    /// CSS class that gives it its colour and dash pattern.
    class: &'a str,
    /// Value of each bucket, in bytes.
    values: Vec<(DateTime<Local>, u64)>,
}

/// Rounds a maximum up to a readable axis top, and the step between its ticks.
fn axis(top: u64) -> (f64, f64) {
    let mb = (top as f64 / (1024.0 * 1024.0)).max(1.0);
    let step = [
        1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0,
    ]
    .into_iter()
    .find(|s| mb / s <= 4.0)
    .unwrap_or_else(|| (mb / 4.0).ceil());
    let ticks = (mb / step).ceil();
    ((ticks * step) * 1024.0 * 1024.0, step * 1024.0 * 1024.0)
}

fn mb(bytes: f64) -> String {
    let mb = bytes / (1024.0 * 1024.0);
    if mb >= 100.0 {
        format!("{mb:.0}")
    } else if mb >= 10.0 {
        format!("{mb:.1}")
    } else {
        format!("{mb:.2}")
    }
}

/// The message memory chart: the min-max band of each bucket, the line of its last value and,
/// when one is set, the limit.
pub fn message_memory(buckets: &[Bucket], limit: u64) -> String {
    let series = vec![Series {
        label: "Message memory",
        class: "c-mem",
        values: buckets.iter().map(|b| (b.start, b.last)).collect(),
    }];
    let band: Vec<(DateTime<Local>, u64, u64)> = buckets.iter().map(|b| (b.start, b.min, b.max)).collect();
    let peak = buckets.iter().map(|b| b.max).max().unwrap_or(0);
    let peak_at = buckets.iter().max_by_key(|b| b.max).map(|b| b.start);
    chart(
        "Message memory",
        buckets,
        &series,
        Some(&band),
        (limit > 0).then_some(limit),
        peak,
        peak_at,
    )
}

/// The process memory chart: Working Set and Private Bytes.
pub fn process(buckets: &[Bucket]) -> String {
    // Buckets before the first process reading have nothing to draw.
    let read: Vec<&Bucket> = buckets.iter().filter(|b| b.working_set > 0).collect();
    let series = vec![
        Series {
            label: if cfg!(target_os = "macos") {
                "Resident"
            } else {
                "Working Set"
            },
            class: "c-ws",
            values: read.iter().map(|b| (b.start, b.working_set)).collect(),
        },
        Series {
            label: if cfg!(target_os = "macos") {
                "Physical footprint"
            } else {
                "Private Bytes"
            },
            class: "c-priv",
            values: read.iter().map(|b| (b.start, b.private_bytes)).collect(),
        },
    ];
    let peak = read.iter().map(|b| b.working_set).max().unwrap_or(0);
    let peak_at = read.iter().max_by_key(|b| b.working_set).map(|b| b.start);
    chart("Process memory", buckets, &series, None, None, peak, peak_at)
}

/// Draws one chart: axes, ticks, the optional band, the lines, the legend, the peak and a
/// tooltip per bucket. No script and no inline style: the classes live in the stylesheet.
fn chart(
    title: &str,
    buckets: &[Bucket],
    series: &[Series<'_>],
    band: Option<&[(DateTime<Local>, u64, u64)]>,
    limit: Option<u64>,
    peak: u64,
    peak_at: Option<DateTime<Local>>,
) -> String {
    // The window always covers the last hour, so an empty period stays empty.
    let end = buckets
        .last()
        .map(|b| b.start + chrono::Duration::seconds(INTERVAL_SECS))
        .unwrap_or_else(Local::now);
    let start = end - chrono::Duration::seconds(INTERVAL_SECS * BUCKETS as i64);
    let span = (end - start).num_seconds().max(1) as f64;
    let x = |at: DateTime<Local>| {
        let offset = (at - start).num_seconds() as f64;
        LEFT + (offset / span) * (RIGHT - LEFT)
    };
    let highest = series
        .iter()
        .flat_map(|s| s.values.iter().map(|(_, v)| *v))
        .chain(band.into_iter().flatten().map(|(_, _, max)| *max))
        .chain(limit)
        .max()
        .unwrap_or(0);
    let (top, step) = axis(highest);
    let y = |value: f64| BOTTOM - (value / top).min(1.0) * (BOTTOM - TOP);

    let now = series.first().and_then(|s| s.values.last()).map(|(_, v)| *v);
    let mut label = format!("{title}, last hour");
    if let Some(v) = now {
        let _ = write!(label, ": now {}", fmt_bytes(v));
    }
    if peak > 0 {
        let _ = write!(label, ", peak {}", fmt_bytes(peak));
        if let Some(at) = peak_at {
            let _ = write!(label, " at {}", at.format("%H:%M:%S"));
        }
    }
    if let Some(l) = limit {
        let _ = write!(label, ", limit {}", fmt_bytes(l));
    }

    let mut s = format!(
        "<figure class=\"chart\"><figcaption>{} <span class=\"peak\">{}</span></figcaption>\
         <svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"{}\" preserveAspectRatio=\"none\">",
        esc(title),
        match (peak, peak_at) {
            (0, _) => "no data yet".to_string(),
            (p, Some(at)) => format!("Peak {} at {}", fmt_bytes(p), at.format("%H:%M:%S")),
            (p, None) => format!("Peak {}", fmt_bytes(p)),
        },
        esc(&label),
    );
    // Value axis: a line and a label per tick.
    let mut value = 0.0f64;
    while value <= top {
        let at = y(value);
        let _ = write!(
            s,
            "<line class=\"c-grid\" x1=\"{LEFT}\" y1=\"{at:.1}\" x2=\"{RIGHT}\" y2=\"{at:.1}\"/>\
             <text class=\"c-tick\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\">{}</text>",
            LEFT - 6.0,
            at + 3.0,
            mb(value)
        );
        value += step;
    }
    let _ = write!(s, "<text class=\"c-tick\" x=\"4\" y=\"{:.1}\">MB</text>", TOP + 4.0);
    // Time axis: a tick every 15 minutes.
    for minutes in [0, 15, 30, 45, 60] {
        let at = start + chrono::Duration::minutes(minutes);
        let px = x(at);
        let _ = write!(
            s,
            "<line class=\"c-grid\" x1=\"{px:.1}\" y1=\"{TOP}\" x2=\"{px:.1}\" y2=\"{BOTTOM}\"/>\
             <text class=\"c-tick\" x=\"{px:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>",
            BOTTOM + 14.0,
            at.format("%H:%M")
        );
    }
    if let Some(l) = limit {
        let at = y(l as f64);
        let _ = write!(
            s,
            "<line class=\"c-limit\" x1=\"{LEFT}\" y1=\"{at:.1}\" x2=\"{RIGHT}\" y2=\"{at:.1}\"/>\
             <text class=\"c-limit-label\" x=\"{:.1}\" y=\"{:.1}\">limit {}</text>",
            LEFT + 4.0,
            at - 4.0,
            mb(l as f64)
        );
    }
    // The min-max band of the buckets, as one closed shape.
    if let Some(band) = band.filter(|b| b.len() > 1) {
        let mut points = String::new();
        for (at, _, max) in band {
            let _ = write!(points, "{:.1},{:.1} ", x(*at), y(*max as f64));
        }
        for (at, min, _) in band.iter().rev() {
            let _ = write!(points, "{:.1},{:.1} ", x(*at), y(*min as f64));
        }
        let _ = write!(s, "<polygon class=\"c-band\" points=\"{}\"/>", points.trim_end());
    }
    for line in series {
        if line.values.len() < 2 {
            continue;
        }
        let mut points = String::new();
        for (at, value) in &line.values {
            let _ = write!(points, "{:.1},{:.1} ", x(*at), y(*value as f64));
        }
        let _ = write!(
            s,
            "<polyline class=\"c-line {}\" points=\"{}\"/>",
            line.class,
            points.trim_end()
        );
    }
    // One transparent column per bucket, for the native tooltip.
    for b in buckets {
        let px = x(b.start);
        let width = ((RIGHT - LEFT) / BUCKETS as f64).max(1.0);
        let _ = write!(
            s,
            "<rect class=\"c-hit\" x=\"{px:.1}\" y=\"{TOP}\" width=\"{width:.1}\" height=\"{:.1}\">\
             <title>{} &#183; {} (min {}, max {})</title></rect>",
            BOTTOM - TOP,
            b.start.format("%H:%M:%S"),
            fmt_bytes(b.last),
            fmt_bytes(b.min),
            fmt_bytes(b.max),
        );
    }
    s.push_str("</svg>");
    // Legend: the same classes as the lines, so colour and dash pattern match.
    s.push_str("<p class=\"legend\">");
    for line in series {
        let _ = write!(
            s,
            "<span class=\"key\"><span class=\"swatch {}\"></span>{}</span>",
            line.class,
            esc(line.label)
        );
    }
    if band.is_some() {
        s.push_str("<span class=\"key\"><span class=\"swatch c-band-key\"></span>minimum to maximum</span>");
    }
    if limit.is_some() {
        s.push_str("<span class=\"key\"><span class=\"swatch c-limit-key\"></span>limit</span>");
    }
    s.push_str("</p></figure>");
    s
}

/// The buckets as a table, folded away under the charts.
pub fn table(buckets: &[Bucket]) -> String {
    let mut s = String::from(
        "<details class=\"chart-data\"><summary>Data</summary><div class=\"table-wrap\"><table>\
         <thead><tr><th scope=\"col\">Time</th><th scope=\"col\" class=\"num\">Message memory</th>\
         <th scope=\"col\" class=\"num\">Minimum</th><th scope=\"col\" class=\"num\">Maximum</th>\
         <th scope=\"col\" class=\"num\">Working Set</th><th scope=\"col\" class=\"num\">Private Bytes</th>\
         </tr></thead><tbody>",
    );
    for b in buckets.iter().rev() {
        let _ = write!(
            s,
            "<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
             <td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            b.start.format("%H:%M:%S"),
            fmt_bytes(b.last),
            fmt_bytes(b.min),
            fmt_bytes(b.max),
            if b.working_set > 0 {
                fmt_bytes(b.working_set)
            } else {
                "-".into()
            },
            if b.private_bytes > 0 {
                fmt_bytes(b.private_bytes)
            } else {
                "-".into()
            },
        );
    }
    if buckets.is_empty() {
        s.push_str("<tr><td colspan=\"6\" class=\"muted\">No readings yet</td></tr>");
    }
    s.push_str("</tbody></table></div></details>");
    s
}

/// The history as JSON, for `/api/memory/history`.
pub fn json(buckets: &[Bucket], limit: u64) -> serde_json::Value {
    let list: Vec<serde_json::Value> = buckets
        .iter()
        .map(|b| {
            serde_json::json!({
                "start": b.start.to_rfc3339(),
                "messageMemory": { "min": b.min, "max": b.max, "last": b.last },
                "workingSet": b.working_set,
                "privateBytes": b.private_bytes,
            })
        })
        .collect();
    serde_json::json!({
        "intervalSeconds": INTERVAL_SECS,
        "limitBytes": if limit == 0 { serde_json::Value::Null } else { serde_json::json!(limit) },
        "buckets": list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Local)
    }

    const MB: u64 = 1024 * 1024;

    #[test]
    fn readings_of_the_same_ten_seconds_share_a_bucket() {
        let h = History::new();
        let base = at("2026-10-21T09:00:00+00:00");
        h.sample(base, 10 * MB, Some((100 * MB, 110 * MB)));
        h.sample(base + chrono::Duration::seconds(3), 400 * MB, None);
        h.sample(base + chrono::Duration::seconds(6), 12 * MB, None);
        let got = h.buckets();
        assert_eq!(got.len(), 1, "one bucket in progress");
        assert_eq!((got[0].min, got[0].max, got[0].last), (10 * MB, 400 * MB, 12 * MB));
        assert_eq!(got[0].working_set, 100 * MB);
        assert_eq!(got[0].start, at("2026-10-21T09:00:00+00:00"));
    }

    #[test]
    fn a_new_bucket_starts_every_ten_seconds() {
        let h = History::new();
        let base = at("2026-10-21T09:00:07+00:00");
        h.sample(base, MB, None);
        h.sample(base + chrono::Duration::seconds(5), 2 * MB, None);
        h.sample(base + chrono::Duration::seconds(15), 3 * MB, None);
        let got = h.buckets();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].start, at("2026-10-21T09:00:00+00:00"));
        assert_eq!(got[1].start, at("2026-10-21T09:00:10+00:00"));
        assert_eq!(got[2].start, at("2026-10-21T09:00:20+00:00"));
        assert_eq!(got[2].last, 3 * MB, "the last bucket is in progress");
    }

    #[test]
    fn only_one_hour_is_kept() {
        let h = History::new();
        let base = at("2026-10-21T09:00:00+00:00");
        // Two hours of readings, one per bucket.
        for n in 0..720 {
            h.sample(base + chrono::Duration::seconds(n * INTERVAL_SECS), n as u64 * MB, None);
        }
        let got = h.buckets();
        assert_eq!(got.len(), BUCKETS + 1, "360 closed plus the one in progress");
        let oldest = got.first().unwrap().start;
        let newest = got.last().unwrap().start;
        assert_eq!((newest - oldest).num_minutes(), 60);
    }

    #[test]
    fn a_recent_start_holds_only_its_own_buckets() {
        let h = History::new();
        let base = at("2026-10-21T09:00:00+00:00");
        for n in 0..30 {
            h.sample(base + chrono::Duration::seconds(n), 5 * MB, None);
        }
        assert_eq!(h.buckets().len(), 3, "five minutes of nothing before the start");
    }

    #[test]
    fn the_axis_top_is_a_round_number() {
        let one_mb = MB as f64;
        assert_eq!(axis(0), (one_mb, one_mb));
        let (top, step) = axis(410 * MB);
        assert!(top >= 410.0 * one_mb, "{top} covers the peak");
        assert!((1.0..=4.0).contains(&(top / step)), "{} ticks", top / step);
    }

    #[test]
    fn the_chart_is_svg_without_script() {
        let h = History::new();
        let base = at("2026-10-21T09:00:00+00:00");
        for n in 0..40 {
            h.sample(
                base + chrono::Duration::seconds(n),
                (n as u64 + 1) * MB,
                Some((200 * MB, 210 * MB)),
            );
        }
        let buckets = h.buckets();
        let svg = message_memory(&buckets, 1024 * MB);
        assert!(svg.contains("<svg viewBox=\"0 0 600 170\""), "{svg}");
        assert!(!svg.contains("<script"), "{svg}");
        assert!(!svg.contains("style="), "no inline style, the CSP forbids it");
        assert!(svg.contains("role=\"img\""));
        assert!(svg.contains("aria-label=\"Message memory, last hour"), "{svg}");
        assert!(svg.contains("limit 1024"), "{svg}");
        assert!(svg.contains("c-band"), "the min-max band is drawn");
        assert!(svg.contains("<title>"), "a tooltip per bucket");
        // The peak is reported with the start of the bucket that holds it.
        assert!(svg.contains("Peak 40.0 MB at 09:00:30"), "{svg}");

        let without_limit = message_memory(&buckets, 0);
        assert!(!without_limit.contains("c-limit"), "no limit line without a limit");

        let process = process(&buckets);
        assert!(process.contains("c-ws") && process.contains("c-priv"), "{process}");
        assert!(table(&buckets).contains("09:00:30"), "the data table lists the buckets");
    }

    #[test]
    fn the_json_carries_every_bucket_oldest_first() {
        let h = History::new();
        let base = at("2026-10-21T09:00:00+00:00");
        for n in 0..25 {
            h.sample(base + chrono::Duration::seconds(n), 7 * MB, Some((1, 2)));
        }
        let j = json(&h.buckets(), 64 * MB);
        assert_eq!(j["intervalSeconds"], 10);
        assert_eq!(j["limitBytes"], 64 * MB);
        let list = j["buckets"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["messageMemory"]["last"], 7 * MB);
        let first = list[0]["start"].as_str().unwrap().to_string();
        let last = list[2]["start"].as_str().unwrap().to_string();
        assert!(first < last, "{first} before {last}");
        assert_eq!(json(&[], 0)["limitBytes"], serde_json::Value::Null);
    }
}
