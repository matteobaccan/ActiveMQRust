// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Codec micro-benchmarks: encoding and decoding a 1 KB `ActiveMQTextMessage` with OpenWire
//! version 12 (newest) and 9 (oldest supported), and the zero-copy encoding of a 50 KB body.

mod common;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

use mqrust::openwire::marshal::{ChunkBuf, Decoder, Encoder};
use mqrust::openwire::model::*;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn codec(c: &mut Criterion) {
    let q = Destination::queue("BENCH.CODEC");
    let mut g = c.benchmark_group("codec");
    g.throughput(Throughput::Elements(1));
    let cmd = Command::Message(Box::new(common::text_message(&q, 1, 1024)));
    for version in [12, 9] {
        let enc = Encoder::new(version);
        let dec = Decoder::new(version);
        let body = enc.frame(&cmd).slice(4..);
        g.bench_function(format!("encode_text_1k_v{version}"), |b| b.iter(|| black_box(enc.frame(black_box(&cmd)))));
        g.bench_function(format!("decode_text_1k_v{version}"), |b| {
            b.iter(|| black_box(dec.decode_frame(black_box(body.clone())).unwrap()))
        });
    }
    let big = Command::Message(Box::new(common::text_message(&q, 2, 50 * 1024)));
    let enc = Encoder::new(12);
    let mut cb = ChunkBuf::new();
    g.bench_function("encode_text_50k_zero_copy", |b| {
        b.iter(|| {
            enc.encode_frame_chunks(black_box(&big), &mut cb);
            black_box(cb.take());
        })
    });
    g.finish();
}

criterion_group!(benches, codec);
criterion_main!(benches);
