// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Decoding of message bodies for display in the admin console. Never changes the message.

use bytes::Bytes;

use crate::broker::compress::decompress_content;
use crate::openwire::codec::{decode_modified_utf8, Reader};
use crate::openwire::model::Message;
use crate::openwire::props::{decode_value, PrimitiveMap, Value};
use crate::openwire::types as t;

pub const TEXT_LIMIT: usize = 64 * 1024;
pub const HEX_LIMIT: usize = 4 * 1024;
const INFLATE_LIMIT: usize = 64 * 1024;

pub enum BodyView {
    NoBody,
    Text { text: String, truncated: bool },
    Bytes { head: Vec<u8>, total: usize },
    Map(Vec<(String, &'static str, String)>),
    Object { size: usize },
    Stream(Vec<(&'static str, String)>),
    Error(String),
}

pub struct Rendered {
    pub view: BodyView,
    /// Set when inflating stopped at the 64 KB limit.
    pub inflate_truncated: bool,
}

/// Java type names as shown by the console.
pub fn java_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Byte(_) => "byte",
        Value::Char(_) => "char",
        Value::Short(_) => "short",
        Value::Int(_) => "int",
        Value::Long(_) => "long",
        Value::Float(_) => "float",
        Value::Double(_) => "double",
        Value::String(_) => "String",
        Value::Bytes(_) => "byte[]",
        Value::Map(_) => "Map",
        Value::List(_) => "List",
    }
}

/// Body size as stored (compressed size for compressed bodies).
pub fn stored_size(m: &Message) -> usize {
    m.content_len()
}

pub fn render(m: &Message) -> Rendered {
    let Some(content) = &m.content else {
        return Rendered { view: BodyView::NoBody, inflate_truncated: false };
    };
    if m.msg_type == t::ACTIVEMQ_OBJECT_MESSAGE {
        return Rendered { view: BodyView::Object { size: content.len() }, inflate_truncated: false };
    }
    let (data, inflate_truncated): (Bytes, bool) = if m.compressed {
        match decompress_content(m.msg_type, content, INFLATE_LIMIT + 1) {
            Ok(v) => {
                let truncated = v.len() > INFLATE_LIMIT;
                let mut v = v;
                v.truncate(INFLATE_LIMIT);
                (Bytes::from(v), truncated)
            }
            Err(e) => return Rendered { view: BodyView::Error(e), inflate_truncated: false },
        }
    } else {
        (content.clone(), false)
    };
    let view = match m.msg_type {
        t::ACTIVEMQ_TEXT_MESSAGE => text(&data),
        t::ACTIVEMQ_BYTES_MESSAGE => {
            let total = if m.compressed && content.len() >= 4 {
                i32::from_be_bytes([content[0], content[1], content[2], content[3]]).max(0) as usize
            } else {
                data.len()
            };
            BodyView::Bytes { head: data[..data.len().min(HEX_LIMIT)].to_vec(), total }
        }
        t::ACTIVEMQ_MAP_MESSAGE => match PrimitiveMap::decode(&data) {
            Ok(Some(map)) => BodyView::Map(
                map.entries.iter().map(|(k, v)| (k.clone(), java_type(v), v.display())).collect(),
            ),
            Ok(None) => BodyView::NoBody,
            Err(e) => BodyView::Error(format!("cannot decode the map body: {e}")),
        },
        t::ACTIVEMQ_STREAM_MESSAGE => {
            let mut r = Reader::new(data.clone());
            let mut values = Vec::new();
            while r.remaining() > 0 {
                match decode_value(&mut r, 0) {
                    Ok(v) => values.push((java_type(&v), v.display())),
                    Err(_) => break,
                }
            }
            BodyView::Stream(values)
        }
        _ => BodyView::Bytes { head: data[..data.len().min(HEX_LIMIT)].to_vec(), total: data.len() },
    };
    Rendered { view, inflate_truncated }
}

/// TextMessage content: i32 length, then modified UTF-8 (`MarshallingSupport.writeUTF8`).
fn text(data: &[u8]) -> BodyView {
    if data.len() < 4 {
        return BodyView::NoBody;
    }
    let len = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if len < 0 {
        return BodyView::NoBody;
    }
    let avail = &data[4..];
    let mut take = avail.len().min(len as usize).min(TEXT_LIMIT);
    // Do not cut a multi-byte sequence in half.
    while take > 0 && take < avail.len() && (avail[take] & 0xC0) == 0x80 {
        take -= 1;
    }
    let truncated = (len as usize) > take;
    match decode_modified_utf8(&avail[..take]) {
        Ok(s) => BodyView::Text { text: s, truncated },
        Err(_) => BodyView::Text { text: String::from_utf8_lossy(&avail[..take]).into_owned(), truncated },
    }
}

/// A classic hex dump: offset, 16 hex bytes, ASCII.
pub fn hex_dump(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 4);
    for (i, chunk) in data.chunks(16).enumerate() {
        out.push_str(&format!("{:08x}  ", i * 16));
        for j in 0..16 {
            match chunk.get(j) {
                Some(b) => out.push_str(&format!("{b:02x} ")),
                None => out.push_str("   "),
            }
        }
        out.push(' ');
        for b in chunk {
            out.push(if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' });
        }
        out.push('\n');
    }
    out
}

/// Whole text of a TextMessage for the XML view, inflating a compressed body: `None` for
/// other bodies, `Some(None)` when the text is longer than `limit` bytes.
pub fn full_text(m: &Message, limit: usize) -> Option<Option<String>> {
    if m.msg_type != t::ACTIVEMQ_TEXT_MESSAGE {
        return None;
    }
    let content = m.content.as_ref()?;
    let data: Bytes = if m.compressed {
        Bytes::from(decompress_content(m.msg_type, content, limit + 5).ok()?)
    } else {
        content.clone()
    };
    if data.len() < 4 {
        return None;
    }
    let len = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if len < 0 {
        return None;
    }
    if len as usize > limit {
        return Some(None);
    }
    let avail = &data[4..];
    let raw = &avail[..avail.len().min(len as usize)];
    Some(Some(decode_modified_utf8(raw).unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::compress::compress_content;
    use crate::openwire::codec::Writer;
    use crate::openwire::props::encode_value;
    use bytes::BytesMut;

    fn text_content(s: &str) -> Bytes {
        let mut v = (s.len() as i32).to_be_bytes().to_vec();
        v.extend_from_slice(s.as_bytes());
        Bytes::from(v)
    }

    fn msg(ty: u8, content: Option<Bytes>) -> Message {
        let mut m = Message::new(ty);
        m.content = content;
        m
    }

    fn compressed(ty: u8, raw: &[u8]) -> Message {
        let mut m = msg(ty, Some(Bytes::from(compress_content(ty, raw))));
        m.compressed = true;
        m
    }

    #[test]
    fn text_and_truncation() {
        let m = msg(t::ACTIVEMQ_TEXT_MESSAGE, Some(text_content("hello")));
        assert!(matches!(render(&m).view, BodyView::Text { ref text, truncated: false } if text == "hello"));
        let long = "x".repeat(100 * 1024);
        let m = msg(t::ACTIVEMQ_TEXT_MESSAGE, Some(text_content(&long)));
        match render(&m).view {
            BodyView::Text { text, truncated } => {
                assert_eq!(text.len(), TEXT_LIMIT);
                assert!(truncated);
            }
            _ => panic!("text expected"),
        }
        // A multi-byte character is never cut in half.
        let accented = format!("{}\u{e9}", "a".repeat(TEXT_LIMIT - 1));
        let m = msg(t::ACTIVEMQ_TEXT_MESSAGE, Some(text_content(&accented)));
        match render(&m).view {
            BodyView::Text { text, truncated } => {
                assert_eq!(text.len(), TEXT_LIMIT - 1);
                assert!(truncated);
            }
            _ => panic!("text expected"),
        }
    }

    #[test]
    fn bytes_hex_dump_of_first_4_kb() {
        let body: Vec<u8> = (0..10_240u32).map(|i| i as u8).collect();
        let m = msg(t::ACTIVEMQ_BYTES_MESSAGE, Some(Bytes::from(body.clone())));
        match render(&m).view {
            BodyView::Bytes { head, total } => {
                assert_eq!(head.len(), HEX_LIMIT);
                assert_eq!(total, 10_240);
                assert_eq!(head, &body[..HEX_LIMIT]);
            }
            _ => panic!("bytes expected"),
        }
        let dump = hex_dump(b"AB\x00");
        assert!(dump.starts_with("00000000  41 42 00 "), "{dump}");
        assert!(dump.trim_end().ends_with("AB."));
        // Compressed bytes: the total comes from the length prefix.
        let m = compressed(t::ACTIVEMQ_BYTES_MESSAGE, &body);
        assert!(matches!(render(&m).view, BodyView::Bytes { total: 10_240, .. }));
    }

    #[test]
    fn map_object_stream_and_empty() {
        let mut map = PrimitiveMap::new();
        map.set("name", Value::String("abc".into()));
        map.set("qty", Value::Int(5));
        let m = msg(t::ACTIVEMQ_MAP_MESSAGE, Some(map.encode()));
        match render(&m).view {
            BodyView::Map(rows) => assert_eq!(
                rows,
                vec![("name".to_string(), "String", "abc".to_string()), ("qty".to_string(), "int", "5".to_string())]
            ),
            _ => panic!("map expected"),
        }
        let m = msg(t::ACTIVEMQ_OBJECT_MESSAGE, Some(Bytes::from_static(b"\xac\xed\x00\x05sr\x00\x0ejava.util.Date")));
        assert!(matches!(render(&m).view, BodyView::Object { size: 22 }));
        let mut buf = BytesMut::new();
        {
            let mut w = Writer::new(&mut buf);
            encode_value(&mut w, &Value::Bool(true));
            encode_value(&mut w, &Value::Long(42));
            encode_value(&mut w, &Value::String("x".into()));
        }
        let m = msg(t::ACTIVEMQ_STREAM_MESSAGE, Some(buf.freeze()));
        match render(&m).view {
            BodyView::Stream(v) => assert_eq!(
                v,
                vec![("boolean", "true".to_string()), ("long", "42".to_string()), ("String", "x".to_string())]
            ),
            _ => panic!("stream expected"),
        }
        assert!(matches!(render(&msg(t::ACTIVEMQ_TEXT_MESSAGE, None)).view, BodyView::NoBody));
        assert!(matches!(render(&msg(t::ACTIVEMQ_MESSAGE, None)).view, BodyView::NoBody));
    }

    #[test]
    fn compressed_text_is_inflated_without_changing_the_message() {
        let text = "compressible ".repeat(4000);
        let m = compressed(t::ACTIVEMQ_TEXT_MESSAGE, &text_content(&text));
        let stored = m.content.clone().unwrap();
        let r = render(&m);
        assert!(!r.inflate_truncated);
        assert!(matches!(r.view, BodyView::Text { text: ref t2, .. } if *t2 == text));
        assert_eq!(m.content.as_ref(), Some(&stored));
        assert_eq!(stored_size(&m), stored.len());
        assert_eq!(full_text(&m, 1024 * 1024), Some(Some(text.clone())));
        assert_eq!(full_text(&m, 1000), Some(None));
    }

    #[test]
    fn zip_bomb_stops_at_64_kb() {
        // 100 MB of zeros, stored compressed in well under 1 MB.
        let mut big = (100 * 1024 * 1024i32).to_be_bytes().to_vec();
        big.resize(100 * 1024 * 1024 + 4, 0);
        let m = compressed(t::ACTIVEMQ_TEXT_MESSAGE, &big);
        drop(big);
        assert!(stored_size(&m) < 1024 * 1024);
        let r = render(&m);
        assert!(r.inflate_truncated);
        match r.view {
            BodyView::Text { text, truncated } => {
                assert!(truncated);
                assert!(text.len() <= TEXT_LIMIT);
            }
            _ => panic!("text expected"),
        }
        assert_eq!(full_text(&m, 1024 * 1024), Some(None));
    }
}
