// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! The write operations of the console: closing a connection, banning and unbanning an
//! address. Every one of them is a `POST` from a confirmation page, or the matching API
//! call, and is refused when `admin.read_only` is set.

use axum::extract::{ConnectInfo, Path, State};
use axum::response::Response;
use axum::Form;
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::fmt::Write;
use std::net::{IpAddr, SocketAddr};

use crate::bans::{Ban, Length, Target};

use super::pages::{esc, layout, notice, Ctx};
use super::sort::{Column, Table};
use super::{json_error, json_ok, redirect, AdminState};
use axum::http::StatusCode;

/// Longest reason an administrator can give.
const REASON_MAX: usize = 200;

// ---------------------------------------------------------------------------
// Rows of the bans table
// ---------------------------------------------------------------------------

pub struct BanRow {
    pub address: String,
    pub reason: String,
    pub created: DateTime<Local>,
    pub created_by: String,
    pub expires: Option<DateTime<Local>>,
    pub automatic: bool,
    pub refused: u64,
}

impl BanRow {
    fn of(ban: &Ban) -> BanRow {
        BanRow {
            address: ban.target.to_string(),
            reason: ban.reason.clone(),
            created: ban.created,
            created_by: ban.created_by.clone(),
            expires: ban.expires,
            automatic: ban.automatic,
            refused: ban.refused,
        }
    }
}

pub static BANS_TABLE: Table<BanRow> = Table {
    prefix: "",
    default: "address",
    columns: &[
        Column::address("address", "Address", |b| b.address.as_str().into()),
        Column::text("reason", "Reason", |b| b.reason.as_str().into()),
        Column::text("by", "Created by", |b| b.created_by.as_str().into()),
        Column::time("created", "Created", |b| b.created.timestamp_millis()),
        // A permanent ban sorts after every expiry.
        Column::time("expires", "Expires", |b| {
            b.expires.map_or(i64::MAX, |t| t.timestamp_millis())
        }),
        Column::number("refused", "Refused", |b| b.refused),
    ],
};

fn rows(state: &AdminState) -> Vec<BanRow> {
    state.broker.bans.list().iter().map(BanRow::of).collect()
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

/// The bans page: the active bans and the form that adds one.
pub async fn bans(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut list = rows(&s);
    let writable = !s.broker.cfg.admin_read_only;
    let mut c = String::from("<h1>Bans</h1>");
    c.push_str(&notice(&ctx));
    c.push_str(
        "<p class=\"muted\">Banned addresses are refused by the OpenWire listeners as soon as they connect. \
         The admin console is never affected.</p>",
    );
    if writable {
        c.push_str(
            "<form class=\"filter-bar\" method=\"get\" action=\"/bans/add\">\
             <label for=\"address\">Address or range</label>\
             <input id=\"address\" name=\"address\" maxlength=\"60\" placeholder=\"203.0.113.7 or 10.1.0.0/16\" required>\
             <button type=\"submit\">Add ban</button></form>",
        );
    }
    c.push_str("<div class=\"table-wrap\"><table><thead><tr>");
    c.push_str(&BANS_TABLE.sort_page(&mut list, &ctx));
    if writable {
        c.push_str("<th>Actions</th>");
    }
    c.push_str("</tr></thead><tbody>");
    for b in &list {
        let kind = if b.automatic {
            " <span class=\"tag\">automatic</span>"
        } else {
            ""
        };
        let _ = write!(
            c,
            "<tr><td class=\"id\">{}{kind}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td>",
            esc(&b.address),
            esc(&b.reason),
            esc(&b.created_by),
            b.created.format("%Y-%m-%d %H:%M:%S"),
            match b.expires {
                Some(at) => at.format("%Y-%m-%d %H:%M:%S").to_string(),
                None => "permanent".to_string(),
            },
            b.refused
        );
        if writable {
            let _ = write!(
                c,
                "<td class=\"row-actions\"><a href=\"/bans/remove?address={}\">Unban</a></td>",
                super::pages::enc(&b.address)
            );
        }
        c.push_str("</tr>");
    }
    if list.is_empty() {
        let span = if writable { 7 } else { 6 };
        let _ = write!(c, "<tr><td colspan=\"{span}\" class=\"muted\">No bans</td></tr>");
    }
    c.push_str("</tbody></table></div>");
    layout(&ctx, "Bans", "/bans", &c)
}

/// Confirmation page for closing one connection.
pub async fn close_confirm(State(s): State<AdminState>, ctx: Ctx, Path(id): Path<String>) -> Response {
    let found = s
        .broker
        .connections()
        .into_iter()
        .find(|c| c.info.lock().connection_id == id);
    let Some(conn) = found else {
        return back("/connections", &format!("Connection {id} is no longer open"));
    };
    let info = conn.info.lock().clone();
    let inflight = conn.dispatched.load(std::sync::atomic::Ordering::Relaxed);
    let body = format!(
        "<h1>Close connection</h1>\
         <p>The client is told that an administrator closed the connection. Unacknowledged messages \
         return to their queues as redelivered, open transactions are rolled back and the temporary \
         destinations of the connection are removed.</p>\
         <dl class=\"facts\">\
         <dt>Connection ID</dt><dd class=\"id\">{}</dd>\
         <dt>User</dt><dd>{}</dd>\
         <dt>Client</dt><dd>{}</dd>\
         <dt>Consumers</dt><dd>{}</dd>\
         <dt>Producers</dt><dd>{}</dd>\
         <dt>Messages dispatched</dt><dd>{inflight}</dd>\
         </dl>\
         <form method=\"post\" action=\"/connections/{}/close\" class=\"confirm\">\
         <button type=\"submit\" class=\"danger\">Close connection</button>\
         <a class=\"chip\" href=\"/connections\">Cancel</a></form>",
        esc(&info.connection_id),
        esc(&info.user),
        esc(&conn.remote.to_string()),
        info.consumers,
        info.producers,
        super::pages::enc(&info.connection_id),
    );
    layout(&ctx, "Close connection", "/connections", &body)
}

/// Closes one connection.
pub async fn close(
    State(s): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ctx: Ctx,
    Path(id): Path<String>,
) -> Response {
    let client = s
        .broker
        .connections()
        .into_iter()
        .find(|c| c.info.lock().connection_id == id)
        .map(|c| c.remote.to_string());
    if s.broker.close_connection(&id) {
        tracing::info!(
            "admin {} from {} closed connection {id} ({})",
            ctx.user.clone().unwrap_or_default(),
            addr.ip(),
            client.unwrap_or_default()
        );
        back("/connections", &format!("Connection {id} closed"))
    } else {
        back("/connections", &format!("Connection {id} is no longer open"))
    }
}

/// Confirmation page for a new ban, reached from a connection row or from the form.
pub async fn ban_confirm(
    State(s): State<AdminState>,
    ctx: Ctx,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let asked = ctx.get("address").unwrap_or_default().to_string();
    let target: Target = match asked.parse() {
        Ok(t) => t,
        Err(e) => {
            let body = format!(
                "<h1>Add ban</h1><p class=\"notice error\">{}</p>\
                 <p><a class=\"chip\" href=\"/bans\">Back to bans</a></p>",
                esc(&e)
            );
            let mut r = layout(&ctx, "Add ban", "/bans", &body);
            *r.status_mut() = StatusCode::BAD_REQUEST;
            return r;
        }
    };
    let open = s.broker.count_from(&target);
    let own = target.matches(addr.ip());
    let loopback = target.matches(IpAddr::from([127, 0, 0, 1])) || target.matches("::1".parse().unwrap());
    let mut warning = String::new();
    if own || loopback {
        warning.push_str(
            "<p class=\"notice warn\">This ban covers your own address or the loopback address. \
             It applies to the OpenWire listeners only: the admin console stays reachable.</p>",
        );
    }
    let body = format!(
        "<h1>Ban {}</h1>{warning}\
         <p>{open} connection(s) are open from this address.</p>\
         <form method=\"post\" action=\"/bans\" class=\"confirm\">\
         <input type=\"hidden\" name=\"address\" value=\"{}\">\
         <fieldset><legend>Duration</legend>\
         <label><input type=\"radio\" name=\"duration\" value=\"1h\"> 1 hour</label>\
         <label><input type=\"radio\" name=\"duration\" value=\"24h\" checked> 24 hours</label>\
         <label><input type=\"radio\" name=\"duration\" value=\"7d\"> 7 days</label>\
         <label><input type=\"radio\" name=\"duration\" value=\"permanent\"> Permanent</label>\
         </fieldset>\
         <label for=\"reason\">Reason (optional)</label>\
         <input id=\"reason\" name=\"reason\" maxlength=\"{REASON_MAX}\">\
         <label class=\"check\"><input type=\"checkbox\" name=\"close\" value=\"1\" checked> \
         Also close these connections</label>\
         <button type=\"submit\" class=\"danger\">Ban address</button>\
         <a class=\"chip\" href=\"/bans\">Cancel</a></form>",
        esc(&target.to_string()),
        esc(&target.to_string()),
    );
    layout(&ctx, "Add ban", "/bans", &body)
}

/// Adds a ban and, unless the box was cleared, closes the connections from it.
pub async fn ban(
    State(s): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ctx: Ctx,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let user = ctx.user.clone().unwrap_or_default();
    let asked = form.get("address").cloned().unwrap_or_default();
    let target: Target = match asked.parse() {
        Ok(t) => t,
        Err(e) => return back("/bans", &e),
    };
    let length = form
        .get("duration")
        .and_then(|d| Length::parse(d))
        .unwrap_or(Length::Day);
    let reason = reason_of(form.get("reason").map(String::as_str).unwrap_or_default());
    let also_close = form.contains_key("close");
    let ban = Ban::new(target, reason.clone(), user.clone(), length.expires(Local::now()));
    let until = ban.expires_text();
    let written = s.broker.bans.add(ban);
    let closed = if also_close { s.broker.close_from(&target) } else { 0 };
    tracing::info!(
        "admin {user} from {} banned {target} until {until}: {reason}",
        addr.ip()
    );
    let mut text = format!("{target} banned until {until}");
    if also_close {
        let _ = write!(text, "; {closed} connection(s) closed");
    }
    if let Err(e) = written {
        let _ = write!(text, ". The ban is active but was not saved: {e}");
    }
    back("/bans", &text)
}

/// Confirmation page for removing a ban.
pub async fn unban_confirm(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let asked = ctx.get("address").unwrap_or_default().to_string();
    let Ok(target) = asked.parse::<Target>() else {
        return back("/bans", &format!("{asked} is not banned"));
    };
    let listed = s.broker.bans.list().into_iter().find(|b| b.target == target);
    let Some(ban) = listed else {
        return back("/bans", &format!("{target} is not banned"));
    };
    let body = format!(
        "<h1>Remove ban</h1>\
         <p>Connections from this address are accepted again at once.</p>\
         <dl class=\"facts\">\
         <dt>Address</dt><dd class=\"id\">{}</dd>\
         <dt>Reason</dt><dd>{}</dd>\
         <dt>Expires</dt><dd>{}</dd>\
         <dt>Refused attempts</dt><dd>{}</dd>\
         </dl>\
         <form method=\"post\" action=\"/bans/remove\" class=\"confirm\">\
         <input type=\"hidden\" name=\"address\" value=\"{}\">\
         <button type=\"submit\" class=\"danger\">Remove ban</button>\
         <a class=\"chip\" href=\"/bans\">Cancel</a></form>",
        esc(&ban.target.to_string()),
        esc(&ban.reason),
        ban.expires_text(),
        ban.refused,
        esc(&ban.target.to_string()),
    );
    layout(&ctx, "Remove ban", "/bans", &body)
}

/// Removes a ban.
pub async fn unban(
    State(s): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ctx: Ctx,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let asked = form.get("address").cloned().unwrap_or_default();
    let Ok(target) = asked.parse::<Target>() else {
        return back("/bans", &format!("{asked} is not banned"));
    };
    match s.broker.bans.remove(&target) {
        Ok(true) => {
            tracing::info!(
                "admin {} from {} removed ban {target}",
                ctx.user.clone().unwrap_or_default(),
                addr.ip()
            );
            back("/bans", &format!("Ban {target} removed"))
        }
        Ok(false) => back("/bans", &format!("{target} is not banned")),
        Err(e) => back("/bans", &format!("Ban {target} removed but not saved: {e}")),
    }
}

/// `303` back to a list page with a notice to show.
fn back(path: &str, text: &str) -> Response {
    redirect(&format!("{path}?notice={}", super::pages::enc(text)))
}

fn reason_of(text: &str) -> String {
    text.chars().take(REASON_MAX).collect::<String>().trim().to_string()
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

fn ban_json(b: &BanRow) -> serde_json::Value {
    serde_json::json!({
        "address": b.address,
        "reason": b.reason,
        "created": b.created.to_rfc3339(),
        "createdBy": b.created_by,
        "expires": b.expires.map(|t| t.to_rfc3339()),
        "automatic": b.automatic,
        "refusedAttempts": b.refused,
    })
}

pub async fn api_bans(State(s): State<AdminState>, ctx: Ctx) -> Response {
    let mut list = rows(&s);
    BANS_TABLE.sort(&mut list, ctx.get("sort"), ctx.get("order"));
    let out: Vec<serde_json::Value> = list.iter().map(ban_json).collect();
    json_ok(serde_json::json!(out))
}

pub async fn api_close(
    State(s): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ctx: Ctx,
    Path(id): Path<String>,
) -> Response {
    if s.broker.close_connection(&id) {
        tracing::info!(
            "admin {} from {} closed connection {id} (API)",
            ctx.user.clone().unwrap_or_default(),
            addr.ip()
        );
        json_ok(serde_json::json!({ "closed": id }))
    } else {
        json_error(StatusCode::NOT_FOUND, "connection not open")
    }
}

pub async fn api_add_ban(
    State(s): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ctx: Ctx,
    body: String,
) -> Response {
    let Ok(input) = serde_json::from_str::<serde_json::Value>(&body) else {
        return json_error(StatusCode::BAD_REQUEST, "the body is not JSON");
    };
    let asked = input.get("address").and_then(|v| v.as_str()).unwrap_or_default();
    let target: Target = match asked.parse() {
        Ok(t) => t,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e),
    };
    let asked_length = input.get("duration").and_then(|v| v.as_str()).unwrap_or("24h");
    let Some(length) = Length::parse(asked_length) else {
        return json_error(StatusCode::BAD_REQUEST, "duration must be 1h, 24h, 7d or permanent");
    };
    let reason = reason_of(input.get("reason").and_then(|v| v.as_str()).unwrap_or_default());
    let also_close = input.get("closeConnections").and_then(|v| v.as_bool()).unwrap_or(true);
    let user = ctx.user.clone().unwrap_or_default();
    let ban = Ban::new(target, reason.clone(), user.clone(), length.expires(Local::now()));
    let view = ban_json(&BanRow::of(&ban));
    let until = ban.expires_text();
    let written = s.broker.bans.add(ban);
    let closed = if also_close { s.broker.close_from(&target) } else { 0 };
    tracing::info!(
        "admin {user} from {} banned {target} until {until}: {reason}",
        addr.ip()
    );
    let mut out = serde_json::json!({ "ban": view, "closedConnections": closed });
    if let Err(e) = written {
        out["warning"] = serde_json::Value::String(e);
    }
    json_ok(out)
}

pub async fn api_remove_ban(
    State(s): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ctx: Ctx,
    Path(address): Path<String>,
) -> Response {
    let target: Target = match address.parse() {
        Ok(t) => t,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, &e),
    };
    match s.broker.bans.remove(&target) {
        Ok(true) => {
            tracing::info!(
                "admin {} from {} removed ban {target}",
                ctx.user.clone().unwrap_or_default(),
                addr.ip()
            );
            json_ok(serde_json::json!({ "removed": target.to_string() }))
        }
        Ok(false) => json_error(StatusCode::NOT_FOUND, "not banned"),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}
