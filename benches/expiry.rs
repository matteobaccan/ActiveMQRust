// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Expiry sweeper: one sweep pass over a queue of 1,000,000 messages with random TTLs, and
//! enqueue + dispatch of 1 KB messages on another queue with and without the sweeper running
//! in the background (the two dispatch results must not differ measurably).

mod common;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mqrust::broker::{now_ms, Broker};
use mqrust::openwire::model::*;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const HELD: usize = 1_000_000;

/// Fills `EXPIRY.HOLD` with messages expiring at random times over the next 10 minutes.
fn fill(broker: &Broker) {
    let q = Destination::queue("EXPIRY.HOLD");
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let now = now_ms();
    for i in 0..HELD {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mut m = common::text_message(&q, i as i64 + 1, 16);
        m.timestamp = now;
        m.expiration = now + 1 + (x % 600_000) as i64;
        broker.deliver(m, false, now).unwrap();
    }
}

fn dispatch_loop(c: &mut Criterion, broker: &Arc<Broker>, name: &str) {
    let q = Destination::queue(&format!("EXPIRY.DISPATCH.{name}"));
    let d = broker.get_or_create(&q, None);
    let mut sub = common::consumer(broker, &d, 1, 1000, None);
    let mut seq = 0i64;
    c.benchmark_group("expiry")
        .throughput(Throughput::Elements(1))
        .bench_function(name, |b| {
            b.iter(|| {
                seq += 1;
                broker
                    .deliver(common::text_message(&q, seq, 1024), false, now_ms())
                    .unwrap();
                common::drain_and_ack(&d, &mut sub);
            })
        });
}

fn expiry(c: &mut Criterion) {
    let broker = common::broker();
    fill(&broker);
    {
        let mut g = c.benchmark_group("expiry");
        g.sample_size(10);
        g.bench_function("sweep_pass_1m_messages", |b| {
            b.iter(|| broker.sweep_expired(now_ms(), 10_000))
        });
        g.finish();
    }
    dispatch_loop(c, &broker, "dispatch_1k_without_sweeper");
    let stop = Arc::new(AtomicBool::new(false));
    let sweeper = {
        let (broker, stop) = (broker.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                broker.sweep_expired(now_ms(), 10_000);
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    dispatch_loop(c, &broker, "dispatch_1k_with_sweeper");
    stop.store(true, Ordering::Relaxed);
    sweeper.join().unwrap();
}

criterion_group!(benches, expiry);
criterion_main!(benches);
