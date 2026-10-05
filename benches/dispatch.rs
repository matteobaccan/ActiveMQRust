// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Broker core micro-benchmarks: enqueue, dispatch and acknowledge of 1 KB messages on a
//! queue with 1 consumer and with 10 consumers (round-robin), without the network.

mod common;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};

use mqrust::broker::now_ms;
use mqrust::openwire::model::*;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn dispatch(c: &mut Criterion) {
    let mut g = c.benchmark_group("dispatch");
    g.throughput(Throughput::Elements(1));
    for consumers in [1usize, 10] {
        let broker = common::broker();
        let q = Destination::queue(&format!("BENCH.DISPATCH.{consumers}"));
        let d = broker.get_or_create(&q, None);
        let mut subs: Vec<_> = (0..consumers).map(|n| common::consumer(&broker, &d, n as i64 + 1, 1000, None)).collect();
        let mut seq = 0i64;
        g.bench_function(format!("enqueue_dispatch_ack_1k_{consumers}_consumers"), |b| {
            b.iter(|| {
                seq += 1;
                broker.deliver(common::text_message(&q, seq, 1024), false, now_ms()).unwrap();
                for s in subs.iter_mut() {
                    common::drain_and_ack(&d, s);
                }
            })
        });
    }
    g.finish();
}

criterion_group!(benches, dispatch);
criterion_main!(benches);
