// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! JSON API of the admin console (same data as the HTML pages).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value as J};
use std::collections::HashMap;

use super::body::{render, stored_size, BodyView, HEX_LIMIT};
use super::pages::{find_queue, sort_params, sort_queues, visible, xml_view, XmlView, PAGE_SIZE};
use super::{fmt_expiration, process_memory, AdminState};
use crate::broker::destination::DestSnapshot;
use crate::broker::entry::Entry;
use crate::broker::now_ms;
use crate::openwire::props::Value;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

type Params = Query<HashMap<String, String>>;

fn not_found(what: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": format!("{what} not found") })),
    )
        .into_response()
}

fn queue_json(s: &DestSnapshot) -> J {
    json!({
        "name": s.dest.name.as_ref(),
        "temporary": s.dest.kind.is_temporary(),
        "pending": s.pending,
        "inflight": s.inflight,
        "consumers": s.consumers.len(),
        "producers": s.producers.len(),
        "enqueued": s.stats.enqueued,
        "consumed": s.stats.dequeued,
        "expired": s.stats.expired,
        "discarded": s.stats.discarded,
        "memory": s.memory,
        "compressed": s.compressed,
    })
}

fn prop_json(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Bool(b) => json!(b),
        Value::Byte(x) => json!(x),
        Value::Char(c) => json!(char::from_u32(*c as u32).map(|c| c.to_string())),
        Value::Short(x) => json!(x),
        Value::Int(x) => json!(x),
        Value::Long(x) => json!(x),
        Value::Float(x) => json!(x),
        Value::Double(x) => json!(x),
        Value::String(s) => json!(s),
        other => json!(other.display()),
    }
}

/// Headers, properties and body metadata of one message. `bodySize` is the stored size
/// (compressed size for a compressed body), also given as `compressedSize` when compressed.
fn message_json(e: &Entry, now: i64) -> J {
    let m = &e.msg;
    let props: serde_json::Map<String, J> = e
        .properties()
        .map(|p| p.entries.iter().map(|(k, v)| (k.clone(), prop_json(v))).collect())
        .unwrap_or_default();
    let mut v = json!({
        "position": e.seq,
        "messageId": m.message_id_text(),
        "correlationId": m.correlation_id,
        "type": m.jms_type,
        "replyTo": m.reply_to.as_ref().map(|d| d.to_string()),
        "deliveryMode": if m.persistent { "PERSISTENT" } else { "NON_PERSISTENT" },
        "priority": m.priority,
        "timestamp": m.timestamp,
        "expiration": m.expiration,
        "expirationText": fmt_expiration(m.expiration, now),
        "expired": super::is_expired(&e.msg, now),
        "expiresInMs": if m.expiration > now { json!(m.expiration - now) } else { J::Null },
        "redeliveryCounter": e.redelivery,
        "properties": props,
        "bodyType": m.type_name(),
        "bodySize": stored_size(m),
        "compressed": m.compressed,
    });
    if m.compressed {
        v["compressedSize"] = json!(stored_size(m));
    }
    v
}

pub async fn overview(State(s): State<AdminState>) -> Json<J> {
    let b = &s.broker;
    let dests = b.destinations();
    let (ws, private) = process_memory();
    Json(json!({
        "product": PROVIDER_NAME,
        "version": PROVIDER_VERSION,
        "uptimeSeconds": (chrono::Local::now() - b.started).num_seconds(),
        "openwire": format!("tcp://{}:{}", b.cfg.bind, b.cfg.port),
        "admin": format!("http://{}:{}", b.cfg.admin_bind, b.cfg.admin_port),
        "connections": b.connections().len(),
        "queues": dests.iter().filter(|d| d.dest.kind.is_queue()).count(),
        "topics": dests.iter().filter(|d| d.dest.kind.is_topic() && visible(d)).count(),
        "messageMemory": b.memory.used(),
        "memoryLimit": if b.memory.limit == 0 { J::Null } else { json!(b.memory.limit) },
        "memoryLimitReached": b.memory_limited(),
        "compressed": b.stats.compressed.load(std::sync::atomic::Ordering::Relaxed),
        "compressDiscarded": b.stats.compress_discarded.load(std::sync::atomic::Ordering::Relaxed),
        "workingSet": ws,
        "privateBytes": private,
    }))
}

pub async fn queues(State(s): State<AdminState>, Query(p): Params) -> Json<J> {
    let mut rows: Vec<DestSnapshot> = s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_queue())
        .map(|d| d.snapshot())
        .collect();
    let (col, desc) = sort_params(p.get("sort").map(String::as_str), p.get("order").map(String::as_str));
    sort_queues(&mut rows, col, desc);
    Json(json!(rows.iter().map(queue_json).collect::<Vec<_>>()))
}

pub async fn queue_detail(State(s): State<AdminState>, Path(name): Path<String>) -> Response {
    let Some(d) = find_queue(&s, &name) else {
        return not_found("queue");
    };
    let snap = d.snapshot();
    let mut v = queue_json(&snap);
    v["consumers"] = json!(snap
        .consumers
        .iter()
        .map(|c| json!({
            "consumerId": c.consumer_id,
            "connectionId": c.connection_id,
            "client": c.remote,
            "prefetch": c.prefetch,
            "inflight": c.inflight,
            "selector": c.selector,
            "browser": c.browser,
        }))
        .collect::<Vec<_>>());
    v["producers"] = json!(snap
        .producers
        .iter()
        .map(|(pid, m)| json!({ "producerId": pid, "connectionId": m.connection_id, "client": m.remote }))
        .collect::<Vec<_>>());
    v["withExpiration"] = json!(snap.with_expiry);
    v["nextExpiration"] = json!(snap.next_expiry);
    v["nextExpirationText"] = json!(snap.next_expiry.map(|e| fmt_expiration(e, now_ms())));
    Json(v).into_response()
}

/// A page of messages: `offset` (default 0) and `limit` (default 50, clamped to 1..50).
pub async fn messages(State(s): State<AdminState>, Path(name): Path<String>, Query(p): Params) -> Response {
    let Some(d) = find_queue(&s, &name) else {
        return not_found("queue");
    };
    let offset: usize = p.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: usize = p
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(PAGE_SIZE)
        .clamp(1, PAGE_SIZE);
    let (total, entries) = d.page(offset, limit);
    let now = now_ms();
    Json(json!({
        "total": total,
        "offset": offset,
        "limit": limit,
        "messages": entries.iter().map(|e| message_json(e, now)).collect::<Vec<_>>(),
    }))
    .into_response()
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// One message with its rendered body; `view=xml` adds `formattedBody` / `formatError`.
pub async fn message(
    State(s): State<AdminState>,
    Path((name, id)): Path<(String, String)>,
    Query(p): Params,
) -> Response {
    let Some(d) = find_queue(&s, &name) else {
        return not_found("queue");
    };
    let seq = p.get("seq").and_then(|v| v.parse().ok());
    let Some((e, inflight)) = d.find(seq, &id) else {
        return not_found("message");
    };
    let mut v = message_json(&e, now_ms());
    v["inflight"] = json!(inflight);
    let rendered = render(&e.msg);
    v["bodyInflateTruncated"] = json!(rendered.inflate_truncated);
    let raw_text = match &rendered.view {
        BodyView::Text { text, .. } => Some(text.clone()),
        _ => None,
    };
    v["body"] = match rendered.view {
        BodyView::NoBody => json!({ "kind": "none" }),
        BodyView::Text { text, truncated } => {
            json!({ "kind": "text", "text": text, "truncated": truncated })
        }
        BodyView::Bytes { head, total } => {
            json!({ "kind": "bytes", "hex": hex(&head), "size": total, "truncated": total > HEX_LIMIT })
        }
        BodyView::Map(rows) => json!({
            "kind": "map",
            "entries": rows.iter().map(|(k, t, v)| json!({ "name": k, "type": t, "value": v })).collect::<Vec<_>>(),
        }),
        BodyView::Object { size } => json!({ "kind": "object", "size": size }),
        BodyView::Stream(values) => json!({
            "kind": "stream",
            "values": values.iter().map(|(t, v)| json!({ "type": t, "value": v })).collect::<Vec<_>>(),
        }),
        BodyView::Error(err) => json!({ "kind": "error", "error": err }),
    };
    if p.get("view").map(String::as_str) == Some("xml") {
        match xml_view(&e, raw_text.as_deref()) {
            XmlView::Formatted(f) => {
                v["formattedBody"] = json!(f.plain);
                v["formattedTruncated"] = json!(f.truncated);
            }
            XmlView::Error(err) => {
                v["formattedBody"] = J::Null;
                v["formatError"] = json!(err.to_string());
            }
            XmlView::None => {
                v["formattedBody"] = J::Null;
                v["formatError"] = json!("not XML");
            }
        }
    }
    Json(v).into_response()
}

pub async fn topics(State(s): State<AdminState>) -> Json<J> {
    let list: Vec<J> = s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_topic() && visible(d))
        .map(|d| {
            let snap = d.snapshot();
            json!({
                "name": d.dest.name.as_ref(),
                "temporary": d.dest.kind.is_temporary(),
                "consumers": snap.consumers.len(),
                "producers": snap.producers.len(),
                "published": snap.stats.enqueued,
                "discarded": snap.stats.discarded,
                "pending": snap.pending,
                "memory": snap.memory,
            })
        })
        .collect();
    Json(json!(list))
}

pub async fn connections(State(s): State<AdminState>) -> Json<J> {
    let list: Vec<J> = s
        .broker
        .connections()
        .iter()
        .map(|c| {
            let i = c.info.lock().clone();
            json!({
                "connectionId": i.connection_id,
                "clientId": i.client_id,
                "user": i.user,
                "client": c.remote.to_string(),
                "openwireVersion": i.version,
                "connectedAt": c.connected_at.to_rfc3339(),
                "sessions": i.sessions,
                "consumers": i.consumers,
                "producers": i.producers,
            })
        })
        .collect();
    Json(json!(list))
}
