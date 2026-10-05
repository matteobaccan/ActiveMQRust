// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Micro-benchmarks: codec, enqueue + dispatch, compression and selector evaluation.
//! Run with `cargo bench` (release profile).

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::compress::compress_content;
use mqrust::broker::conn::ConnHandle;
use mqrust::broker::destination::SubSpec;
use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::marshal::{ChunkBuf, Decoder, Encoder};
use mqrust::openwire::model::*;
use mqrust::openwire::types as t;
use mqrust::selector::Selector;

fn text_message(size: usize) -> Message {
    let pid = ProducerId { connection_id: Arc::from("ID:bench-1-1-1:1"), session_id: 1, value: 1 };
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.producer_id = Some(pid.clone());
    m.destination = Some(Destination::queue("BENCH.SEL"));
    m.message_id = Some(MessageId { text_view: None, producer_id: Some(pid), producer_sequence_id: 1, broker_sequence_id: 0 });
    m.correlation_id = Some("ORD-A".into());
    m.timestamp = 1;
    m.content = Some(Bytes::from(vec![b'x'; size]));
    m
}

fn codec(c: &mut Criterion) {
    let mut g = c.benchmark_group("codec");
    g.throughput(Throughput::Elements(1));
    let cmd = Command::Message(Box::new(text_message(1024)));
    let enc = Encoder::new(12);
    let frame = enc.frame(&cmd);
    g.bench_function("encode_text_1k", |b| b.iter(|| black_box(enc.frame(black_box(&cmd)))));
    let dec = Decoder::new(12);
    let body = frame.slice(4..);
    g.bench_function("decode_text_1k", |b| b.iter(|| black_box(dec.decode_frame(black_box(body.clone())).unwrap())));
    let big = Command::Message(Box::new(text_message(50 * 1024)));
    g.bench_function("encode_text_50k_zero_copy", |b| {
        b.iter(|| {
            let mut cb = ChunkBuf::new();
            enc.encode_frame_chunks(black_box(&big), &mut cb);
            black_box(cb.take());
        })
    });
    g.finish();
}

fn dispatch(c: &mut Criterion) {
    let cfg = build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap();
    let broker = Broker::new(Arc::new(cfg));
    let q = Destination::queue("BENCH.DISPATCH");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = Arc::new(ConnHandle::new(1, "127.0.0.1:1".parse().unwrap(), tx));
    let d = broker.get_or_create(&q, None);
    d.add_sub(
        SubSpec {
            id: ConsumerId { connection_id: Arc::from("c"), session_id: 1, value: 1 },
            conn: handle,
            prefetch: i32::MAX,
            selector: None,
            no_local: false,
            browser: false,
        },
        now_ms(),
    );
    let mut g = c.benchmark_group("broker");
    g.throughput(Throughput::Elements(1));
    let mut seq = 0i64;
    g.bench_function("enqueue_and_dispatch_1k", |b| {
        b.iter(|| {
            seq += 1;
            let mut m = text_message(1024);
            m.destination = Some(q.clone());
            m.message_id.as_mut().unwrap().producer_sequence_id = seq;
            broker.deliver(m, false, now_ms()).unwrap();
            while rx.try_recv().is_ok() {}
        })
    });
    g.finish();
}

fn compression(c: &mut Criterion) {
    let mut g = c.benchmark_group("compression");
    let body: Vec<u8> = (0..50 * 1024).map(|i| b"<field>value</field>"[i % 20]).collect();
    g.throughput(Throughput::Bytes(body.len() as u64));
    g.bench_function("zlib_level1_50k", |b| b.iter(|| black_box(compress_content(t::ACTIVEMQ_TEXT_MESSAGE, black_box(&body)))));
    g.finish();
}

fn selectors(c: &mut Criterion) {
    let cfg = build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap();
    let broker = Broker::new(Arc::new(cfg));
    let q = Destination::queue("BENCH.SEL");
    broker.deliver(text_message(64), true, now_ms()).unwrap();
    let (_, entries) = broker.get_dest(&q).unwrap().page(0, 1);
    let entry = entries[0].clone();
    let sel = Selector::compile("JMSCorrelationID IN ('ORD-A','ORD-C') AND JMSPriority >= 4").unwrap().unwrap();
    let mut g = c.benchmark_group("selector");
    g.throughput(Throughput::Elements(1));
    g.bench_function("header_selector", |b| b.iter(|| black_box(sel.matches(black_box(&entry)))));
    g.finish();
}

criterion_group!(benches, codec, dispatch, compression, selectors, selective_dispatch);
criterion_main!(benches);

/// Selective dispatch: 10 consumers with disjoint property selectors (`n = '0'` .. `n = '9'`)
/// attached to a queue that already holds 100,000 messages; measures until every message
/// has been dispatched to its consumer.
fn selective_dispatch(c: &mut Criterion) {
    use mqrust::openwire::props::{PrimitiveMap, Value};
    const MESSAGES: usize = 100_000;
    const CONSUMERS: usize = 10;
    let q = Destination::queue("BENCH.SELECTIVE");
    let fill = || {
        let cfg = build(FileConfig::default(), ConfigSource::Defaults, &Overrides::default()).unwrap();
        let broker = Broker::new(Arc::new(cfg));
        for i in 0..MESSAGES {
            let mut m = text_message(64);
            m.destination = Some(q.clone());
            m.message_id.as_mut().unwrap().producer_sequence_id = i as i64 + 1;
            let mut p = PrimitiveMap::new();
            p.set("n", Value::String((i % CONSUMERS).to_string()));
            m.marshalled_properties = Some(p.encode());
            broker.deliver(m, false, now_ms()).unwrap();
        }
        broker
    };
    let mut g = c.benchmark_group("selector");
    g.sample_size(10);
    g.throughput(Throughput::Elements(MESSAGES as u64));
    g.bench_function("dispatch_10_selective_consumers_100k", |b| {
        b.iter_batched(
            fill,
            |broker| {
                let d = broker.get_or_create(&q, None);
                let mut receivers = Vec::with_capacity(CONSUMERS);
                for k in 0..CONSUMERS {
                    let (tx, rx) = mpsc::unbounded_channel();
                    let handle = Arc::new(ConnHandle::new(k as u64 + 1, "127.0.0.1:1".parse().unwrap(), tx));
                    let selector = Selector::compile(&format!("n = '{k}'")).unwrap().map(Arc::new);
                    d.add_sub(
                        SubSpec {
                            id: ConsumerId { connection_id: Arc::from(format!("c{k}")), session_id: 1, value: 1 },
                            conn: handle,
                            prefetch: i32::MAX,
                            selector,
                            no_local: false,
                            browser: false,
                        },
                        now_ms(),
                    );
                    receivers.push(rx);
                }
                let mut received = 0;
                for rx in receivers.iter_mut() {
                    while rx.try_recv().is_ok() {
                        received += 1;
                    }
                }
                assert!(received >= MESSAGES);
                black_box(broker)
            },
            criterion::BatchSize::PerIteration,
        )
    });
    g.finish();
}
