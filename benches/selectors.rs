// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Selector benchmarks: evaluation of a header selector, and dispatch to 10 selective consumers
//! of a queue that holds 100,000 messages.

mod common;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::conn::ConnHandle;
use mqrust::broker::destination::SubSpec;
use mqrust::broker::now_ms;
use mqrust::openwire::model::*;
use mqrust::openwire::props::{PrimitiveMap, Value};
use mqrust::selector::Selector;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn header_selector(c: &mut Criterion) {
    let broker = common::broker();
    let q = Destination::queue("BENCH.SEL");
    broker.deliver(common::text_message(&q, 1, 64), true, now_ms()).unwrap();
    let (_, entries) = broker.get_dest(&q).unwrap().page(0, 1);
    let entry = entries[0].clone();
    let sel = Selector::compile("JMSCorrelationID IN ('ORD-A','ORD-C') AND JMSPriority >= 4").unwrap().unwrap();
    let mut g = c.benchmark_group("selector");
    g.throughput(Throughput::Elements(1));
    g.bench_function("header_selector", |b| b.iter(|| black_box(sel.matches(black_box(&entry)))));
    g.finish();
}

/// Selective dispatch: 10 consumers with disjoint property selectors (`n = '0'` .. `n = '9'`)
/// attached to a queue that already holds 100,000 messages; measures until every message
/// has been dispatched to its consumer.
fn selective_dispatch(c: &mut Criterion) {
    const MESSAGES: usize = 100_000;
    const CONSUMERS: usize = 10;
    let q = Destination::queue("BENCH.SELECTIVE");
    let fill = || {
        let broker = common::broker();
        for i in 0..MESSAGES {
            let mut m = common::text_message(&q, i as i64 + 1, 64);
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

criterion_group!(benches, header_selector, selective_dispatch);
criterion_main!(benches);
