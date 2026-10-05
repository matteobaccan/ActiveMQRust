// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Selector semantics: conformance with ActiveMQ's selector engine over a table of selectors,
//! and selective dispatch on queues, topics and browsers without the network.

use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::SubSpec;
use mqrust::broker::entry::{Entry, Memory, Meta};
use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::model::*;
use mqrust::openwire::props::{PrimitiveMap, Value};
use mqrust::openwire::types as t;
use mqrust::selector::{EvalError, SVal, Selector};

// ---------------------------------------------------------------------------
// Conformance with ActiveMQ
// ---------------------------------------------------------------------------

/// The message the conformance table was computed on with ActiveMQ's selector engine.
fn conformance_message() -> Entry {
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.message_id = Some(MessageId {
        text_view: Some(Arc::from("ID:host-1-2-3:1:1:1:1")),
        producer_id: None,
        producer_sequence_id: 0,
        broker_sequence_id: 0,
    });
    m.destination = Some(Destination::queue("Q"));
    m.reply_to = Some(Destination::new(DestKind::Topic, "R"));
    m.correlation_id = Some("ORD-A-100".into());
    m.priority = 4;
    m.persistent = true;
    m.jms_type = Some("T1".into());
    m.timestamp = 1000;
    m.expiration = 0;
    m.group_id = Some("G1".into());
    m.group_sequence = 2;
    m.transaction_id = Some(TransactionId::Local {
        value: 5,
        connection_id: Some(ConnectionId {
            value: Arc::from("ID:c"),
        }),
    });
    m.broker_path = Some(vec![
        DataStructure::BrokerId(BrokerId { value: Arc::from("B1") }),
        DataStructure::BrokerId(BrokerId { value: Arc::from("B2") }),
    ]);
    m.broker_in_time = 2000;
    m.broker_out_time = 3000;
    let mut p = PrimitiveMap::new();
    let props = [
        ("color", Value::String("red".into())),
        ("size", Value::Int(3)),
        ("weight", Value::Double(2.5)),
        ("flag", Value::Bool(true)),
        ("off", Value::Bool(false)),
        ("b", Value::Byte(7)),
        ("s", Value::Short(300)),
        ("l", Value::Long(10_000_000_000)),
        ("f", Value::Float(1.5)),
        ("c", Value::Char('x' as u16)),
        ("name", Value::String("O'Brien".into())),
        ("bytes", Value::Bytes(Bytes::from_static(&[1, 2]))),
        ("str5", Value::String("5".into())),
        ("empty", Value::String(String::new())),
        ("t", Value::String("true".into())),
        ("big", Value::Int(16_777_217)),
        ("ff", Value::Float(16_777_216.0)),
        ("bmp", Value::String("\u{FFFD}".into())),
        ("zero", Value::Int(0)),
        ("dzero", Value::Double(0.0)),
        ("code1", Value::String("A_1".into())),
        ("code2", Value::String("AB1".into())),
        ("pct", Value::String("100%".into())),
        ("nl", Value::String("a\nb".into())),
        ("bytes2", Value::Bytes(Bytes::from_static(&[1, 2]))),
        ("lit", Value::String("r%".into())),
        ("uc", Value::String("\u{C9}lan".into())),
        ("JMSXUserID", Value::String("u1".into())),
    ];
    for (k, v) in props {
        p.set(k, v);
    }
    m.marshalled_properties = Some(p.encode());
    let meta = Meta::new(Arc::new(Memory::new(0)), &m);
    Entry {
        seq: 1,
        msg: Arc::new(m),
        meta,
        redelivery: 1,
    }
}

#[test]
fn selectors_evaluate_like_activemq() {
    let table = include_str!("data/selector_conformance.tsv");
    let entry = conformance_message();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in table.lines().map(|l| l.trim_end_matches('\r')) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (expected, selector) = line.split_once('\t').expect("result<TAB>selector");
        count += 1;
        let got = match Selector::compile(selector) {
            Err(_) => "ERR",
            Ok(None) => "EMPTY",
            Ok(Some(s)) => match s.evaluate(&entry) {
                Ok(SVal::Bool(true)) => "TRUE",
                Ok(SVal::Bool(false)) => "FALSE",
                Ok(SVal::Null) => "NULL",
                Ok(_) => "VALUE",
                Err(EvalError) => "EXC",
            },
        };
        if got != expected {
            failures.push(format!("{selector:?}: expected {expected}, got {got}"));
        }
    }
    assert!(count > 500, "table loaded");
    assert!(
        failures.is_empty(),
        "{} of {count} differ from ActiveMQ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Selective dispatch
// ---------------------------------------------------------------------------

fn broker() -> Arc<Broker> {
    Broker::new(Arc::new(
        build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap(),
    ))
}

struct Client {
    handle: Arc<ConnHandle>,
    rx: mpsc::UnboundedReceiver<Out>,
    conn: &'static str,
}

impl Client {
    fn new(b: &Broker, conn: &'static str) -> Client {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = Arc::new(ConnHandle::new(b.new_conn_id(), "127.0.0.1:1".parse().unwrap(), tx));
        Client { handle, rx, conn }
    }

    fn consumer_id(&self, n: i64) -> ConsumerId {
        ConsumerId {
            connection_id: Arc::from(self.conn),
            session_id: 1,
            value: n,
        }
    }

    /// Received dispatches: (body, redelivery counter); `None` body for the end-of-browse marker.
    fn drain(&mut self) -> Vec<(Option<String>, i32)> {
        let mut v = Vec::new();
        while let Ok(o) = self.rx.try_recv() {
            if let Out::Cmd(Command::MessageDispatch(md)) = o {
                v.push(match md.message {
                    Some(m) => (
                        Some(String::from_utf8_lossy(m.content.as_deref().unwrap_or(b"")).into_owned()),
                        m.redelivery_counter,
                    ),
                    None => (None, 0),
                });
            }
        }
        v
    }

    fn texts(&mut self) -> Vec<String> {
        self.drain().into_iter().filter_map(|x| x.0).collect()
    }
}

fn producer() -> ProducerId {
    ProducerId {
        connection_id: Arc::from("ID:prod-1-1-1:1"),
        session_id: 1,
        value: 1,
    }
}

static SEQ: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

/// A text message whose body is `body`, with string properties.
fn msg(dest: &Destination, body: &str, props: &[(&str, &str)]) -> Message {
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.destination = Some(dest.clone());
    m.producer_id = Some(producer());
    m.message_id = Some(MessageId {
        text_view: None,
        producer_id: Some(producer()),
        producer_sequence_id: SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        broker_sequence_id: 0,
    });
    m.content = Some(Bytes::from(body.to_string()));
    m.timestamp = now_ms();
    if !props.is_empty() {
        let mut p = PrimitiveMap::new();
        for (k, v) in props {
            p.set(k, Value::String(v.to_string()));
        }
        m.marshalled_properties = Some(p.encode());
    }
    m
}

fn send(b: &Broker, dest: &Destination, body: &str, props: &[(&str, &str)]) {
    b.deliver(msg(dest, body, props), true, now_ms()).unwrap();
}

fn subscribe(
    b: &Broker,
    c: &Client,
    n: i64,
    dest: &Destination,
    prefetch: i32,
    selector: Option<&str>,
    browser: bool,
) -> ConsumerId {
    let id = c.consumer_id(n);
    b.get_or_create(dest, None).add_sub(
        SubSpec {
            id: id.clone(),
            conn: c.handle.clone(),
            prefetch,
            selector: selector.and_then(|s| Selector::compile(s).unwrap().map(Arc::new)),
            no_local: false,
            browser,
        },
        now_ms(),
    );
    id
}

#[test]
fn disjoint_selectors_each_get_their_fifo_subsequence() {
    let b = broker();
    let q = Destination::queue("SEL.DISJOINT");
    let mut a = Client::new(&b, "ca");
    let mut bb = Client::new(&b, "cb");
    subscribe(&b, &a, 1, &q, 1000, Some("type = 'A'"), false);
    subscribe(&b, &bb, 1, &q, 1000, Some("type = 'B'"), false);
    for i in 1..=20 {
        let kind = if i % 2 == 0 { "A" } else { "B" };
        send(&b, &q, &format!("{kind}{i}"), &[("type", kind)]);
    }
    assert_eq!(
        a.texts(),
        (1..=20)
            .filter(|i| i % 2 == 0)
            .map(|i| format!("A{i}"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        bb.texts(),
        (1..=20)
            .filter(|i| i % 2 == 1)
            .map(|i| format!("B{i}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn unmatched_messages_stay_without_blocking() {
    let b = broker();
    let q = Destination::queue("SEL.NOBLOCK");
    let mut c = Client::new(&b, "c1");
    send(&b, &q, "ORD-B-1", &[("kind", "B")]);
    send(&b, &q, "ORD-A-1", &[("kind", "A")]);
    let id = subscribe(&b, &c, 1, &q, 1000, Some("kind = 'A'"), false);
    assert_eq!(c.texts(), vec!["ORD-A-1"]);
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().pending, 1, "ORD-B-1 stays in the queue");
    // Ack the delivered message with the close; a consumer without selector gets the rest.
    d.remove_sub(&id, i64::MAX, now_ms());
    let mut all = Client::new(&b, "c2");
    subscribe(&b, &all, 1, &q, 1000, None, false);
    assert!(all.texts().contains(&"ORD-B-1".to_string()));
}

#[test]
fn matching_message_behind_100000_non_matching_is_delivered() {
    let b = broker();
    let q = Destination::queue("SEL.BACKLOG");
    for i in 0..100_000 {
        send(&b, &q, &format!("B{i}"), &[("type", "B")]);
    }
    send(&b, &q, "A", &[("type", "A")]);
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 10, Some("type = 'A'"), false);
    assert_eq!(c.texts(), vec!["A"]);
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 100_000);
    // A new matching message is found without rescanning the backlog.
    send(&b, &q, "A2", &[("type", "A")]);
    assert_eq!(c.texts(), vec!["A2"]);
}

#[test]
fn redelivered_message_is_re_examined_by_the_cursor() {
    let b = broker();
    let q = Destination::queue("SEL.CURSOR");
    let mut holder = Client::new(&b, "holder");
    let holder_id = subscribe(&b, &holder, 1, &q, 1, Some("type = 'A'"), false);
    for i in 1..=100 {
        let kind = if i == 10 { "A" } else { "B" };
        send(&b, &q, &format!("m{i}"), &[("type", kind)]);
    }
    assert_eq!(holder.texts(), vec!["m10"]);
    // The selective consumer examines 1..100 while m10 is in flight and finds nothing.
    let mut sel = Client::new(&b, "sel");
    subscribe(&b, &sel, 1, &q, 1000, Some("type = 'A'"), false);
    assert!(sel.texts().is_empty());
    // The holder closes without acknowledging: m10 returns to its original position and the
    // cursor moves back to it, before any later matching message.
    b.get_dest(&q).unwrap().remove_sub(&holder_id, -1, now_ms());
    send(&b, &q, "m101", &[("type", "A")]);
    let got = sel.drain();
    assert_eq!(
        got.iter().filter_map(|x| x.0.clone()).collect::<Vec<_>>(),
        vec!["m10", "m101"]
    );
    assert_eq!(got[0].1, 1, "m10 is a redelivery");
}

#[test]
fn topic_subscribers_receive_only_matching_messages() {
    let b = broker();
    let topic = Destination::new(DestKind::Topic, "SEL.TOPIC");
    let mut errors = Client::new(&b, "c1");
    let mut warnings = Client::new(&b, "c2");
    let mut all = Client::new(&b, "c3");
    subscribe(&b, &errors, 1, &topic, 100, Some("level = 'ERROR'"), false);
    subscribe(&b, &warnings, 1, &topic, 100, Some("level = 'WARN'"), false);
    subscribe(&b, &all, 1, &topic, 100, None, false);
    for level in ["ERROR", "WARN", "INFO", "ERROR"] {
        send(&b, &topic, level, &[("level", level)]);
    }
    assert_eq!(errors.texts(), vec!["ERROR", "ERROR"]);
    assert_eq!(warnings.texts(), vec!["WARN"]);
    assert_eq!(all.texts(), vec!["ERROR", "WARN", "INFO", "ERROR"]);
}

#[test]
fn filtered_browse_returns_matching_messages_in_fifo_order() {
    let b = broker();
    let q = Destination::queue("SEL.BROWSE");
    for id in ["ORD-A-1", "ORD-B-1", "ORD-A-2"] {
        let mut m = msg(&q, id, &[]);
        m.correlation_id = Some(id.to_string());
        b.deliver(m, true, now_ms()).unwrap();
    }
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 9, &q, 100, Some("JMSCorrelationID LIKE 'ORD-A%'"), true);
    let got = c.drain();
    assert_eq!(
        got.iter().filter_map(|x| x.0.clone()).collect::<Vec<_>>(),
        vec!["ORD-A-1", "ORD-A-2"]
    );
    assert!(got.last().unwrap().0.is_none(), "end-of-browse marker");
    assert_eq!(
        b.get_dest(&q).unwrap().snapshot().pending,
        3,
        "browsing does not consume"
    );
}

#[test]
fn header_only_selector_never_decodes_properties() {
    let b = broker();
    let q = Destination::queue("SEL.HEADERS");
    for i in 0..10 {
        let mut m = msg(&q, &format!("m{i}"), &[("color", "red")]);
        m.correlation_id = Some(if i == 9 { "match" } else { "other" }.to_string());
        b.deliver(m, true, now_ms()).unwrap();
    }
    let mut c = Client::new(&b, "c1");
    subscribe(
        &b,
        &c,
        1,
        &q,
        100,
        Some("JMSCorrelationID = 'match' AND JMSPriority >= 0"),
        false,
    );
    assert_eq!(c.texts(), vec!["m9"]);
    let (_, entries) = b.get_dest(&q).unwrap().page(0, 100);
    assert_eq!(entries.len(), 9);
    assert!(
        entries.iter().all(|e| !e.properties_decoded()),
        "examined messages kept their properties encoded"
    );
}

#[test]
fn undecodable_properties_do_not_match_and_do_not_block() {
    let b = broker();
    let q = Destination::queue("SEL.BROKEN");
    let mut bad = msg(&q, "broken", &[]);
    bad.marshalled_properties = Some(Bytes::from_static(&[0, 0, 0, 3, 0]));
    b.deliver(bad, true, now_ms()).unwrap();
    send(&b, &q, "good", &[("color", "red")]);
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 100, Some("color = 'red' OR color IS NULL"), false);
    assert_eq!(c.texts(), vec!["good"]);
    assert_eq!(
        b.get_dest(&q).unwrap().snapshot().pending,
        1,
        "the undecodable message stays queued"
    );
}
