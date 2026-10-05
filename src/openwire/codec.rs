// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Loose-encoding primitives of OpenWire (Java `DataInput`/`DataOutput` semantics).

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::fmt;
use std::sync::Arc;

/// Error raised while decoding or encoding OpenWire data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError(pub String);

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CodecError {}

pub type CodecResult<T> = Result<T, CodecError>;

pub fn err<T>(msg: impl Into<String>) -> CodecResult<T> {
    Err(CodecError(msg.into()))
}

/// Reads big-endian primitives from a frame body without copying byte sequences.
pub struct Reader {
    buf: Bytes,
}

impl Reader {
    pub fn new(buf: Bytes) -> Self {
        Reader { buf }
    }

    pub fn remaining(&self) -> usize {
        self.buf.remaining()
    }

    fn need(&self, n: usize) -> CodecResult<()> {
        if self.buf.remaining() < n {
            err(format!("unexpected end of frame: need {n} bytes, have {}", self.buf.remaining()))
        } else {
            Ok(())
        }
    }

    pub fn u8(&mut self) -> CodecResult<u8> {
        self.need(1)?;
        Ok(self.buf.get_u8())
    }

    pub fn bool(&mut self) -> CodecResult<bool> {
        Ok(self.u8()? != 0)
    }

    pub fn u16(&mut self) -> CodecResult<u16> {
        self.need(2)?;
        Ok(self.buf.get_u16())
    }

    pub fn i16(&mut self) -> CodecResult<i16> {
        self.need(2)?;
        Ok(self.buf.get_i16())
    }

    pub fn i32(&mut self) -> CodecResult<i32> {
        self.need(4)?;
        Ok(self.buf.get_i32())
    }

    pub fn i64(&mut self) -> CodecResult<i64> {
        self.need(8)?;
        Ok(self.buf.get_i64())
    }

    pub fn f32(&mut self) -> CodecResult<f32> {
        self.need(4)?;
        Ok(self.buf.get_f32())
    }

    pub fn f64(&mut self) -> CodecResult<f64> {
        self.need(8)?;
        Ok(self.buf.get_f64())
    }

    /// Takes `n` bytes as a zero-copy slice of the frame.
    pub fn take(&mut self, n: usize) -> CodecResult<Bytes> {
        self.need(n)?;
        Ok(self.buf.split_to(n))
    }

    /// `DataInput.readUTF`: u16 length followed by modified UTF-8.
    pub fn utf(&mut self) -> CodecResult<String> {
        let len = self.u16()? as usize;
        let raw = self.take(len)?;
        decode_modified_utf8(&raw)
    }

    /// Loose `String`: presence flag then `readUTF`.
    pub fn opt_string(&mut self) -> CodecResult<Option<String>> {
        if self.bool()? {
            Ok(Some(self.utf()?))
        } else {
            Ok(None)
        }
    }

    /// Loose string stored as a shared `Arc<str>`.
    pub fn opt_arc_str(&mut self) -> CodecResult<Option<Arc<str>>> {
        Ok(self.opt_string()?.map(Arc::from))
    }

    /// Loose byte sequence / byte array: presence flag, i32 length, bytes.
    pub fn opt_bytes(&mut self) -> CodecResult<Option<Bytes>> {
        if self.bool()? {
            let len = self.i32()?;
            if len < 0 {
                return err(format!("negative byte sequence length {len}"));
            }
            Ok(Some(self.take(len as usize)?))
        } else {
            Ok(None)
        }
    }
}

/// Byte sequences at least this long are passed by reference instead of copied.
pub const ZERO_COPY_MIN: usize = 2048;

/// Writes big-endian primitives into a growable buffer. With a sink, large shared byte
/// sequences are not copied: the buffer is split and the shared `Bytes` is appended to the sink.
pub struct Writer<'a> {
    pub buf: &'a mut BytesMut,
    sink: Option<&'a mut Vec<Bytes>>,
}

impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut BytesMut) -> Self {
        Writer { buf, sink: None }
    }

    pub fn with_sink(buf: &'a mut BytesMut, sink: &'a mut Vec<Bytes>) -> Self {
        Writer { buf, sink: Some(sink) }
    }

    /// Loose byte sequence from shared bytes, zero-copy when a sink is present.
    pub fn shared_bytes(&mut self, b: Option<&Bytes>) {
        match b {
            Some(b) if b.len() >= ZERO_COPY_MIN && self.sink.is_some() => {
                self.bool(true);
                self.i32(b.len() as i32);
                let head = self.buf.split().freeze();
                let sink = self.sink.as_mut().unwrap();
                sink.push(head);
                sink.push(b.clone());
            }
            Some(b) => self.opt_bytes(Some(b)),
            None => self.bool(false),
        }
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.put_u8(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.put_u8(v as u8);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.put_u16(v);
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.put_i32(v);
    }

    pub fn i64(&mut self, v: i64) {
        self.buf.put_i64(v);
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.put_f32(v);
    }

    pub fn f64(&mut self, v: f64) {
        self.buf.put_f64(v);
    }

    pub fn raw(&mut self, v: &[u8]) {
        self.buf.put_slice(v);
    }

    /// `DataOutput.writeUTF`. Strings longer than 65535 encoded bytes are an error in Java;
    /// here they are truncated at a character boundary to keep the stream valid.
    pub fn utf(&mut self, s: &str) {
        let encoded = encode_modified_utf8(s);
        let len = encoded.len().min(u16::MAX as usize);
        let len = utf_safe_len(&encoded, len);
        self.u16(len as u16);
        self.raw(&encoded[..len]);
    }

    pub fn opt_string(&mut self, s: Option<&str>) {
        match s {
            Some(s) => {
                self.bool(true);
                self.utf(s);
            }
            None => self.bool(false),
        }
    }

    pub fn opt_bytes(&mut self, b: Option<&[u8]>) {
        match b {
            Some(b) => {
                self.bool(true);
                self.i32(b.len() as i32);
                self.raw(b);
            }
            None => self.bool(false),
        }
    }
}

/// Finds the largest length <= `max` that does not split a multi-byte sequence.
fn utf_safe_len(encoded: &[u8], max: usize) -> usize {
    let mut len = max;
    while len > 0 && len < encoded.len() && (encoded[len] & 0xC0) == 0x80 {
        len -= 1;
    }
    len
}

/// Encodes a string as Java modified UTF-8 (NUL as C0 80, supplementary characters as surrogate pairs).
pub fn encode_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        let c = unit as u32;
        if (0x0001..=0x007F).contains(&c) {
            out.push(c as u8);
        } else if c <= 0x07FF {
            out.push((0xC0 | ((c >> 6) & 0x1F)) as u8);
            out.push((0x80 | (c & 0x3F)) as u8);
        } else {
            out.push((0xE0 | ((c >> 12) & 0x0F)) as u8);
            out.push((0x80 | ((c >> 6) & 0x3F)) as u8);
            out.push((0x80 | (c & 0x3F)) as u8);
        }
    }
    out
}

/// Number of bytes `encode_modified_utf8` would produce.
pub fn modified_utf8_len(s: &str) -> usize {
    s.encode_utf16()
        .map(|u| {
            let c = u as u32;
            if (0x0001..=0x007F).contains(&c) {
                1
            } else if c <= 0x07FF {
                2
            } else {
                3
            }
        })
        .sum()
}

/// Decodes Java modified UTF-8 (also accepts standard UTF-8 sequences up to 3 bytes).
pub fn decode_modified_utf8(raw: &[u8]) -> CodecResult<String> {
    // Fast path: plain ASCII without NUL is identical in both encodings.
    if raw.iter().all(|&b| b != 0 && b < 0x80) {
        // SAFETY of from_utf8: ASCII is valid UTF-8.
        return Ok(String::from_utf8(raw.to_vec()).unwrap());
    }
    let mut units: Vec<u16> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let a = raw[i] as u32;
        if a & 0x80 == 0 {
            units.push(a as u16);
            i += 1;
        } else if a & 0xE0 == 0xC0 {
            if i + 1 >= raw.len() {
                return err("malformed modified UTF-8: truncated 2-byte sequence");
            }
            let b = raw[i + 1] as u32;
            units.push((((a & 0x1F) << 6) | (b & 0x3F)) as u16);
            i += 2;
        } else if a & 0xF0 == 0xE0 {
            if i + 2 >= raw.len() {
                return err("malformed modified UTF-8: truncated 3-byte sequence");
            }
            let b = raw[i + 1] as u32;
            let c = raw[i + 2] as u32;
            units.push((((a & 0x0F) << 12) | ((b & 0x3F) << 6) | (c & 0x3F)) as u16);
            i += 3;
        } else if a & 0xF8 == 0xF0 && i + 3 < raw.len() {
            // Standard 4-byte UTF-8, tolerated for robustness.
            let cp = ((a & 0x07) << 18)
                | ((raw[i + 1] as u32 & 0x3F) << 12)
                | ((raw[i + 2] as u32 & 0x3F) << 6)
                | (raw[i + 3] as u32 & 0x3F);
            let mut tmp = [0u16; 2];
            if let Some(ch) = char::from_u32(cp) {
                units.extend_from_slice(ch.encode_utf16(&mut tmp));
            }
            i += 4;
        } else {
            return err(format!("malformed modified UTF-8 byte 0x{a:02x}"));
        }
    }
    Ok(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_utf8_round_trip() {
        for s in ["", "abc", "àèìòù", "\u{0}x", "日本語", "emoji 😀"] {
            let enc = encode_modified_utf8(s);
            assert_eq!(enc.len(), modified_utf8_len(s));
            assert_eq!(decode_modified_utf8(&enc).unwrap(), s);
        }
        assert_eq!(encode_modified_utf8("\u{0}"), vec![0xC0, 0x80]);
        assert_eq!(encode_modified_utf8("😀").len(), 6);
    }

    #[test]
    fn reader_writer_primitives() {
        let mut buf = BytesMut::new();
        {
            let mut w = Writer::new(&mut buf);
            w.bool(true);
            w.i32(-5);
            w.i64(1 << 40);
            w.opt_string(Some("hello"));
            w.opt_string(None);
            w.opt_bytes(Some(b"xyz"));
        }
        let mut r = Reader::new(buf.freeze());
        assert!(r.bool().unwrap());
        assert_eq!(r.i32().unwrap(), -5);
        assert_eq!(r.i64().unwrap(), 1 << 40);
        assert_eq!(r.opt_string().unwrap().as_deref(), Some("hello"));
        assert_eq!(r.opt_string().unwrap(), None);
        assert_eq!(&r.opt_bytes().unwrap().unwrap()[..], b"xyz");
        assert!(r.i32().is_err());
    }
}
