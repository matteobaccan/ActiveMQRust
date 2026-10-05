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
