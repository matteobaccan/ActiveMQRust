// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Broker semantics without the network: FIFO, round-robin, redelivery, selectors, acks,
//! DLQ, expiry, topics, browsers, pull, duplicates, memory limit and compression.

use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::SubSpec;
use mqrust::broker::{now_ms, Broker, DLQ_NAME};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::model::*;
use mqrust::openwire::types as t;
use mqrust::selector::Selector;

fn broker_with(f: impl FnOnce(&mut FileConfig)) -> Arc<Broker> {
    let mut fc = FileConfig::default();
    f(&mut fc);
    Broker::new(Arc::new(build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap()))
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
        ConsumerId { connection_id: Arc::from(self.conn), session_id: 1, value: n }
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
    ProducerId { connection_id: Arc::from("ID:prod-1-1-1:1"), session_id: 1, value: 1 }
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

fn ack(b: &Broker, dest: &Destination, consumer: &ConsumerId, kind: u8, first: Option<i64>, last: i64, persistent_poison: bool) {
    let _ = persistent_poison;
    let mid = |seq: i64| MessageId { text_view: None, producer_id: Some(producer()), producer_sequence_id: 0, broker_sequence_id: seq };
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
    assert_eq!(got, vec!["ORD-A-1", "ORD-C-1", "ORD-A-2", "ORD-C-2", "ORD-A-3", "ORD-C-3", "ORD-A-4", "ORD-C-4"]);
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
    assert!(b.get_dest(&Destination::queue(DLQ_NAME)).is_none(), "never moved to a DLQ");
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
        SubSpec { id, conn: c.handle.clone(), prefetch: 100, selector: None, no_local: false, browser: true },
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
    let b = broker();
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
    let mid = MessageId { text_view: None, producer_id: Some(producer()), producer_sequence_id: 0, broker_sequence_id: last };
    let a = MessageAck {
        header: Header::default(),
        destination: Some(dest.clone()),
        transaction_id: Some(TransactionId::Local { value: 1, connection_id: None }),
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
