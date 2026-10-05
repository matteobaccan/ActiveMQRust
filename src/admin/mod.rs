// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Read-only admin console: HTML pages and JSON API behind HTTP Basic authentication.

mod api;
mod body;
mod pages;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Response, StatusCode};
use axum::middleware::{self, Next};
use axum::routing::get;
use axum::Router;
use base64::Engine;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;

use crate::auth;
use crate::broker::Broker;

pub const CSS: &str = include_str!("style.css");

#[derive(Clone)]
pub struct AdminState {
    pub broker: Arc<Broker>,
    /// Authorization header values already verified (avoids an Argon2 check per request).
    verified: Arc<Mutex<HashSet<String>>>,
}

/// Starts the console. A bind failure is logged and the broker keeps running without it.
pub async fn start(broker: Arc<Broker>, mut shutdown: watch::Receiver<bool>) {
    let addr = SocketAddr::new(broker.cfg.admin_bind, broker.cfg.admin_port);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot start the admin console on {addr}: {e}; continuing without it");
            return;
        }
    };
    tracing::info!("admin listening on http://{addr}");
    let state = AdminState { broker, verified: Arc::new(Mutex::new(HashSet::new())) };
    let app = Router::new()
        .route("/", get(pages::overview))
        .route("/style.css", get(pages::style))
        .route("/queues", get(pages::queues))
        .route("/queues/{name}", get(pages::queue_detail))
        .route("/queues/{name}/messages/{id}", get(pages::message_detail))
        .route("/topics", get(pages::topics))
        .route("/connections", get(pages::connections))
        .route("/api/overview", get(api::overview))
        .route("/api/queues", get(api::queues))
        .route("/api/queues/{name}", get(api::queue_detail))
        .route("/api/queues/{name}/messages", get(api::messages))
        .route("/api/topics", get(api::topics))
        .route("/api/connections", get(api::connections))
        .fallback(pages::not_found)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state);
    tokio::spawn(async move {
        let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async move {
                let _ = shutdown.changed().await;
            });
        if let Err(e) = serve.await {
            tracing::warn!("admin console stopped: {e}");
        }
    });
}

fn unauthorized() -> Response<Body> {
    let mut r = Response::new(Body::from("Authentication required"));
    *r.status_mut() = StatusCode::UNAUTHORIZED;
    r.headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"ActiveMQRust\""));
    r
}

/// Authentication, read-only enforcement and security headers for every request.
async fn guard(State(state): State<AdminState>, req: Request, next: Next) -> Response<Body> {
    let remote = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|c| c.0.to_string())
        .unwrap_or_else(|| "?".into());
    let auth_header = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).map(str::to_string);
    let Some(auth_header) = auth_header else { return unauthorized() };
    let ok = if state.verified.lock().contains(&auth_header) {
        true
    } else {
        let decoded = auth_header
            .strip_prefix("Basic ")
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b.trim()).ok())
            .and_then(|b| String::from_utf8(b).ok());
        let (user, pass) = match decoded.as_deref().and_then(|s| s.split_once(':')) {
            Some((u, p)) => (u.to_string(), p.to_string()),
            None => (String::new(), String::new()),
        };
        let admin = &state.broker.cfg.admin_user;
        let good = auth::constant_time_eq(user.as_bytes(), admin.username.as_bytes()) && auth::verify(&admin.secret, &pass);
        if good {
            let mut v = state.verified.lock();
            if v.len() > 64 {
                v.clear();
            }
            v.insert(auth_header);
        } else {
            tracing::warn!("admin login failed: {remote} user={user}");
        }
        good
    };
    if !ok {
        return unauthorized();
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("Content-Security-Policy", HeaderValue::from_static("default-src 'none'; style-src 'self'"));
    h.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

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
