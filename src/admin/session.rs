// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Console sessions and login throttling, held only in memory.
//!
//! Session tokens are 32 random bytes from the operating system, sent to the browser in
//! URL-safe base64; the store keeps only their SHA-256, so a memory dump yields no usable
//! cookie. Failed logins are counted per client IP in a bounded table.

use base64::Engine;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Most sessions held at once; the oldest is dropped beyond this.
pub const MAX_SESSIONS: usize = 1_000;
/// Most client IPs tracked by the throttling; the oldest is dropped beyond this.
pub const MAX_TRACKED_IPS: usize = 10_000;
/// Window in which failed logins are counted.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

struct Session {
    user: String,
    created: Instant,
    last_seen: Instant,
}

pub struct Sessions {
    map: Mutex<HashMap<[u8; 32], Session>>,
    idle: Duration,
    max_age: Duration,
}

impl Sessions {
    pub fn new(idle: Duration, max_age: Duration) -> Self {
        Sessions {
            map: Mutex::new(HashMap::new()),
            idle,
            max_age,
        }
    }

    fn alive(&self, s: &Session, now: Instant) -> bool {
        now.saturating_duration_since(s.last_seen) < self.idle
            && now.saturating_duration_since(s.created) < self.max_age
    }

    /// Opens a session for `user` and returns its token (to be sent only in the cookie).
    pub fn create(&self, user: &str) -> String {
        self.create_at(user, Instant::now())
    }

    pub fn create_at(&self, user: &str, now: Instant) -> String {
        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).expect("operating system random generator");
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
        let mut map = self.map.lock();
        map.retain(|_, s| self.alive(s, now));
        while map.len() >= MAX_SESSIONS {
            let Some(oldest) = map.iter().min_by_key(|(_, s)| s.created).map(|(k, _)| *k) else {
                break;
            };
            map.remove(&oldest);
        }
        map.insert(
            sha256(token.as_bytes()),
            Session {
                user: user.to_string(),
                created: now,
                last_seen: now,
            },
        );
        token
    }

    /// The user of a live session, refreshing its idle timer; expired sessions are removed.
    pub fn lookup(&self, token: &str) -> Option<String> {
        self.lookup_at(token, Instant::now())
    }

    pub fn lookup_at(&self, token: &str, now: Instant) -> Option<String> {
        let key = sha256(token.as_bytes());
        let mut map = self.map.lock();
        let s = map.get_mut(&key)?;
        if !self.alive(s, now) {
            map.remove(&key);
            return None;
        }
        s.last_seen = now;
        Some(s.user.clone())
    }

    pub fn remove(&self, token: &str) {
        self.map.lock().remove(&sha256(token.as_bytes()));
    }

    /// Drops expired sessions (run every minute).
    pub fn sweep(&self) {
        let now = Instant::now();
        self.map.lock().retain(|_, s| self.alive(s, now));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.lock().len()
    }
}

struct Failures {
    count: u32,
    first: Instant,
    locked_until: Option<Instant>,
    /// The refused attempts of the current lockout have been logged.
    logged: bool,
}

/// Outcome of checking a client IP before verifying its credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Open,
    /// Locked out for this long; `log` is true for the first refusal of the lockout.
    Locked {
        remaining: Duration,
        log: bool,
    },
}

pub struct Throttle {
    map: Mutex<HashMap<IpAddr, Failures>>,
    max_failures: u32,
    lockout: Duration,
}

impl Throttle {
    pub fn new(max_failures: u32, lockout: Duration) -> Self {
        Throttle {
            map: Mutex::new(HashMap::new()),
            max_failures,
            lockout,
        }
    }

    pub fn check(&self, ip: IpAddr) -> Gate {
        self.check_at(ip, Instant::now())
    }

    pub fn check_at(&self, ip: IpAddr, now: Instant) -> Gate {
        if self.max_failures == 0 {
            return Gate::Open;
        }
        let mut map = self.map.lock();
        let Some(f) = map.get_mut(&ip) else {
            return Gate::Open;
        };
        match f.locked_until {
            Some(until) if now < until => {
                let log = !f.logged;
                f.logged = true;
                Gate::Locked {
                    remaining: until - now,
                    log,
                }
            }
            Some(_) => {
                // The lockout is over: start counting again.
                map.remove(&ip);
                Gate::Open
            }
            None => Gate::Open,
        }
    }

    /// Records a failed login; returns true when it starts a lockout.
    pub fn failure(&self, ip: IpAddr) -> bool {
        self.failure_at(ip, Instant::now())
    }

    pub fn failure_at(&self, ip: IpAddr, now: Instant) -> bool {
        if self.max_failures == 0 {
            return false;
        }
        let mut map = self.map.lock();
        if !map.contains_key(&ip) && map.len() >= MAX_TRACKED_IPS {
            map.retain(|_, f| {
                now.saturating_duration_since(f.first) < FAILURE_WINDOW || f.locked_until.is_some_and(|u| now < u)
            });
            while map.len() >= MAX_TRACKED_IPS {
                let Some(oldest) = map.iter().min_by_key(|(_, f)| f.first).map(|(k, _)| *k) else {
                    break;
                };
                map.remove(&oldest);
            }
        }
        let f = map.entry(ip).or_insert(Failures {
            count: 0,
            first: now,
            locked_until: None,
            logged: false,
        });
        if now.saturating_duration_since(f.first) >= FAILURE_WINDOW {
            *f = Failures {
                count: 0,
                first: now,
                locked_until: None,
                logged: false,
            };
        }
        f.count += 1;
        if f.count >= self.max_failures && f.locked_until.is_none() {
            f.locked_until = Some(now + self.lockout);
            return true;
        }
        false
    }

    /// A successful login resets the counter of the IP.
    pub fn success(&self, ip: IpAddr) {
        if self.max_failures > 0 {
            self.map.lock().remove(&ip);
        }
    }

    #[cfg(test)]
    pub fn tracked(&self) -> usize {
        self.map.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, n])
    }

    #[test]
    fn tokens_are_random_and_stored_hashed() {
        let s = Sessions::new(Duration::from_secs(60), Duration::from_secs(3600));
        let a = s.create("admin");
        let b = s.create("admin");
        assert_ne!(a, b);
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&a)
                .unwrap()
                .len(),
            32
        );
        assert!(s.map.lock().contains_key(&sha256(a.as_bytes())));
        assert!(!s.map.lock().keys().any(|k| k.as_slice() == a.as_bytes()));
        assert_eq!(s.lookup(&a).as_deref(), Some("admin"));
        s.remove(&a);
        assert_eq!(s.lookup(&a), None);
        assert_eq!(s.lookup("not-a-token"), None);
    }

    #[test]
    fn idle_timeout_and_absolute_lifetime() {
        let s = Sessions::new(Duration::from_secs(30 * 60), Duration::from_secs(8 * 3600));
        let t0 = Instant::now();
        let a = s.create_at("admin", t0);
        assert!(s.lookup_at(&a, t0 + Duration::from_secs(29 * 60)).is_some());
        // Idle for 31 minutes after the last request.
        assert!(s.lookup_at(&a, t0 + Duration::from_secs(60 * 60)).is_none());
        // Kept alive every 5 seconds, but ended after 8 hours.
        let b = s.create_at("admin", t0);
        let mut t = t0;
        while t + Duration::from_secs(300) < t0 + Duration::from_secs(8 * 3600) {
            t += Duration::from_secs(300);
            assert!(s.lookup_at(&b, t).is_some());
        }
        assert!(s.lookup_at(&b, t0 + Duration::from_secs(8 * 3600)).is_none());
    }

    #[test]
    fn session_cap() {
        let s = Sessions::new(Duration::from_secs(60), Duration::from_secs(3600));
        let t0 = Instant::now();
        let first = s.create_at("admin", t0);
        for i in 1..=MAX_SESSIONS {
            s.create_at("admin", t0 + Duration::from_millis(i as u64));
        }
        assert_eq!(s.len(), MAX_SESSIONS);
        assert!(s.lookup_at(&first, t0 + Duration::from_secs(1)).is_none());
    }

    #[test]
    fn lockout_after_failures_then_expiry() {
        let t = Throttle::new(5, Duration::from_secs(60));
        let t0 = Instant::now();
        for i in 0..4 {
            assert!(!t.failure_at(ip(1), t0 + Duration::from_secs(i)));
            assert_eq!(t.check_at(ip(1), t0 + Duration::from_secs(i)), Gate::Open);
        }
        assert!(t.failure_at(ip(1), t0 + Duration::from_secs(5)));
        assert!(matches!(
            t.check_at(ip(1), t0 + Duration::from_secs(6)),
            Gate::Locked { log: true, .. }
        ));
        assert!(matches!(
            t.check_at(ip(1), t0 + Duration::from_secs(7)),
            Gate::Locked { log: false, .. }
        ));
        // Other clients are not affected.
        assert_eq!(t.check_at(ip(2), t0 + Duration::from_secs(7)), Gate::Open);
        // 61 seconds after the lockout started.
        assert_eq!(t.check_at(ip(1), t0 + Duration::from_secs(66)), Gate::Open);
        assert_eq!(t.tracked(), 0);
    }

    #[test]
    fn window_success_and_disabled() {
        let t = Throttle::new(3, Duration::from_secs(60));
        let t0 = Instant::now();
        t.failure_at(ip(1), t0);
        t.failure_at(ip(1), t0);
        // Failures older than 15 minutes do not count.
        assert!(!t.failure_at(ip(1), t0 + FAILURE_WINDOW));
        assert!(!t.failure_at(ip(1), t0 + FAILURE_WINDOW));
        t.success(ip(1));
        assert!(!t.failure_at(ip(1), t0 + FAILURE_WINDOW));
        let off = Throttle::new(0, Duration::from_secs(60));
        for _ in 0..50 {
            assert!(!off.failure(ip(1)));
        }
        assert_eq!(off.check(ip(1)), Gate::Open);
    }

    #[test]
    fn failure_table_is_bounded() {
        let t = Throttle::new(5, Duration::from_secs(60));
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED_IPS as u32 + 100) {
            t.failure_at(IpAddr::from(i.to_be_bytes()), t0 + Duration::from_millis(i as u64));
        }
        assert!(t.tracked() <= MAX_TRACKED_IPS);
    }
}
