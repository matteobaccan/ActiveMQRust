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
use super::xml::{self, XmlError};
use super::{fmt_bytes, fmt_duration, fmt_expiration, fmt_time_ms, process_memory, AdminState, CurrentUser, CSS};
use crate::broker::destination::{Dest, DestSnapshot};
use crate::broker::entry::Entry;
use crate::broker::now_ms;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

/// Project repository, from the crate metadata.
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// Messages per contents page.
pub const PAGE_SIZE: usize = 50;
/// Query parameters a page keeps on its own links, in this order.
const KEPT: [&str; 6] = ["page", "sort", "order", "view", "seq", "refresh"];

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

    fn get(&self, k: &str) -> Option<&str> {
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
        let mut params: Vec<(&str, &str)> = Vec::new();
        for k in KEPT {
            if k == key {
                if let Some(v) = value {
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

/// Sortable columns of the queues table: (key, label).
pub const QUEUE_COLUMNS: [(&str, &str); 8] = [
    ("name", "Name"),
    ("pending", "Pending"),
    ("inflight", "Inflight"),
    ("consumers", "Consumers"),
    ("producers", "Producers"),
    ("enqueued", "Enqueued"),
    ("consumed", "Consumed"),
    ("expired", "Expired"),
];

/// Normalized sort column and direction: unknown columns fall back to Name.
pub fn sort_params(sort: Option<&str>, order: Option<&str>) -> (&'static str, bool) {
    let col = QUEUE_COLUMNS
        .iter()
        .map(|c| c.0)
        .find(|c| Some(*c) == sort)
        .unwrap_or("name");
    (col, order == Some("desc"))
}

/// Server-side sort: numeric columns numerically, Name case-insensitively, ties by name.
pub fn sort_queues(rows: &mut [DestSnapshot], col: &str, desc: bool) {
    let key = |s: &DestSnapshot| -> u64 {
        match col {
            "pending" => s.pending as u64,
            "inflight" => s.inflight as u64,
            "consumers" => s.consumers.len() as u64,
            "producers" => s.producers.len() as u64,
            "enqueued" => s.stats.enqueued,
            "consumed" => s.stats.dequeued,
            "expired" => s.stats.expired,
            _ => 0,
        }
    };
    let by_name = |a: &DestSnapshot, b: &DestSnapshot| {
        a.dest
            .name
            .to_lowercase()
            .cmp(&b.dest.name.to_lowercase())
            .then_with(|| a.dest.name.cmp(&b.dest.name))
    };
    rows.sort_by(|a, b| {
        let primary = if col == "name" {
            by_name(a, b)
        } else {
            key(a).cmp(&key(b))
        };
        let primary = if desc { primary.reverse() } else { primary };
        if col == "name" {
            primary
        } else {
            primary.then_with(|| by_name(a, b))
        }
    });
}

pub async fn queues(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut rows: Vec<DestSnapshot> = s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_queue())
        .map(|d| d.snapshot())
        .collect();
    let (col, desc) = sort_params(ctx.get("sort"), ctx.get("order"));
    sort_queues(&mut rows, col, desc);
    let mut c = String::from("<h1>Queues</h1><div class=\"table-wrap\"><table><thead><tr>");
    for (key, label) in QUEUE_COLUMNS {
        let num = if key == "name" { "" } else { " class=\"num\"" };
        let (next, aria, arrow) = if key == col {
            if desc {
                (
                    "asc",
                    " aria-sort=\"descending\"",
                    " <span aria-hidden=\"true\">&#9660;</span>",
                )
            } else {
                (
                    "desc",
                    " aria-sort=\"ascending\"",
                    " <span aria-hidden=\"true\">&#9650;</span>",
                )
            }
        } else {
            ("asc", "", "")
        };
        let href = ctx.link("/queues", &[("sort", key), ("order", next)]);
        let _ = write!(
            c,
            "<th scope=\"col\"{num}{aria}><a href=\"{}\">{label}{arrow}</a></th>",
            esc(&href)
        );
    }
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
        c.push_str("<tr><td colspan=\"8\" class=\"muted\">No queues</td></tr>");
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
    c.push_str("</div><h2>Consumers</h2><div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Consumer ID</th><th scope=\"col\">Connection ID</th><th scope=\"col\">Client</th><th scope=\"col\" class=\"num\">Prefetch</th><th scope=\"col\" class=\"num\">Inflight</th><th scope=\"col\">Selector</th></tr></thead><tbody>");
    for x in &snap.consumers {
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
    if snap.consumers.is_empty() {
        c.push_str("<tr><td colspan=\"6\" class=\"muted\">No consumers</td></tr>");
    }
    c.push_str("</tbody></table></div><h2>Producers</h2><div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Producer ID</th><th scope=\"col\">Connection ID</th><th scope=\"col\">Client</th></tr></thead><tbody>");
    for (pid, m) in &snap.producers {
        let _ = write!(
            c,
            "<tr><td class=\"id\">{}</td><td class=\"id\">{}</td><td>{}</td></tr>",
            esc(pid),
            esc(&m.connection_id),
            esc(&m.remote)
        );
    }
    if snap.producers.is_empty() {
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
            esc(&ctx.link(&qpath, &[("page", &p)]))
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
            esc(&ctx.link(&qpath, &[("page", &p)]))
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
    c.push_str("</tbody></table></div><h2>Properties</h2><div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Name</th><th scope=\"col\">Type</th><th scope=\"col\">Value</th></tr></thead><tbody>");
    match e.properties() {
        Some(props) if !props.entries.is_empty() => {
            for (k, v) in &props.entries {
                let _ = write!(
                    c,
                    "<tr><td class=\"id\">{}</td><td>{}</td><td class=\"id\">{}</td></tr>",
                    esc(k),
                    super::body::java_type(v),
                    esc(&v.display())
                );
            }
        }
        _ => c.push_str("<tr><td colspan=\"3\" class=\"muted\">No properties</td></tr>"),
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
        _ => body_html(&mut c, rendered.view),
    }
    layout(&ctx, &format!("Message {id}"), "/queues", &c)
}

fn body_html(c: &mut String, view: BodyView) {
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
        BodyView::Map(entries) => {
            c.push_str("<div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Key</th><th scope=\"col\">Type</th><th scope=\"col\">Value</th></tr></thead><tbody>");
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
    let mut c = String::from("<h1>Topics</h1><div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Name</th><th scope=\"col\" class=\"num\">Consumers</th><th scope=\"col\" class=\"num\">Producers</th><th scope=\"col\" class=\"num\">Published</th><th scope=\"col\" class=\"num\">Discarded</th></tr></thead><tbody>");
    let mut any = false;
    for d in s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_topic() && visible(d))
    {
        any = true;
        let snap = d.snapshot();
        let kind = if d.dest.kind.is_temporary() {
            badge("info", "temporary")
        } else {
            String::new()
        };
        let _ = write!(
            c,
            "<tr><th scope=\"row\" class=\"id\">{}{kind}</th><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
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
    c.push_str("</tbody></table></div>");
    layout(&ctx, "Topics", "/topics", &c)
}

pub async fn connections(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut c = String::from("<h1>Connections</h1><div class=\"table-wrap\"><table><thead><tr><th scope=\"col\">Connection ID</th><th scope=\"col\">User</th><th scope=\"col\">Client</th><th scope=\"col\" class=\"num\">OpenWire</th><th scope=\"col\">Connected</th><th scope=\"col\" class=\"num\">Sessions</th><th scope=\"col\" class=\"num\">Consumers</th><th scope=\"col\" class=\"num\">Producers</th></tr></thead><tbody>");
    let conns = s.broker.connections();
    for x in &conns {
        let i = x.info.lock().clone();
        let _ = write!(
            c,
            "<tr><td class=\"id\">{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
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
    fn sort_parameters() {
        assert_eq!(sort_params(Some("pending"), Some("desc")), ("pending", true));
        assert_eq!(sort_params(Some("bogus"), None), ("name", false));
        assert_eq!(sort_params(None, Some("asc")), ("name", false));
    }
}
