// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Broker semantics without the network: FIFO, round-robin, redelivery, selectors, acks,
//! DLQ, expiry and the sweeper, topics, browsers, pull, duplicates, memory limit, temporary
//! destinations and their advisories, idle-destination removal, broker IDs and compression.

use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::SubSpec;
use mqrust::broker::entry::{Entry, Meta};
use mqrust::broker::{now_ms, Broker, DLQ_NAME};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::model::*;
use mqrust::openwire::types as t;
use mqrust::selector::Selector;

fn broker_with(f: impl FnOnce(&mut FileConfig)) -> Arc<Broker> {
    let mut fc = FileConfig::default();
    f(&mut fc);
    Broker::new(Arc::new(
        build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap(),
    ))
}

fn broker() -> Arc<Broker> {
    broker_with(|_| {})
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

    /// Received dispatches: (text, broker seq, redelivery counter); `None` text for null dispatches.
    fn drain(&mut self) -> Vec<(Option<String>, i64, i32)> {
        let mut v = Vec::new();
        while let Ok(o) = self.rx.try_recv() {
            if let Out::Cmd(Command::MessageDispatch(md)) = o {
                match md.message {
                    Some(m) => v.push((
                        Some(String::from_utf8_lossy(m.content.as_deref().unwrap_or(b"")).into_owned()),
                        m.message_id.as_ref().unwrap().broker_sequence_id,
                        m.redelivery_counter,
                    )),
                    None => v.push((None, 0, 0)),
                }
            }
        }
        v
    }
}

fn texts(v: &[(Option<String>, i64, i32)]) -> Vec<String> {
    v.iter().filter_map(|x| x.0.clone()).collect()
}

fn producer() -> ProducerId {
    ProducerId {
        connection_id: Arc::from("ID:prod-1-1-1:1"),
        session_id: 1,
        value: 1,
    }
}

static SEQ: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn msg(dest: &Destination, body: &str) -> Message {
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
    m
}

fn send(b: &Broker, dest: &Destination, body: &str) {
    b.deliver(msg(dest, body), true, now_ms()).unwrap();
}

fn subscribe(b: &Broker, c: &Client, n: i64, dest: &Destination, prefetch: i32, selector: Option<&str>) -> ConsumerId {
    let id = c.consumer_id(n);
    let d = b.get_or_create(dest, None);
    d.add_sub(
        SubSpec {
            id: id.clone(),
            conn: c.handle.clone(),
            prefetch,
            selector: selector.map(|s| Arc::new(Selector::compile(s).unwrap().unwrap())),
            no_local: false,
            browser: false,
        },
        now_ms(),
    );
    id
}

fn ack(
    b: &Broker,
    dest: &Destination,
    consumer: &ConsumerId,
    kind: u8,
    first: Option<i64>,
    last: i64,
    persistent_poison: bool,
) {
    let _ = persistent_poison;
    let mid = |seq: i64| MessageId {
        text_view: None,
        producer_id: Some(producer()),
        producer_sequence_id: 0,
        broker_sequence_id: seq,
    };
    let a = MessageAck {
        header: Header::default(),
        destination: Some(dest.clone()),
        transaction_id: None,
        consumer_id: Some(consumer.clone()),
        ack_type: kind,
        first_message_id: first.map(mid),
        last_message_id: Some(mid(last)),
        message_count: 1,
        poison_cause: None,
    };
    let d = b.get_dest(dest).unwrap();
    let effects = d.ack(&a, false, now_ms());
    b.run_effects(effects, now_ms());
}

#[test]
fn fifo_single_consumer() {
    let b = broker();
    let q = Destination::queue("FIFO");
    for i in 1..=10 {
        send(&b, &q, &format!("msg-{i}"));
    }
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 1000, None);
    let got = texts(&c.drain());
    assert_eq!(got, (1..=10).map(|i| format!("msg-{i}")).collect::<Vec<_>>());
}

#[test]
fn round_robin_keeps_fifo_per_consumer() {
    let b = broker();
    let q = Destination::queue("RR");
    let mut c1 = Client::new(&b, "c1");
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c1, 1, &q, 1000, None);
    subscribe(&b, &c2, 1, &q, 1000, None);
    for i in 1..=10 {
        send(&b, &q, &format!("m{i:02}"));
    }
    let a = texts(&c1.drain());
    let bb = texts(&c2.drain());
    assert_eq!(a.len() + bb.len(), 10);
    assert_eq!(a.len(), 5);
    let mut sorted = a.clone();
    sorted.sort();
    assert_eq!(a, sorted, "each consumer gets an ordered subsequence");
}

#[test]
fn redelivered_messages_return_to_original_position() {
    let b = broker();
    let q = Destination::queue("REDELIVER");
    for i in 1..=5 {
        send(&b, &q, &format!("m{i}"));
    }
    let mut c1 = Client::new(&b, "c1");
    let id1 = subscribe(&b, &c1, 1, &q, 2, None);
    let first = c1.drain();
    assert_eq!(texts(&first), vec!["m1", "m2"]);
    // Close without ack: m1 and m2 go back in front of m3..m5.
    b.get_dest(&q).unwrap().remove_sub(&id1, -1, now_ms());
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 1000, None);
    let got = c2.drain();
    assert_eq!(texts(&got), vec!["m1", "m2", "m3", "m4", "m5"]);
    assert_eq!(got[0].2, 1, "redelivery counter incremented");
    assert_eq!(got[2].2, 0);
}

#[test]
fn last_delivered_limits_redelivery_increment() {
    let b = broker();
    let q = Destination::queue("LASTDELIVERED");
    send(&b, &q, "a");
    send(&b, &q, "b");
    let mut c1 = Client::new(&b, "c1");
    let id1 = subscribe(&b, &c1, 1, &q, 10, None);
    let got = c1.drain();
    let first_seq = got[0].1;
    b.get_dest(&q).unwrap().remove_sub(&id1, first_seq, now_ms());
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 10, None);
    let again = c2.drain();
    assert_eq!(again[0].2, 1);
    assert_eq!(again[1].2, 0, "prefetched but never delivered: no increment");
}

#[test]
fn selector_does_not_block_and_keeps_fifo() {
    let b = broker();
    let q = Destination::queue("CORR");
    for i in 1..=4 {
        for c in ["ORD-A", "ORD-B", "ORD-C"] {
            let mut m = msg(&q, &format!("{c}-{i}"));
            m.correlation_id = Some(c.to_string());
            b.deliver(m, true, now_ms()).unwrap();
        }
    }
    let mut c1 = Client::new(&b, "c1");
    let id = subscribe(&b, &c1, 1, &q, 1000, Some("JMSCorrelationID IN ('ORD-A','ORD-C')"));
    let got = texts(&c1.drain());
    assert_eq!(
        got,
        vec!["ORD-A-1", "ORD-C-1", "ORD-A-2", "ORD-C-2", "ORD-A-3", "ORD-C-3", "ORD-A-4", "ORD-C-4"]
    );
    // Ack all, close, and an unfiltered consumer gets exactly the B messages.
    let last = c1.handle.dispatched.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(last, 8);
    let d = b.get_dest(&q).unwrap();
    let snap = d.snapshot();
    assert_eq!(snap.pending, 4);
    d.remove_sub(&id, i64::MAX, now_ms());
    // The closed consumer had 8 unacked messages: they come back first, then the B messages.
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 1000, Some("JMSCorrelationID LIKE 'ORD-B%'"));
    assert_eq!(texts(&c2.drain()), vec!["ORD-B-1", "ORD-B-2", "ORD-B-3", "ORD-B-4"]);
}

#[test]
fn prefetch_window_and_acks() {
    let b = broker();
    let q = Destination::queue("PREFETCH");
    for i in 1..=6 {
        send(&b, &q, &format!("m{i}"));
    }
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 2, None);
    let got = c.drain();
    assert_eq!(texts(&got), vec!["m1", "m2"]);
    // DELIVERED ack frees the window without removing.
    ack(&b, &q, &id, ack_type::DELIVERED, None, got[1].1, false);
    let more = c.drain();
    assert_eq!(texts(&more), vec!["m3", "m4"]);
    // STANDARD ack is cumulative up to m4.
    ack(&b, &q, &id, ack_type::STANDARD, None, more[1].1, false);
    let rest = c.drain();
    assert_eq!(texts(&rest), vec!["m5", "m6"]);
    let snap = b.get_dest(&q).unwrap().snapshot();
    assert_eq!(snap.stats.dequeued, 4);
    assert_eq!(snap.inflight, 2);
}

#[test]
fn poison_persistent_to_dlq_non_persistent_discarded() {
    let b = broker();
    let q = Destination::queue("POISON");
    let mut p = msg(&q, "persistent");
    p.persistent = true;
    b.deliver(p, true, now_ms()).unwrap();
    send(&b, &q, "transient");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack(&b, &q, &id, ack_type::POISON, Some(got[0].1), got[0].1, true);
    ack(&b, &q, &id, ack_type::POISON, Some(got[1].1), got[1].1, false);
    let dlq = b.get_dest(&Destination::queue(DLQ_NAME)).unwrap();
    let (n, entries) = dlq.page(0, 10);
    assert_eq!(n, 1);
    let props = entries[0].properties().unwrap();
    assert!(props.get("dlqDeliveryFailureCause").is_some());
    assert_eq!(b.get_dest(&q).unwrap().snapshot().stats.discarded, 1);
}

#[test]
fn expired_messages_are_deleted_not_dispatched() {
    let b = broker();
    let q = Destination::queue("EXPIRE");
    let mut m = msg(&q, "short");
    m.expiration = now_ms() + 50;
    b.deliver(m, true, now_ms()).unwrap();
    send(&b, &q, "long");
    std::thread::sleep(std::time::Duration::from_millis(80));
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.sweep_expired(now_ms(), 100), 1);
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 10, None);
    assert_eq!(texts(&c.drain()), vec!["long"]);
    assert_eq!(d.snapshot().stats.expired, 1);
    assert!(
        b.get_dest(&Destination::queue(DLQ_NAME)).is_none(),
        "never moved to a DLQ"
    );
}

#[test]
fn already_expired_on_arrival_is_rejected_by_options() {
    let b = broker();
    let q = Destination::queue("EXPIRED-ARRIVAL");
    let mut m = msg(&q, "old");
    m.expiration = now_ms() - 1000;
    assert!(!b.apply_expiry_options(&mut m, now_ms()));
}

#[test]
fn expiry_options() {
    let b = broker_with(|f| {
        f.expiry.default_ttl_ms = 5000;
        f.expiry.ttl_ceiling_ms = 2000;
    });
    let q = Destination::queue("OPTS");
    let now = now_ms();
    let mut m = msg(&q, "x");
    m.timestamp = now;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, now + 2000, "default TTL capped by the ceiling");
    let clock = broker_with(|f| f.expiry.use_broker_clock = true);
    let mut m = msg(&q, "y");
    m.timestamp = now - 3_600_000;
    m.expiration = m.timestamp + 10_000;
    assert!(clock.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, now + 10_000);
}

#[test]
fn topic_fanout_selector_and_eviction() {
    let b = broker_with(|f| f.broker.topic_max_pending_per_consumer = 3);
    let topic = Destination::new(DestKind::Topic, "T");
    let mut all = Client::new(&b, "c1");
    let mut sel = Client::new(&b, "c2");
    let mut slow = Client::new(&b, "c3");
    subscribe(&b, &all, 1, &topic, 100, None);
    subscribe(&b, &sel, 1, &topic, 100, Some("JMSCorrelationID = 'x'"));
    subscribe(&b, &slow, 1, &topic, 1, None);
    for i in 1..=6 {
        let mut m = msg(&topic, &format!("t{i}"));
        if i % 2 == 0 {
            m.correlation_id = Some("x".into());
        }
        b.deliver(m, true, now_ms()).unwrap();
    }
    assert_eq!(texts(&all.drain()).len(), 6);
    assert_eq!(texts(&sel.drain()), vec!["t2", "t4", "t6"]);
    // Slow consumer: 1 inflight, at most 3 pending (oldest evicted).
    assert_eq!(texts(&slow.drain()), vec!["t1"]);
    let snap = b.get_dest(&topic).unwrap().snapshot();
    assert_eq!(snap.stats.discarded, 2);
}

#[test]
fn topic_without_subscribers_discards() {
    let b = broker();
    let topic = Destination::new(DestKind::Topic, "EMPTY");
    send(&b, &topic, "lost");
    assert_eq!(b.get_dest(&topic).unwrap().message_count(), 0);
}

#[test]
fn browser_snapshot_then_end_marker() {
    let b = broker();
    let q = Destination::queue("BROWSE");
    send(&b, &q, "a");
    send(&b, &q, "b");
    let mut c = Client::new(&b, "c1");
    let id = c.consumer_id(9);
    let d = b.get_dest(&q).unwrap();
    d.add_sub(
        SubSpec {
            id,
            conn: c.handle.clone(),
            prefetch: 100,
            selector: None,
            no_local: false,
            browser: true,
        },
        now_ms(),
    );
    let got = c.drain();
    assert_eq!(texts(&got), vec!["a", "b"]);
    assert!(got.last().unwrap().0.is_none(), "end-of-browse marker");
    assert_eq!(d.snapshot().pending, 2, "browsing does not consume");
}

#[test]
fn pull_with_prefetch_zero() {
    let b = broker();
    let q = Destination::queue("PULL");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 0, None);
    send(&b, &q, "a");
    assert!(c.drain().is_empty(), "nothing without a pull");
    let d = b.get_dest(&q).unwrap();
    assert!(d.pull(&id, 0, now_ms()).is_none());
    assert_eq!(texts(&c.drain()), vec!["a"]);
    // receiveNoWait on an empty queue answers with a null dispatch at once.
    d.pull(&id, -1, now_ms());
    let got = c.drain();
    assert_eq!(got.len(), 1);
    assert!(got[0].0.is_none());
    // A timed pull asks for a timer.
    assert!(d.pull(&id, 500, now_ms()).is_some());
}

#[test]
fn duplicates_are_ignored() {
    let b = broker();
    let q = Destination::queue("DUP");
    let m = msg(&q, "once");
    b.deliver(m.clone(), true, now_ms()).unwrap();
    b.deliver(m, true, now_ms()).unwrap();
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 1);
}

#[test]
fn memory_limit_rejects_and_recovers() {
    let b = broker_with(|f| f.broker.max_memory_mb = 1);
    let q = Destination::queue("MEM");
    let big = "x".repeat(400 * 1024);
    send(&b, &q, &big);
    send(&b, &q, &big);
    let err = b.deliver(msg(&q, &big), true, now_ms());
    assert!(err.is_err(), "third 400 KB message exceeds 1 MB");
    // Consume everything: memory drops and sends are accepted again.
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack(&b, &q, &id, ack_type::STANDARD, None, got.last().unwrap().1, false);
    drop(got);
    assert!(b.deliver(msg(&q, "small"), true, now_ms()).is_ok());
}

#[test]
fn broker_compresses_large_bodies() {
    let b = broker_with(|f| f.broker.compress_threshold_kb = 32);
    let q = Destination::queue("ZIP");
    let mut m = msg(&q, "");
    m.content = Some(Bytes::from(vec![b'a'; 40 * 1024]));
    b.compress(&mut m);
    assert!(m.compressed);
    let mut small = msg(&q, "");
    small.content = Some(Bytes::from(vec![b'a'; 32 * 1024]));
    b.compress(&mut small);
    assert!(!small.compressed, "exactly at the threshold is not compressed");
}

#[test]
fn delete_destination_rules() {
    let b = broker();
    let q = Destination::queue("DEL");
    send(&b, &q, "a");
    let c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    assert!(b.delete_dest(&q).is_err(), "refused while consumers are active");
    b.get_dest(&q).unwrap().remove_sub(&id, -1, now_ms());
    assert_eq!(b.delete_dest(&q), Ok(true));
    assert!(b.get_dest(&q).is_none());
}

fn tx_ack(b: &Broker, dest: &Destination, consumer: &ConsumerId, last: i64) -> MessageAck {
    let mid = MessageId {
        text_view: None,
        producer_id: Some(producer()),
        producer_sequence_id: 0,
        broker_sequence_id: last,
    };
    let a = MessageAck {
        header: Header::default(),
        destination: Some(dest.clone()),
        transaction_id: Some(TransactionId::Local {
            value: 1,
            connection_id: None,
        }),
        consumer_id: Some(consumer.clone()),
        ack_type: ack_type::STANDARD,
        first_message_id: None,
        last_message_id: Some(mid),
        message_count: 1,
        poison_cause: None,
    };
    b.get_dest(dest).unwrap().ack(&a, true, now_ms());
    a
}

#[test]
fn transacted_ack_then_close_then_commit_consumes() {
    let b = broker();
    let q = Destination::queue("TX.COMMIT");
    send(&b, &q, "a");
    send(&b, &q, "b");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    let mut a = tx_ack(&b, &q, &id, got[1].1);
    let d = b.get_dest(&q).unwrap();
    d.remove_sub(&id, -1, now_ms());
    assert_eq!(d.snapshot().pending, 0, "transacted-acked messages stay reserved");
    a.transaction_id = None;
    d.ack(&a, false, now_ms());
    assert_eq!(d.snapshot().stats.dequeued, 2);
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 10, None);
    assert!(c2.drain().is_empty(), "nothing is redelivered after commit");
}

#[test]
fn transacted_ack_then_close_then_rollback_returns() {
    let b = broker();
    let q = Destination::queue("TX.ROLLBACK");
    send(&b, &q, "a");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    tx_ack(&b, &q, &id, got[0].1);
    let d = b.get_dest(&q).unwrap();
    d.remove_sub(&id, -1, now_ms());
    d.release_reserved(&id, now_ms());
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 10, None);
    let again = c2.drain();
    assert_eq!(texts(&again), vec!["a"]);
    assert_eq!(again[0].2, 1);
}

// ---------------------------------------------------------------------------
// Helpers for the tests below
// ---------------------------------------------------------------------------

impl Client {
    /// Received dispatches as they arrived.
    fn dispatches(&mut self) -> Vec<MessageDispatch> {
        let mut v = Vec::new();
        while let Ok(o) = self.rx.try_recv() {
            if let Out::Cmd(Command::MessageDispatch(md)) = o {
                v.push(md);
            }
        }
        v
    }
}

fn ack_with(b: &Broker, dest: &Destination, consumer: &ConsumerId, kind: u8, first: Option<i64>, last: i64) {
    ack(b, dest, consumer, kind, first, last, false);
}

fn sub_spec(c: &Client, n: i64, prefetch: i32) -> SubSpec {
    SubSpec {
        id: c.consumer_id(n),
        conn: c.handle.clone(),
        prefetch,
        selector: None,
        no_local: false,
        browser: false,
    }
}

fn ttl_msg(dest: &Destination, body: &str, ttl_ms: i64) -> Message {
    let mut m = msg(dest, body);
    m.expiration = now_ms() + ttl_ms;
    m
}

fn accounted(m: &Message) -> u64 {
    m.content_len() as u64 + m.properties_len() as u64 + mqrust::broker::entry::ENTRY_OVERHEAD
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

// ---------------------------------------------------------------------------
// Queues: FIFO, acks, temporary destinations, auto-delete, duplicates, memory
// ---------------------------------------------------------------------------

#[test]
fn priority_does_not_reorder() {
    let b = broker();
    let q = Destination::queue("PRIORITY");
    for (i, p) in [1u8, 9, 4, 0, 7].iter().enumerate() {
        let mut m = msg(&q, &format!("m{i}"));
        m.priority = *p;
        b.deliver(m, true, now_ms()).unwrap();
    }
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 100, None);
    assert_eq!(texts(&c.drain()), vec!["m0", "m1", "m2", "m3", "m4"]);
}

#[test]
fn fifo_with_many_consumers_round_robin() {
    let b = broker();
    let q = Destination::queue("RR3");
    let mut clients: Vec<Client> = ["a", "b", "c"].iter().map(|n| Client::new(&b, n)).collect();
    for c in &clients {
        subscribe(&b, c, 1, &q, 1000, None);
    }
    for i in 0..30 {
        send(&b, &q, &format!("m{i:02}"));
    }
    let mut all = Vec::new();
    for c in clients.iter_mut() {
        let got = texts(&c.drain());
        assert_eq!(got.len(), 10, "even round-robin distribution");
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(got, sorted);
        all.extend(got);
    }
    all.sort();
    assert_eq!(all, (0..30).map(|i| format!("m{i:02}")).collect::<Vec<_>>());
}

#[test]
fn individual_ack_removes_only_that_message() {
    let b = broker();
    let q = Destination::queue("ACK.INDIVIDUAL");
    for i in 1..=3 {
        send(&b, &q, &format!("m{i}"));
    }
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack_with(&b, &q, &id, ack_type::INDIVIDUAL, Some(got[1].1), got[1].1);
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().stats.dequeued, 1);
    assert_eq!(d.snapshot().inflight, 2);
    // Closing returns m1 and m3 only.
    d.remove_sub(&id, -1, now_ms());
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 10, None);
    assert_eq!(texts(&c2.drain()), vec!["m1", "m3"]);
}

#[test]
fn unmatched_ack_is_cumulative_like_standard() {
    let b = broker();
    let q = Destination::queue("ACK.UNMATCHED");
    for i in 1..=3 {
        send(&b, &q, &format!("m{i}"));
    }
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack_with(&b, &q, &id, ack_type::UNMATCHED, None, got[1].1);
    let snap = b.get_dest(&q).unwrap().snapshot();
    assert_eq!(snap.stats.dequeued, 2);
    assert_eq!(snap.inflight, 1);
}

#[test]
fn redelivered_ack_increments_counter_without_removing() {
    let b = broker();
    let q = Destination::queue("ACK.REDELIVERED");
    send(&b, &q, "a");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack_with(&b, &q, &id, ack_type::REDELIVERED, Some(got[0].1), got[0].1);
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().inflight, 1, "still inflight");
    // Returned as never delivered (lastDelivered before it): only the REDELIVERED increment counts.
    d.remove_sub(&id, 0, now_ms());
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 10, None);
    assert_eq!(c2.drain()[0].2, 1);
}

#[test]
fn expired_ack_deletes_and_resumes_dispatch() {
    let b = broker();
    let q = Destination::queue("ACK.EXPIRED");
    send(&b, &q, "a");
    send(&b, &q, "b");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 1, None);
    let got = c.drain();
    assert_eq!(texts(&got), vec!["a"]);
    let before = b.memory.used();
    ack_with(&b, &q, &id, ack_type::EXPIRED, Some(got[0].1), got[0].1);
    assert_eq!(texts(&c.drain()), vec!["b"], "the freed slot is used at once");
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().stats.expired, 1);
    assert_eq!(d.snapshot().stats.dequeued, 0);
    assert!(b.memory.used() < before, "memory released");
    assert!(b.get_dest(&Destination::queue(DLQ_NAME)).is_none());
}

#[test]
fn cumulative_ack_after_reinsertion_follows_dispatch_order() {
    let b = broker();
    let q = Destination::queue("ACK.REINSERT");
    for i in 1..=3 {
        send(&b, &q, &format!("m{i}"));
    }
    let mut a = Client::new(&b, "a");
    let ida = subscribe(&b, &a, 1, &q, 2, None);
    assert_eq!(texts(&a.drain()), vec!["m1", "m2"]);
    let mut c = Client::new(&b, "c");
    let idc = subscribe(&b, &c, 1, &q, 10, None);
    assert_eq!(texts(&c.drain()), vec!["m3"]);
    // A closes: m1 and m2 return in front and go to C after m3.
    b.get_dest(&q).unwrap().remove_sub(&ida, -1, now_ms());
    let second = c.drain();
    assert_eq!(texts(&second), vec!["m1", "m2"]);
    // A cumulative ack up to m1 covers m3 and m1 (dispatch order), not m2.
    ack_with(&b, &q, &idc, ack_type::STANDARD, None, second[0].1);
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().stats.dequeued, 2);
    d.remove_sub(&idc, -1, now_ms());
    let mut e = Client::new(&b, "e");
    subscribe(&b, &e, 1, &q, 10, None);
    assert_eq!(texts(&e.drain()), vec!["m2"]);
}

#[test]
fn ack_for_unknown_consumer_is_ignored() {
    let b = broker();
    let q = Destination::queue("ACK.UNKNOWN");
    send(&b, &q, "a");
    let c = Client::new(&b, "c1");
    ack_with(&b, &q, &c.consumer_id(77), ack_type::STANDARD, None, 1);
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 1);
}

#[test]
fn temporary_queue_is_deleted_with_its_owner_and_memory_released() {
    let b = broker();
    let mut owner = Client::new(&b, "owner");
    let tq = Destination::new(DestKind::TempQueue, "ID:owner:1:1");
    let base = b.memory.used();
    let d = b.get_or_create(&tq, Some(owner.handle.id));
    for i in 0..5 {
        send(&b, &tq, &format!("r{i}"));
    }
    // One of them inflight to a consumer of the owner when the connection goes away.
    d.add_sub(sub_spec(&owner, 1, 1), now_ms());
    assert_eq!(texts(&owner.drain()), vec!["r0"]);
    assert!(b.memory.used() > base);
    b.drop_temp_destinations(owner.handle.id);
    assert!(b.get_dest(&tq).is_none());
    assert_eq!(b.memory.used(), base, "messages of the temporary queue released");
    // A send to the deleted temporary queue is refused and does not recreate it.
    assert!(b.deliver(msg(&tq, "late"), true, now_ms()).is_err());
    assert!(b.get_dest(&tq).is_none());
}

#[test]
fn temp_destination_advisories() {
    let b = broker();
    let mut watcher_q = Client::new(&b, "wq");
    let mut watcher_t = Client::new(&b, "wt");
    let advisory_q = Destination::new(DestKind::Topic, mqrust::broker::ADVISORY_TEMP_QUEUE);
    let advisory_t = Destination::new(DestKind::Topic, mqrust::broker::ADVISORY_TEMP_TOPIC);
    let existing = Destination::new(DestKind::TempQueue, "ID:x:1:1");
    b.get_or_create(&existing, Some(99));
    b.add_advisory_sub(watcher_q.handle.clone(), watcher_q.consumer_id(1), advisory_q.clone());
    b.add_advisory_sub(watcher_t.handle.clone(), watcher_t.consumer_id(1), advisory_t);
    let info = |md: &MessageDispatch| match md.message.as_ref().unwrap().data_structure.as_ref() {
        Some(DataStructure::DestinationInfo(di)) => (di.destination.clone().unwrap(), di.operation_type),
        other => panic!("not a DestinationInfo advisory: {other:?}"),
    };
    // A new advisory consumer first learns the existing temporary queues.
    let got = watcher_q.dispatches();
    assert_eq!(got.len(), 1);
    assert_eq!(info(&got[0]), (existing.clone(), dest_op::ADD));
    assert_eq!(got[0].destination.as_ref(), Some(&advisory_q));
    // Creation and deletion are announced only on the matching advisory topic.
    let tq = Destination::new(DestKind::TempQueue, "ID:x:1:2");
    b.get_or_create(&tq, Some(99));
    b.temp_advisory(&tq, dest_op::ADD);
    b.drop_temp_destinations(99);
    let got: Vec<_> = watcher_q.dispatches().iter().map(info).collect();
    assert!(got.contains(&(tq.clone(), dest_op::ADD)));
    assert!(got.contains(&(tq, dest_op::REMOVE)));
    assert!(got.contains(&(existing, dest_op::REMOVE)));
    assert!(
        watcher_t.dispatches().is_empty(),
        "temporary queues are not announced on the TempTopic advisory"
    );
    // Normal destinations produce no advisories.
    send(&b, &Destination::queue("PLAIN"), "x");
    assert!(watcher_q.dispatches().is_empty());
}

#[test]
fn idle_destinations_are_deleted_except_dlq_and_busy_ones() {
    let b = broker_with(|f| f.broker.auto_delete_empty_after_secs = 1);
    let idle = Destination::queue("IDLE");
    let busy = Destination::queue("BUSY");
    let dlq = Destination::queue(DLQ_NAME);
    b.get_or_create(&idle, None);
    b.get_or_create(&dlq, None);
    send(&b, &busy, "kept");
    let c = Client::new(&b, "c1");
    let watched = Destination::queue("WATCHED");
    subscribe(&b, &c, 1, &watched, 10, None);
    sleep_ms(1100);
    b.delete_idle_destinations(1);
    assert!(b.get_dest(&idle).is_none(), "empty and unused for 1 s");
    assert!(b.get_dest(&busy).is_some(), "holds a message");
    assert!(b.get_dest(&watched).is_some(), "has a consumer");
    assert!(b.get_dest(&dlq).is_some(), "the DLQ is never deleted automatically");
    // Used again: created again, empty.
    send(&b, &idle, "again");
    assert_eq!(b.get_dest(&idle).unwrap().snapshot().pending, 1);
}

#[test]
fn async_send_dropped_by_memory_limit_is_counted() {
    let b = broker_with(|f| f.broker.max_memory_mb = 1);
    let q = Destination::queue("MEM.ASYNC");
    let big = "x".repeat(600 * 1024);
    send(&b, &q, &big);
    assert!(b.deliver(msg(&q, &big), false, now_ms()).is_err());
    assert!(b.memory_limited());
    assert_eq!(b.stats.dropped_async.load(std::sync::atomic::Ordering::Relaxed), 1);
    let snap = b.get_dest(&q).unwrap().snapshot();
    assert_eq!(snap.pending, 1);
    assert_eq!(
        snap.stats.discarded, 1,
        "a dropped asynchronous message is counted as discarded"
    );
    // A synchronous refusal is reported to the sender, not counted as discarded.
    let r = b.deliver(msg(&q, &big), true, now_ms());
    assert!(matches!(
        r,
        Err(mqrust::broker::Rejection::Error {
            class: "javax.jms.ResourceAllocationException",
            ..
        })
    ));
    assert_eq!(b.get_dest(&q).unwrap().snapshot().stats.discarded, 1);
}

#[test]
fn memory_limit_hysteresis_and_returned_messages() {
    let b = broker_with(|f| f.broker.max_memory_mb = 1);
    let q = Destination::queue("MEM.HYST");
    let limit = 1024 * 1024u64;
    // 20 messages of 50 KB fill about 98% of the limit; the 21st does not fit.
    let body = "y".repeat(50 * 1024);
    for _ in 0..20 {
        send(&b, &q, &body);
    }
    assert!(b.deliver(msg(&q, &body), true, now_ms()).is_err(), "over the limit");
    assert!(b.memory_limited());
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 100, None);
    let got = c.drain();
    assert_eq!(got.len(), 20, "consumers are still served while limited");
    // One consumed: about 93% of the limit, still refused.
    ack_with(&b, &q, &id, ack_type::STANDARD, None, got[0].1);
    assert!(b.memory.used() > limit * 9 / 10);
    assert!(
        b.deliver(msg(&q, "tiny"), true, now_ms()).is_err(),
        "still limited above 90%"
    );
    // Messages returning to pending are never refused.
    b.get_dest(&q).unwrap().remove_sub(&id, -1, now_ms());
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 19);
    // Below 90%: accepted again.
    let mut c2 = Client::new(&b, "c2");
    let id2 = subscribe(&b, &c2, 1, &q, 100, None);
    let got = c2.drain();
    ack_with(&b, &q, &id2, ack_type::STANDARD, None, got[0].1);
    assert!(b.memory.used() < limit * 9 / 10);
    assert!(b.deliver(msg(&q, "tiny"), true, now_ms()).is_ok());
    assert!(!b.memory_limited());
}

#[test]
fn dlq_move_is_not_refused_by_the_memory_limit() {
    let b = broker_with(|f| f.broker.max_memory_mb = 1);
    let q = Destination::queue("MEM.DLQ");
    let mut p = msg(&q, &"z".repeat(500 * 1024));
    p.persistent = true;
    b.deliver(p, true, now_ms()).unwrap();
    send(&b, &q, &"z".repeat(400 * 1024));
    assert!(b.deliver(msg(&q, &"z".repeat(200 * 1024)), true, now_ms()).is_err());
    assert!(b.memory_limited());
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack_with(&b, &q, &id, ack_type::POISON, Some(got[0].1), got[0].1);
    assert_eq!(b.get_dest(&Destination::queue(DLQ_NAME)).unwrap().snapshot().pending, 1);
}

#[test]
fn no_memory_limit_by_default() {
    let b = broker();
    assert_eq!(b.memory.limit, 0);
    let q = Destination::queue("MEM.NOLIMIT");
    let big = "w".repeat(4 * 1024 * 1024);
    for _ in 0..4 {
        b.deliver(msg(&q, &big), true, now_ms()).unwrap();
    }
    assert!(!b.memory_limited());
}

#[test]
fn memory_accounting_up_and_down() {
    let b = broker();
    let q = Destination::queue("MEM.ACCOUNT");
    let base = b.memory.used();
    let mut total = 0;
    for i in 0..100 {
        let m = msg(&q, &format!("{i:01024}"));
        total += accounted(&m);
        b.deliver(m, true, now_ms()).unwrap();
    }
    assert_eq!(b.memory.used() - base, total);
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 1000, None);
    let got = c.drain();
    ack_with(&b, &q, &id, ack_type::STANDARD, None, got.last().unwrap().1);
    assert_eq!(b.memory.used(), base);
}

#[test]
fn duplicate_window_released_for_closed_connections() {
    let b = broker();
    let q = Destination::queue("DUP.RELEASE");
    let m = msg(&q, "once");
    b.deliver(m.clone(), true, now_ms()).unwrap();
    b.deliver(m.clone(), true, now_ms()).unwrap();
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 1);
    assert_eq!(b.stats.duplicates.load(std::sync::atomic::Ordering::Relaxed), 1);
    // The producer's connection closes: its windows are forgotten everywhere.
    b.release_producer_audits(&producer().connection_id);
    b.deliver(m, true, now_ms()).unwrap();
    assert_eq!(b.get_dest(&q).unwrap().snapshot().pending, 2);
}

// ---------------------------------------------------------------------------
// Broker-generated IDs
// ---------------------------------------------------------------------------

#[test]
fn broker_ids_follow_the_activemq_format_and_are_unique() {
    let b = broker();
    let id = b.broker_id.value.to_string();
    // ID:<host>-<port>-<timestamp>-<n>:<n>
    let rest = id.strip_prefix("ID:").expect("ID: prefix");
    let (head, seq) = rest.rsplit_once(':').unwrap();
    assert_eq!(seq, "1", "first id of the broker generator");
    let parts: Vec<&str> = head.rsplitn(4, '-').collect();
    assert_eq!(parts.len(), 4, "{id}");
    assert!(parts[0].parse::<u64>().is_ok(), "instance counter in {id}");
    assert!(
        parts[1].parse::<i64>().unwrap() > 1_600_000_000_000,
        "timestamp in {id}"
    );
    assert_eq!(parts[2], "61616", "port in {id}");
    assert!(b.generate_id().starts_with(&format!("ID:{head}:")));
    let g = mqrust::broker::IdGenerator::new("host", 61616, now_ms());
    let other = mqrust::broker::IdGenerator::new("host", 61616, now_ms());
    assert_ne!(g.seed(), other.seed(), "two generators never share a seed");
    let mut seen = std::collections::HashSet::with_capacity(1_000_000);
    for _ in 0..1_000_000 {
        assert!(seen.insert(g.generate_id()));
    }
    assert_eq!(seen.len(), 1_000_000);
}

// ---------------------------------------------------------------------------
// Topics
// ---------------------------------------------------------------------------

#[test]
fn topic_fan_out_in_publish_order_and_late_subscriber() {
    let b = broker();
    let t = Destination::new(DestKind::Topic, "FANOUT");
    let mut subs: Vec<Client> = ["a", "b", "c", "d"].iter().map(|n| Client::new(&b, n)).collect();
    for c in &subs {
        subscribe(&b, c, 1, &t, 1000, None);
    }
    for i in 0..20 {
        send(&b, &t, &format!("e{i:02}"));
    }
    let expected: Vec<String> = (0..20).map(|i| format!("e{i:02}")).collect();
    for c in subs.iter_mut() {
        assert_eq!(texts(&c.drain()), expected);
    }
    let mut late = Client::new(&b, "late");
    subscribe(&b, &late, 1, &t, 1000, None);
    assert!(
        late.drain().is_empty(),
        "a late subscriber gets nothing published before it"
    );
    send(&b, &t, "new");
    assert_eq!(texts(&late.drain()), vec!["new"]);
}

#[test]
fn topic_message_shared_once_in_memory() {
    let b = broker();
    let t = Destination::new(DestKind::Topic, "SHARED");
    let mut subs: Vec<Client> = ["a", "b", "c"].iter().map(|n| Client::new(&b, n)).collect();
    let ids: Vec<ConsumerId> = subs.iter().map(|c| subscribe(&b, c, 1, &t, 10, None)).collect();
    let base = b.memory.used();
    let m = msg(&t, &"s".repeat(10_000));
    let size = accounted(&m);
    b.deliver(m, true, now_ms()).unwrap();
    assert_eq!(b.memory.used() - base, size, "one copy for three subscribers");
    let seqs: Vec<i64> = subs.iter_mut().map(|c| c.drain()[0].1).collect();
    ack_with(&b, &t, &ids[0], ack_type::STANDARD, None, seqs[0]);
    ack_with(&b, &t, &ids[1], ack_type::STANDARD, None, seqs[1]);
    assert_eq!(b.memory.used() - base, size, "still referenced by the third subscriber");
    // The last holder ends its subscription: released.
    b.get_dest(&t).unwrap().remove_sub(&ids[2], -1, now_ms());
    assert_eq!(b.memory.used(), base);
}

#[test]
fn topic_eviction_with_slow_and_fast_subscribers() {
    let b = broker_with(|f| f.broker.topic_max_pending_per_consumer = 100);
    let t = Destination::new(DestKind::Topic, "EVICT");
    let mut fast = Client::new(&b, "fast");
    let mut slow = Client::new(&b, "slow");
    subscribe(&b, &fast, 1, &t, 1000, None);
    subscribe(&b, &slow, 1, &t, 10, None);
    for i in 1..=200 {
        send(&b, &t, &format!("{i}"));
    }
    assert_eq!(texts(&fast.drain()).len(), 200);
    assert_eq!(
        texts(&slow.drain()),
        (1..=10).map(|i| i.to_string()).collect::<Vec<_>>()
    );
    let snap = b.get_dest(&t).unwrap().snapshot();
    assert_eq!(snap.stats.discarded, 90);
    assert_eq!(snap.pending, 100, "messages 101-200 pending for the slow subscriber");
}

#[test]
fn topic_eviction_disabled_with_zero() {
    let b = broker_with(|f| f.broker.topic_max_pending_per_consumer = 0);
    let t = Destination::new(DestKind::Topic, "NOEVICT");
    let mut slow = Client::new(&b, "slow");
    subscribe(&b, &slow, 1, &t, 1, None);
    for i in 0..20_000 {
        send(&b, &t, &format!("{i}"));
    }
    assert_eq!(texts(&slow.drain()).len(), 1);
    let snap = b.get_dest(&t).unwrap().snapshot();
    assert_eq!(snap.stats.discarded, 0);
    assert_eq!(snap.pending, 19_999);
}

#[test]
fn topic_no_local_skips_own_connection() {
    let b = broker();
    let t = Destination::new(DestKind::Topic, "NOLOCAL");
    // `producer()` belongs to connection "ID:prod-1-1-1:1".
    let mut own = Client::new(&b, "ID:prod-1-1-1:1");
    let mut other = Client::new(&b, "other");
    let d = b.get_or_create(&t, None);
    for c in [&own, &other] {
        let mut spec = sub_spec(c, 1, 100);
        spec.no_local = true;
        d.add_sub(spec, now_ms());
    }
    send(&b, &t, "mine");
    assert!(own.drain().is_empty(), "noLocal hides messages of the same connection");
    assert_eq!(texts(&other.drain()), vec!["mine"]);
}

#[test]
fn topic_poison_persistent_to_dlq_non_persistent_discarded() {
    let b = broker();
    let t = Destination::new(DestKind::Topic, "TPOISON");
    let mut c = Client::new(&b, "c1");
    let mut other = Client::new(&b, "c2");
    let id = subscribe(&b, &c, 1, &t, 10, None);
    subscribe(&b, &other, 1, &t, 10, None);
    let mut p = msg(&t, "persistent");
    p.persistent = true;
    b.deliver(p, true, now_ms()).unwrap();
    send(&b, &t, "transient");
    let got = c.drain();
    ack_with(&b, &t, &id, ack_type::POISON, Some(got[0].1), got[0].1);
    ack_with(&b, &t, &id, ack_type::POISON, Some(got[1].1), got[1].1);
    let dlq = b.get_dest(&Destination::queue(DLQ_NAME)).unwrap();
    let (n, entries) = dlq.page(0, 10);
    assert_eq!(n, 1);
    assert_eq!(entries[0].msg.original_destination.as_ref(), Some(&t));
    assert_eq!(b.get_dest(&t).unwrap().snapshot().stats.discarded, 1);
    assert_eq!(
        texts(&other.drain()),
        vec!["persistent", "transient"],
        "other subscribers unaffected"
    );
}

#[test]
fn temporary_topic_deleted_with_owner() {
    let b = broker();
    let mut owner = Client::new(&b, "owner");
    let tt = Destination::new(DestKind::TempTopic, "ID:owner:1:9");
    let d = b.get_or_create(&tt, Some(owner.handle.id));
    let base = b.memory.used();
    d.add_sub(sub_spec(&owner, 1, 1), now_ms());
    for i in 0..5 {
        send(&b, &tt, &format!("{i}"));
    }
    assert_eq!(texts(&owner.drain()), vec!["0"]);
    assert!(b.memory.used() > base);
    b.drop_temp_destinations(owner.handle.id);
    assert!(b.get_dest(&tt).is_none());
    assert_eq!(b.memory.used(), base, "pending and inflight messages released");
}

// ---------------------------------------------------------------------------
// Expiration
// ---------------------------------------------------------------------------

#[test]
fn expiry_options_client_clock_ahead_and_behind() {
    let b = broker_with(|f| f.expiry.use_broker_clock = true);
    let q = Destination::queue("CLOCK");
    let now = now_ms();
    let hour = 3_600_000;
    for skew in [hour, -hour] {
        let mut m = msg(&q, "x");
        m.timestamp = now + skew;
        m.expiration = m.timestamp + 10_000;
        assert!(
            b.apply_expiry_options(&mut m, now),
            "not expired on arrival (skew {skew})"
        );
        assert_eq!(m.timestamp, now);
        assert_eq!(m.expiration, now + 10_000);
    }
    // Without the option a client one hour behind produces an already expired message.
    let plain = broker();
    let mut m = msg(&q, "x");
    m.timestamp = now - hour;
    m.expiration = m.timestamp + 10_000;
    assert!(!plain.apply_expiry_options(&mut m, now));
}

#[test]
fn expiry_options_default_ttl_ceiling_and_defaults() {
    let q = Destination::queue("TTLOPTS");
    let now = now_ms();
    // Default settings change nothing.
    let b = broker();
    let mut m = msg(&q, "x");
    m.timestamp = now - 5;
    m.expiration = now + 60_000;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!((m.timestamp, m.expiration), (now - 5, now + 60_000));
    let mut m = msg(&q, "x");
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, 0);
    // Ceiling on an explicit TTL; no expiration without a default TTL.
    let b = broker_with(|f| f.expiry.ttl_ceiling_ms = 1000);
    let mut m = msg(&q, "x");
    m.timestamp = now;
    m.expiration = now + 60_000;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, now + 1000);
    let mut m = msg(&q, "x");
    m.timestamp = now;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, 0);
    // Default TTL from the timestamp, explicit TTL kept, arrival time when the timestamp is 0.
    let b = broker_with(|f| f.expiry.default_ttl_ms = 500);
    let mut m = msg(&q, "x");
    m.timestamp = now - 100;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, now - 100 + 500);
    let mut m = msg(&q, "x");
    m.timestamp = now;
    m.expiration = now + 60_000;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, now + 60_000);
    let mut m = msg(&q, "x");
    m.timestamp = 0;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.expiration, now + 500);
}

#[test]
fn broker_clock_with_default_ttl_stamps_arrival_time() {
    let b = broker_with(|f| {
        f.expiry.use_broker_clock = true;
        f.expiry.default_ttl_ms = 500;
    });
    let q = Destination::queue("CLOCKTTL");
    let now = now_ms();
    let mut m = msg(&q, "x");
    m.timestamp = now - 3_600_000;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!(m.timestamp, now);
    assert_eq!(m.expiration, now + 500);
    // A message without timestamp but with an expiration is left unchanged.
    let mut m = msg(&q, "y");
    m.timestamp = 0;
    m.expiration = now + 7_000;
    assert!(b.apply_expiry_options(&mut m, now));
    assert_eq!((m.timestamp, m.expiration), (0, now + 7_000));
}

#[test]
fn sweeper_visits_only_destinations_with_expiring_messages() {
    let b = broker();
    let q = Destination::queue("SWEEP.SET");
    send(&b, &Destination::queue("SWEEP.PLAIN"), "no ttl");
    assert_eq!(b.expiring_destinations(), 0, "messages without expiration cost nothing");
    assert_eq!(b.sweep_expired(now_ms(), 10_000), 0);
    let base = b.memory.used();
    let m = ttl_msg(&q, "short", 100);
    let size = accounted(&m);
    b.deliver(m, true, now_ms()).unwrap();
    assert_eq!(b.expiring_destinations(), 1);
    assert_eq!(b.memory.used() - base, size);
    sleep_ms(150);
    assert_eq!(b.sweep_expired(now_ms(), 10_000), 1);
    assert_eq!(b.memory.used(), base, "memory released at once");
    assert_eq!(b.get_dest(&q).unwrap().snapshot().stats.expired, 1);
    assert_eq!(b.expiring_destinations(), 0, "an idle round has nothing to visit");
    assert_eq!(b.sweep_expired(now_ms(), 10_000), 0);
}

#[test]
fn sweeper_keeps_destination_while_expiring_messages_are_inflight() {
    let b = broker();
    let q = Destination::queue("SWEEP.INFLIGHT");
    b.deliver(ttl_msg(&q, "a", 100), true, now_ms()).unwrap();
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    assert_eq!(texts(&c.drain()), vec!["a"]);
    sleep_ms(150);
    b.sweep_expired(now_ms(), 10_000);
    let d = b.get_dest(&q).unwrap();
    assert_eq!(d.snapshot().inflight, 1, "an inflight message is not revoked");
    assert_eq!(b.expiring_destinations(), 1);
    // Returned after its TTL: deleted on reinsertion, then the destination leaves the set.
    d.remove_sub(&id, -1, now_ms());
    assert_eq!(d.snapshot().pending, 0);
    assert_eq!(d.snapshot().stats.expired, 1);
    b.sweep_expired(now_ms(), 10_000);
    assert_eq!(b.expiring_destinations(), 0);
}

#[test]
fn sweeper_rounds_are_bounded() {
    let b = broker();
    let q = Destination::queue("SWEEP.BULK");
    let base = b.memory.used();
    let exp = now_ms() + 50;
    for i in 0..25_000 {
        let mut m = msg(&q, &format!("{i}"));
        m.expiration = exp;
        b.deliver(m, true, now_ms()).unwrap();
    }
    sleep_ms(100);
    let now = now_ms();
    assert_eq!(b.sweep_expired(now, 10_000), 10_000);
    assert_eq!(b.sweep_expired(now, 10_000), 10_000);
    assert_eq!(b.sweep_expired(now, 10_000), 5_000);
    assert_eq!(b.get_dest(&q).unwrap().snapshot().stats.expired, 25_000);
    assert_eq!(b.memory.used(), base);
    assert_eq!(b.expiring_destinations(), 0);
}

#[test]
fn alternating_expired_and_valid_messages_keep_fifo() {
    let b = broker();
    let q = Destination::queue("EXP.ALT");
    for i in 0..10 {
        let m = if i % 2 == 0 {
            ttl_msg(&q, &format!("x{i}"), 30)
        } else {
            msg(&q, &format!("v{i}"))
        };
        b.deliver(m, true, now_ms()).unwrap();
    }
    sleep_ms(60);
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 100, None);
    assert_eq!(texts(&c.drain()), vec!["v1", "v3", "v5", "v7", "v9"]);
    assert_eq!(b.get_dest(&q).unwrap().snapshot().stats.expired, 5);
}

#[test]
fn pull_skips_expired_messages() {
    let b = broker();
    let q = Destination::queue("EXP.PULL");
    b.deliver(ttl_msg(&q, "old", 20), true, now_ms()).unwrap();
    sleep_ms(40);
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 0, None);
    let d = b.get_dest(&q).unwrap();
    assert!(
        d.pull(&id, 500, now_ms()).is_some(),
        "nothing valid: the normal timeout applies"
    );
    assert!(c.drain().is_empty());
    assert_eq!(d.snapshot().stats.expired, 1);
}

#[test]
fn browser_skips_and_deletes_expired_messages() {
    let b = broker();
    let q = Destination::queue("EXP.BROWSE");
    b.deliver(ttl_msg(&q, "gone", 20), true, now_ms()).unwrap();
    send(&b, &q, "a");
    b.deliver(ttl_msg(&q, "later", 150), true, now_ms()).unwrap();
    send(&b, &q, "b");
    sleep_ms(40);
    let mut c = Client::new(&b, "c1");
    let id = c.consumer_id(5);
    let d = b.get_dest(&q).unwrap();
    // Prefetch 1: "later" expires while the browser waits for credit.
    d.add_sub(
        SubSpec {
            id: id.clone(),
            conn: c.handle.clone(),
            prefetch: 1,
            selector: None,
            no_local: false,
            browser: true,
        },
        now_ms(),
    );
    assert_eq!(d.snapshot().stats.expired, 1, "expired before browsing: deleted");
    let first = c.drain();
    assert_eq!(texts(&first), vec!["a"]);
    sleep_ms(150);
    ack_with(&b, &q, &id, ack_type::STANDARD, None, first[0].1);
    assert_eq!(texts(&c.drain()), vec!["b"]);
    assert_eq!(
        d.snapshot().stats.expired,
        2,
        "expired while browsing: skipped and deleted"
    );
    assert_eq!(d.snapshot().pending, 2, "browsing does not consume the valid messages");
}

#[test]
fn expiry_during_rollback_is_not_redelivered() {
    let b = broker();
    let q = Destination::queue("EXP.ROLLBACK");
    b.deliver(ttl_msg(&q, "short", 50), true, now_ms()).unwrap();
    send(&b, &q, "long");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    tx_ack(&b, &q, &id, got[1].1);
    let d = b.get_dest(&q).unwrap();
    d.remove_sub(&id, -1, now_ms());
    sleep_ms(80);
    d.release_reserved(&id, now_ms());
    let mut c2 = Client::new(&b, "c2");
    subscribe(&b, &c2, 1, &q, 10, None);
    assert_eq!(texts(&c2.drain()), vec!["long"]);
    assert_eq!(d.snapshot().stats.expired, 1);
}

#[test]
fn slow_topic_subscriber_expired_message_removed() {
    let b = broker();
    let t = Destination::new(DestKind::Topic, "EXP.TOPIC");
    let mut fast = Client::new(&b, "fast");
    let mut slow = Client::new(&b, "slow");
    subscribe(&b, &fast, 1, &t, 100, None);
    subscribe(&b, &slow, 1, &t, 1, None);
    send(&b, &t, "first");
    b.deliver(ttl_msg(&t, "short", 50), true, now_ms()).unwrap();
    assert_eq!(texts(&fast.drain()), vec!["first", "short"]);
    assert_eq!(texts(&slow.drain()), vec!["first"]);
    sleep_ms(80);
    assert_eq!(b.sweep_expired(now_ms(), 10_000), 1);
    let snap = b.get_dest(&t).unwrap().snapshot();
    assert_eq!(snap.pending, 0, "removed from the slow subscriber's list");
    assert_eq!(snap.inflight, 3, "the fast subscriber keeps its delivered copy");
}

#[test]
fn persistent_and_non_persistent_expire_without_dlq() {
    let b = broker();
    let q = Destination::queue("EXP.BOTH");
    let mut p = ttl_msg(&q, "p", 50);
    p.persistent = true;
    b.deliver(p, true, now_ms()).unwrap();
    b.deliver(ttl_msg(&q, "n", 50), true, now_ms()).unwrap();
    sleep_ms(80);
    assert_eq!(b.sweep_expired(now_ms(), 10_000), 2);
    assert!(
        b.get_dest(&Destination::queue(DLQ_NAME)).is_none(),
        "ActiveMQ.DLQ stays empty"
    );
}

#[test]
fn poisoned_message_with_ttl_expires_in_the_dlq() {
    let b = broker();
    let q = Destination::queue("EXP.DLQ");
    let mut p = ttl_msg(&q, "p", 300);
    p.persistent = true;
    let exp = p.expiration;
    b.deliver(p, true, now_ms()).unwrap();
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    ack_with(&b, &q, &id, ack_type::POISON, Some(got[0].1), got[0].1);
    let dlq = b.get_dest(&Destination::queue(DLQ_NAME)).unwrap();
    assert_eq!(dlq.page(0, 1).1[0].msg.expiration, exp, "expiration kept in the DLQ");
    sleep_ms(350);
    b.sweep_expired(now_ms(), 10_000);
    let snap = dlq.snapshot();
    assert_eq!(snap.pending, 0);
    assert_eq!(snap.stats.expired, 1);
}

#[test]
fn expiry_summary_counts_every_kind_of_expiry() {
    let b = broker();
    let q = Destination::queue("EXP.SUMMARY");
    let d = b.get_or_create(&q, None);
    // On arrival.
    b.expire_before_storing(&d, &ttl_msg(&q, "arrival", -10));
    // One removed by the sweeper (batch of 1), one by the dispatch-time check.
    b.deliver(ttl_msg(&q, "x", 20), true, now_ms()).unwrap();
    b.deliver(ttl_msg(&q, "y", 20), true, now_ms()).unwrap();
    sleep_ms(40);
    assert_eq!(b.sweep_expired(now_ms(), 1), 1);
    send(&b, &q, "acked");
    let mut c = Client::new(&b, "c1");
    let id = subscribe(&b, &c, 1, &q, 10, None);
    let got = c.drain();
    assert_eq!(texts(&got), vec!["acked"]);
    // An EXPIRED ack.
    ack_with(&b, &q, &id, ack_type::EXPIRED, Some(got[0].1), got[0].1);
    assert_eq!(d.snapshot().stats.expired, 4);
    assert_eq!(d.take_expired_since_summary(), 4);
    assert_eq!(d.take_expired_since_summary(), 0, "reset after each summary");
}

#[test]
fn enqueue_out_of_sequence_is_still_dispatched() {
    // Two connections take sequence numbers 1 and 2, but the second one reaches the queue first:
    // the consumer must still receive the message with the lower sequence number.
    let b = broker();
    let q = Destination::queue("RACE");
    let mut c = Client::new(&b, "c1");
    subscribe(&b, &c, 1, &q, 1000, None);
    let d = b.get_or_create(&q, None);
    let entry = |seq: u64, body: &str| {
        let m = msg(&q, body);
        let meta = Meta::new(b.memory.clone(), &m);
        Entry {
            seq,
            msg: Arc::new(m),
            meta,
            redelivery: 0,
        }
    };
    d.enqueue(entry(1_000_002, "second"), now_ms());
    d.enqueue(entry(1_000_001, "first"), now_ms());
    let mut got = texts(&c.drain());
    got.sort();
    assert_eq!(got, vec!["first".to_string(), "second".to_string()]);
}
