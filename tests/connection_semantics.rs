// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Command handling over a real socket with a minimal OpenWire client: responses, destination
//! rules, local transactions, XA refusal, temporary destinations and connection cleanup.

use bytes::Bytes;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::marshal::{Decoder, Encoder};
use mqrust::openwire::model::*;
use mqrust::openwire::props::{PrimitiveMap, Value};
use mqrust::openwire::types as t;
use mqrust::openwire::wireformat::MAGIC;

const VERSION: i32 = 12;
const INVALID_DESTINATION: &str = "javax.jms.InvalidDestinationException";
const JMS_EXCEPTION: &str = "javax.jms.JMSException";
const XA_TEXT: &str = "XA transactions not supported";

fn broker_with(f: impl FnOnce(&mut FileConfig)) -> Arc<Broker> {
    let mut fc = FileConfig::default();
    f(&mut fc);
    Broker::new(Arc::new(build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap()))
}

/// A minimal OpenWire client speaking version 12 loose encoding.
struct Wire {
    r: OwnedReadHalf,
    w: OwnedWriteHalf,
    enc: Encoder,
    dec: Decoder,
    next: i32,
    conn: Arc<str>,
    backlog: VecDeque<Command>,
    seq: i64,
}

impl Wire {
    /// Connects a new client to `b` and logs in with the default credentials.
    async fn open(b: &Arc<Broker>, conn: &str) -> Wire {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let broker = b.clone();
        tokio::spawn(async move {
            let (s, remote) = listener.accept().await.unwrap();
            let (tx, rx) = tokio::sync::watch::channel(false);
            mqrust::connection::serve(s, remote, broker, rx).await;
            drop(tx);
        });
        let (r, w) = TcpStream::connect(addr).await.unwrap().into_split();
        let mut wire = Wire {
            r,
            w,
            enc: Encoder::new(VERSION),
            dec: Decoder::new(VERSION),
            next: 1,
            conn: Arc::from(conn),
            backlog: VecDeque::new(),
            seq: 0,
        };
        let mut p = PrimitiveMap::new();
        p.set("MaxInactivityDuration", Value::Long(0));
        p.set("TightEncodingEnabled", Value::Bool(false));
        p.set("CacheEnabled", Value::Bool(false));
        wire.write(&Command::WireFormatInfo(WireFormatInfo { magic: MAGIC, version: VERSION, properties: p })).await;
        assert!(matches!(wire.read().await, Command::WireFormatInfo(_)));
        assert!(matches!(wire.read().await, Command::BrokerInfo(_)));
        let conn_id = ConnectionId { value: wire.conn.clone() };
        let r = wire
            .request(|header| {
                Command::ConnectionInfo(ConnectionInfo {
                    header,
                    connection_id: Some(conn_id),
                    client_id: None,
                    password: Some("admin".into()),
                    user_name: Some("admin".into()),
                    broker_path: None,
                    broker_master_connector: false,
                    manageable: false,
                    client_master: true,
                    fault_tolerant: false,
                    failover_reconnect: false,
                    client_ip: None,
                })
            })
            .await;
        assert_ok(&r);
        let sid = SessionId { connection_id: wire.conn.clone(), value: 1 };
        let r = wire.request(|header| Command::SessionInfo(SessionInfo { header, session_id: Some(sid) })).await;
        assert_ok(&r);
        wire
    }

    async fn write(&mut self, cmd: &Command) {
        self.w.write_all(&self.enc.frame(cmd)).await.unwrap();
    }

    async fn read(&mut self) -> Command {
        tokio::time::timeout(Duration::from_secs(5), self.read_frame()).await.expect("no frame within 5 s")
    }

    async fn read_frame(&mut self) -> Command {
        loop {
            let mut len = [0u8; 4];
            self.r.read_exact(&mut len).await.unwrap();
            let mut body = vec![0u8; i32::from_be_bytes(len) as usize];
            self.r.read_exact(&mut body).await.unwrap();
            if let Some(c) = self.dec.decode_frame(Bytes::from(body)).unwrap() {
                return c;
            }
        }
    }

    /// Sends a command with `responseRequired` and returns its `Response` or `ExceptionResponse`.
    async fn request(&mut self, make: impl FnOnce(Header) -> Command) -> Command {
        let id = self.next;
        self.next += 1;
        self.write(&make(Header { command_id: id, response_required: true })).await;
        loop {
            let c = self.read().await;
            match &c {
                Command::Response { correlation_id, .. } | Command::ExceptionResponse { correlation_id, .. }
                    if *correlation_id == id =>
                {
                    return c
                }
                _ => self.backlog.push_back(c),
            }
        }
    }

    /// Sends a command without `responseRequired`.
    async fn post(&mut self, make: impl FnOnce(Header) -> Command) {
        let id = self.next;
        self.next += 1;
        self.write(&make(Header { command_id: id, response_required: false })).await;
    }

    /// Next `MessageDispatch`, or `None` after `ms` milliseconds without one.
    async fn dispatch(&mut self, ms: u64) -> Option<MessageDispatch> {
        loop {
            if let Some(pos) = self.backlog.iter().position(|c| matches!(c, Command::MessageDispatch(_))) {
                if let Some(Command::MessageDispatch(md)) = self.backlog.remove(pos) {
                    return Some(md);
                }
            }
            match tokio::time::timeout(Duration::from_millis(ms), self.read_frame()).await {
                Ok(c) => self.backlog.push_back(c),
                Err(_) => return None,
            }
        }
    }

    fn consumer_id(&self, n: i64) -> ConsumerId {
        ConsumerId { connection_id: self.conn.clone(), session_id: 1, value: n }
    }

    fn producer_id(&self) -> ProducerId {
        ProducerId { connection_id: self.conn.clone(), session_id: 1, value: 1 }
    }

    fn message(&mut self, dest: &Destination, body: &str) -> Message {
        self.seq += 1;
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.destination = Some(dest.clone());
        m.producer_id = Some(self.producer_id());
        m.message_id = Some(MessageId {
            text_view: None,
            producer_id: Some(self.producer_id()),
            producer_sequence_id: self.seq,
            broker_sequence_id: 0,
        });
        m.content = Some(Bytes::from(body.to_string()));
        m.timestamp = now_ms();
        m
    }

    async fn send(&mut self, m: Message) -> Command {
        self.request(|header| {
            let mut m = m;
            m.header = header;
            Command::Message(Box::new(m))
        })
        .await
    }

    async fn send_async(&mut self, m: Message) {
        self.post(|header| {
            let mut m = m;
            m.header = header;
            Command::Message(Box::new(m))
        })
        .await
    }

    async fn consume(&mut self, n: i64, dest: &Destination, prefetch: i32) -> Command {
        let id = self.consumer_id(n);
        self.request(|header| Command::ConsumerInfo(consumer_info(header, id, dest.clone(), prefetch))).await
    }

    async fn tx(&mut self, value: i64, op: u8) -> Command {
        let txid = TransactionId::Local { value, connection_id: Some(ConnectionId { value: self.conn.clone() }) };
        let conn = ConnectionId { value: self.conn.clone() };
        self.request(|header| {
            Command::TransactionInfo(TransactionInfo { header, connection_id: Some(conn), transaction_id: Some(txid), tx_type: op })
        })
        .await
    }

    fn local_tx(&self, value: i64) -> TransactionId {
        TransactionId::Local { value, connection_id: Some(ConnectionId { value: self.conn.clone() }) }
    }

    async fn ack(&mut self, n: i64, dest: &Destination, kind: u8, first: i64, last: i64, tx: Option<TransactionId>) -> Command {
        let a = ack_cmd(self.consumer_id(n), dest, kind, first, last, tx);
        self.request(|header| {
            let mut a = a;
            a.header = header;
            Command::MessageAck(a)
        })
        .await
    }
}

fn consumer_info(header: Header, id: ConsumerId, dest: Destination, prefetch: i32) -> ConsumerInfo {
    ConsumerInfo {
        header,
        consumer_id: Some(id),
        browser: false,
        destination: Some(dest),
        prefetch_size: prefetch,
        maximum_pending_message_limit: 0,
        dispatch_async: true,
        selector: None,
        client_id: None,
        subscription_name: None,
        no_local: false,
        exclusive: false,
        retroactive: false,
        priority: 0,
        broker_path: None,
        additional_predicate: None,
        network_subscription: false,
        optimized_acknowledge: false,
        no_range_acks: false,
        network_consumer_path: None,
    }
}

fn ack_cmd(consumer: ConsumerId, dest: &Destination, kind: u8, first: i64, last: i64, tx: Option<TransactionId>) -> MessageAck {
    let mid = |seq: i64| MessageId { text_view: None, producer_id: None, producer_sequence_id: 0, broker_sequence_id: seq };
    MessageAck {
        header: Header::default(),
        destination: Some(dest.clone()),
        transaction_id: tx,
        consumer_id: Some(consumer),
        ack_type: kind,
        first_message_id: Some(mid(first)),
        last_message_id: Some(mid(last)),
        message_count: 1,
        poison_cause: None,
    }
}

/// Text the broker cannot compress (bodies above the threshold are compressed on arrival).
fn incompressible(len: usize) -> String {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (b'!' + (x % 90) as u8) as char
        })
        .collect()
}

fn assert_ok(c: &Command) {
    assert!(matches!(c, Command::Response { .. }), "expected Response, got {c:?}");
}

/// Asserts an `ExceptionResponse` with this class whose message contains `text`.
fn assert_exception(c: &Command, class: &str, text: &str) {
    match c {
        Command::ExceptionResponse { exception: Some(e), .. } => {
            assert_eq!(e.class_name, class, "{e:?}");
            let m = e.message.as_deref().unwrap_or("");
            assert!(m.contains(text), "'{m}' does not contain '{text}'");
        }
        other => panic!("expected ExceptionResponse {class}, got {other:?}"),
    }
}

fn body(md: &MessageDispatch) -> String {
    String::from_utf8_lossy(md.message.as_ref().unwrap().content.as_deref().unwrap_or(b"")).into_owned()
}

fn seq(md: &MessageDispatch) -> i64 {
    md.message.as_ref().unwrap().message_id.as_ref().unwrap().broker_sequence_id
}

/// Waits until `cond` holds (connection cleanup runs asynchronously).
async fn eventually(what: &str, cond: impl Fn() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for: {what}");
}

#[tokio::test]
async fn message_pull_with_response_required_gets_a_response() {
    let b = broker_with(|_| {});
    let q = Destination::queue("PULL.RESPONSE");
    let mut c = Wire::open(&b, "ID:pull-1").await;
    assert_ok(&c.consume(1, &q, 0).await);
    let id = c.consumer_id(1);
    let dest = q.clone();
    let r = c
        .request(|header| {
            Command::MessagePull(MessagePull { header, consumer_id: Some(id), destination: Some(dest), timeout: -1, correlation_id: None, message_id: None })
        })
        .await;
    assert_ok(&r);
    let md = c.dispatch(1000).await.expect("null dispatch for receiveNoWait");
    assert!(md.message.is_none());
}

#[tokio::test]
async fn wildcard_and_composite_destinations_are_rejected() {
    let b = broker_with(|_| {});
    let mut c = Wire::open(&b, "ID:wild-1").await;
    let wild = Destination::queue("ORDERS.>");
    assert_exception(&c.consume(1, &wild, 10).await, INVALID_DESTINATION, "ORDERS.>");
    assert!(b.get_dest(&wild).is_none());
    let composite = Destination::queue("A,B");
    let pid = c.producer_id();
    let dest = composite.clone();
    let r = c
        .request(|header| {
            Command::ProducerInfo(ProducerInfo { header, producer_id: Some(pid), destination: Some(dest), broker_path: None, dispatch_async: false, window_size: 0 })
        })
        .await;
    assert_exception(&r, INVALID_DESTINATION, "A,B");
    assert!(b.get_dest(&composite).is_none() && b.get_dest(&Destination::queue("A")).is_none());
    for op in [dest_op::ADD, dest_op::REMOVE] {
        let star = Destination::queue("X.*");
        let r = c
            .request(|header| {
                Command::DestinationInfo(DestinationInfo { header, connection_id: None, destination: Some(star), operation_type: op, timeout: 0, broker_path: None })
            })
            .await;
        assert_exception(&r, INVALID_DESTINATION, "X.*");
    }
    let m = c.message(&Destination::queue("W.>"), "x");
    assert_exception(&c.send(m).await, INVALID_DESTINATION, "W.>");
    let m = c.message(&Destination::queue("W1,W2"), "x");
    c.send_async(m).await;
    // The connection stays usable and nothing was created.
    assert_ok(&c.consume(2, &Destination::queue("FINE"), 10).await);
    assert!(b.destinations().iter().all(|d| !d.dest.is_wildcard() && !d.dest.is_composite()));
}

#[tokio::test]
async fn xa_transactions_are_refused_with_the_exact_text() {
    let b = broker_with(|_| {});
    let mut c = Wire::open(&b, "ID:xa-1").await;
    let xa = TransactionId::Xa { format_id: 1, global_transaction_id: Some(Bytes::from_static(b"g")), branch_qualifier: Some(Bytes::from_static(b"b")) };
    let x = xa.clone();
    let r = c
        .request(|header| Command::TransactionInfo(TransactionInfo { header, connection_id: None, transaction_id: Some(x), tx_type: tx_type::BEGIN }))
        .await;
    assert_exception(&r, JMS_EXCEPTION, XA_TEXT);
    for op in [tx_type::PREPARE, tx_type::COMMIT_TWO_PHASE, tx_type::RECOVER] {
        assert_exception(&c.tx(1, op).await, JMS_EXCEPTION, XA_TEXT);
    }
    let q = Destination::queue("XA.Q");
    let mut m = c.message(&q, "x");
    m.transaction_id = Some(xa.clone());
    assert_exception(&c.send(m).await, JMS_EXCEPTION, XA_TEXT);
    assert_exception(&c.ack(9, &q, ack_type::STANDARD, 1, 1, Some(xa)).await, JMS_EXCEPTION, XA_TEXT);
    assert!(b.get_dest(&q).is_none_or(|d| d.snapshot().pending == 0));
    assert_ok(&c.consume(1, &q, 10).await);
}

#[tokio::test]
async fn unknown_transactions_are_reported() {
    let b = broker_with(|_| {});
    let mut c = Wire::open(&b, "ID:unknown-1").await;
    for op in [tx_type::COMMIT_ONE_PHASE, tx_type::ROLLBACK] {
        assert_exception(&c.tx(7, op).await, JMS_EXCEPTION, "has not been started.");
    }
    let q = Destination::queue("TX.UNKNOWN");
    let mut m = c.message(&q, "x");
    m.transaction_id = Some(c.local_tx(8));
    assert_exception(&c.send(m).await, JMS_EXCEPTION, "Transaction 'TX:ID:unknown-1:8' has not been started.");
    assert_ok(&c.consume(1, &q, 10).await);
    let tx = Some(c.local_tx(8));
    assert_exception(&c.ack(1, &q, ack_type::STANDARD, 1, 1, tx).await, JMS_EXCEPTION, "has not been started.");
    // END and FORGET are acknowledged with no effect.
    assert_ok(&c.tx(8, tx_type::END).await);
    assert_ok(&c.tx(8, tx_type::FORGET).await);
}

#[tokio::test]
async fn transacted_sends_are_invisible_until_commit_and_ordered() {
    let b = broker_with(|_| {});
    let mut p = Wire::open(&b, "ID:txp-1").await;
    let q = Destination::queue("TX.ORDER");
    let other = Destination::queue("TX.OTHER");
    assert_ok(&p.tx(1, tx_type::BEGIN).await);
    let before = b.memory.used();
    for i in 1..=3 {
        let mut m = p.message(&q, &format!("t-{i}"));
        m.transaction_id = Some(p.local_tx(1));
        assert_ok(&p.send(m).await);
    }
    let mut m = p.message(&other, "t-other");
    m.transaction_id = Some(p.local_tx(1));
    assert_ok(&p.send(m).await);
    // Destinations exist from the send on; nothing is visible yet; buffered memory is accounted.
    assert_eq!(b.get_dest(&q).expect("created at send").snapshot().pending, 0);
    assert_eq!(b.get_dest(&other).expect("created at send").snapshot().pending, 0);
    assert!(b.memory.used() > before);
    let mut c = Wire::open(&b, "ID:txc-1").await;
    assert_ok(&c.consume(1, &q, 100).await);
    assert!(c.dispatch(200).await.is_none(), "invisible before commit");
    let m = p.message(&q, "n-1");
    assert_ok(&p.send(m).await);
    assert_ok(&p.tx(1, tx_type::COMMIT_ONE_PHASE).await);
    let mut got = Vec::new();
    for _ in 0..4 {
        got.push(body(&c.dispatch(2000).await.unwrap()));
    }
    assert_eq!(got, vec!["n-1", "t-1", "t-2", "t-3"]);
    assert_eq!(b.get_dest(&other).unwrap().snapshot().pending, 1, "commit across destinations");
}

#[tokio::test]
async fn rollback_of_sends_releases_memory() {
    let b = broker_with(|_| {});
    let mut p = Wire::open(&b, "ID:txr-1").await;
    let q = Destination::queue("TX.ROLLBACK.SENDS");
    let before = b.memory.used();
    assert_ok(&p.tx(1, tx_type::BEGIN).await);
    for i in 0..5 {
        let mut m = p.message(&q, &format!("r{i}"));
        m.transaction_id = Some(p.local_tx(1));
        assert_ok(&p.send(m).await);
    }
    assert!(b.memory.used() > before);
    assert_ok(&p.tx(1, tx_type::ROLLBACK).await);
    assert_eq!(b.memory.used(), before);
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 0);
}

#[tokio::test]
async fn memory_limit_applies_to_transacted_sends() {
    let b = broker_with(|f| f.broker.max_memory_mb = 1);
    let mut p = Wire::open(&b, "ID:txm-1").await;
    let q = Destination::queue("TX.MEMORY");
    assert_ok(&p.tx(1, tx_type::BEGIN).await);
    let big = incompressible(400 * 1024);
    for _ in 0..2 {
        let mut m = p.message(&q, &big);
        m.transaction_id = Some(p.local_tx(1));
        assert_ok(&p.send(m).await);
    }
    let mut m = p.message(&q, &big);
    m.transaction_id = Some(p.local_tx(1));
    assert_exception(&p.send(m).await, "javax.jms.ResourceAllocationException", "Memory Limit");
    // An asynchronous transacted send that does not fit: discarded and counted.
    let mut m = p.message(&q, &big);
    m.transaction_id = Some(p.local_tx(1));
    p.send_async(m).await;
    assert_ok(&p.tx(1, tx_type::COMMIT_ONE_PHASE).await);
    let snap = b.get_dest(&q).unwrap().snapshot();
    assert_eq!(snap.pending, 2);
    assert_eq!(snap.stats.discarded, 1);
}

#[tokio::test]
async fn deferred_acks_free_the_window_and_count_at_commit() {
    let b = broker_with(|_| {});
    let mut c = Wire::open(&b, "ID:txa-1").await;
    let q = Destination::queue("TX.ACKS");
    for i in 1..=4 {
        let m = c.message(&q, &format!("m{i}"));
        assert_ok(&c.send(m).await);
    }
    assert_ok(&c.consume(1, &q, 2).await);
    let first = c.dispatch(2000).await.unwrap();
    let second = c.dispatch(2000).await.unwrap();
    assert!(c.dispatch(200).await.is_none(), "prefetch 2");
    assert_ok(&c.tx(1, tx_type::BEGIN).await);
    let tx = Some(c.local_tx(1));
    assert_ok(&c.ack(1, &q, ack_type::STANDARD, seq(&first), seq(&second), tx).await);
    // The window is free before the commit; the messages are still held.
    assert_eq!(body(&c.dispatch(2000).await.unwrap()), "m3");
    let fourth = c.dispatch(2000).await.unwrap();
    assert_eq!(body(&fourth), "m4");
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().stats.dequeued, 0);
    assert_eq!(d.snapshot().inflight, 4);
    // REDELIVERED, DELIVERED and EXPIRED acks are never deferred, even with an unknown transaction id.
    let unknown = Some(c.local_tx(99));
    assert_ok(&c.ack(1, &q, ack_type::REDELIVERED, seq(&fourth), seq(&fourth), unknown.clone()).await);
    assert_ok(&c.ack(1, &q, ack_type::DELIVERED, seq(&fourth), seq(&fourth), unknown.clone()).await);
    assert_ok(&c.ack(1, &q, ack_type::EXPIRED, seq(&fourth), seq(&fourth), unknown.clone()).await);
    assert_eq!(d.snapshot().stats.expired, 1, "EXPIRED applied at once");
    assert_exception(&c.ack(1, &q, ack_type::INDIVIDUAL, 1, 1, unknown).await, JMS_EXCEPTION, "has not been started.");
    assert_ok(&c.tx(1, tx_type::COMMIT_ONE_PHASE).await);
    assert_eq!(d.snapshot().stats.dequeued, 2, "consumed counter after commit");
    assert_eq!(d.snapshot().inflight, 1);
}

#[tokio::test]
async fn rollback_of_receives_then_close_redelivers_first() {
    let b = broker_with(|_| {});
    let mut c = Wire::open(&b, "ID:txb-1").await;
    let q = Destination::queue("TX.ROLLBACK.RECEIVES");
    for i in 1..=4 {
        let m = c.message(&q, &format!("m{i}"));
        assert_ok(&c.send(m).await);
    }
    assert_ok(&c.consume(1, &q, 2).await);
    let first = c.dispatch(2000).await.unwrap();
    let second = c.dispatch(2000).await.unwrap();
    assert_ok(&c.tx(1, tx_type::BEGIN).await);
    let tx = Some(c.local_tx(1));
    assert_ok(&c.ack(1, &q, ack_type::STANDARD, seq(&first), seq(&second), tx).await);
    let _third = c.dispatch(2000).await.unwrap();
    let _fourth = c.dispatch(2000).await.unwrap();
    assert_ok(&c.tx(1, tx_type::ROLLBACK).await);
    // Still inflight to the open consumer: no duplicate dispatch.
    assert!(c.dispatch(200).await.is_none());
    // The consumer closes having delivered m1 and m2 to the application.
    let cid = c.consumer_id(1);
    let last = seq(&second);
    assert_ok(&c.request(|header| Command::RemoveInfo(RemoveInfo { header, object_id: Some(DataStructure::ConsumerId(cid)), last_delivered_sequence_id: last })).await);
    assert_ok(&c.consume(2, &q, 10).await);
    let mut got = Vec::new();
    for _ in 0..4 {
        let md = c.dispatch(2000).await.unwrap();
        got.push((body(&md), md.redelivery_counter));
    }
    assert_eq!(got, vec![("m1".into(), 1), ("m2".into(), 1), ("m3".into(), 0), ("m4".into(), 0)]);
}

#[tokio::test]
async fn connection_drop_mid_transaction_rolls_back() {
    let b = broker_with(|_| {});
    let q = Destination::queue("TX.DROP");
    let out = Destination::queue("TX.DROP.OUT");
    let before;
    {
        let mut c = Wire::open(&b, "ID:txd-1").await;
        for i in 1..=3 {
            let m = c.message(&q, &format!("m{i}"));
            assert_ok(&c.send(m).await);
        }
        before = b.memory.used();
        assert_ok(&c.consume(1, &q, 10).await);
        let mut last = 0;
        for _ in 0..3 {
            last = seq(&c.dispatch(2000).await.unwrap());
        }
        assert_ok(&c.tx(1, tx_type::BEGIN).await);
        for i in 1..=2 {
            let mut m = c.message(&out, &format!("s{i}"));
            m.transaction_id = Some(c.local_tx(1));
            assert_ok(&c.send(m).await);
        }
        let tx = Some(c.local_tx(1));
        assert_ok(&c.ack(1, &q, ack_type::STANDARD, last, last, tx).await);
        // The client process dies: the socket closes without ShutdownInfo.
    }
    eventually("connection cleanup", || b.connections().is_empty()).await;
    assert_eq!(b.memory.used(), before, "buffered sends discarded");
    assert_eq!(b.get_dest(&out).unwrap().snapshot().pending, 0);
    let mut c2 = Wire::open(&b, "ID:txd-2").await;
    assert_ok(&c2.consume(1, &q, 10).await);
    let mut got = Vec::new();
    for _ in 0..3 {
        let md = c2.dispatch(2000).await.unwrap();
        got.push((body(&md), md.redelivery_counter));
    }
    assert_eq!(got, vec![("m1".into(), 1), ("m2".into(), 1), ("m3".into(), 1)]);
}

#[tokio::test]
async fn transacted_message_expired_at_commit_is_counted() {
    let b = broker_with(|_| {});
    let mut p = Wire::open(&b, "ID:txe-1").await;
    let q = Destination::queue("TX.EXPIRE");
    assert_ok(&p.tx(1, tx_type::BEGIN).await);
    let mut m = p.message(&q, "short");
    m.expiration = now_ms() + 100;
    m.transaction_id = Some(p.local_tx(1));
    assert_ok(&p.send(m).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_ok(&p.tx(1, tx_type::COMMIT_ONE_PHASE).await);
    let snap = b.get_dest(&q).unwrap().snapshot();
    assert_eq!(snap.pending, 0);
    assert_eq!(snap.stats.expired, 1);
}

#[tokio::test]
async fn temporary_queue_lifecycle_and_advisories() {
    let b = broker_with(|_| {});
    let mut watcher = Wire::open(&b, "ID:watch-1").await;
    let advisory = Destination::new(DestKind::Topic, "ActiveMQ.Advisory.TempQueue,ActiveMQ.Advisory.TempTopic");
    assert_ok(&watcher.consume(1, &advisory, 100).await);
    let base = b.memory.used();
    let tq = Destination::new(DestKind::TempQueue, "ID:owner-1:1:1");
    {
        let mut owner = Wire::open(&b, "ID:owner-1").await;
        let d = tq.clone();
        let r = owner
            .request(|header| Command::DestinationInfo(DestinationInfo { header, connection_id: None, destination: Some(d), operation_type: dest_op::ADD, timeout: 0, broker_path: None }))
            .await;
        assert_ok(&r);
        for i in 0..5 {
            let m = watcher.message(&tq, &format!("reply-{i}"));
            assert_ok(&watcher.send(m).await);
        }
        assert!(b.memory.used() > base);
        // Only the owner may consume.
        assert_exception(&watcher.consume(2, &tq, 10).await, INVALID_DESTINATION, "temporary destination");
        assert_ok(&owner.consume(1, &tq, 1).await);
        assert_eq!(body(&owner.dispatch(2000).await.unwrap()), "reply-0");
    }
    eventually("temporary queue deleted with its owner", || b.get_dest(&tq).is_none()).await;
    assert_eq!(b.memory.used(), base, "memory of the temporary queue released");
    let advisory_op = |md: &MessageDispatch| match md.message.as_ref().unwrap().data_structure.as_ref() {
        Some(DataStructure::DestinationInfo(di)) => (di.destination.clone().unwrap(), di.operation_type),
        other => panic!("not an advisory: {other:?}"),
    };
    assert_eq!(advisory_op(&watcher.dispatch(2000).await.unwrap()), (tq.clone(), dest_op::ADD));
    assert_eq!(advisory_op(&watcher.dispatch(2000).await.unwrap()), (tq.clone(), dest_op::REMOVE));
    // Sends to the deleted temporary queue: refused when synchronous, nothing recreated.
    let mut m = watcher.message(&tq, "late");
    m.expiration = now_ms() - 1000;
    assert_exception(&watcher.send(m).await, INVALID_DESTINATION, "deleted");
    assert!(b.get_dest(&tq).is_none());
}

#[tokio::test]
async fn temporary_topic_ownership_and_durable_refusal() {
    let b = broker_with(|_| {});
    let mut owner = Wire::open(&b, "ID:towner-1").await;
    let mut other = Wire::open(&b, "ID:tother-1").await;
    let tt = Destination::new(DestKind::TempTopic, "ID:towner-1:1:1");
    let d = tt.clone();
    assert_ok(&owner.request(|header| Command::DestinationInfo(DestinationInfo { header, connection_id: None, destination: Some(d), operation_type: dest_op::ADD, timeout: 0, broker_path: None })).await);
    assert_exception(&other.consume(1, &tt, 10).await, INVALID_DESTINATION, "temporary destination");
    assert_ok(&owner.consume(1, &tt, 10).await);
    let m = other.message(&tt, "event");
    assert_ok(&other.send(m).await);
    assert_eq!(body(&owner.dispatch(2000).await.unwrap()), "event");
    // Removal refused while a consumer is attached.
    let d = tt.clone();
    let r = owner.request(|header| Command::DestinationInfo(DestinationInfo { header, connection_id: None, destination: Some(d), operation_type: dest_op::REMOVE, timeout: 0, broker_path: None })).await;
    assert_exception(&r, JMS_EXCEPTION, "active subscription");
    // Durable subscriptions are refused.
    let topic = Destination::new(DestKind::Topic, "DURABLE");
    let id = other.consumer_id(5);
    let r = other
        .request(|header| {
            let mut ci = consumer_info(header, id, topic, 10);
            ci.subscription_name = Some("sub".into());
            Command::ConsumerInfo(ci)
        })
        .await;
    assert_exception(&r, JMS_EXCEPTION, "Durable subscriptions are not supported");
    let r = other
        .request(|header| Command::RemoveSubscriptionInfo(RemoveSubscriptionInfo { header, connection_id: None, subscription_name: Some("sub".into()), client_id: Some("c".into()) }))
        .await;
    assert_exception(&r, JMS_EXCEPTION, "Durable subscriptions are not supported");
    drop(owner);
    eventually("temporary topic deleted with its owner", || b.get_dest(&tt).is_none()).await;
}

#[tokio::test]
async fn duplicate_windows_are_released_when_the_connection_closes() {
    let b = broker_with(|_| {});
    let q = Destination::queue("DUP.CONN");
    let resent;
    {
        // An anonymous producer: no ProducerInfo, the destination is only in the message.
        let mut p = Wire::open(&b, "ID:anon-1").await;
        let m = p.message(&q, "once");
        resent = m.clone();
        assert_ok(&p.send(m.clone()).await);
        assert_ok(&p.send(m).await);
        assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 1, "duplicate discarded with a Response");
    }
    eventually("connection cleanup", || b.connections().is_empty()).await;
    let mut p = Wire::open(&b, "ID:anon-1").await;
    assert_ok(&p.send(resent).await);
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 2, "window forgotten with the old connection");
}
