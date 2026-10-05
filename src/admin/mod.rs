// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Read-only admin console: HTML pages behind a form login with server-side sessions, and a
//! JSON API that also accepts HTTP Basic. Only `POST /login` and `POST /logout` change
//! anything, and they change only the console session.

mod api;
mod body;
mod pages;
mod session;
mod xml;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, Response, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Form, Router};
use base64::Engine;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

use crate::auth;
use crate::broker::Broker;
use session::{sha256, Gate, Sessions, Throttle};

pub const CSS: &str = include_str!("style.css");
pub const SESSION_COOKIE: &str = "mqrust_session";
const CSP: &str = "default-src 'none'; style-src 'self'; form-action 'self'; frame-ancestors 'none'";

#[derive(Clone)]
pub struct AdminState {
    pub broker: Arc<Broker>,
    sessions: Arc<Sessions>,
    throttle: Arc<Throttle>,
    /// SHA-256 of the last `Authorization` header verified on the API (avoids an Argon2
    /// check per request); compared in constant time and never written by a failure.
    basic_cache: Arc<Mutex<Option<[u8; 32]>>>,
}

/// The user of the session (or of the Basic credentials) that opened the request.
#[derive(Clone)]
pub struct CurrentUser(pub String);

/// Starts the console and returns its listening address. A bind failure is logged and the
/// broker keeps running without the console (`None`).
pub async fn start(broker: Arc<Broker>, mut shutdown: watch::Receiver<bool>) -> Option<SocketAddr> {
    let addr = SocketAddr::new(broker.cfg.admin_bind, broker.cfg.admin_port);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot start the admin console on {addr}: {e}; continuing without it");
            return None;
        }
    };
    let addr = listener.local_addr().unwrap_or(addr);
    tracing::info!("admin listening on http://{addr}");
    let cfg = &broker.cfg;
    let state = AdminState {
        sessions: Arc::new(Sessions::new(
            Duration::from_secs(cfg.admin_session_idle_minutes * 60),
            Duration::from_secs(cfg.admin_session_max_hours * 3600),
        )),
        throttle: Arc::new(Throttle::new(
            cfg.admin_login_max_failures,
            Duration::from_secs(cfg.admin_login_lockout_seconds),
        )),
        basic_cache: Arc::new(Mutex::new(None)),
        broker,
    };
    let sessions = state.sessions.clone();
    let app = router(state);
    let mut sweep_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = tick.tick() => sessions.sweep(),
                _ = sweep_shutdown.changed() => return,
            }
        }
    });
    tokio::spawn(async move {
        let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async move {
                let _ = shutdown.changed().await;
            });
        if let Err(e) = serve.await {
            tracing::warn!("admin console stopped: {e}");
        }
    });
    Some(addr)
}

fn router(state: AdminState) -> Router {
    Router::new()
        .route("/", get(pages::overview))
        .route("/style.css", get(pages::style))
        .route("/login", get(pages::login).post(login_submit))
        .route("/logout", post(logout))
        .route("/queues", get(pages::queues))
        .route("/queues/{name}", get(pages::queue_detail))
        .route("/queues/{name}/messages/{id}", get(pages::message_detail))
        .route("/topics", get(pages::topics))
        .route("/connections", get(pages::connections))
        .route("/api/overview", get(api::overview))
        .route("/api/queues", get(api::queues))
        .route("/api/queues/{name}", get(api::queue_detail))
        .route("/api/queues/{name}/messages", get(api::messages))
        .route("/api/queues/{name}/messages/{id}", get(api::message))
        .route("/api/topics", get(api::topics))
        .route("/api/connections", get(api::connections))
        .fallback(pages::not_found)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

// -- helpers ------------------------------------------------------------------

fn remote_ip<B>(req: &axum::http::Request<B>) -> IpAddr {
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// Value of a cookie sent by the browser.
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// A redirect target that stays on this console: a path starting with a single `/`.
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n)
            if n.starts_with('/')
                && !n.starts_with("//")
                && !n.starts_with("/\\")
                && !n.chars().any(|c| c.is_control()) =>
        {
            // Percent-encode what cannot appear in a Location header as is.
            let mut out = String::with_capacity(n.len());
            for b in n.bytes() {
                if (0x21..0x7f).contains(&b) {
                    out.push(b as char);
                } else {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
            out
        }
        _ => "/".to_string(),
    }
}

fn redirect(location: &str) -> Response<Body> {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::SEE_OTHER;
    if let Ok(v) = HeaderValue::from_str(location) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

fn json_error(status: StatusCode, error: &str) -> Response<Body> {
    let mut r = Response::new(Body::from(serde_json::json!({ "error": error }).to_string()));
    *r.status_mut() = status;
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    r
}

/// Security headers on every response; `Cache-Control: no-store` except for the stylesheet.
fn secure(mut resp: Response<Body>, no_store: bool) -> Response<Body> {
    let h = resp.headers_mut();
    h.insert("Content-Security-Policy", HeaderValue::from_static(CSP));
    h.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    h.insert("Referrer-Policy", HeaderValue::from_static("same-origin"));
    if no_store {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

/// `Origin` (or else `Referer`) must name the host the request was sent to.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return false };
    let expected = format!("http://{host}");
    if let Some(origin) = headers.get(header::ORIGIN) {
        return origin.to_str().is_ok_and(|o| o.eq_ignore_ascii_case(&expected));
    }
    if let Some(referer) = headers.get(header::REFERER).and_then(|r| r.to_str().ok()) {
        let lower = referer.to_ascii_lowercase();
        let exp = expected.to_ascii_lowercase();
        return lower == exp || lower.starts_with(&format!("{exp}/"));
    }
    true
}

/// Checks admin credentials. The password is verified even for a wrong username, so the
/// response time does not tell whether the username exists. Runs off the async threads.
async fn verify_admin(state: &AdminState, user: String, password: String) -> bool {
    let broker = state.broker.clone();
    tokio::task::spawn_blocking(move || {
        let admin = &broker.cfg.admin_user;
        let password_ok = auth::verify(&admin.secret, &password);
        let user_ok = auth::constant_time_eq(user.as_bytes(), admin.username.as_bytes());
        user_ok & password_ok
    })
    .await
    .unwrap_or(false)
}

fn log_failure(state: &AdminState, ip: IpAddr, user: &str) {
    tracing::warn!("admin login failed: {ip} user={user}");
    if state.throttle.failure(ip) {
        tracing::warn!(
            "admin login: {ip} locked out for {} s after {} failed attempts",
            state.broker.cfg.admin_login_lockout_seconds,
            state.broker.cfg.admin_login_max_failures
        );
    }
}

fn log_locked(ip: IpAddr, log: bool) {
    if log {
        tracing::warn!("admin login refused: {ip} is locked out after repeated failures");
    }
}

enum ApiDenied {
    Unauthorized,
    Locked,
}

/// HTTP Basic on the API, with the digest cache and the per-IP throttling.
async fn api_basic(state: &AdminState, headers: &HeaderMap, ip: IpAddr) -> Result<String, ApiDenied> {
    let Some(value) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return Err(ApiDenied::Unauthorized);
    };
    if let Gate::Locked { log, .. } = state.throttle.check(ip) {
        log_locked(ip, log);
        return Err(ApiDenied::Locked);
    }
    let decoded = value
        .strip_prefix("Basic ")
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
        .and_then(|b| String::from_utf8(b).ok());
    let (user, password) = match decoded.as_deref().and_then(|s| s.split_once(':')) {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => (String::new(), String::new()),
    };
    let digest = sha256(value.as_bytes());
    let cached = state.basic_cache.lock().is_some_and(|d| auth::constant_time_eq(&d, &digest));
    if cached || verify_admin(state, user.clone(), password).await {
        *state.basic_cache.lock() = Some(digest);
        state.throttle.success(ip);
        Ok(user)
    } else {
        log_failure(state, ip, &user);
        Err(ApiDenied::Unauthorized)
    }
}

// -- guard --------------------------------------------------------------------

/// Methods, authentication and security headers for every request.
async fn guard(State(state): State<AdminState>, mut req: Request, next: Next) -> Response<Body> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let login_post = method == Method::POST && (path == "/login" || path == "/logout");
    if !(method == Method::GET || method == Method::HEAD || login_post) {
        let mut r = Response::new(Body::from("Method Not Allowed"));
        *r.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
        r.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return secure(r, true);
    }
    if path == "/style.css" {
        return secure(next.run(req).await, false);
    }
    if path == "/login" || path == "/logout" {
        return secure(next.run(req).await, true);
    }
    let session_user = cookie(req.headers(), SESSION_COOKIE).and_then(|t| state.sessions.lookup(&t));
    let user = if path == "/api" || path.starts_with("/api/") {
        match session_user {
            Some(u) => u,
            None => match api_basic(&state, req.headers(), remote_ip(&req)).await {
                Ok(u) => u,
                Err(ApiDenied::Unauthorized) => return secure(json_error(StatusCode::UNAUTHORIZED, "unauthorized"), true),
                Err(ApiDenied::Locked) => {
                    return secure(json_error(StatusCode::TOO_MANY_REQUESTS, "too many failed logins"), true)
                }
            },
        }
    } else {
        match session_user {
            Some(u) => u,
            None => {
                let target = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
                return secure(redirect(&format!("/login?next={}", pages::enc(target))), true);
            }
        }
    };
    req.extensions_mut().insert(CurrentUser(user));
    secure(next.run(req).await, true)
}

// -- login, logout -------------------------------------------------------

async fn login_submit(
    State(state): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response<Body> {
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "Forbidden: cross-site request").into_response();
    }
    let ip = addr.ip();
    let user = form.get("username").cloned().unwrap_or_default();
    let next = form.get("next").map(String::as_str);
    if let Gate::Locked { remaining, log } = state.throttle.check(ip) {
        log_locked(ip, log);
        let msg = format!("Too many failed logins. Try again in {} seconds.", remaining.as_secs().max(1));
        let mut r = pages::login_page(&state, &user, Some(&msg), next).into_response();
        *r.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        return r;
    }
    let ok = match (form.get("username"), form.get("password")) {
        (Some(u), Some(p)) => verify_admin(&state, u.clone(), p.clone()).await,
        _ => false,
    };
    if !ok {
        log_failure(&state, ip, &user);
        return pages::login_page(&state, &user, Some("Invalid username or password"), next).into_response();
    }
    state.throttle.success(ip);
    // A new session at every login: a cookie sent with the login is discarded.
    if let Some(old) = cookie(&headers, SESSION_COOKIE) {
        state.sessions.remove(&old);
    }
    let token = state.sessions.create(&user);
    tracing::info!("admin login: {ip} user={user}");
    let mut r = redirect(&safe_next(next));
    if let Ok(v) = HeaderValue::from_str(&format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/")) {
        r.headers_mut().insert(header::SET_COOKIE, v);
    }
    r
}

async fn logout(State(state): State<AdminState>, headers: HeaderMap) -> Response<Body> {
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "Forbidden: cross-site request").into_response();
    }
    if let Some(token) = cookie(&headers, SESSION_COOKIE) {
        state.sessions.remove(&token);
    }
    let mut r = redirect("/login");
    r.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("mqrust_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    r
}

// -- formatting ---------------------------------------------------------------

/// Process Working Set and Private Bytes, in bytes.
pub fn process_memory() -> (u64, u64) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut _, c.cb) != 0 {
            return (c.WorkingSetSize as u64, c.PrivateUsage as u64);
        }
        (0, 0)
    }
    #[cfg(not(windows))]
    {
        (0, 0)
    }
}

pub fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

pub fn fmt_time_ms(ms: i64) -> String {
    if ms <= 0 {
        return "-".into();
    }
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(t) => t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        None => ms.to_string(),
    }
}

pub fn fmt_duration(secs: i64) -> String {
    let (d, h, m, s) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h {m}m {s}s")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// True when a message has an expiration that has passed (it may still await removal).
pub fn is_expired(msg: &crate::openwire::model::Message, now: i64) -> bool {
    msg.expiration > 0 && msg.expiration <= now
}

/// Text for an expiration: "never", "expired" or the local time with the remaining time.
pub fn fmt_expiration(exp: i64, now: i64) -> String {
    if exp <= 0 {
        "never".into()
    } else if exp <= now {
        format!("{} (expired)", fmt_time_ms(exp))
    } else {
        format!("{} (in {})", fmt_time_ms(exp), fmt_duration((exp - now) / 1000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_targets_stay_local() {
        assert_eq!(safe_next(Some("/queues?refresh=5")), "/queues?refresh=5");
        assert_eq!(safe_next(Some("/queues/a b")), "/queues/a%20b");
        for bad in ["//evil.example/", "https://evil.example/", "/\\evil.example", "", "queues", "/a\r\nSet-Cookie: x"] {
            assert_eq!(safe_next(Some(bad)), "/", "{bad:?}");
        }
        assert_eq!(safe_next(None), "/");
    }

    #[test]
    fn origin_check() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8161"));
        assert!(same_origin(&h));
        h.insert(header::REFERER, HeaderValue::from_static("http://127.0.0.1:8161/login?next=%2F"));
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://127.0.0.1:8161"));
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("https://evil.example"));
        assert!(!same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("null"));
        assert!(!same_origin(&h));
        h.remove(header::ORIGIN);
        h.insert(header::REFERER, HeaderValue::from_static("http://127.0.0.1:81610/"));
        assert!(!same_origin(&h));
    }

    #[test]
    fn cookies_are_parsed() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; mqrust_session=tok; other=dark"));
        assert_eq!(cookie(&h, SESSION_COOKIE).as_deref(), Some("tok"));
        assert_eq!(cookie(&h, "other").as_deref(), Some("dark"));
        assert_eq!(cookie(&h, "missing"), None);
    }

    #[test]
    fn expiration_text() {
        assert_eq!(fmt_expiration(0, 1000), "never");
        assert!(fmt_expiration(500, 1000).ends_with("(expired)"));
        assert!(fmt_expiration(61_000 + 1000, 1000).ends_with("(in 1m 1s)"));
    }
}
