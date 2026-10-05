// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Message body compression compatible with the ActiveMQ client
//! (`storeContent()` / `doCompress()` of each `ActiveMQ*Message`).
//!
//! * Text, Map, Object and Stream messages: the content is a zlib stream (`DeflaterOutputStream`).
//! * Bytes messages: 4-byte big-endian uncompressed length, then a zlib stream (`Deflater`).

use bytes::Bytes;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

use crate::openwire::model::Message;
use crate::openwire::types as t;

/// zlib level used by the broker. Level 1 of `zlib-rs` uses static Huffman codes only, which
/// save nothing on base64 text (the usual binary payload inside XML or JSON); level 2 is the
/// lowest level with dynamic Huffman codes: about 25% saved on XML with a base64 payload where
/// level 1 saves under 1%, and a third less output than level 1 on plain text, at a similar speed.
pub const LEVEL: u32 = 2;

/// Message types the broker may compress. This match is the exclusion table: a type that fails
/// the Java verification is removed from it and is then always stored as received.
pub fn compressible(msg_type: u8) -> bool {
    matches!(
        msg_type,
        t::ACTIVEMQ_TEXT_MESSAGE
            | t::ACTIVEMQ_BYTES_MESSAGE
            | t::ACTIVEMQ_MAP_MESSAGE
            | t::ACTIVEMQ_OBJECT_MESSAGE
            | t::ACTIVEMQ_STREAM_MESSAGE
    )
}

fn zlib(data: &[u8], level: u32, out: &mut Vec<u8>) {
    let mut enc = ZlibEncoder::new(out, Compression::new(level));
    enc.write_all(data).expect("in-memory write");
    enc.finish().expect("in-memory write");
}

/// Compressed form of `content` for `msg_type` at `level`.
pub fn compress_content_with_level(msg_type: u8, content: &[u8], level: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() / 2 + 16);
    if msg_type == t::ACTIVEMQ_BYTES_MESSAGE {
        out.extend_from_slice(&(content.len() as i32).to_be_bytes());
    }
    zlib(content, level, &mut out);
    out
}

/// Compressed form of `content` for `msg_type` at the broker's level.
pub fn compress_content(msg_type: u8, content: &[u8]) -> Vec<u8> {
    compress_content_with_level(msg_type, content, LEVEL)
}

/// Decompresses at most `limit` bytes of a compressed body (for the admin preview).
/// Callers detect truncation by asking for one byte more than they show.
pub fn decompress_content(msg_type: u8, content: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let data = if msg_type == t::ACTIVEMQ_BYTES_MESSAGE {
        if content.len() < 4 {
            return Err("compressed bytes body is too short".into());
        }
        &content[4..]
    } else {
        content
    };
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .take(limit as u64)
        .read_to_end(&mut out)
        .map_err(|e| format!("corrupt compressed body: {e}"))?;
    Ok(out)
}

/// What broker-side compression did with one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a candidate: disabled, already compressed, unsupported type or not above the threshold.
    Skipped,
    /// The body was replaced by its compressed form.
    Compressed,
    /// The body was compressed, but the saving was below the minimum: the original was kept.
    Discarded,
}

/// Compresses the message body in place when it is above the threshold and the saving is
/// at least `min_saving_pct`.
pub fn maybe_compress(msg: &mut Message, threshold: u64, min_saving_pct: u64) -> Outcome {
    if threshold == 0 || msg.compressed || !compressible(msg.msg_type) {
        return Outcome::Skipped;
    }
    let Some(content) = &msg.content else {
        return Outcome::Skipped;
    };
    if (content.len() as u64) <= threshold {
        return Outcome::Skipped;
    }
    let compressed = compress_content(msg.msg_type, content);
    let keep_limit = content.len() as u64 * (100 - min_saving_pct) / 100;
    if compressed.len() as u64 <= keep_limit {
        msg.content = Some(Bytes::from(compressed));
        msg.compressed = true;
        // The properties are a slice of the received frame: copy them so the stored message
        // no longer keeps the whole uncompressed frame alive.
        if let Some(p) = &msg.marshalled_properties {
            msg.marshalled_properties = Some(Bytes::copy_from_slice(p));
        }
        Outcome::Compressed
    } else {
        Outcome::Discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip() {
        let body = vec![b'a'; 100_000];
        let c = compress_content(t::ACTIVEMQ_TEXT_MESSAGE, &body);
        assert!(c.len() < body.len() / 10);
        assert_eq!(
            decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, &c, usize::MAX).unwrap(),
            body
        );
    }

    #[test]
    fn bytes_has_length_prefix() {
        let body = vec![7u8; 50_000];
        let c = compress_content(t::ACTIVEMQ_BYTES_MESSAGE, &body);
        assert_eq!(&c[..4], &(50_000i32).to_be_bytes());
        assert_eq!(
            decompress_content(t::ACTIVEMQ_BYTES_MESSAGE, &c, usize::MAX).unwrap(),
            body
        );
    }

    #[test]
    fn threshold_and_saving() {
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.content = Some(Bytes::from(vec![b'x'; 1023]));
        assert_eq!(
            maybe_compress(&mut m, 1024, 10),
            Outcome::Skipped,
            "below the threshold"
        );
        m.content = Some(Bytes::from(vec![b'x'; 1024]));
        assert_eq!(
            maybe_compress(&mut m, 1024, 10),
            Outcome::Skipped,
            "exactly at threshold is not compressed"
        );
        m.content = Some(Bytes::from(vec![b'x'; 1025]));
        assert_eq!(maybe_compress(&mut m, 1024, 10), Outcome::Compressed);
        assert!(m.compressed);
        // Already compressed messages are never compressed again.
        assert_eq!(maybe_compress(&mut m, 1024, 10), Outcome::Skipped);
        // Incompressible data is kept as is.
        let mut r = Message::new(t::ACTIVEMQ_BYTES_MESSAGE);
        let mut seed = 12345u64;
        let noise: Vec<u8> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (seed >> 33) as u8
            })
            .collect();
        r.content = Some(Bytes::from(noise.clone()));
        assert_eq!(maybe_compress(&mut r, 1024, 10), Outcome::Discarded);
        assert!(!r.compressed);
        assert_eq!(r.content.as_deref(), Some(&noise[..]));
    }

    #[test]
    fn threshold_zero_disables() {
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.content = Some(Bytes::from(vec![b'x'; 1024 * 1024]));
        assert_eq!(maybe_compress(&mut m, 0, 10), Outcome::Skipped);
        assert!(!m.compressed);
    }

    #[test]
    fn message_without_body_type_is_never_compressed() {
        let mut m = Message::new(t::ACTIVEMQ_MESSAGE);
        m.content = Some(Bytes::from(vec![b'x'; 100_000]));
        assert_eq!(maybe_compress(&mut m, 1024, 10), Outcome::Skipped);
    }

    #[test]
    fn compressed_message_drops_the_frame() {
        let frame = Bytes::from(vec![b'p'; 200_000]);
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.content = Some(frame.slice(100..150_000));
        m.marshalled_properties = Some(frame.slice(150_000..150_100));
        assert_eq!(maybe_compress(&mut m, 1024, 10), Outcome::Compressed);
        let p = m.marshalled_properties.as_ref().unwrap();
        let range = frame.as_ptr() as usize..frame.as_ptr() as usize + frame.len();
        assert!(
            !range.contains(&(p.as_ptr() as usize)),
            "properties still point into the frame"
        );
        assert_eq!(&p[..], &frame[150_000..150_100]);
    }

    #[test]
    fn preview_limit() {
        let body = vec![b'z'; 200_000];
        let c = compress_content(t::ACTIVEMQ_TEXT_MESSAGE, &body);
        assert_eq!(
            decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, &c, 65536).unwrap().len(),
            65536
        );
    }

    #[test]
    fn zip_bomb_stops_at_the_limit() {
        // 100 MB of zeros compress to about 100 KB; the decoder must stop at the cap.
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..100 {
            enc.write_all(&chunk).unwrap();
        }
        let bomb = enc.finish().unwrap();
        assert!(bomb.len() < 1024 * 1024);
        let out = decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, &bomb, 64 * 1024 + 1).unwrap();
        assert_eq!(out.len(), 64 * 1024 + 1);
    }

    #[test]
    fn corrupt_body_is_an_error() {
        assert!(decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, b"not deflate data at all", 1024).is_err());
        assert!(decompress_content(t::ACTIVEMQ_BYTES_MESSAGE, b"ab", 1024).is_err());
    }
}
