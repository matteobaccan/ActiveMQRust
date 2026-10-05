// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! JSON API of the admin console (same data as the HTML pages).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value as J};
use std::collections::HashMap;

use super::pages::{find_queue, visible};
use super::{process_memory, AdminState};
use crate::broker::destination::DestSnapshot;
use crate::broker::entry::Entry;
use crate::openwire::props::Value;
use crate::openwire::wireformat::{PROVIDER_NAME, PROVIDER_VERSION};

fn not_found(what: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": format!("{what} not found") }))).into_response()
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

fn message_json(e: &Entry) -> J {
    let m = &e.msg;
    let props: serde_json::Map<String, J> = e
        .properties()
        .map(|p| p.entries.iter().map(|(k, v)| (k.clone(), prop_json(v))).collect())
        .unwrap_or_default();
    json!({
        "position": e.seq,
        "messageId": m.message_id_text(),
        "correlationId": m.correlation_id,
        "type": m.jms_type,
        "replyTo": m.reply_to.as_ref().map(|d| d.to_string()),
        "deliveryMode": if m.persistent { "PERSISTENT" } else { "NON_PERSISTENT" },
        "priority": m.priority,
        "timestamp": m.timestamp,
        "expiration": m.expiration,
        "redeliveryCounter": e.redelivery,
        "properties": props,
        "bodyType": m.type_name(),
        "bodySize": m.content_len(),
        "compressed": m.compressed,
    })
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
        "workingSet": ws,
        "privateBytes": private,
    }))
}

pub async fn queues(State(s): State<AdminState>) -> Json<J> {
    let list: Vec<J> = s
        .broker
        .destinations()
        .iter()
        .filter(|d| d.dest.kind.is_queue())
        .map(|d| queue_json(&d.snapshot()))
        .collect();
    Json(json!(list))
}

pub async fn queue_detail(State(s): State<AdminState>, Path(name): Path<String>) -> Response {
    let Some(d) = find_queue(&s, &name) else { return not_found("queue") };
    let snap = d.snapshot();
    let mut v = queue_json(&snap);
    v["consumers"] = json!(snap
        .consumers
        .iter()
        .map(|c| json!({
            "consumerId": c.consumer_id,
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
    Json(v).into_response()
}

pub async fn messages(
    State(s): State<AdminState>,
    Path(name): Path<String>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    let Some(d) = find_queue(&s, &name) else { return not_found("queue") };
    let offset: usize = p.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: usize = p.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50).clamp(1, 50);
    let (total, entries) = d.page(offset, limit);
    Json(json!({
        "total": total,
        "offset": offset,
        "messages": entries.iter().map(message_json).collect::<Vec<_>>(),
    }))
    .into_response()
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
