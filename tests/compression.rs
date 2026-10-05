// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Message compression: golden vectors from the real Java client for the five body types,
//! the choice of the zlib level, and the configuration defaults.

#[path = "../benches/common/payload.rs"]
mod payload;

use bytes::Bytes;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::compress::{self, compress_content, decompress_content, Outcome};
use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::SubSpec;
use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::marshal::{ChunkBuf, Decoder, Encoder, LooseCodec, WireCodec};
use mqrust::openwire::model::*;
use mqrust::openwire::types as t;

fn broker_with(f: impl FnOnce(&mut FileConfig)) -> Arc<Broker> {
    let mut fc = FileConfig::default();
    f(&mut fc);
    Broker::new(Arc::new(
        build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap(),
    ))
}

/// Frame bodies (without the size prefix) of a recorded stream.
fn frames(data: &[u8]) -> Vec<Bytes> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        let size = i32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        out.push(Bytes::copy_from_slice(&data[i + 4..i + 4 + size]));
        i += 4 + size;
    }
    assert_eq!(i, data.len());
    out
}

fn decode_message(frame: &Bytes) -> Message {
    match Decoder::new(12).decode_frame(frame.clone()).unwrap() {
        Some(Command::Message(m)) => *m,
        other => panic!("not a message: {other:?}"),
    }
}

/// Golden vectors written by `mqrust-acceptance.jar compression-golden` (activemq-client
/// 5.18): for each type, the frame a client with `useCompression=true` marshals, then the frame
/// of the same body without compression.
#[test]
fn golden_vectors_of_the_five_types() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("compression");
    let types = [
        ("text", t::ACTIVEMQ_TEXT_MESSAGE),
        ("bytes", t::ACTIVEMQ_BYTES_MESSAGE),
        ("map", t::ACTIVEMQ_MAP_MESSAGE),
        ("stream", t::ACTIVEMQ_STREAM_MESSAGE),
        ("object", t::ACTIVEMQ_OBJECT_MESSAGE),
    ];
    for (name, msg_type) in types {
        let data = std::fs::read(dir.join(format!("{name}.bin"))).unwrap_or_else(|e| panic!("{name}.bin: {e}"));
        let f = frames(&data);
        assert_eq!(f.len(), 2, "{name}: compressed and plain frames");
        let zipped = decode_message(&f[0]);
        let plain = decode_message(&f[1]);
        assert_eq!(zipped.msg_type, msg_type, "{name}");
        assert!(zipped.compressed && !plain.compressed, "{name}: compressed flags");
        // The codec re-encodes the client's frames byte for byte.
        assert_eq!(
            &Encoder::new(12).frame(&Command::Message(Box::new(zipped.clone())))[4..],
            &f[0][..],
            "{name}"
        );
        let zc = zipped.content.clone().unwrap();
        let pc = plain.content.clone().unwrap();
        // The client's compressed form decodes to exactly the uncompressed form.
        assert_eq!(
            decompress_content(msg_type, &zc, usize::MAX).unwrap(),
            pc.to_vec(),
            "{name}: client body"
        );
        // The broker's form has the same layout and decodes to the same bytes.
        let ours = compress_content(msg_type, &pc);
        assert_eq!(
            decompress_content(msg_type, &ours, usize::MAX).unwrap(),
            pc.to_vec(),
            "{name}: broker body"
        );
        let header = if msg_type == t::ACTIVEMQ_BYTES_MESSAGE { 4 } else { 0 };
        assert_eq!(&ours[..header], &zc[..header], "{name}: length prefix");
        assert_eq!(ours[header], 0x78, "{name}: zlib header");
        assert_eq!(zc[header], 0x78, "{name}: zlib header of the client");
        // Broker-side compression of the plain message produces the client's message shape.
        let mut m = plain.clone();
        assert_eq!(
            compress::maybe_compress(&mut m, 1024, 10),
            Outcome::Compressed,
            "{name}"
        );
        assert_eq!(m.compressed, zipped.compressed);
        assert_eq!(m.marshalled_properties, plain.marshalled_properties);
        assert_eq!(m.message_id, plain.message_id);
        // A client-compressed message passes through the broker unchanged.
        let b = broker_with(|_| {});
        let q = zipped.destination.clone().unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = Arc::new(ConnHandle::new(1, "127.0.0.1:1".parse().unwrap(), tx));
        b.get_or_create(&q, None).add_sub(
            SubSpec {
                id: ConsumerId {
                    connection_id: Arc::from("golden"),
                    session_id: 1,
                    value: 1,
                },
                conn: handle,
                prefetch: 10,
                selector: None,
                no_local: false,
                browser: false,
            },
            now_ms(),
        );
        let mut incoming = zipped.clone();
        assert_eq!(
            b.compress(&mut incoming),
            Outcome::Skipped,
            "{name}: never compressed twice"
        );
        b.deliver(incoming, true, now_ms()).unwrap();
        let Ok(Out::Cmd(cmd)) = rx.try_recv() else {
            panic!("{name}: no dispatch")
        };
        let mut cb = ChunkBuf::new();
        LooseCodec::new(12).encode(&cmd, &mut cb);
        let wire: Vec<u8> = cb.take().into_iter().flat_map(|x| x.to_vec()).collect();
        match Decoder::new(12).decode_frame(Bytes::from(wire).slice(4..)).unwrap() {
            Some(Command::MessageDispatch(md)) => {
                let m = md.message.unwrap();
                assert!(m.compressed, "{name}");
                assert_eq!(
                    m.content.as_ref().unwrap(),
                    &zc,
                    "{name}: content changed by the broker"
                );
            }
            other => panic!("{name}: unexpected {other:?}"),
        }
    }
}

fn zlib_size(data: &[u8], level: u32) -> usize {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(level));
    e.write_all(data).unwrap();
    e.finish().unwrap().len()
}

/// The broker's level is the lowest one that saves at least 15% on XML with a base64 payload
/// (the payload of the Java benchmark). Level 1 of `zlib-rs` uses static Huffman codes only
/// and saves almost nothing on base64 text. Sizes only: speed is measured by the benchmarks.
#[test]
fn level_is_the_lowest_with_a_real_saving_on_xml_base64() {
    let xml = payload::xml_base64(1024 * 1024, 7);
    let text = payload::plain_text(1024 * 1024, 3);
    assert_eq!(xml.len(), 1024 * 1024);
    let mut chosen = None;
    for level in 1..=6 {
        let x = zlib_size(&xml, level);
        let p = zlib_size(&text, level);
        println!(
            "level {level}: xml+base64 {:.1}% of original, plain text {:.1}% of original",
            x as f64 * 100.0 / xml.len() as f64,
            p as f64 * 100.0 / text.len() as f64
        );
        if chosen.is_none() && x * 100 <= xml.len() * 85 {
            chosen = Some(level);
        }
    }
    assert_eq!(
        chosen,
        Some(compress::LEVEL),
        "the lowest level saving 15% on xml+base64"
    );
    assert!(
        zlib_size(&text, compress::LEVEL) < zlib_size(&text, 1),
        "the level also helps plain text"
    );
}

#[test]
fn defaults_threshold_zero_and_discard_counter() {
    let defaults = broker_with(|_| {});
    assert_eq!(defaults.cfg.compress_threshold_bytes, 0);
    assert_eq!(defaults.cfg.compress_min_saving_pct, 10);
    let b = broker_with(|f| f.broker.compress_threshold_kb = 32);
    assert_eq!(b.cfg.compress_threshold_bytes, 32 * 1024);
    let q = Destination::queue("ZIP.CFG");
    let text = |content: Bytes| {
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.destination = Some(q.clone());
        m.content = Some(content);
        m
    };
    let big = Bytes::from(payload::plain_text(1024 * 1024, 1));
    // Default: a 1 MB compressible body is stored as received.
    let mut m = text(big.clone());
    assert_eq!(defaults.compress(&mut m), Outcome::Skipped);
    assert_eq!(m.content.as_ref(), Some(&big));
    // Disabled explicitly: the same.
    let off = broker_with(|f| f.broker.compress_threshold_kb = 0);
    let mut m = text(big.clone());
    assert_eq!(off.compress(&mut m), Outcome::Skipped);
    assert_eq!(m.content.as_ref(), Some(&big));
    // Threshold 32 KB: compressed and counted.
    let mut m = text(big.clone());
    assert_eq!(b.compress(&mut m), Outcome::Compressed);
    // Incompressible: compressed, then discarded and counted.
    let mut x: u64 = 1;
    let noise: Vec<u8> = (0..100_000)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (x >> 56) as u8
        })
        .collect();
    let mut m = text(Bytes::from(noise));
    assert_eq!(b.compress(&mut m), Outcome::Discarded);
    assert!(!m.compressed);
    use std::sync::atomic::Ordering::Relaxed;
    assert_eq!(b.stats.compressed.load(Relaxed), 1);
    assert_eq!(b.stats.compress_discarded.load(Relaxed), 1);
    // Minimum saving of 0%: any result not larger than the original is kept.
    let keep_all = broker_with(|f| {
        f.broker.compress_threshold_kb = 32;
        f.broker.compress_min_saving_pct = 0;
    });
    let mut m = text(Bytes::from(vec![b'z'; 40 * 1024]));
    assert_eq!(keep_all.compress(&mut m), Outcome::Compressed);
}
