// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! ActiveMQ primitive maps (`MarshallingSupport.marshalPrimitiveMap`), used for
//! `WireFormatInfo` properties, message properties and `MapMessage` bodies.

use bytes::{Bytes, BytesMut};

use super::codec::{decode_modified_utf8, encode_modified_utf8, err, CodecResult, Reader, Writer};

/// Number of marshalled property maps decoded since start-up (exposed to tests: the hot path
/// must not decode properties unless a selector needs them).
static DECODE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Property maps decoded so far, by any part of the broker.
pub fn decode_count() -> u64 {
    DECODE_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

pub const NULL: u8 = 0;
pub const BOOLEAN: u8 = 1;
pub const BYTE: u8 = 2;
pub const CHAR: u8 = 3;
pub const SHORT: u8 = 4;
pub const INTEGER: u8 = 5;
pub const LONG: u8 = 6;
pub const DOUBLE: u8 = 7;
pub const FLOAT: u8 = 8;
pub const STRING: u8 = 9;
pub const BYTE_ARRAY: u8 = 10;
pub const MAP: u8 = 11;
pub const LIST: u8 = 12;
pub const BIG_STRING: u8 = 13;

const MAX_NESTING: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Byte(i8),
    Char(u16),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Bytes(Bytes),
    Map(PrimitiveMap),
    List(Vec<Value>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Byte(_) => "byte",
            Value::Char(_) => "char",
            Value::Short(_) => "short",
            Value::Int(_) => "int",
            Value::Long(_) => "long",
            Value::Float(_) => "float",
            Value::Double(_) => "double",
            Value::String(_) => "string",
            Value::Bytes(_) => "byte[]",
            Value::Map(_) => "map",
            Value::List(_) => "list",
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Byte(v) => v.to_string(),
            Value::Char(c) => char::from_u32(*c as u32).map(|c| c.to_string()).unwrap_or_default(),
            Value::Short(v) => v.to_string(),
            Value::Int(v) => v.to_string(),
            Value::Long(v) => v.to_string(),
            Value::Float(v) => v.to_string(),
            Value::Double(v) => v.to_string(),
            Value::String(s) => s.clone(),
            Value::Bytes(b) => format!("<{} bytes>", b.len()),
            Value::Map(m) => format!("<map of {} entries>", m.entries.len()),
            Value::List(l) => format!("<list of {} values>", l.len()),
        }
    }
}

/// An ordered map of named primitive values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrimitiveMap {
    pub entries: Vec<(String, Value)>,
}

impl PrimitiveMap {
    pub fn new() -> Self {
        PrimitiveMap { entries: Vec::new() }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(e) = self.entries.iter_mut().find(|(k, _)| k == key) {
            e.1 = value;
        } else {
            self.entries.push((key.to_string(), value));
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    pub fn get_long(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(Value::Long(v)) => Some(*v),
            Some(Value::Int(v)) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Decodes a marshalled map (i32 count, then name/value pairs). A negative count means null.
    pub fn decode(data: &Bytes) -> CodecResult<Option<PrimitiveMap>> {
        DECODE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut r = Reader::new(data.clone());
        decode_map(&mut r, 0)
    }

    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        let mut w = Writer::new(&mut buf);
        encode_map(&mut w, self);
        buf.freeze()
    }
}

fn decode_map(r: &mut Reader, depth: usize) -> CodecResult<Option<PrimitiveMap>> {
    let size = r.i32()?;
    if size < 0 {
        return Ok(None);
    }
    let mut map = PrimitiveMap {
        entries: Vec::with_capacity((size as usize).min(1024)),
    };
    for _ in 0..size {
        let name = r.utf()?;
        let value = decode_value(r, depth)?;
        map.entries.push((name, value));
    }
    Ok(Some(map))
}

pub fn decode_value(r: &mut Reader, depth: usize) -> CodecResult<Value> {
    if depth > MAX_NESTING {
        return err("primitive map nested too deeply");
    }
    let t = r.u8()?;
    Ok(match t {
        NULL => Value::Null,
        BOOLEAN => Value::Bool(r.bool()?),
        BYTE => Value::Byte(r.u8()? as i8),
        CHAR => Value::Char(r.u16()?),
        SHORT => Value::Short(r.i16()?),
        INTEGER => Value::Int(r.i32()?),
        LONG => Value::Long(r.i64()?),
        FLOAT => Value::Float(r.f32()?),
        DOUBLE => Value::Double(r.f64()?),
        BYTE_ARRAY => {
            let len = r.i32()?;
            if len < 0 {
                return err("negative byte array length");
            }
            Value::Bytes(r.take(len as usize)?)
        }
        STRING => {
            let len = r.u16()? as usize;
            Value::String(decode_modified_utf8(&r.take(len)?)?)
        }
        BIG_STRING => {
            let len = r.i32()?;
            if len < 0 {
                Value::Null
            } else {
                Value::String(decode_modified_utf8(&r.take(len as usize)?)?)
            }
        }
        MAP => match decode_map(r, depth + 1)? {
            Some(m) => Value::Map(m),
            None => Value::Null,
        },
        LIST => {
            let size = r.i32()?;
            let mut list = Vec::with_capacity((size.max(0) as usize).min(1024));
            for _ in 0..size.max(0) {
                list.push(decode_value(r, depth + 1)?);
            }
            Value::List(list)
        }
        other => return err(format!("unknown primitive type {other}")),
    })
}

fn encode_map(w: &mut Writer, map: &PrimitiveMap) {
    w.i32(map.entries.len() as i32);
    for (k, v) in &map.entries {
        w.utf(k);
        encode_value(w, v);
    }
}

pub fn encode_value(w: &mut Writer, v: &Value) {
    match v {
        Value::Null => w.u8(NULL),
        Value::Bool(b) => {
            w.u8(BOOLEAN);
            w.bool(*b);
        }
        Value::Byte(b) => {
            w.u8(BYTE);
            w.u8(*b as u8);
        }
        Value::Char(c) => {
            w.u8(CHAR);
            w.u16(*c);
        }
        Value::Short(s) => {
            w.u8(SHORT);
            w.u16(*s as u16);
        }
        Value::Int(i) => {
            w.u8(INTEGER);
            w.i32(*i);
        }
        Value::Long(l) => {
            w.u8(LONG);
            w.i64(*l);
        }
        Value::Float(f) => {
            w.u8(FLOAT);
            w.f32(*f);
        }
        Value::Double(d) => {
            w.u8(DOUBLE);
            w.f64(*d);
        }
        Value::String(s) => {
            // Same threshold as MarshallingSupport.marshalString (Short.MAX_VALUE / 4).
            if s.encode_utf16().count() < 8191 {
                w.u8(STRING);
                w.utf(s);
            } else {
                w.u8(BIG_STRING);
                let enc = encode_modified_utf8(s);
                w.i32(enc.len() as i32);
                w.raw(&enc);
            }
        }
        Value::Bytes(b) => {
            w.u8(BYTE_ARRAY);
            w.i32(b.len() as i32);
            w.raw(b);
        }
        Value::Map(m) => {
            w.u8(MAP);
            encode_map(w, m);
        }
        Value::List(l) => {
            w.u8(LIST);
            w.i32(l.len() as i32);
            for v in l {
                encode_value(w, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_round_trip() {
        let mut m = PrimitiveMap::new();
        m.set("a", Value::Bool(true));
        m.set("b", Value::Long(30000));
        m.set("c", Value::String("x".repeat(9000)));
        m.set("d", Value::Int(-1));
        m.set("e", Value::Bytes(Bytes::from_static(b"zz")));
        let enc = m.encode();
        let back = PrimitiveMap::decode(&enc).unwrap().unwrap();
        assert_eq!(back, m);
        assert_eq!(back.get_long("b"), Some(30000));
    }

    /// One-entry maps `{"k": value}` as written by `MarshallingSupport.marshalPrimitiveMap`
    /// (identical bytes from activemq-client 5.18.7 and 6.3.2).
    fn golden() -> Vec<(&'static str, Vec<u8>, Value)> {
        const HEAD: [u8; 7] = [0x00, 0x00, 0x00, 0x01, 0x00, 0x01, b'k'];
        let with = |tail: &[u8]| HEAD.iter().chain(tail).copied().collect::<Vec<u8>>();
        let mut inner = PrimitiveMap::new();
        inner.set("n", Value::Int(1));
        vec![
            ("null", with(&[0x00]), Value::Null),
            ("boolean", with(&[0x01, 0x01]), Value::Bool(true)),
            ("byte", with(&[0x02, 0xfb]), Value::Byte(-5)),
            ("char", with(&[0x03, 0x00, 0xe9]), Value::Char(0xe9)),
            ("short", with(&[0x04, 0xfe, 0xd4]), Value::Short(-300)),
            ("int", with(&[0x05, 0x00, 0x01, 0xe2, 0x40]), Value::Int(123456)),
            (
                "long",
                with(&[0x06, 0xff, 0xff, 0xfe, 0xe0, 0x8e, 0x04, 0xfb, 0x35]),
                Value::Long(-1234567890123),
            ),
            ("float", with(&[0x08, 0x3f, 0xc0, 0x00, 0x00]), Value::Float(1.5)),
            (
                "double",
                with(&[0x07, 0xc0, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
                Value::Double(-2.25),
            ),
            (
                "string",
                with(&[0x09, 0x00, 0x08, 0x68, 0xc3, 0xa9, 0x6c, 0x6c, 0x6f, 0xc0, 0x80]),
                Value::String("h\u{e9}llo\u{0}".into()),
            ),
            (
                "byte[]",
                with(&[0x0a, 0x00, 0x00, 0x00, 0x03, 0x01, 0x02, 0x03]),
                Value::Bytes(Bytes::from_static(&[1, 2, 3])),
            ),
            (
                "map",
                with(&[
                    0x0b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, b'n', 0x05, 0x00, 0x00, 0x00, 0x01,
                ]),
                Value::Map(inner),
            ),
            (
                "list",
                with(&[
                    0x0c, 0x00, 0x00, 0x00, 0x02, 0x05, 0x00, 0x00, 0x00, 0x01, 0x09, 0x00, 0x01, b'a',
                ]),
                Value::List(vec![Value::Int(1), Value::String("a".into())]),
            ),
            (
                "big string",
                with(&[[0x0d, 0x00, 0x00, 0x23, 0x28].as_slice(), &[b'x'; 9000]].concat()),
                Value::String("x".repeat(9000)),
            ),
        ]
    }

    #[test]
    fn every_type_round_trips_from_golden_bytes() {
        for (name, bytes, value) in golden() {
            let decoded = PrimitiveMap::decode(&Bytes::from(bytes.clone())).unwrap().unwrap();
            assert_eq!(decoded.get("k"), Some(&value), "{name}");
            let mut m = PrimitiveMap::new();
            m.set("k", value);
            assert_eq!(m.encode().as_ref(), bytes.as_slice(), "{name}");
        }
    }
}
