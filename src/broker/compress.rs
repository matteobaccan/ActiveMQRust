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

/// Message types the broker may compress.
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

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::with_capacity(data.len() / 2), Compression::fast());
    enc.write_all(data).expect("in-memory write");
    enc.finish().expect("in-memory write")
}

/// Compressed form of `content` for `msg_type`.
pub fn compress_content(msg_type: u8, content: &[u8]) -> Vec<u8> {
    if msg_type == t::ACTIVEMQ_BYTES_MESSAGE {
        let mut out = Vec::with_capacity(content.len() / 2 + 4);
        out.extend_from_slice(&(content.len() as i32).to_be_bytes());
        out.extend_from_slice(&zlib(content));
        out
    } else {
        zlib(content)
    }
}

/// Decompresses at most `limit` bytes of a compressed body (for the admin preview).
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

/// Compresses the message body in place when it is above the threshold and the saving is
/// at least `min_saving_pct`. Returns true when the body was replaced.
pub fn maybe_compress(msg: &mut Message, threshold: u64, min_saving_pct: u64) -> bool {
    if threshold == 0 || msg.compressed || !compressible(msg.msg_type) {
        return false;
    }
    let Some(content) = &msg.content else { return false };
    if (content.len() as u64) <= threshold {
        return false;
    }
    let compressed = compress_content(msg.msg_type, content);
    let keep_limit = content.len() as u64 * (100 - min_saving_pct) / 100;
    if compressed.len() as u64 <= keep_limit {
        msg.content = Some(Bytes::from(compressed));
        msg.compressed = true;
        true
    } else {
        false
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
        assert_eq!(decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, &c, usize::MAX).unwrap(), body);
    }

    #[test]
    fn bytes_has_length_prefix() {
        let body = vec![7u8; 50_000];
        let c = compress_content(t::ACTIVEMQ_BYTES_MESSAGE, &body);
        assert_eq!(&c[..4], &(50_000i32).to_be_bytes());
        assert_eq!(decompress_content(t::ACTIVEMQ_BYTES_MESSAGE, &c, usize::MAX).unwrap(), body);
    }

    #[test]
    fn threshold_and_saving() {
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.content = Some(Bytes::from(vec![b'x'; 1024]));
        assert!(!maybe_compress(&mut m, 1024, 10), "exactly at threshold is not compressed");
        m.content = Some(Bytes::from(vec![b'x'; 1025]));
        assert!(maybe_compress(&mut m, 1024, 10));
        assert!(m.compressed);
        // Incompressible data is kept as is.
        let mut r = Message::new(t::ACTIVEMQ_BYTES_MESSAGE);
        let mut seed = 12345u64;
        let noise: Vec<u8> = (0..4096)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (seed >> 33) as u8
            })
            .collect();
        r.content = Some(Bytes::from(noise));
        assert!(!maybe_compress(&mut r, 1024, 10));
        assert!(!r.compressed);
    }

    #[test]
    fn preview_limit() {
        let body = vec![b'z'; 200_000];
        let c = compress_content(t::ACTIVEMQ_TEXT_MESSAGE, &body);
        assert_eq!(decompress_content(t::ACTIVEMQ_TEXT_MESSAGE, &c, 65536).unwrap().len(), 65536);
    }
}
