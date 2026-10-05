// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Admin console over real HTTP: authentication, read-only methods, escaping and JSON API.

use bytes::Bytes;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::model::*;
use mqrust::openwire::types as t;

async fn start(port: u16) -> Arc<Broker> {
    let mut fc = FileConfig::default();
    fc.admin.port = port as i64;
    let broker = Broker::new(Arc::new(build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap()));
    let (_tx, rx) = tokio::sync::watch::channel(false);
    std::mem::forget(_tx);
    mqrust::admin::start(broker.clone(), rx).await;
    broker
}

/// Sends one HTTP/1.1 request and returns (status, headers, body).
async fn request(port: u16, method: &str, path: &str, auth: Option<&str>) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(a) = auth {
        use base64::Engine;
        req.push_str(&format!("Authorization: Basic {}\r\n", base64::engine::general_purpose::STANDARD.encode(a)));
    }
    req.push_str("Content-Length: 0\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_string(), body.to_string())
}

static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn put(b: &Broker, queue: &str, body: &str) {
    let pid = ProducerId { connection_id: Arc::from("ID:test-1-1-1:1"), session_id: 1, value: 1 };
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.destination = Some(Destination::queue(queue));
    m.producer_id = Some(pid.clone());
    m.message_id = Some(MessageId { text_view: None, producer_id: Some(pid), producer_sequence_id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), broker_sequence_id: 0 });
    let mut content = (body.len() as i32).to_be_bytes().to_vec();
    content.extend_from_slice(body.as_bytes());
    m.content = Some(Bytes::from(content));
    b.deliver(m, true, now_ms()).unwrap();
}

#[tokio::test]
async fn authentication_methods_and_headers() {
    let port = 18161;
    let _b = start(port).await;
    let (st, head, _) = request(port, "GET", "/", None).await;
    assert_eq!(st, 401);
    assert!(head.to_ascii_lowercase().contains("www-authenticate: basic realm=\"activemqrust\""));
    assert_eq!(request(port, "GET", "/api/queues", Some("admin:wrong")).await.0, 401);
    let (st, head, body) = request(port, "GET", "/", Some("admin:admin")).await;
    assert_eq!(st, 200);
    assert!(body.contains("ActiveMQRust"));
    assert!(head.contains("default-src 'none'; style-src 'self'"));
    assert!(head.to_ascii_lowercase().contains("x-content-type-options: nosniff"));
    assert_eq!(request(port, "POST", "/queues", Some("admin:admin")).await.0, 405);
    assert_eq!(request(port, "DELETE", "/api/queues", Some("admin:admin")).await.0, 405);
    assert_eq!(request(port, "GET", "/style.css", Some("admin:admin")).await.0, 200);
}

#[tokio::test]
async fn pages_escape_broker_data_and_api_reports_contents() {
    let port = 18162;
    let b = start(port).await;
    put(&b, "Q<script>", "<b>bold</b>");
    put(&b, "ORDERS", "first");
    put(&b, "ORDERS", "second");
    let (st, _, body) = request(port, "GET", "/queues", Some("admin:admin")).await;
    assert_eq!(st, 200);
    assert!(body.contains("Q&lt;script&gt;"));
    assert!(!body.contains("Q<script>"));
    let (st, _, body) = request(port, "GET", "/api/queues/ORDERS", Some("admin:admin")).await;
    assert_eq!(st, 200);
    assert!(body.contains("\"pending\":2"), "{body}");
    let (st, _, body) = request(port, "GET", "/api/queues/ORDERS/messages?limit=500", Some("admin:admin")).await;
    assert_eq!(st, 200);
    assert!(body.contains("\"total\":2"));
    assert_eq!(request(port, "GET", "/api/queues/NOPE", Some("admin:admin")).await.0, 404);
    // Browsing the contents does not consume anything.
    let (_, _, page) = request(port, "GET", "/queues/ORDERS", Some("admin:admin")).await;
    assert!(page.contains("Messages (2 pending)"));
    assert_eq!(b.get_dest(&Destination::queue("ORDERS")).unwrap().snapshot().pending, 2);
    let (_, _, body) = request(port, "GET", "/api/overview", Some("admin:admin")).await;
    assert!(body.contains("\"product\":\"ActiveMQRust\""));
}
