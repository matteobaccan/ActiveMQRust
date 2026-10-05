// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Broker-side compression throughput at the broker's zlib level, for bodies of 32 KB, 64 KB,
//! 1 MB and 10 MB: XML with a base64 payload (like the Java benchmark's `XmlPayload`) and plain text.

mod common;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

use mqrust::broker::compress::compress_content;
use mqrust::openwire::types as t;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn compression(c: &mut Criterion) {
    let mut g = c.benchmark_group("compression");
    for (label, size) in [("32k", 32 * 1024), ("64k", 64 * 1024), ("1m", 1024 * 1024), ("10m", 10 * 1024 * 1024)] {
        if size >= 1024 * 1024 {
            g.sample_size(10);
        }
        for (kind, body) in [("xml_base64", common::payload::xml_base64(size, 7)), ("text", common::payload::plain_text(size, 3))] {
            g.throughput(Throughput::Bytes(body.len() as u64));
            g.bench_function(format!("{kind}_{label}"), |b| {
                b.iter(|| black_box(compress_content(t::ACTIVEMQ_TEXT_MESSAGE, black_box(&body))))
            });
        }
    }
    g.finish();
}

criterion_group!(benches, compression);
criterion_main!(benches);
