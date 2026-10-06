// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! HTML pages of the admin console. Every value from broker data goes through `esc`.

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Write;
use std::sync::Arc;

use super::body::{full_text, hex_dump, render, stored_size, BodyView, HEX_LIMIT};
use super::sort::{Column, Table};
use super::xml::{self, XmlError};
use super::{fmt_bytes, fmt_duration, fmt_expiration, fmt_time_ms, process_memory, AdminState, CurrentUser, CSS};
use crate::broker::conn::{ConnHandle, ConnInfo};
use crate::broker::destination::{Dest, DestSnapshot, ProducerMeta, SubSnapshot};
use crate::broker::entry::Entry;
use crate::broker::now_ms;
use crate::openwire::props::Value;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

/// Project repository, from the crate metadata.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// Messages per contents page.
pub const PAGE_SIZE: usize = 50;
/// Query parameters a page keeps on its own links, in this order.
const KEPT: [&str; 17] = [
    "page",
    "q",
    "pending",
    "noconsumers",
    "sort",
    "order",
    "csort",
    "corder",
    "psort",
    "porder",
    "prsort",
    "prorder",
    "msort",
    "morder",
    "view",
    "seq",
    "refresh",
];

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

/// Percent-encodes a path segment or query value.
pub fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            let _ = write!(o, "%{b:02X}");
        }
    }
    o
}

/// What a page needs from the request: user, path and query.
pub struct Ctx {
    pub user: Option<String>,
    /// Request path as received (percent-encoded).
    pub path: String,
    pub q: HashMap<String, String>,
}

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let q = Query::<HashMap<String, String>>::try_from_uri(&parts.uri)
            .map(|q| q.0)
            .unwrap_or_default();
        Ok(Ctx {
            user: parts.extensions.get::<CurrentUser>().map(|u| u.0.clone()),
            path: parts.uri.path().to_string(),
            q,
        })
    }
}

impl Ctx {
    fn refresh(&self) -> bool {
        self.q.get("refresh").map(String::as_str) == Some("5")
    }

    pub(super) fn get(&self, k: &str) -> Option<&str> {
        self.q.get(k).map(String::as_str)
    }

    /// `path` with the given parameters, plus `refresh=5` when auto-refresh is on.
    fn link(&self, path: &str, params: &[(&str, &str)]) -> String {
        let mut all: Vec<(&str, &str)> = params.to_vec();
        if self.refresh() && !params.iter().any(|(k, _)| *k == "refresh") {
            all.push(("refresh", "5"));
        }
        url(path, &all)
    }

    /// The current page with one parameter changed (`None` removes it).
    fn with(&self, key: &str, value: Option<&str>) -> String {
        self.with_kept(&[(key, value)])
    }

    /// The current page with the given parameters updated (`None` removes them),
    /// keeping all other parameters present in `KEPT`.
    pub(super) fn with_kept(&self, updates: &[(&str, Option<&str>)]) -> String {
        let mut params: Vec<(&str, &str)> = Vec::new();
        for k in KEPT {
            if let Some((_, opt_val)) = updates.iter().find(|(uk, _)| *uk == k) {
                if let Some(v) = opt_val {
                    params.push((k, v));
                }
            } else if let Some(v) = self.get(k) {
                params.push((k, v));
            }
        }
        url(&self.path, &params)
    }
}

fn url(path: &str, params: &[(&str, &str)]) -> String {
    let mut u = path.to_string();
    for (i, (k, v)) in params.iter().enumerate() {
        u.push(if i == 0 { '?' } else { '&' });
        let _ = write!(u, "{k}={}", enc(v));
    }
    u
}

fn head(title: &str, refresh: bool) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"light dark\">{}\
         <title>{} - {PROVIDER_NAME}</title><link rel=\"stylesheet\" href=\"/style.css\"></head>",
        if refresh {
            "<meta http-equiv=\"refresh\" content=\"5\">"
        } else {
            ""
        },
        esc(title),
    )
}

fn footer() -> String {
    format!(
        "<footer class=\"foot\"><span>{PROVIDER_NAME} {PROVIDER_VERSION}</span><span class=\"sep\" aria-hidden=\"true\">&middot;</span>\
         <a href=\"{REPOSITORY}\" rel=\"noopener noreferrer\">{REPOSITORY}</a><span class=\"sep\" aria-hidden=\"true\">&middot;</span>\
         <span>by Matteo Baccan</span></footer>"
    )
}

/// The page shell: top bar (navigation, refresh, user, logout), content and footer.
fn layout(ctx: &Ctx, title: &str, section: &str, content: &str) -> Response {
    let mut nav = String::new();
    for (href, label) in [
        ("/", "Overview"),
        ("/queues", "Queues"),
        ("/topics", "Topics"),
        ("/connections", "Connections"),
    ] {
        let current = if section == href { " aria-current=\"page\"" } else { "" };
        let _ = write!(nav, "<a href=\"{}\"{current}>{label}</a>", ctx.link(href, &[]));
    }
    let refresh = if ctx.refresh() {
        format!(
            "<a class=\"chip on\" href=\"{}\">Auto-refresh on</a>",
            esc(&ctx.with("refresh", None))
        )
    } else {
        format!(
            "<a class=\"chip\" href=\"{}\">Auto-refresh off</a>",
            esc(&ctx.with("refresh", Some("5")))
        )
    };
    let user = match &ctx.user {
        Some(u) => format!(
            "<span class=\"user\" title=\"Logged in user\">{}</span>\
             <form method=\"post\" action=\"/logout\" class=\"logout\"><button type=\"submit\">Log out</button></form>",
            esc(u)
        ),
        None => String::new(),
    };
    let html = format!(
        "{}<body><header class=\"top\"><a class=\"brand\" href=\"{}\">ActiveMQ<span>Rust</span></a>\
         <nav class=\"nav-wide\" aria-label=\"Main\">{nav}</nav>\
         <details class=\"nav-menu\"><summary>Menu</summary><nav aria-label=\"Main menu\">{nav}</nav></details>\
         <div class=\"tools\">{refresh}{user}</div></header>\
         <main>{content}</main>{}</body></html>",
        head(title, ctx.refresh()),
        ctx.link("/", &[]),
        footer(),
    );
    Html(html).into_response()
}

pub async fn style() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS).into_response()
}

pub async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Html("<!doctype html><html lang=\"en\"><title>Not found</title><h1>404 Not Found</h1></html>".to_string()),
    )
        .into_response()
}

fn not_found_page(ctx: &Ctx, what: &str) -> Response {
    let mut r = layout(
        ctx,
        "Not found",
        "",
        &format!("<h1>Not found</h1><p class=\"notice\">{}</p>", esc(what)),
    );
    *r.status_mut() = StatusCode::NOT_FOUND;
    r
}

// -- login --------------------------------------------------------------------

pub fn login_page(state: &AdminState, user: &str, error: Option<&str>, next: Option<&str>) -> Html<String> {
    let mut c = String::new();
    let _ = write!(
        c,
        "{}<body class=\"login\"><main><div class=\"login-box\">",
        head("Log in", false)
    );
    let _ = write!(c, "<p class=\"brand\">ActiveMQ<span>Rust</span></p><h1>Log in</h1>");
    if state.broker.cfg.default_credentials {
        c.push_str(
            "<p class=\"notice warn\" role=\"alert\"><strong>Warning:</strong> the default admin credentials are in use. \
             Change the default password in mqrust.toml (<code>[admin]</code>).</p>",
        );
    }
    if let Some(e) = error {
        let _ = write!(c, "<p class=\"notice error\" role=\"alert\">{}</p>", esc(e));
    }
    let _ = write!(
        c,
        "<form method=\"post\" action=\"/login\" class=\"login-form\">\
         <label for=\"username\">Username</label>\
         <input id=\"username\" name=\"username\" type=\"text\" autocomplete=\"username\" autocapitalize=\"none\" spellcheck=\"false\" required value=\"{}\">\
         <label for=\"password\">Password</label>\
         <input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required>\
         <input type=\"hidden\" name=\"next\" value=\"{}\">\
         <button type=\"submit\">Log in</button></form>",
        esc(user),
        esc(next.unwrap_or("/")),
    );
    let _ = write!(c, "</div></main>{}</body></html>", footer());
    Html(c)
}

pub async fn login(State(s): State<AdminState>, ctx: Ctx) -> Html<String> {
    login_page(&s, "", None, ctx.get("next"))
}

// -- data pages -------------------------------------------------------------------

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

fn card(label: &str, value: &str) -> String {
    format!("<div class=\"card\"><span class=\"label\">{label}</span><span class=\"value\">{value}</span></div>")
}

fn badge(kind: &str, word: &str) -> String {
    format!(" <span class=\"badge {kind}\">{word}</span>")
}

pub async fn overview(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let b = &s.broker;
    let dests = b.destinations();
    let queues = dests.iter().filter(|d| d.dest.kind.is_queue()).count();
    let topics = dests.iter().filter(|d| d.dest.kind.is_topic() && visible(d)).count();
    let (ws, private) = process_memory();
    let uptime = (chrono::Local::now() - b.started).num_seconds();
    let limit = if b.memory.limit == 0 {
        "no limit".to_string()
    } else {
        format!("of {}", fmt_bytes(b.memory.limit))
    };
    let openwire = esc(&format!("tcp://{}:{}", b.cfg.bind, b.cfg.port));
    let admin = esc(&format!("http://{}:{}", b.cfg.admin_bind, b.cfg.admin_port));
    let mut c = format!(
        "<h1>{PROVIDER_NAME} {PROVIDER_VERSION}</h1>\
         <p class=\"addresses\"><span><span class=\"label\">OpenWire</span> <code>{openwire}</code></span>\
         <span><span class=\"label\">Admin console</span> <code>{admin}</code></span></p>\
         <div class=\"cards\">"
    );
    c.push_str(&card("Uptime", &fmt_duration(uptime)));
    c.push_str(&card("Active connections", &b.connections().len().to_string()));
    c.push_str(&card("Queues", &queues.to_string()));
    c.push_str(&card("Topics", &topics.to_string()));
    let mem = format!(
        "{} <small>{limit}</small>{}",
        fmt_bytes(b.memory.used()),
        if b.memory_limited() {
            badge("warn", "limit reached")
        } else {
            String::new()
        }
    );
    c.push_str(&card("Message memory", &mem));
    let relaxed = std::sync::atomic::Ordering::Relaxed;
    c.push_str(&card(
        "Compressed by the broker",
        &b.stats.compressed.load(relaxed).to_string(),
    ));
    c.push_str(&card(
        "Compressions discarded",
        &b.stats.compress_discarded.load(relaxed).to_string(),
    ));
    c.push_str(&card("Working Set (RSS)", &fmt_bytes(ws)));
    c.push_str(&card("Private Bytes", &fmt_bytes(private)));
    c.push_str("</div>");
    layout(&ctx, "Overview", "/", &c)
}

/// The filter of the queues page and `/api/queues`: name contains `q` (any case), and optionally
/// only queues with pending messages and only queues without consumers, combined with AND.
#[derive(Debug, Default)]
pub struct QueueFilter {
    /// Trimmed, at most 200 characters; empty = no name filter.
    pub q: String,
    needle: String,
    pub pending: bool,
    pub noconsumers: bool,
}

impl QueueFilter {
    pub fn from_params(p: &HashMap<String, String>) -> Self {
        let q: String = p
            .get("q")
            .map(|q| q.trim().chars().take(200).collect())
            .unwrap_or_default();
        let on = |k: &str| p.get(k).map(String::as_str) == Some("1");
        QueueFilter {
            needle: q.to_lowercase(),
            q,
            pending: on("pending"),
            noconsumers: on("noconsumers"),
        }
    }

    pub fn is_active(&self) -> bool {
        !self.q.is_empty() || self.pending || self.noconsumers
    }

    pub fn matches(&self, s: &DestSnapshot) -> bool {
        (!self.pending || s.pending > 0)
            && (!self.noconsumers || s.consumers.is_empty())
            && (self.needle.is_empty() || s.dest.name.to_lowercase().contains(&self.needle))
    }
}

pub static QUEUES_TABLE: Table<DestSnapshot> = Table {
    prefix: "",
    default: "name",
    columns: &[
        Column::text("name", "Name", |s| s.dest.name.as_ref().into()),
        Column::number("pending", "Pending", |s| s.pending as u64),
        Column::number("inflight", "Inflight", |s| s.inflight as u64),
        Column::number("consumers", "Consumers", |s| s.consumers.len() as u64),
        Column::number("producers", "Producers", |s| s.producers.len() as u64),
        Column::number("enqueued", "Enqueued", |s| s.stats.enqueued),
        Column::number("consumed", "Consumed", |s| s.stats.dequeued),
        Column::number("expired", "Expired", |s| s.stats.expired),
    ],
};

pub static TOPICS_TABLE: Table<DestSnapshot> = Table {
    prefix: "",
    default: "name",
    columns: &[
        Column::text("name", "Name", |s| s.dest.name.as_ref().into()),
        Column::number("consumers", "Consumers", |s| s.consumers.len() as u64),
        Column::number("producers", "Producers", |s| s.producers.len() as u64),
        Column::number("published", "Published", |s| s.stats.enqueued),
        Column::number("discarded", "Discarded", |s| s.stats.discarded),
    ],
};

/// One row of the connections table: the connection's info and where it comes from.
pub struct ConnRow {
    pub info: ConnInfo,
    pub remote: String,
    pub connected_at: chrono::DateTime<chrono::Local>,
}

impl ConnRow {
    pub fn of(c: &ConnHandle) -> Self {
        ConnRow {
            info: c.info.lock().clone(),
            remote: c.remote.to_string(),
            connected_at: c.connected_at,
        }
    }
}

pub static CONNECTIONS_TABLE: Table<ConnRow> = Table {
    prefix: "",
    default: "connected",
    columns: &[
        Column::text("connectionId", "Connection ID", |c| {
            c.info.connection_id.as_str().into()
        }),
        Column::text("user", "User", |c| c.info.user.as_str().into()),
        Column::address("client", "Client", |c| c.remote.as_str().into()),
        Column::number("openwire", "OpenWire", |c| c.info.version as u64),
        Column::time("connected", "Connected", |c| c.connected_at.timestamp_millis()),
        Column::number("sessions", "Sessions", |c| c.info.sessions as u64),
        Column::number("consumers", "Consumers", |c| c.info.consumers as u64),
        Column::number("producers", "Producers", |c| c.info.producers as u64),
    ],
};

pub static CONSUMERS_TABLE: Table<SubSnapshot> = Table {
    prefix: "c",
    default: "consumerId",
    columns: &[
        Column::text("consumerId", "Consumer ID", |c| c.consumer_id.as_str().into()),
        Column::text("connectionId", "Connection ID", |c| c.connection_id.as_str().into()),
        Column::address("client", "Client", |c| c.remote.as_str().into()),
        Column::number("prefetch", "Prefetch", |c| c.prefetch as u64),
        Column::number("inflight", "Inflight", |c| c.inflight as u64),
        Column::text("selector", "Selector", |c| c.selector.as_deref().unwrap_or("").into()),
    ],
};

pub static PRODUCERS_TABLE: Table<(String, ProducerMeta)> = Table {
    prefix: "p",
    default: "producerId",
    columns: &[
        Column::text("producerId", "Producer ID", |p| p.0.as_str().into()),
        Column::text("connectionId", "Connection ID", |p| p.1.connection_id.as_str().into()),
        Column::address("client", "Client", |p| p.1.remote.as_str().into()),
    ],
};

pub static PROPERTIES_TABLE: Table<(String, Value)> = Table {
    prefix: "pr",
    default: "name",
    columns: &[
        Column::text("name", "Name", |p| p.0.as_str().into()),
        Column::text("type", "Type", |p| super::body::java_type(&p.1).into()),
        Column::text("value", "Value", |p| p.1.display().into()),
    ],
};

pub static MAP_ENTRIES_TABLE: Table<(String, &'static str, String)> = Table {
    prefix: "m",
    default: "key",
    columns: &[
        Column::text("key", "Key", |m| m.0.as_str().into()),
        Column::text("type", "Type", |m| m.1.into()),
        Column::text("value", "Value", |m| m.2.as_str().into()),
    ],
};

pub async fn queues(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut rows: Vec<DestSnapshot> = s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_queue())
        .map(|d| d.snapshot())
        .collect();
    let total = rows.len();
    let filter = QueueFilter::from_params(&ctx.q);
    rows.retain(|q| filter.matches(q));
    let checked = |on: bool| if on { " checked" } else { "" };
    let mut form = format!(
        "<form method=\"get\" action=\"/queues\" role=\"search\" class=\"filter-bar\">         <div class=\"filter-field\"><label for=\"q\">Name</label>         <input id=\"q\" name=\"q\" type=\"text\" value=\"{}\"></div>         <label class=\"filter-check\"><input type=\"checkbox\" name=\"pending\" value=\"1\"{}> Only with pending messages</label>         <label class=\"filter-check\"><input type=\"checkbox\" name=\"noconsumers\" value=\"1\"{}> Only without consumers</label>",
        esc(&filter.q),
        checked(filter.pending),
        checked(filter.noconsumers)
    );
    for k in ["sort", "order", "refresh"] {
        if let Some(v) = ctx.get(k) {
            let _ = write!(form, "<input type=\"hidden\" name=\"{k}\" value=\"{}\">", esc(v));
        }
    }
    form.push_str("<button type=\"submit\">Filter</button></form>");

    let count_line = if filter.is_active() {
        let clear_href = ctx.with_kept(&[("q", None), ("pending", None), ("noconsumers", None)]);
        format!(
            "<p class=\"filter-status\">Showing {} of {} queues <span class=\"sep\" aria-hidden=\"true\">&middot;</span> <a href=\"{}\">Clear filter</a></p>",
            rows.len(),
            total,
            esc(&clear_href)
        )
    } else {
        String::new()
    };

    let mut c = format!("<h1>Queues</h1>{form}{count_line}<div class=\"table-wrap\"><table><thead><tr>");
    c.push_str(&QUEUES_TABLE.sort_page(&mut rows, &ctx));
    c.push_str("</tr></thead><tbody>");
    for q in &rows {
        let kind = if q.dest.kind.is_temporary() {
            badge("info", "temporary")
        } else {
            String::new()
        };
        let name = esc(&q.dest.name);
        let _ = write!(
            c,
            "<tr><th scope=\"row\"><a class=\"id\" href=\"{}\" title=\"{name}\">{name}</a>{kind}</th><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&ctx.link(&format!("/queues/{}", enc(&q.dest.name)), &[])),
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
        let empty = if filter.is_active() {
            "No queues match the filter"
        } else {
            "No queues"
        };
        let _ = write!(c, "<tr><td colspan=\"8\" class=\"muted\">{empty}</td></tr>");
    }
    c.push_str("</tbody></table></div>");
    layout(&ctx, "Queues", "/queues", &c)
}

pub async fn queue_detail(State(s): State<AdminState>, Path(name): Path<String>, ctx: Ctx) -> Response {
    let Some(d) = find_queue(&s, &name) else {
        return not_found_page(&ctx, &format!("No queue named {name}"));
    };
    let snap = d.snapshot();
    let page: usize = ctx
        .get("page")
        .and_then(|v| v.parse().ok())
        .filter(|v: &usize| *v >= 1)
        .unwrap_or(1);
    let (total, entries) = d.page((page - 1).saturating_mul(PAGE_SIZE), PAGE_SIZE);
    let now = now_ms();
    let qpath = format!("/queues/{}", enc(&name));
    let mut c = format!(
        "<h1>Queue <span class=\"id\">{}</span></h1><div class=\"cards\">",
        esc(&name)
    );
    for (label, value) in [
        ("Pending", snap.pending.to_string()),
        ("Inflight", snap.inflight.to_string()),
        ("Consumers", snap.consumers.len().to_string()),
        ("Producers", snap.producers.len().to_string()),
        ("Enqueued", snap.stats.enqueued.to_string()),
        ("Consumed", snap.stats.dequeued.to_string()),
        ("Expired", snap.stats.expired.to_string()),
        ("Discarded", snap.stats.discarded.to_string()),
        ("Message memory", fmt_bytes(snap.memory)),
        ("Compressed pending", snap.compressed.to_string()),
        ("Pending with expiration", snap.with_expiry.to_string()),
        (
            "Next expiration",
            snap.next_expiry
                .map(|e| fmt_expiration(e, now))
                .unwrap_or_else(|| "-".into()),
        ),
    ] {
        c.push_str(&card(label, &esc(&value)));
    }
    c.push_str("</div><h2>Consumers</h2><div class=\"table-wrap\"><table><thead><tr>");
    let mut consumers = snap.consumers;
    c.push_str(&CONSUMERS_TABLE.sort_page(&mut consumers, &ctx));
    c.push_str("</tr></thead><tbody>");
    for x in &consumers {
        let _ = write!(
            c,
            "<tr><td class=\"id\">{}{}</td><td class=\"id\">{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"id\">{}</td></tr>",
            esc(&x.consumer_id),
            if x.browser { badge("info", "browser") } else { String::new() },
            esc(&x.connection_id),
            esc(&x.remote),
            x.prefetch,
            x.inflight,
            esc(x.selector.as_deref().unwrap_or(""))
        );
    }
    if consumers.is_empty() {
        c.push_str("<tr><td colspan=\"6\" class=\"muted\">No consumers</td></tr>");
    }
    c.push_str("</tbody></table></div><h2>Producers</h2><div class=\"table-wrap\"><table><thead><tr>");
    let mut producers = snap.producers;
    c.push_str(&PRODUCERS_TABLE.sort_page(&mut producers, &ctx));
    c.push_str("</tr></thead><tbody>");
    for (pid, m) in &producers {
        let _ = write!(
            c,
            "<tr><td class=\"id\">{}</td><td class=\"id\">{}</td><td>{}</td></tr>",
            esc(pid),
            esc(&m.connection_id),
            esc(&m.remote)
        );
    }
    if producers.is_empty() {
        c.push_str("<tr><td colspan=\"3\" class=\"muted\">No producers</td></tr>");
    }
    let _ = write!(
        c,
        "</tbody></table></div><h2>Messages ({total} pending)</h2><div class=\"table-wrap\"><table><thead><tr><th scope=\"col\" class=\"num\">#</th><th scope=\"col\">Message ID</th><th scope=\"col\">Timestamp</th><th scope=\"col\">Expiration</th><th scope=\"col\">Type</th><th scope=\"col\">Correlation ID</th><th scope=\"col\">Body</th><th scope=\"col\" class=\"num\">Size</th></tr></thead><tbody>"
    );
    for (i, e) in entries.iter().enumerate() {
        let id = e.msg.message_id_text();
        let mut flags = String::new();
        if super::is_expired(&e.msg, now) {
            flags.push_str(&badge("warn", "expired"));
        }
        if e.msg.compressed {
            flags.push_str(&badge("info", "compressed"));
        }
        let seq = e.seq.to_string();
        let _ = write!(
            c,
            "<tr><td class=\"num\">{}</td><td><a class=\"id\" href=\"{}\">{}</a>{flags}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"id\">{}</td><td>{}</td><td class=\"num\">{}</td></tr>",
            (page - 1) * PAGE_SIZE + i + 1,
            esc(&ctx.link(&format!("{qpath}/messages/{}", enc(&id)), &[("seq", &seq)])),
            esc(&id),
            fmt_time_ms(e.msg.timestamp),
            esc(&fmt_expiration(e.msg.expiration, now)),
            esc(e.msg.jms_type.as_deref().unwrap_or("")),
            esc(e.msg.correlation_id.as_deref().unwrap_or("")),
            e.msg.type_name(),
            stored_size(&e.msg)
        );
    }
    if entries.is_empty() {
        c.push_str("<tr><td colspan=\"8\" class=\"muted\">No messages on this page</td></tr>");
    }
    c.push_str("</tbody></table></div><nav class=\"pager\" aria-label=\"Pages\">");
    if page > 1 {
        let p = (page - 1).to_string();
        let _ = write!(
            c,
            "<a href=\"{}\" rel=\"prev\">&larr; Previous</a>",
            esc(&ctx.with_kept(&[("page", Some(&p))]))
        );
    }
    let _ = write!(
        c,
        "<span class=\"muted\">Page {page} of {}</span>",
        total.div_ceil(PAGE_SIZE).max(1)
    );
    if page.saturating_mul(PAGE_SIZE) < total {
        let p = (page + 1).to_string();
        let _ = write!(
            c,
            "<a href=\"{}\" rel=\"next\">Next &rarr;</a>",
            esc(&ctx.with_kept(&[("page", Some(&p))]))
        );
    }
    c.push_str("</nav>");
    layout(&ctx, &format!("Queue {name}"), "/queues", &c)
}

/// XML state of a text body for the message page and the API.
pub enum XmlView {
    /// Not a text body, or not XML-looking text.
    None,
    Formatted(xml::Formatted),
    Error(XmlError),
}

pub fn xml_view(e: &Entry, raw_text: Option<&str>) -> XmlView {
    match raw_text {
        Some(t) if xml::looks_like_xml(t) => {}
        _ => return XmlView::None,
    }
    match full_text(&e.msg, xml::MAX_INPUT) {
        Some(Some(text)) => match xml::format(&text) {
            Ok(f) => XmlView::Formatted(f),
            Err(XmlError::NotXml) => XmlView::None,
            Err(err) => XmlView::Error(err),
        },
        Some(None) => XmlView::Error(XmlError::TooLarge),
        None => XmlView::None,
    }
}

pub async fn message_detail(
    State(s): State<AdminState>,
    Path((name, id)): Path<(String, String)>,
    ctx: Ctx,
) -> Response {
    let Some(d) = find_queue(&s, &name) else {
        return not_found_page(&ctx, &format!("No queue named {name}"));
    };
    let seq = ctx.get("seq").and_then(|v| v.parse().ok());
    let Some((e, inflight)) = d.find(seq, &id) else {
        return not_found_page(&ctx, "This message is no longer in the queue.");
    };
    let m = &e.msg;
    let now = now_ms();
    let back = ctx.link(&format!("/queues/{}", enc(&name)), &[]);
    let mut c = format!(
        "<p class=\"crumbs\"><a href=\"{}\">&larr; Queue {}</a></p><h1>Message</h1><p class=\"id big\">{}</p>",
        esc(&back),
        esc(&name),
        esc(&id)
    );
    if inflight {
        c.push_str("<p class=\"notice\">This message has been delivered to a consumer and is waiting for its acknowledgement (in flight).</p>");
    }
    let expiration = if super::is_expired(&e.msg, now) {
        format!(
            "{}{}",
            esc(&fmt_expiration(m.expiration, now)),
            badge("warn", "expired")
        )
    } else {
        esc(&fmt_expiration(m.expiration, now))
    };
    let size = if m.compressed {
        format!("{} bytes stored{}", stored_size(m), badge("info", "compressed"))
    } else {
        format!("{} bytes", stored_size(m))
    };
    c.push_str("<h2>Headers</h2><div class=\"table-wrap\"><table class=\"kv\"><tbody>");
    let rows: Vec<(&str, String)> = vec![
        ("MessageID", esc(&m.message_id_text())),
        ("CorrelationID", esc(m.correlation_id.as_deref().unwrap_or(""))),
        ("Type", esc(m.jms_type.as_deref().unwrap_or(""))),
        (
            "ReplyTo",
            esc(&m.reply_to.as_ref().map(|d| d.to_string()).unwrap_or_default()),
        ),
        (
            "DeliveryMode",
            if m.persistent { "PERSISTENT" } else { "NON_PERSISTENT" }.to_string(),
        ),
        ("Priority", m.priority.to_string()),
        ("Timestamp", fmt_time_ms(m.timestamp)),
        ("Expiration", expiration),
        ("RedeliveryCounter", e.redelivery.to_string()),
        ("Body type", m.type_name().to_string()),
        ("Body size", size),
    ];
    for (k, v) in rows {
        let _ = write!(c, "<tr><th scope=\"row\">{k}</th><td class=\"id\">{v}</td></tr>");
    }
    c.push_str("</tbody></table></div><h2>Properties</h2><div class=\"table-wrap\"><table><thead><tr>");
    let mut prop_rows: Vec<(String, Value)> = e.properties().map(|p| p.entries.clone()).unwrap_or_default();
    c.push_str(&PROPERTIES_TABLE.sort_page(&mut prop_rows, &ctx));
    c.push_str("</tr></thead><tbody>");
    if prop_rows.is_empty() {
        c.push_str("<tr><td colspan=\"3\" class=\"muted\">No properties</td></tr>");
    } else {
        for (k, v) in &prop_rows {
            let _ = write!(
                c,
                "<tr><td class=\"id\">{}</td><td>{}</td><td class=\"id\">{}</td></tr>",
                esc(k),
                super::body::java_type(v),
                esc(&v.display())
            );
        }
    }
    c.push_str("</tbody></table></div><h2>Body</h2>");
    let rendered = render(m);
    if rendered.inflate_truncated {
        c.push_str("<p class=\"notice\">Showing the first 64 KB of the decompressed body.</p>");
    }
    let raw_text = match &rendered.view {
        BodyView::Text { text, .. } => Some(text.as_str()),
        _ => None,
    };
    let xml_state = xml_view(&e, raw_text);
    let formatted = ctx.get("view") == Some("xml");
    match &xml_state {
        XmlView::Formatted(_) => {
            let (raw_cur, fmt_cur) = if formatted {
                ("", " aria-current=\"true\"")
            } else {
                (" aria-current=\"true\"", "")
            };
            let _ = write!(
                c,
                "<nav class=\"views\" aria-label=\"Body view\"><a href=\"{}\"{raw_cur}>Raw</a><a href=\"{}\"{fmt_cur}>Formatted</a></nav>",
                esc(&ctx.with("view", None)),
                esc(&ctx.with("view", Some("xml")))
            );
        }
        XmlView::Error(err) => {
            let _ = write!(c, "<p class=\"notice warn\">{}</p>", esc(&err.to_string()));
        }
        XmlView::None => {}
    }
    match (&xml_state, formatted) {
        (XmlView::Formatted(f), true) => {
            let _ = write!(c, "<pre class=\"body xml\">{}</pre>", f.html);
            if f.truncated {
                c.push_str("<p class=\"notice\">Formatted view truncated at 256 KB.</p>");
            }
        }
        _ => body_html(&mut c, rendered.view, &ctx),
    }
    layout(&ctx, &format!("Message {id}"), "/queues", &c)
}

fn body_html(c: &mut String, view: BodyView, ctx: &Ctx) {
    match view {
        BodyView::NoBody => c.push_str("<p class=\"muted\">no body</p>"),
        BodyView::Text { text, truncated } => {
            let _ = write!(c, "<pre class=\"body\">{}</pre>", esc(&text));
            if truncated {
                c.push_str("<p class=\"notice\">Text truncated at 64 KB.</p>");
            }
        }
        BodyView::Bytes { head, total } => {
            let _ = write!(c, "<pre class=\"body\">{}</pre>", esc(&hex_dump(&head)));
            if total > HEX_LIMIT {
                let _ = write!(
                    c,
                    "<p class=\"notice\">Showing the first 4096 bytes; the body is {total} bytes long.</p>"
                );
            }
        }
        BodyView::Map(mut entries) => {
            c.push_str("<div class=\"table-wrap\"><table><thead><tr>");
            c.push_str(&MAP_ENTRIES_TABLE.sort_page(&mut entries, ctx));
            c.push_str("</tr></thead><tbody>");
            for (k, ty, v) in entries {
                let _ = write!(
                    c,
                    "<tr><td class=\"id\">{}</td><td>{ty}</td><td class=\"id\">{}</td></tr>",
                    esc(&k),
                    esc(&v)
                );
            }
            c.push_str("</tbody></table></div>");
        }
        BodyView::Object { size } => {
            let _ = write!(
                c,
                "<p>{size} bytes: serialized Java object (not deserialized by the broker)</p>"
            );
        }
        BodyView::Stream(values) => {
            c.push_str("<div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Type</th><th scope=\"col\">Value</th></tr></thead><tbody>");
            for (ty, v) in values {
                let _ = write!(c, "<tr><td>{ty}</td><td class=\"id\">{}</td></tr>", esc(&v));
            }
            c.push_str("</tbody></table></div>");
        }
        BodyView::Error(err) => {
            let _ = write!(c, "<p class=\"notice warn\">Cannot display the body: {}</p>", esc(&err));
        }
    }
}

pub async fn topics(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut rows: Vec<DestSnapshot> = s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_topic() && visible(d))
        .map(|d| d.snapshot())
        .collect();
    let mut c = String::from("<h1>Topics</h1><div class=\"table-wrap\"><table><thead><tr>");
    c.push_str(&TOPICS_TABLE.sort_page(&mut rows, &ctx));
    c.push_str("</tr></thead><tbody>");
    for q in &rows {
        let kind = if q.dest.kind.is_temporary() {
            badge("info", "temporary")
        } else {
            String::new()
        };
        let _ = write!(
            c,
            "<tr><th scope=\"row\" class=\"id\">{}{kind}</th><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&q.dest.name),
            q.consumers.len(),
            q.producers.len(),
            q.stats.enqueued,
            q.stats.discarded
        );
    }
    if rows.is_empty() {
        c.push_str("<tr><td colspan=\"5\" class=\"muted\">No topics</td></tr>");
    }
    c.push_str("</tbody></table></div>");
    layout(&ctx, "Topics", "/topics", &c)
}

pub async fn connections(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut rows: Vec<ConnRow> = s.broker.connections().iter().map(|c| ConnRow::of(c)).collect();
    let mut c = String::from("<h1>Connections</h1><div class=\"table-wrap\"><table><thead><tr>");
    c.push_str(&CONNECTIONS_TABLE.sort_page(&mut rows, &ctx));
    c.push_str("</tr></thead><tbody>");
    for x in &rows {
        let _ = write!(
            c,
            "<tr><td class=\"id\">{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            esc(&x.info.connection_id),
            esc(&x.info.user),
            esc(&x.remote),
            x.info.version,
            x.connected_at.format("%Y-%m-%d %H:%M:%S"),
            x.info.sessions,
            x.info.consumers,
            x.info.producers
        );
    }
    if rows.is_empty() {
        c.push_str("<tr><td colspan=\"8\" class=\"muted\">No connections</td></tr>");
    }
    c.push_str("</tbody></table></div>");
    layout(&ctx, "Connections", "/connections", &c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_and_encoding() {
        assert_eq!(
            esc("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
        assert_eq!(enc("orders/eu 1"), "orders%2Feu%201");
        assert_eq!(enc("/queues?refresh=5"), "%2Fqueues%3Frefresh%3D5");
    }

    #[test]
    fn links_keep_parameters() {
        let q: HashMap<String, String> = [("sort", "pending"), ("order", "desc"), ("refresh", "5"), ("junk", "x")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let ctx = Ctx {
            user: None,
            path: "/queues".into(),
            q,
        };
        assert_eq!(ctx.with("refresh", None), "/queues?sort=pending&order=desc");
        assert_eq!(ctx.link("/topics", &[]), "/topics?refresh=5");
        assert_eq!(
            ctx.link("/queues", &[("sort", "name"), ("order", "asc")]),
            "/queues?sort=name&order=asc&refresh=5"
        );
    }

    #[test]
    fn table_defaults_and_fallbacks() {
        assert_eq!(QUEUES_TABLE.params(None, None), ("name", false));
        assert_eq!(QUEUES_TABLE.params(Some("pending"), Some("desc")), ("pending", true));
        assert_eq!(QUEUES_TABLE.params(Some("pending"), Some("bogus")), ("pending", false));
        assert_eq!(QUEUES_TABLE.params(Some("bogus"), None), ("name", false));

        assert_eq!(TOPICS_TABLE.params(None, None), ("name", false));
        assert_eq!(TOPICS_TABLE.params(Some("bogus"), None), ("name", false));

        assert_eq!(CONNECTIONS_TABLE.params(None, None), ("connected", false));
        assert_eq!(CONNECTIONS_TABLE.params(Some("bogus"), None), ("connected", false));

        assert_eq!(CONSUMERS_TABLE.params(None, None), ("consumerId", false));
        assert_eq!(CONSUMERS_TABLE.params(Some("bogus"), None), ("consumerId", false));

        assert_eq!(PRODUCERS_TABLE.params(None, None), ("producerId", false));
        assert_eq!(PRODUCERS_TABLE.params(Some("bogus"), None), ("producerId", false));

        assert_eq!(PROPERTIES_TABLE.params(None, None), ("name", false));
        assert_eq!(PROPERTIES_TABLE.params(Some("bogus"), None), ("name", false));

        assert_eq!(MAP_ENTRIES_TABLE.params(None, None), ("key", false));
        assert_eq!(MAP_ENTRIES_TABLE.params(Some("bogus"), None), ("key", false));
    }

    #[test]
    fn with_kept_preserves_parameters() {
        let q: HashMap<String, String> = [
            ("page", "3"),
            ("csort", "prefetch"),
            ("corder", "desc"),
            ("psort", "client"),
            ("porder", "asc"),
            ("refresh", "5"),
            ("other", "junk"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let ctx = Ctx {
            user: None,
            path: "/queues/test".into(),
            q,
        };
        let u = ctx.with_kept(&[("page", Some("4"))]);
        assert_eq!(
            u,
            "/queues/test?page=4&csort=prefetch&corder=desc&psort=client&porder=asc&refresh=5"
        );
        let u = ctx.with_kept(&[("csort", Some("inflight")), ("corder", Some("asc"))]);
        assert_eq!(
            u,
            "/queues/test?page=3&csort=inflight&corder=asc&psort=client&porder=asc&refresh=5"
        );
    }

    fn test_snap(name: &str, pending: usize, consumers: usize) -> DestSnapshot {
        DestSnapshot {
            dest: crate::openwire::model::Destination::queue(name),
            pending,
            inflight: 0,
            consumers: (0..consumers)
                .map(|i| SubSnapshot {
                    consumer_id: format!("c{i}"),
                    connection_id: "conn1".into(),
                    remote: "127.0.0.1:50000".into(),
                    prefetch: 10,
                    inflight: 0,
                    pending: 0,
                    selector: None,
                    browser: false,
                    dispatched: 0,
                })
                .collect(),
            producers: Vec::new(),
            stats: Default::default(),
            with_expiry: 0,
            next_expiry: None,
            memory: 0,
            compressed: 0,
        }
    }

    #[test]
    fn queue_filter_case_insensitive_contains() {
        let filter = QueueFilter::from_params(&HashMap::from([("q".to_string(), "orders".to_string())]));
        assert!(filter.is_active());
        assert!(filter.matches(&test_snap("ORDERS.DLQ", 0, 0)));
        assert!(filter.matches(&test_snap("app.orders.in", 0, 0)));
        assert!(filter.matches(&test_snap("my.Orders", 0, 0)));
        assert!(!filter.matches(&test_snap("billing", 0, 0)));
    }

    #[test]
    fn queue_filter_empty_and_long_q() {
        let p_empty = HashMap::from([("q".to_string(), "".to_string())]);
        let f_empty = QueueFilter::from_params(&p_empty);
        assert!(!f_empty.is_active());
        assert!(f_empty.q.is_empty());
        assert!(f_empty.matches(&test_snap("any.queue", 0, 0)));

        let p_spaces = HashMap::from([("q".to_string(), "   \t  ".to_string())]);
        let f_spaces = QueueFilter::from_params(&p_spaces);
        assert!(!f_spaces.is_active());
        assert!(f_spaces.q.is_empty());

        let p_trimmed = HashMap::from([("q".to_string(), "  orders  ".to_string())]);
        let f_trimmed = QueueFilter::from_params(&p_trimmed);
        assert_eq!(f_trimmed.q, "orders");

        let long_str = "a".repeat(250);
        let p_long = HashMap::from([("q".to_string(), long_str)]);
        let f_long = QueueFilter::from_params(&p_long);
        assert_eq!(f_long.q.len(), 200);
        let matching_queue = "a".repeat(200);
        assert!(f_long.matches(&test_snap(&matching_queue, 0, 0)));
        let shorter_queue = "a".repeat(199);
        assert!(!f_long.matches(&test_snap(&shorter_queue, 0, 0)));
    }

    #[test]
    fn queue_filter_checkboxes() {
        let p_pending = HashMap::from([("pending".to_string(), "1".to_string())]);
        let f_pending = QueueFilter::from_params(&p_pending);
        assert!(f_pending.is_active());
        assert!(f_pending.matches(&test_snap("q1", 5, 0)));
        assert!(f_pending.matches(&test_snap("q2", 1, 1)));
        assert!(!f_pending.matches(&test_snap("q3", 0, 0)));

        let p_bogus = HashMap::from([("pending".to_string(), "true".to_string())]);
        assert!(!QueueFilter::from_params(&p_bogus).is_active());

        let p_nocons = HashMap::from([("noconsumers".to_string(), "1".to_string())]);
        let f_nocons = QueueFilter::from_params(&p_nocons);
        assert!(f_nocons.is_active());
        assert!(f_nocons.matches(&test_snap("q1", 0, 0)));
        assert!(f_nocons.matches(&test_snap("q2", 5, 0)));
        assert!(!f_nocons.matches(&test_snap("q3", 0, 1)));
    }

    #[test]
    fn queue_filter_and_combination() {
        let p = HashMap::from([
            ("q".to_string(), "order".to_string()),
            ("pending".to_string(), "1".to_string()),
            ("noconsumers".to_string(), "1".to_string()),
        ]);
        let f = QueueFilter::from_params(&p);
        assert!(f.is_active());
        assert!(f.matches(&test_snap("new.orders", 3, 0)));
        assert!(!f.matches(&test_snap("billing", 3, 0)));
        assert!(!f.matches(&test_snap("new.orders", 0, 0)));
        assert!(!f.matches(&test_snap("new.orders", 3, 1)));
    }

    #[test]
    fn with_kept_filter_parameters_and_clear() {
        let q: HashMap<String, String> = [
            ("q", "orders"),
            ("pending", "1"),
            ("noconsumers", "1"),
            ("sort", "pending"),
            ("order", "desc"),
            ("refresh", "5"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let ctx = Ctx {
            user: None,
            path: "/queues".into(),
            q,
        };
        let u = ctx.with_kept(&[("sort", Some("consumers")), ("order", Some("asc"))]);
        assert_eq!(
            u,
            "/queues?q=orders&pending=1&noconsumers=1&sort=consumers&order=asc&refresh=5"
        );
        let u_clear = ctx.with_kept(&[("q", None), ("pending", None), ("noconsumers", None)]);
        assert_eq!(u_clear, "/queues?sort=pending&order=desc&refresh=5");
    }
}
