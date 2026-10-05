// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Hot-path guarantees: allocations per message, no property decoding without selectors,
//! opaque bodies, cross-version dispatch and independent destination locks.

use bytes::Bytes;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::SubSpec;
use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::connection::FrameReader;
use mqrust::openwire::marshal::{ChunkBuf, Decoder, Encoder, LooseCodec, WireCodec};
use mqrust::openwire::model::*;
use mqrust::openwire::props::{self, PrimitiveMap, Value};
use mqrust::openwire::types as t;

// -- counting allocator (counts only on the thread that enabled it) -------------------------

struct Counting;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

fn note_alloc() {
    let _ = ENABLED.try_with(|e| {
        if e.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_alloc();
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_alloc();
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_alloc();
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn counting(on: bool) {
    ENABLED.with(|e| e.set(on));
}

fn allocations() -> u64 {
    COUNT.with(|c| c.get())
}

// -- helpers ----------------------------------------------------------------------------------

fn broker() -> Arc<Broker> {
    Broker::new(Arc::new(
        build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap(),
    ))
}

fn producer() -> ProducerId {
    ProducerId {
        connection_id: Arc::from("ID:hot-path-1-1-1:1"),
        session_id: 1,
        value: 1,
    }
}

fn message(msg_type: u8, dest: &Destination, seq: i64, content: Bytes) -> Message {
    let mut m = Message::new(msg_type);
    m.producer_id = Some(producer());
    m.destination = Some(dest.clone());
    m.message_id = Some(MessageId {
        text_view: None,
        producer_id: Some(producer()),
        producer_sequence_id: seq,
        broker_sequence_id: 0,
    });
    m.timestamp = 1;
    m.content = Some(content);
    let mut p = PrimitiveMap::new();
    p.set("k", Value::Int((seq % 10) as i32));
    m.marshalled_properties = Some(p.encode());
    m
}

struct Consumer {
    id: ConsumerId,
    rx: mpsc::UnboundedReceiver<Out>,
}

fn consumer(b: &Broker, dest: &Destination, n: i64, selector: Option<&str>) -> Consumer {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = Arc::new(ConnHandle::new(b.new_conn_id(), "127.0.0.1:1".parse().unwrap(), tx));
    let id = ConsumerId {
        connection_id: Arc::from("ID:hot-path-consumer"),
        session_id: 1,
        value: n,
    };
    b.get_or_create(dest, None).add_sub(
        SubSpec {
            id: id.clone(),
            conn: handle,
            prefetch: 1000,
            selector: selector.map(|s| Arc::new(mqrust::selector::Selector::compile(s).unwrap().unwrap())),
            no_local: false,
            browser: false,
        },
        now_ms(),
    );
    Consumer { id, rx }
}

fn ack_for(dest: &Destination, c: &ConsumerId, m: &Message) -> MessageAck {
    MessageAck {
        header: Header::default(),
        destination: Some(dest.clone()),
        transaction_id: None,
        consumer_id: Some(c.clone()),
        ack_type: ack_type::STANDARD,
        first_message_id: None,
        last_message_id: m.message_id.clone(),
        message_count: 1,
        poison_cause: None,
    }
}

/// Sends `n` messages of `size` bytes through the receive, store, dispatch and encode path and
/// returns the heap allocations counted per message (acks are excluded).
fn allocations_per_message(size: usize, warmup: usize, n: usize) -> f64 {
    let b = broker();
    let q = Destination::queue(&format!("ALLOC.{size}"));
    let mut c = consumer(&b, &q, 1, None);
    let d = b.get_or_create(&q, None);
    let enc = Encoder::new(12);
    let mut stream = Vec::new();
    for i in 0..(warmup + n) {
        let m = message(
            t::ACTIVEMQ_TEXT_MESSAGE,
            &q,
            i as i64 + 1,
            Bytes::from(vec![b'a'; size]),
        );
        stream.extend_from_slice(&enc.frame(&Command::Message(Box::new(m))));
    }
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let codec = LooseCodec::new(12);
    let mut out = ChunkBuf::new();
    let mut reader = FrameReader::new(&stream[..]);
    let mut start = 0;
    for i in 0..(warmup + n) {
        if i == warmup {
            start = allocations();
        }
        counting(true);
        let frame = rt.block_on(reader.next(i64::MAX)).unwrap().unwrap();
        let Some(Command::Message(m)) = codec.decode(frame).unwrap() else {
            panic!("not a message")
        };
        b.deliver(*m, false, now_ms()).unwrap();
        let Ok(Out::Cmd(cmd)) = c.rx.try_recv() else {
            panic!("no dispatch")
        };
        codec.encode(&cmd, &mut out);
        drop(out.take());
        counting(false);
        let Command::MessageDispatch(md) = cmd else {
            panic!("not a dispatch")
        };
        let msg = md.message.unwrap();
        d.ack(&ack_for(&q, &c.id, &msg), false, now_ms());
    }
    (allocations() - start) as f64 / n as f64
}

/// Measured: 9.0 per message. Receive: the message frame (1) and its promotion to a shared
/// buffer when the body is sliced (1), the decoded `Message` box (1), the producer, message-id
/// and destination strings (3). Store: `Arc<Message>` (1) and the shared accounting record (1).
/// Dispatch: on average 1 for the writer's output buffer. The number does not depend on the
/// body size.
const MAX_ALLOCATIONS_PER_MESSAGE: f64 = 9.0;
/// Occasional growth of queues and maps, amortized over the run.
const GROWTH: f64 = 0.1;

#[test]
fn allocations_per_message_are_bounded_and_size_independent() {
    let small = allocations_per_message(1024, 1_000, 10_000);
    let large = allocations_per_message(100 * 1024, 100, 1_000);
    println!("allocations per message: 1 KB = {small:.2}, 100 KB = {large:.2}");
    assert!(
        small <= MAX_ALLOCATIONS_PER_MESSAGE + GROWTH,
        "1 KB: {small:.2} allocations per message"
    );
    assert!(
        large <= MAX_ALLOCATIONS_PER_MESSAGE + GROWTH,
        "100 KB: {large:.2} allocations per message"
    );
    assert!(
        (small - large).abs() < 0.5,
        "allocations depend on the body size: {small:.2} vs {large:.2}"
    );
}

#[test]
fn no_property_decoding_without_selectors() {
    let b = broker();
    let q = Destination::queue("NOPROPS");
    let mut c = consumer(&b, &q, 1, None);
    let d = b.get_or_create(&q, None);
    let enc = Encoder::new(12);
    let dec = Decoder::new(12);
    let before = props::decode_count();
    for i in 0..10_000 {
        let m = message(
            t::ACTIVEMQ_TEXT_MESSAGE,
            &q,
            i + 1,
            Bytes::from_static(b"\x00\x00\x00\x01x"),
        );
        let frame = enc.frame(&Command::Message(Box::new(m)));
        let Some(Command::Message(m)) = dec.decode_frame(frame.slice(4..)).unwrap() else {
            panic!()
        };
        b.deliver(*m, true, now_ms()).unwrap();
        while let Ok(Out::Cmd(Command::MessageDispatch(md))) = c.rx.try_recv() {
            let m = md.message.unwrap();
            let mut cb = ChunkBuf::new();
            LooseCodec::new(12).encode(
                &Command::MessageDispatch(MessageDispatch {
                    header: Header::default(),
                    consumer_id: Some(c.id.clone()),
                    destination: Some(q.clone()),
                    message: Some(m.clone()),
                    redelivery_counter: 0,
                }),
                &mut cb,
            );
            d.ack(&ack_for(&q, &c.id, &m), false, now_ms());
        }
    }
    // The counter is global: no other test of this binary uses a selector. A selector run checks\n    // that the counter does count.
    let without = props::decode_count() - before;
    let q2 = Destination::queue("WITHPROPS");
    let mut s = consumer(&b, &q2, 2, Some("k = 3"));
    let before = props::decode_count();
    for i in 0..100 {
        b.deliver(
            message(t::ACTIVEMQ_TEXT_MESSAGE, &q2, i + 1, Bytes::from_static(b"x")),
            true,
            now_ms(),
        )
        .unwrap();
    }
    let with = props::decode_count() - before;
    while s.rx.try_recv().is_ok() {}
    assert!(with >= 100, "a selector decodes properties ({with})");
    assert_eq!(without, 0, "properties decoded {without} times without any selector");
}

#[test]
fn undecodable_map_body_is_delivered_intact() {
    let b = broker();
    let q = Destination::queue("OPAQUE.MAP");
    let mut c = consumer(&b, &q, 1, None);
    let garbage = Bytes::from_static(b"\xff\xfe this is not a marshalled map \x00\x01");
    let m = message(t::ACTIVEMQ_MAP_MESSAGE, &q, 1, garbage.clone());
    let frame = Encoder::new(12).frame(&Command::Message(Box::new(m)));
    let Some(Command::Message(m)) = Decoder::new(12).decode_frame(frame.slice(4..)).unwrap() else {
        panic!()
    };
    b.deliver(*m, true, now_ms()).unwrap();
    let Ok(Out::Cmd(cmd)) = c.rx.try_recv() else {
        panic!("no dispatch")
    };
    let mut cb = ChunkBuf::new();
    LooseCodec::new(12).encode(&cmd, &mut cb);
    let wire: Vec<u8> = cb.take().into_iter().flat_map(|x| x.to_vec()).collect();
    match Decoder::new(12).decode_frame(Bytes::from(wire).slice(4..)).unwrap() {
        Some(Command::MessageDispatch(md)) => {
            assert_eq!(md.message.unwrap().content.as_ref().unwrap(), &garbage)
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn received_body_is_not_copied() {
    let q = Destination::queue("NOCOPY");
    let m = message(t::ACTIVEMQ_BYTES_MESSAGE, &q, 1, Bytes::from(vec![5u8; 1024 * 1024]));
    let frame = Encoder::new(12).frame(&Command::Message(Box::new(m))).slice(4..);
    let Some(Command::Message(m)) = Decoder::new(12).decode_frame(frame.clone()).unwrap() else {
        panic!()
    };
    let b = broker();
    b.deliver(*m, true, now_ms()).unwrap();
    let (_, stored) = b.get_dest(&q).unwrap().page(0, 1);
    let content = stored[0].msg.content.as_ref().unwrap();
    let range = frame.as_ptr() as usize..frame.as_ptr() as usize + frame.len();
    assert!(
        range.contains(&(content.as_ptr() as usize)),
        "stored content does not point into the received frame"
    );
}

#[test]
fn producer_v12_consumer_v9() {
    let b = broker();
    let q = Destination::queue("XVERSION");
    let mut c = consumer(&b, &q, 1, None);
    let body = Bytes::from((0..5000u32).map(|i| (i % 251) as u8).collect::<Vec<u8>>());
    let mut m = message(t::ACTIVEMQ_BYTES_MESSAGE, &q, 1, body.clone());
    m.jmsx_group_first_for_consumer = true;
    let frame = LooseCodec::new(12);
    let mut cb = ChunkBuf::new();
    frame.encode(&Command::Message(Box::new(m)), &mut cb);
    let wire: Vec<u8> = cb.take().into_iter().flat_map(|x| x.to_vec()).collect();
    let Some(Command::Message(m)) = LooseCodec::new(12).decode(Bytes::from(wire).slice(4..)).unwrap() else {
        panic!()
    };
    b.deliver(*m, true, now_ms()).unwrap();
    let Ok(Out::Cmd(cmd)) = c.rx.try_recv() else {
        panic!("no dispatch")
    };
    let v9 = LooseCodec::new(9);
    let mut cb = ChunkBuf::new();
    v9.encode(&cmd, &mut cb);
    let wire: Vec<u8> = cb.take().into_iter().flat_map(|x| x.to_vec()).collect();
    let size = i32::from_be_bytes(wire[..4].try_into().unwrap()) as usize;
    assert_eq!(size, wire.len() - 4);
    match v9.decode(Bytes::from(wire).slice(4..)).unwrap() {
        Some(Command::MessageDispatch(md)) => {
            let m = md.message.unwrap();
            assert_eq!(m.content.as_ref().unwrap(), &body);
            assert_eq!(m.message_id.as_ref().unwrap().producer_sequence_id, 1);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn held_lock_on_one_queue_does_not_block_another() {
    let b = broker();
    let a = Destination::queue("LOCK.A");
    let other = Destination::queue("LOCK.B");
    let da = b.get_or_create(&a, None);
    b.get_or_create(&other, None);
    let guard = da.hold_lock();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let b2 = b.clone();
    let sender = std::thread::spawn(move || {
        for i in 0..100 {
            b2.deliver(
                message(t::ACTIVEMQ_TEXT_MESSAGE, &other, i + 1, Bytes::from_static(b"x")),
                true,
                now_ms(),
            )
            .unwrap();
        }
        done_tx.send(()).unwrap();
    });
    let finished = done_rx.recv_timeout(Duration::from_secs(10)).is_ok();
    drop(guard);
    sender.join().unwrap();
    assert!(finished, "sends to queue B waited for the lock of queue A");
    assert_eq!(b.get_dest(&Destination::queue("LOCK.B")).unwrap().message_count(), 100);
}
