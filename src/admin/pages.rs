// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! HTML pages of the admin console.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use super::body::{hex_dump, render, stored_size, BodyView, HEX_LIMIT};
use super::{fmt_bytes, fmt_duration, fmt_expiration, fmt_time_ms, process_memory, AdminState, CSS};
use crate::broker::destination::{Dest, DestSnapshot};
use crate::broker::entry::Entry;
use crate::broker::now_ms;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

type Params = Query<HashMap<String, String>>;

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

/// Percent-encodes a path segment.
pub fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~:".contains(&b) {
            o.push(b as char);
        } else {
            let _ = write!(o, "%{b:02X}");
        }
    }
    o
}

fn refresh_q(p: &HashMap<String, String>) -> &'static str {
    if p.get("refresh").map(String::as_str) == Some("5") {
        "refresh=5"
    } else {
        ""
    }
}

fn with_q(url: &str, extra: &str) -> String {
    if extra.is_empty() {
        url.to_string()
    } else if url.contains('?') {
        format!("{url}&{extra}")
    } else {
        format!("{url}?{extra}")
    }
}

fn layout(title: &str, p: &HashMap<String, String>, content: &str) -> Html<String> {
    let r = refresh_q(p);
    let meta = if r.is_empty() { String::new() } else { "<meta http-equiv=\"refresh\" content=\"5\">".to_string() };
    let toggle = if r.is_empty() { "<a href=\"?refresh=5\">auto-refresh</a>" } else { "<a href=\"?\">stop refresh</a>" };
    Html(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">{meta}<title>{} - ActiveMQRust</title><link rel=\"stylesheet\" href=\"/style.css\"></head><body><header><div class=\"brand\">ActiveMQ<span>Rust</span></div><nav><a href=\"{}\">Overview</a><a href=\"{}\">Queues</a><a href=\"{}\">Topics</a><a href=\"{}\">Connections</a></nav><div class=\"muted\">{toggle}</div></header><main>{content}</main></body></html>",
        esc(title),
        with_q("/", r),
        with_q("/queues", r),
        with_q("/topics", r),
        with_q("/connections", r),
    ))
}

pub async fn style() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS).into_response()
}

pub async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Html("<h1>404 Not Found</h1>".to_string())).into_response()
}

pub fn visible(d: &Dest) -> bool {
    !d.dest.is_advisory()
}

pub fn find_queue(state: &AdminState, name: &str) -> Option<Arc<Dest>> {
    state
        .broker
        .destinations()
        .into_iter()
        .find(|d| d.dest.kind.is_queue() && d.dest.name.as_ref() == name)
}

pub async fn overview(State(s): State<AdminState>, Query(p): Params) -> Html<String> {
    let b = &s.broker;
    let dests = b.destinations();
    let queues = dests.iter().filter(|d| d.dest.kind.is_queue()).count();
    let topics = dests.iter().filter(|d| d.dest.kind.is_topic() && visible(d)).count();
    let (ws, private) = process_memory();
    let uptime = (chrono::Local::now() - b.started).num_seconds();
    let limit = if b.memory.limit == 0 { "no limit".to_string() } else { fmt_bytes(b.memory.limit) };
    let mut c = String::new();
    let _ = write!(
        c,
        "<h1>{PROVIDER_NAME} {PROVIDER_VERSION}</h1><table class=\"kv\">\
         <tr><th>Uptime</th><td>{}</td></tr>\
         <tr><th>OpenWire</th><td>tcp://{}:{}</td></tr>\
         <tr><th>Admin</th><td>http://{}:{}</td></tr>\
         <tr><th>Active connections</th><td>{}</td></tr>\
         <tr><th>Queues</th><td>{queues}</td></tr>\
         <tr><th>Topics</th><td>{topics}</td></tr>\
         <tr><th>Message memory</th><td>{} of {limit}{}</td></tr>\
         <tr><th>Process memory</th><td>Working Set {} &middot; Private Bytes {}</td></tr>\
         </table>",
        fmt_duration(uptime),
        b.cfg.bind,
        b.cfg.port,
        b.cfg.admin_bind,
        b.cfg.admin_port,
        b.connections().len(),
        fmt_bytes(b.memory.used()),
        if b.memory_limited() { " <span class=\"tag\">limit reached</span>" } else { "" },
        fmt_bytes(ws),
        fmt_bytes(private),
    );
    layout("Overview", &p, &c)
}

fn sort_rows(rows: &mut [DestSnapshot], p: &HashMap<String, String>) {
    let col = p.get("sort").map(String::as_str).unwrap_or("name");
    let desc = p.get("order").map(String::as_str) == Some("desc");
    rows.sort_by(|a, b| {
        let o = match col {
            "pending" => a.pending.cmp(&b.pending),
            "inflight" => a.inflight.cmp(&b.inflight),
            "consumers" => a.consumers.len().cmp(&b.consumers.len()),
            "producers" => a.producers.len().cmp(&b.producers.len()),
            "enqueued" => a.stats.enqueued.cmp(&b.stats.enqueued),
            "consumed" => a.stats.dequeued.cmp(&b.stats.dequeued),
            "expired" => a.stats.expired.cmp(&b.stats.expired),
            _ => a.dest.name.cmp(&b.dest.name),
        };
        if desc {
            o.reverse()
        } else {
            o
        }
    });
}

fn sort_header(label: &str, col: &str, p: &HashMap<String, String>, num: bool) -> String {
    let current = p.get("sort").map(String::as_str).unwrap_or("name");
    let desc = p.get("order").map(String::as_str) == Some("desc");
    let next = if current == col && !desc { "desc" } else { "asc" };
    let arrow = if current == col { if desc { " &darr;" } else { " &uarr;" } } else { "" };
    let r = refresh_q(p);
    let href = with_q(&format!("/queues?sort={col}&order={next}"), r);
    format!("<th{}><a href=\"{href}\">{label}{arrow}</a></th>", if num { " class=\"num\"" } else { "" })
}

pub async fn queues(State(s): State<AdminState>, Query(p): Params) -> Html<String> {
    let mut rows: Vec<DestSnapshot> =
        s.broker.destinations().iter().filter(|d| d.dest.kind.is_queue()).map(|d| d.snapshot()).collect();
    sort_rows(&mut rows, &p);
    let r = refresh_q(&p);
    let mut c = String::from("<h1>Queues</h1><div class=\"wrap\"><table><tr>");
    for (label, col, num) in [
        ("Name", "name", false),
        ("Pending", "pending", true),
        ("Inflight", "inflight", true),
        ("Consumers", "consumers", true),
        ("Producers", "producers", true),
        ("Enqueued", "enqueued", true),
        ("Consumed", "consumed", true),
        ("Expired", "expired", true),
    ] {
        c.push_str(&sort_header(label, col, &p, num));
    }
    c.push_str("</tr>");
    for q in &rows {
        let kind = if q.dest.kind.is_temporary() { " <span class=\"tag\">temp</span>" } else { "" };
        let _ = write!(
            c,
            "<tr><td><a href=\"{}\">{}</a>{kind}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            with_q(&format!("/queues/{}", enc(&q.dest.name)), r),
            esc(&q.dest.name),
            q.pending,
            q.inflight,
            q.consumers.len(),
            q.producers.len(),
            q.stats.enqueued,
            q.stats.dequeued,
            q.stats.expired
        );
    }
    if rows.is_empty() {
        c.push_str("<tr><td colspan=\"8\" class=\"muted\">No queues</td></tr>");
    }
    c.push_str("</table></div>");
    layout("Queues", &p, &c)
}

fn body_type(e: &Entry) -> &'static str {
    e.msg.type_name()
}

pub async fn queue_detail(State(s): State<AdminState>, Path(name): Path<String>, Query(p): Params) -> Response {
    let Some(d) = find_queue(&s, &name) else { return not_found().await };
    let snap = d.snapshot();
    let page: usize = p.get("page").and_then(|v| v.parse().ok()).filter(|v: &usize| *v >= 1).unwrap_or(1);
    let (total, entries) = d.page((page - 1) * 50, 50);
    let now = now_ms();
    let r = refresh_q(&p);
    let qpath = format!("/queues/{}", enc(&name));
    let compressed_pending = entries.iter().filter(|e| e.msg.compressed).count();
    let mut c = String::new();
    let _ = write!(c, "<h1>Queue {}</h1><table class=\"kv\">", esc(&name));
    let _ = write!(
        c,
        "<tr><th>Pending</th><td>{}</td></tr><tr><th>Inflight</th><td>{}</td></tr><tr><th>Consumers</th><td>{}</td></tr><tr><th>Producers</th><td>{}</td></tr><tr><th>Enqueued</th><td>{}</td></tr><tr><th>Consumed</th><td>{}</td></tr><tr><th>Expired</th><td>{}</td></tr><tr><th>Discarded</th><td>{}</td></tr><tr><th>Message memory</th><td>{}</td></tr><tr><th>Pending with expiration</th><td>{}</td></tr><tr><th>Next expiration</th><td>{}</td></tr><tr><th>Compressed (this page)</th><td>{}</td></tr></table>",
        snap.pending,
        snap.inflight,
        snap.consumers.len(),
        snap.producers.len(),
        snap.stats.enqueued,
        snap.stats.dequeued,
        snap.stats.expired,
        snap.stats.discarded,
        fmt_bytes(snap.memory),
        snap.with_expiry,
        snap.next_expiry.map(|e| fmt_expiration(e, now)).unwrap_or_else(|| "-".into()),
        compressed_pending,
    );
    c.push_str("<h2>Consumers</h2><div class=\"wrap\"><table><tr><th>Consumer</th><th>Client</th><th class=\"num\">Prefetch</th><th class=\"num\">Inflight</th><th>Selector</th></tr>");
    for x in &snap.consumers {
        let _ = write!(
            c,
            "<tr><td>{}{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>{}</td></tr>",
            esc(&x.consumer_id),
            if x.browser { " <span class=\"tag\">browser</span>" } else { "" },
            esc(&x.remote),
            x.prefetch,
            x.inflight,
            esc(x.selector.as_deref().unwrap_or(""))
        );
    }
    if snap.consumers.is_empty() {
        c.push_str("<tr><td colspan=\"5\" class=\"muted\">No consumers</td></tr>");
    }
    c.push_str("</table></div><h2>Producers</h2><div class=\"wrap\"><table><tr><th>Producer</th><th>Connection</th><th>Client</th></tr>");
    for (pid, m) in &snap.producers {
        let _ = write!(c, "<tr><td>{}</td><td>{}</td><td>{}</td></tr>", esc(pid), esc(&m.connection_id), esc(&m.remote));
    }
    if snap.producers.is_empty() {
        c.push_str("<tr><td colspan=\"3\" class=\"muted\">No producers</td></tr>");
    }
    let _ = write!(c, "</table></div><h2>Messages ({total} pending)</h2><div class=\"wrap\"><table><tr><th class=\"num\">#</th><th>Message ID</th><th>Timestamp</th><th>Type</th><th>Correlation ID</th><th>Body</th><th class=\"num\">Size</th></tr>");
    for (i, e) in entries.iter().enumerate() {
        let id = e.msg.message_id_text();
        let mut flags = String::new();
        if e.expired(now) {
            flags.push_str(" <span class=\"tag\">expired</span>");
        }
        if e.msg.compressed {
            flags.push_str(" <span class=\"tag\">compressed</span>");
        }
        let _ = write!(
            c,
            "<tr><td class=\"num\">{}</td><td><a href=\"{}\">{}</a>{flags}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td></tr>",
            (page - 1) * 50 + i + 1,
            with_q(&format!("{qpath}/messages/{}?seq={}", enc(&id), e.seq), r),
            esc(&id),
            fmt_time_ms(e.msg.timestamp),
            esc(e.msg.jms_type.as_deref().unwrap_or("")),
            esc(e.msg.correlation_id.as_deref().unwrap_or("")),
            body_type(e),
            stored_size(&e.msg)
        );
    }
    if entries.is_empty() {
        c.push_str("<tr><td colspan=\"7\" class=\"muted\">No messages on this page</td></tr>");
    }
    c.push_str("</table></div><div class=\"pager\">");
    if page > 1 {
        let _ = write!(c, "<a href=\"{}\">&larr; previous</a>", with_q(&format!("{qpath}?page={}", page - 1), r));
    }
    if page * 50 < total {
        let _ = write!(c, "<a href=\"{}\">next &rarr;</a>", with_q(&format!("{qpath}?page={}", page + 1), r));
    }
    let _ = write!(c, "<span class=\"muted\">page {page} of {}</span></div>", total.div_ceil(50).max(1));
    layout(&format!("Queue {name}"), &p, &c).into_response()
}

pub async fn message_detail(
    State(s): State<AdminState>,
    Path((name, id)): Path<(String, String)>,
    Query(p): Params,
) -> Response {
    let Some(d) = find_queue(&s, &name) else { return not_found().await };
    let seq = p.get("seq").and_then(|v| v.parse().ok());
    let Some((e, inflight)) = d.find(seq, &id) else { return not_found().await };
    let m = &e.msg;
    let now = now_ms();
    let mut c = String::new();
    let _ = write!(c, "<h1>Message {}</h1>", esc(&id));
    if inflight {
        c.push_str("<p class=\"muted\">This message is delivered to a consumer and waiting for its acknowledgement.</p>");
    }
    c.push_str("<h2>Headers</h2><table class=\"kv\">");
    let rows: Vec<(&str, String)> = vec![
        ("MessageID", m.message_id_text()),
        ("CorrelationID", m.correlation_id.clone().unwrap_or_default()),
        ("Type", m.jms_type.clone().unwrap_or_default()),
        ("ReplyTo", m.reply_to.as_ref().map(|d| d.to_string()).unwrap_or_default()),
        ("DeliveryMode", if m.persistent { "PERSISTENT" } else { "NON_PERSISTENT" }.to_string()),
        ("Priority", m.priority.to_string()),
        ("Timestamp", fmt_time_ms(m.timestamp)),
        ("Expiration", fmt_expiration(m.expiration, now)),
        ("RedeliveryCounter", e.redelivery.to_string()),
        ("Body type", m.type_name().to_string()),
        (
            "Body size",
            if m.compressed { format!("{} bytes (compressed)", stored_size(m)) } else { format!("{} bytes", stored_size(m)) },
        ),
    ];
    for (k, v) in rows {
        let _ = write!(c, "<tr><th>{k}</th><td>{}</td></tr>", esc(&v));
    }
    c.push_str("</table><h2>Properties</h2><table><tr><th>Name</th><th>Type</th><th>Value</th></tr>");
    match e.properties() {
        Some(props) if !props.entries.is_empty() => {
            for (k, v) in &props.entries {
                let _ = write!(c, "<tr><td>{}</td><td>{}</td><td>{}</td></tr>", esc(k), super::body::java_type(v), esc(&v.display()));
            }
        }
        _ => c.push_str("<tr><td colspan=\"3\" class=\"muted\">No properties</td></tr>"),
    }
    c.push_str("</table><h2>Body</h2>");
    let rendered = render(m);
    if rendered.inflate_truncated {
        c.push_str("<p class=\"muted\">Showing the first 64 KB of the decompressed body.</p>");
    }
    match rendered.view {
        BodyView::NoBody => c.push_str("<p class=\"muted\">no body</p>"),
        BodyView::Text { text, truncated } => {
            let _ = write!(c, "<pre>{}</pre>", esc(&text));
            if truncated {
                c.push_str("<p class=\"muted\">Text truncated at 64 KB.</p>");
            }
        }
        BodyView::Bytes { head, total } => {
            let _ = write!(c, "<pre>{}</pre>", esc(&hex_dump(&head)));
            if total > HEX_LIMIT {
                let _ = write!(c, "<p class=\"muted\">Showing the first 4096 bytes; the body is {total} bytes long.</p>");
            }
        }
        BodyView::Map(entries) => {
            c.push_str("<table><tr><th>Key</th><th>Type</th><th>Value</th></tr>");
            for (k, ty, v) in entries {
                let _ = write!(c, "<tr><td>{}</td><td>{ty}</td><td>{}</td></tr>", esc(&k), esc(&v));
            }
            c.push_str("</table>");
        }
        BodyView::Object { size } => {
            let _ = write!(c, "<p>{size} bytes: serialized Java object (not deserialized by the broker)</p>");
        }
        BodyView::Stream(values) => {
            c.push_str("<table><tr><th>Type</th><th>Value</th></tr>");
            for (ty, v) in values {
                let _ = write!(c, "<tr><td>{ty}</td><td>{}</td></tr>", esc(&v));
            }
            c.push_str("</table>");
        }
        BodyView::Error(err) => {
            let _ = write!(c, "<p class=\"muted\">Cannot display the body: {}</p>", esc(&err));
        }
    }
    layout(&format!("Message {id}"), &p, &c).into_response()
}

pub async fn topics(State(s): State<AdminState>, Query(p): Params) -> Html<String> {
    let mut c = String::from("<h1>Topics</h1><div class=\"wrap\"><table><tr><th>Name</th><th class=\"num\">Consumers</th><th class=\"num\">Producers</th><th class=\"num\">Published</th><th class=\"num\">Discarded</th></tr>");
    let mut any = false;
    for d in s.broker.destinations().iter().filter(|d| d.dest.kind.is_topic() && visible(d)) {
        any = true;
        let snap = d.snapshot();
        let kind = if d.dest.kind.is_temporary() { " <span class=\"tag\">temp</span>" } else { "" };
        let _ = write!(
            c,
            "<tr><td>{}{kind}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&d.dest.name),
            snap.consumers.len(),
            snap.producers.len(),
            snap.stats.enqueued,
            snap.stats.discarded
        );
    }
    if !any {
        c.push_str("<tr><td colspan=\"5\" class=\"muted\">No topics</td></tr>");
    }
    c.push_str("</table></div>");
    layout("Topics", &p, &c)
}

pub async fn connections(State(s): State<AdminState>, Query(p): Params) -> Html<String> {
    let mut c = String::from("<h1>Connections</h1><div class=\"wrap\"><table><tr><th>Connection ID</th><th>User</th><th>Client</th><th class=\"num\">OpenWire</th><th>Connected</th><th class=\"num\">Sessions</th><th class=\"num\">Consumers</th><th class=\"num\">Producers</th></tr>");
    let conns = s.broker.connections();
    for x in &conns {
        let i = x.info.lock().clone();
        let _ = write!(
            c,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&i.connection_id),
            esc(&i.user),
            esc(&x.remote.to_string()),
            i.version,
            x.connected_at.format("%Y-%m-%d %H:%M:%S"),
            i.sessions,
            i.consumers,
            i.producers
        );
    }
    if conns.is_empty() {
        c.push_str("<tr><td colspan=\"8\" class=\"muted\">No connections</td></tr>");
    }
    c.push_str("</table></div>");
    layout("Connections", &p, &c)
}
