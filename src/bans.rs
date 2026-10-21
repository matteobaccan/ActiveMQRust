// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Addresses kept out of the OpenWire listeners.
//!
//! A ban is an address or a CIDR range, added by an administrator and stored in `bans.toml`,
//! or earned automatically by failed logins and kept in memory only. Banned connections are
//! refused right after `accept`, before any byte is read, so a client that reconnects costs
//! the broker one lookup.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{DateTime, Duration as Span, Local};
use parking_lot::Mutex;
use serde::Deserialize;

/// Name of the file next to the configuration.
pub const FILE: &str = "bans.toml";
/// Failed logins inside this window create the first automatic ban.
const WINDOW_MINUTES: i64 = 15;
/// Failed logins in the window before the first automatic ban.
const FAILURES: usize = 3;
/// Length of the first automatic ban, in seconds; each later one doubles it.
const FIRST_BAN_SECS: i64 = 60;
/// Longest automatic ban, in seconds (24 hours).
const MAX_BAN_SECS: i64 = 24 * 3600;
/// An address with no failed login for this long starts again from zero.
const FORGET_HOURS: i64 = 24;
/// At most one warning per address per minute summarises its refused connections.
const WARN_EVERY_SECS: i64 = 60;

/// What a ban matches: one address, or every address of a CIDR range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Addr(IpAddr),
    Range { base: IpAddr, bits: u8 },
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Addr(ip) => write!(f, "{ip}"),
            Target::Range { base, bits } => write!(f, "{base}/{bits}"),
        }
    }
}

impl FromStr for Target {
    type Err = String;

    /// `203.0.113.7`, `2001:db8::1`, `10.1.0.0/16` or `2001:db8::/32`. An IPv4-mapped IPv6
    /// address is read as the IPv4 address it holds.
    fn from_str(s: &str) -> Result<Target, String> {
        let s = s.trim();
        let Some((addr, prefix)) = s.split_once('/') else {
            let ip = s.parse::<IpAddr>().map_err(|_| invalid(s))?;
            return Ok(Target::Addr(normalize(ip)));
        };
        let base = normalize(addr.parse::<IpAddr>().map_err(|_| invalid(s))?);
        let bits: u8 = prefix.parse().map_err(|_| invalid(s))?;
        let width = if base.is_ipv4() { 32 } else { 128 };
        if bits > width {
            return Err(format!("invalid range \"{s}\": the prefix must be 0-{width}"));
        }
        // A range that matches a single address is kept as that address.
        if bits == width {
            return Ok(Target::Addr(base));
        }
        Ok(Target::Range { base, bits })
    }
}

fn invalid(s: &str) -> String {
    format!("invalid address or range \"{s}\"")
}

/// IPv4-mapped IPv6 addresses are matched as the IPv4 address they hold, so a ban works
/// whether the listener is bound to IPv4 or to a dual-stack IPv6 socket.
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

impl Target {
    pub fn matches(&self, ip: IpAddr) -> bool {
        let ip = normalize(ip);
        match self {
            Target::Addr(a) => *a == ip,
            Target::Range { base, bits } => same_prefix(*base, ip, *bits),
        }
    }
}

/// True when both addresses are of the same family and share the first `bits` bits.
fn same_prefix(base: IpAddr, ip: IpAddr, bits: u8) -> bool {
    let (a, b) = match (base, ip) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (a.octets().to_vec(), b.octets().to_vec()),
        (IpAddr::V6(a), IpAddr::V6(b)) => (a.octets().to_vec(), b.octets().to_vec()),
        _ => return false,
    };
    let whole = (bits / 8) as usize;
    if a[..whole] != b[..whole] {
        return false;
    }
    let rest = bits % 8;
    if rest == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - rest);
    a[whole] & mask == b[whole] & mask
}

/// How long a new ban lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    Hour,
    Day,
    Week,
    Permanent,
}

impl Length {
    pub fn parse(s: &str) -> Option<Length> {
        match s {
            "1h" => Some(Length::Hour),
            "24h" => Some(Length::Day),
            "7d" => Some(Length::Week),
            "permanent" => Some(Length::Permanent),
            _ => None,
        }
    }

    pub fn expires(&self, now: DateTime<Local>) -> Option<DateTime<Local>> {
        let span = match self {
            Length::Hour => Span::hours(1),
            Length::Day => Span::hours(24),
            Length::Week => Span::days(7),
            Length::Permanent => return None,
        };
        now.checked_add_signed(span)
    }
}

/// One ban, manual or automatic.
#[derive(Debug, Clone)]
pub struct Ban {
    pub target: Target,
    pub reason: String,
    pub created: DateTime<Local>,
    pub created_by: String,
    /// `None` is a permanent ban.
    pub expires: Option<DateTime<Local>>,
    /// Earned by failed logins: kept in memory and never written to the file.
    pub automatic: bool,
    /// Connections refused since the broker started.
    pub refused: u64,
}

impl Ban {
    pub fn new(target: Target, reason: String, created_by: String, expires: Option<DateTime<Local>>) -> Ban {
        Ban {
            target,
            reason,
            created: Local::now(),
            created_by,
            expires,
            automatic: false,
            refused: 0,
        }
    }

    fn active(&self, now: DateTime<Local>) -> bool {
        self.expires.is_none_or(|at| at > now)
    }

    /// `permanent`, or the expiry in the usual local format.
    pub fn expires_text(&self) -> String {
        match self.expires {
            Some(at) => at.format("%Y-%m-%d %H:%M:%S").to_string(),
            None => "permanent".to_string(),
        }
    }
}

/// Failed logins of one address and the length of its next automatic ban.
#[derive(Debug, Default)]
struct Failures {
    /// Failures inside the window, oldest first.
    times: Vec<DateTime<Local>>,
    /// Length of the next automatic ban; `None` until the first one is earned.
    next_secs: Option<i64>,
    last: Option<DateTime<Local>>,
}

#[derive(Debug, Default)]
struct State {
    /// Bans of a single address, by the address they match.
    exact: HashMap<IpAddr, Ban>,
    ranges: Vec<Ban>,
    fails: HashMap<IpAddr, Failures>,
    /// Last warning per address and the refusals since then.
    warned: HashMap<IpAddr, (DateTime<Local>, u64)>,
}

/// An automatic ban the broker has just added.
pub struct Automatic {
    pub target: Target,
    pub seconds: i64,
    pub failures: usize,
}

/// The ban list: manual bans loaded from `bans.toml`, automatic ones added by failed logins.
pub struct Bans {
    /// Where manual bans are stored; every change rewrites it.
    path: PathBuf,
    state: Mutex<State>,
}

impl Bans {
    pub fn new(path: PathBuf, loaded: Vec<Ban>) -> Bans {
        let mut state = State::default();
        for ban in loaded {
            state.insert(ban);
        }
        Bans {
            path,
            state: Mutex::new(state),
        }
    }

    /// Every active ban, manual first, each with its refused count.
    pub fn list(&self) -> Vec<Ban> {
        let now = Local::now();
        let state = self.state.lock();
        state
            .exact
            .values()
            .chain(state.ranges.iter())
            .filter(|b| b.active(now))
            .cloned()
            .collect()
    }

    pub fn is_banned(&self, ip: IpAddr) -> bool {
        let now = Local::now();
        self.state.lock().find(ip, now).is_some()
    }

    /// Checks an address at `accept`: counts the refusal, logs it, and returns true when the
    /// connection must be closed without reading anything.
    pub fn refuse(&self, ip: IpAddr) -> bool {
        let now = Local::now();
        let mut state = self.state.lock();
        let Some(target) = state.find(ip, now) else {
            return false;
        };
        if let Some(ban) = state.ban_mut(&target) {
            ban.refused += 1;
        }
        tracing::debug!("connection from {ip} refused: banned by {target}");
        let key = normalize(ip);
        let summary = match state.warned.get_mut(&key) {
            Some((at, count)) if now.signed_duration_since(*at).num_seconds() < WARN_EVERY_SECS => {
                *count += 1;
                None
            }
            Some((at, count)) => {
                let total = *count + 1;
                *at = now;
                *count = 0;
                Some(total)
            }
            None => {
                state.warned.insert(key, (now, 0));
                Some(1)
            }
        };
        drop(state);
        if let Some(count) = summary {
            tracing::warn!("refused {count} connection(s) from {ip}: banned by {target}");
        }
        true
    }

    /// Adds or replaces a manual ban. The error is a file write that failed: the ban is
    /// active in memory either way.
    pub fn add(&self, ban: Ban) -> Result<(), String> {
        self.state.lock().insert(ban);
        self.save()
    }

    /// Removes a ban and the escalation of that address. Returns false when it was not banned.
    pub fn remove(&self, target: &Target) -> Result<bool, String> {
        let mut state = self.state.lock();
        let removed = state.take(target);
        if let Target::Addr(ip) = target {
            state.fails.remove(ip);
        }
        let was_manual = removed.as_ref().is_some_and(|b| !b.automatic);
        let existed = removed.is_some();
        drop(state);
        if was_manual {
            self.save()?;
        }
        Ok(existed)
    }

    /// Counts a refused login and bans the address when it has failed often enough.
    pub fn login_failed(&self, ip: IpAddr) -> Option<Automatic> {
        self.login_failed_at(ip, Local::now())
    }

    fn login_failed_at(&self, ip: IpAddr, now: DateTime<Local>) -> Option<Automatic> {
        let ip = normalize(ip);
        let mut state = self.state.lock();
        let f = state.fails.entry(ip).or_default();
        f.last = Some(now);
        let window = Span::minutes(WINDOW_MINUTES);
        f.times.retain(|t| now.signed_duration_since(*t) < window);
        f.times.push(now);
        let seconds = match f.next_secs {
            // Every failure after the first automatic ban earns the next, longer one.
            Some(secs) => secs,
            None if f.times.len() >= FAILURES => FIRST_BAN_SECS,
            None => return None,
        };
        let failures = f.times.len();
        f.next_secs = Some((seconds * 2).min(MAX_BAN_SECS));
        f.times.clear();
        let target = Target::Addr(ip);
        // A manual ban of the same address stays as the administrator set it.
        if state.exact.get(&ip).is_some_and(|b| !b.automatic && b.active(now)) {
            return None;
        }
        state.insert(Ban {
            target,
            reason: format!("{failures} failed logins"),
            created: now,
            created_by: "broker".into(),
            expires: now.checked_add_signed(Span::seconds(seconds)),
            automatic: true,
            refused: 0,
        });
        Some(Automatic {
            target,
            seconds,
            failures,
        })
    }

    /// A successful login clears the escalation of that address.
    pub fn login_ok(&self, ip: IpAddr) {
        self.state.lock().fails.remove(&normalize(ip));
    }

    /// Drops expired bans and addresses that have been quiet for a day; rewrites the file
    /// when a manual ban expired. Called by the broker housekeeping.
    pub fn sweep(&self) {
        let now = Local::now();
        let mut state = self.state.lock();
        let mut expired_manual = false;
        state.exact.retain(|_, b| {
            let keep = b.active(now);
            expired_manual |= !keep && !b.automatic;
            keep
        });
        state.ranges.retain(|b| {
            let keep = b.active(now);
            expired_manual |= !keep && !b.automatic;
            keep
        });
        let forget = Span::hours(FORGET_HOURS);
        state
            .fails
            .retain(|_, f| f.last.is_some_and(|at| now.signed_duration_since(at) < forget));
        state
            .warned
            .retain(|_, (at, _)| now.signed_duration_since(*at) < forget);
        drop(state);
        if expired_manual {
            if let Err(e) = self.save() {
                tracing::warn!("{e}");
            }
        }
    }

    /// Rewrites `bans.toml` with the manual bans, atomically.
    fn save(&self) -> Result<(), String> {
        let text = self.to_toml();
        crate::setup::write_atomic(&self.path, &text).map_err(|e| format!("cannot write {}: {e}", self.path.display()))
    }

    fn to_toml(&self) -> String {
        let now = Local::now();
        let state = self.state.lock();
        let mut out = String::from("# ActiveMQRust bans. Edit only while the broker is stopped.\n");
        let mut manual: Vec<&Ban> = state
            .exact
            .values()
            .chain(state.ranges.iter())
            .filter(|b| !b.automatic && b.active(now))
            .collect();
        manual.sort_by_key(|b| b.target.to_string());
        for ban in manual {
            out.push_str("\n[[ban]]\n");
            out.push_str(&format!("address = {}\n", quote(&ban.target.to_string())));
            out.push_str(&format!("reason = {}\n", quote(&ban.reason)));
            out.push_str(&format!("created = {}\n", quote(&ban.created.to_rfc3339())));
            out.push_str(&format!("created_by = {}\n", quote(&ban.created_by)));
            if let Some(at) = ban.expires {
                out.push_str(&format!("expires = {}\n", quote(&at.to_rfc3339())));
            }
        }
        out
    }
}

/// A TOML basic string: the values written here are addresses, names and reasons.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl State {
    /// The target of the ban that keeps `ip` out, if any.
    fn find(&self, ip: IpAddr, now: DateTime<Local>) -> Option<Target> {
        let key = normalize(ip);
        if let Some(ban) = self.exact.get(&key) {
            if ban.active(now) {
                return Some(ban.target);
            }
        }
        self.ranges
            .iter()
            .find(|b| b.active(now) && b.target.matches(key))
            .map(|b| b.target)
    }

    fn ban_mut(&mut self, target: &Target) -> Option<&mut Ban> {
        match target {
            Target::Addr(ip) => self.exact.get_mut(ip),
            range => self.ranges.iter_mut().find(|b| b.target == *range),
        }
    }

    fn insert(&mut self, ban: Ban) {
        match ban.target {
            Target::Addr(ip) => {
                self.exact.insert(ip, ban);
            }
            range => {
                self.ranges.retain(|b| b.target != range);
                self.ranges.push(ban);
            }
        }
    }

    fn take(&mut self, target: &Target) -> Option<Ban> {
        match target {
            Target::Addr(ip) => self.exact.remove(ip),
            range => {
                let at = self.ranges.iter().position(|b| b.target == *range)?;
                Some(self.ranges.remove(at))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// bans.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct FileBans {
    ban: Vec<FileBan>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBan {
    address: String,
    #[serde(default)]
    reason: String,
    created: Option<String>,
    #[serde(default)]
    created_by: String,
    expires: Option<String>,
}

/// Reads and validates `bans.toml`; a missing file means no bans. The error names the file,
/// the line and what is wrong with the entry.
pub fn load(path: &Path) -> Result<Vec<Ban>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

pub fn parse(text: &str, path: &Path) -> Result<Vec<Ban>, String> {
    let file: FileBans = toml::from_str(text).map_err(|e| {
        let line = e.span().map(|s| line_at(text, s.start)).unwrap_or(1);
        format!("{} line {line}: {}", path.display(), e.message().trim())
    })?;
    let mut bans = Vec::new();
    for entry in &file.ban {
        let line = line_of(text, &entry.address);
        let where_ = format!("{} line {line}, ban \"{}\"", path.display(), entry.address);
        let target: Target = entry.address.parse().map_err(|e| format!("{where_}: {e}"))?;
        let created = match &entry.created {
            Some(t) => time(t).ok_or_else(|| format!("{where_}: created is not an RFC 3339 time"))?,
            None => Local::now(),
        };
        let expires = match &entry.expires {
            Some(t) => Some(time(t).ok_or_else(|| format!("{where_}: expires is not an RFC 3339 time"))?),
            None => None,
        };
        bans.push(Ban {
            target,
            reason: entry.reason.clone(),
            created,
            created_by: entry.created_by.clone(),
            expires,
            automatic: false,
            refused: 0,
        });
    }
    Ok(bans)
}

fn time(text: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|t| t.with_timezone(&Local))
}

fn line_at(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

/// The line that holds this address, for an error message.
fn line_of(text: &str, address: &str) -> usize {
    text.lines().position(|l| l.contains(address)).map_or(1, |i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Local)
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn bans() -> Bans {
        let dir = std::env::temp_dir().join(format!("mqrust-bans-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        Bans::new(
            dir.join(format!("bans-{:?}.toml", std::thread::current().id())),
            Vec::new(),
        )
    }

    fn manual(target: &str) -> Ban {
        Ban::new(target.parse().unwrap(), "test".into(), "ops".into(), None)
    }

    #[test]
    fn addresses_and_ranges_are_parsed() {
        assert_eq!("203.0.113.7".parse::<Target>().unwrap().to_string(), "203.0.113.7");
        assert_eq!("10.1.0.0/16".parse::<Target>().unwrap().to_string(), "10.1.0.0/16");
        assert_eq!("2001:db8::/32".parse::<Target>().unwrap().to_string(), "2001:db8::/32");
        // A full-width prefix is the address itself.
        assert_eq!("10.1.2.3/32".parse::<Target>().unwrap().to_string(), "10.1.2.3");
        // IPv4-mapped IPv6 is read as IPv4.
        assert_eq!(
            "::ffff:203.0.113.7".parse::<Target>().unwrap().to_string(),
            "203.0.113.7"
        );
        for bad in ["300.1.1.1", "10.1.0.0/40", "", "nope", "10.1.0.0/", "2001:db8::/200"] {
            assert!(bad.parse::<Target>().is_err(), "{bad}");
        }
    }

    #[test]
    fn ranges_match_only_their_addresses() {
        let range: Target = "10.1.0.0/16".parse().unwrap();
        assert!(range.matches(ip("10.1.2.3")));
        assert!(!range.matches(ip("10.2.0.1")));
        let odd: Target = "192.168.1.0/28".parse().unwrap();
        assert!(odd.matches(ip("192.168.1.15")));
        assert!(!odd.matches(ip("192.168.1.16")));
        let v6: Target = "2001:db8::/32".parse().unwrap();
        assert!(v6.matches(ip("2001:db8::1")));
        assert!(!v6.matches(ip("2001:db9::1")));
        assert!(!v6.matches(ip("10.1.2.3")), "families do not mix");
        // The same address through a dual-stack socket.
        assert!("203.0.113.7"
            .parse::<Target>()
            .unwrap()
            .matches(ip("::ffff:203.0.113.7")));
    }

    #[test]
    fn a_ban_refuses_until_it_expires() {
        let b = bans();
        b.add(manual("203.0.113.7")).unwrap();
        assert!(b.is_banned(ip("203.0.113.7")));
        assert!(b.refuse(ip("203.0.113.7")));
        assert!(!b.refuse(ip("203.0.113.8")));
        assert_eq!(b.list()[0].refused, 1, "refusals are counted");

        let expired = Ban {
            expires: Some(Local::now() - Span::minutes(1)),
            ..manual("198.51.100.1")
        };
        b.add(expired).unwrap();
        assert!(!b.refuse(ip("198.51.100.1")), "an expired ban refuses nothing");
        assert!(!b.list().iter().any(|x| x.target.to_string() == "198.51.100.1"));
    }

    #[test]
    fn removing_a_ban_lets_the_address_in() {
        let b = bans();
        b.add(manual("10.1.0.0/16")).unwrap();
        assert!(b.refuse(ip("10.1.2.3")));
        assert!(b.remove(&"10.1.0.0/16".parse().unwrap()).unwrap());
        assert!(!b.refuse(ip("10.1.2.3")));
        assert!(!b.remove(&"10.1.0.0/16".parse().unwrap()).unwrap(), "already gone");
    }

    #[test]
    fn three_failures_in_the_window_ban_for_a_minute() {
        let b = bans();
        // Real time here: the ban that follows is checked against the broker's clock.
        let start = Local::now();
        let from = ip("203.0.113.7");
        assert!(b.login_failed_at(from, start).is_none());
        assert!(b.login_failed_at(from, start + Span::seconds(10)).is_none());
        let auto = b.login_failed_at(from, start + Span::seconds(20)).unwrap();
        assert_eq!((auto.seconds, auto.failures), (60, 3));
        assert!(b.is_banned(from));
        let listed = b.list();
        assert!(listed[0].automatic);
        assert_eq!(listed[0].created_by, "broker");
    }

    #[test]
    fn failures_spread_out_do_not_ban() {
        let b = bans();
        let start = at("2026-10-21T09:00:00+00:00");
        let from = ip("203.0.113.9");
        for n in 0..3 {
            assert!(b.login_failed_at(from, start + Span::minutes(n * 10)).is_none(), "{n}");
        }
        assert!(!b.is_banned(from));
    }

    #[test]
    fn each_later_failure_doubles_the_ban() {
        let b = bans();
        let start = at("2026-10-21T09:00:00+00:00");
        let from = ip("203.0.113.7");
        for n in 0..2 {
            b.login_failed_at(from, start + Span::seconds(n));
        }
        assert_eq!(b.login_failed_at(from, start + Span::seconds(3)).unwrap().seconds, 60);
        assert_eq!(b.login_failed_at(from, start + Span::minutes(2)).unwrap().seconds, 120);
        assert_eq!(b.login_failed_at(from, start + Span::minutes(6)).unwrap().seconds, 240);
    }

    #[test]
    fn the_doubling_stops_at_a_day() {
        let b = bans();
        let mut now = at("2026-10-21T09:00:00+00:00");
        let from = ip("203.0.113.7");
        let mut seconds = 0;
        for _ in 0..40 {
            if let Some(auto) = b.login_failed_at(from, now) {
                seconds = auto.seconds;
            }
            now += Span::seconds(1);
        }
        assert_eq!(seconds, MAX_BAN_SECS);
    }

    #[test]
    fn a_successful_login_resets_the_escalation() {
        let b = bans();
        let start = at("2026-10-21T09:00:00+00:00");
        let from = ip("203.0.113.7");
        for n in 0..3 {
            b.login_failed_at(from, start + Span::seconds(n));
        }
        b.remove(&Target::Addr(from)).unwrap();
        b.login_ok(from);
        // Counting starts from zero: two failures are not enough.
        assert!(b.login_failed_at(from, start + Span::minutes(30)).is_none());
        assert!(b.login_failed_at(from, start + Span::minutes(31)).is_none());
        assert_eq!(
            b.login_failed_at(from, start + Span::minutes(32)).unwrap().seconds,
            FIRST_BAN_SECS
        );
    }

    #[test]
    fn a_manual_ban_wins_over_an_automatic_one() {
        let b = bans();
        let start = at("2026-10-21T09:00:00+00:00");
        let from = ip("203.0.113.7");
        b.add(manual("203.0.113.7")).unwrap();
        for n in 0..3 {
            b.login_failed_at(from, start + Span::seconds(n));
        }
        let listed = b.list();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].automatic, "the administrator's ban is kept");
        assert_eq!(listed[0].expires_text(), "permanent");
    }

    #[test]
    fn sweep_forgets_expired_bans_and_old_failures() {
        let b = bans();
        b.add(Ban {
            expires: Some(Local::now() - Span::seconds(1)),
            ..manual("203.0.113.7")
        })
        .unwrap();
        b.add(manual("10.1.0.0/16")).unwrap();
        b.sweep();
        let left: Vec<String> = b.list().iter().map(|x| x.target.to_string()).collect();
        assert_eq!(left, vec!["10.1.0.0/16".to_string()]);
    }

    #[test]
    fn the_file_is_a_round_trip() {
        let b = bans();
        b.add(manual("10.1.0.0/16")).unwrap();
        b.add(Ban::new(
            "203.0.113.7".parse().unwrap(),
            "noisy \"client\"".into(),
            "ops".into(),
            Local::now().checked_add_signed(Span::hours(24)),
        ))
        .unwrap();
        let start = at("2026-10-21T09:00:00+00:00");
        for n in 0..3 {
            b.login_failed_at(ip("198.51.100.5"), start + Span::seconds(n));
        }
        let text = b.to_toml();
        assert!(!text.contains("198.51.100.5"), "automatic bans are not stored:\n{text}");
        let back = parse(&text, Path::new(FILE)).unwrap();
        assert_eq!(back.len(), 2);
        let noisy = back.iter().find(|x| x.target.to_string() == "203.0.113.7").unwrap();
        assert_eq!(noisy.reason, "noisy \"client\"");
        assert_eq!(noisy.created_by, "ops");
        assert!(noisy.expires.is_some());
        assert!(back.iter().any(|x| x.target.to_string() == "10.1.0.0/16"));
    }

    #[test]
    fn a_broken_file_names_the_line_and_the_entry() {
        let path = Path::new("bans.toml");
        let text = "[[ban]]\naddress = \"10.1.0.0/16\"\n\n[[ban]]\naddress = \"10.1.0.0/40\"\n";
        let e = parse(text, path).unwrap_err();
        assert!(e.contains("bans.toml line 5"), "{e}");
        assert!(e.contains("10.1.0.0/40"), "{e}");

        let e = parse("[[ban]]\naddress = 7\n", path).unwrap_err();
        assert!(e.contains("bans.toml line 2"), "{e}");

        let e = parse("[[ban]]\naddress = \"10.0.0.1\"\nwhat = 1\n", path).unwrap_err();
        assert!(e.contains("bans.toml line 3"), "{e}");

        let e = parse("[[ban]]\naddress = \"10.0.0.1\"\nexpires = \"soon\"\n", path).unwrap_err();
        assert!(e.contains("expires"), "{e}");

        assert!(parse("", path).unwrap().is_empty());
    }

    #[test]
    fn durations_are_read_from_the_form() {
        let now = at("2026-10-21T09:00:00+00:00");
        assert_eq!(Length::parse("1h").unwrap().expires(now).unwrap(), now + Span::hours(1));
        assert_eq!(
            Length::parse("24h").unwrap().expires(now).unwrap(),
            now + Span::hours(24)
        );
        assert_eq!(Length::parse("7d").unwrap().expires(now).unwrap(), now + Span::days(7));
        assert!(Length::parse("permanent").unwrap().expires(now).is_none());
        assert!(Length::parse("5m").is_none());
    }
}
