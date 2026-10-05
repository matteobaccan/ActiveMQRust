// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! A stored message: the shared message, its memory ticket and cached properties.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::openwire::model::{DataStructure, Message, TransactionId};
use crate::openwire::props::{PrimitiveMap, Value};
use crate::selector::{EvalError, Header, MessageView, SVal};

/// Fixed per-message bookkeeping overhead added to the accounted size.
pub const ENTRY_OVERHEAD: u64 = 256;

/// Broker-wide message memory counter.
#[derive(Default)]
pub struct Memory {
    pub used: AtomicU64,
    pub limit: u64,
}

impl Memory {
    pub fn new(limit: u64) -> Self {
        Memory { used: AtomicU64::new(0), limit }
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }
}

/// Accounts a message's memory for as long as any copy of it is held.
pub struct MemTicket {
    memory: Arc<Memory>,
    size: u64,
}

impl MemTicket {
    pub fn new(memory: Arc<Memory>, size: u64) -> Self {
        memory.used.fetch_add(size, Ordering::Relaxed);
        MemTicket { memory, size }
    }
}

impl Drop for MemTicket {
    fn drop(&mut self) {
        self.memory.used.fetch_sub(self.size, Ordering::Relaxed);
    }
}

/// Decoded application properties of a message.
enum Props {
    /// No `marshalledProperties` (or a null map).
    Absent,
    Map(Arc<PrimitiveMap>),
    /// `marshalledProperties` that cannot be decoded: property lookups fail.
    Undecodable,
}

/// Data shared by every copy of one stored message.
pub struct Meta {
    pub size: u64,
    _ticket: MemTicket,
    props: OnceLock<Props>,
}

impl Meta {
    pub fn new(memory: Arc<Memory>, msg: &Message) -> Arc<Meta> {
        let size = msg.content_len() as u64 + msg.properties_len() as u64 + ENTRY_OVERHEAD;
        Arc::new(Meta { size, _ticket: MemTicket::new(memory, size), props: OnceLock::new() })
    }
}

#[cfg(test)]
thread_local! {
    /// Number of property decodings performed by the current thread (tests only).
    static DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One message held by a destination or a subscription.
#[derive(Clone)]
pub struct Entry {
    /// Position in the destination (also the message's broker sequence id).
    pub seq: u64,
    pub msg: Arc<Message>,
    pub meta: Arc<Meta>,
    pub redelivery: i32,
}

impl Entry {
    pub fn expired(&self, now_ms: i64) -> bool {
        self.msg.expiration > 0 && self.msg.expiration <= now_ms
    }

    /// Decodes the properties on first use; every copy of the message shares the result.
    /// A decoding failure is logged once per message.
    fn props(&self) -> &Props {
        self.meta.props.get_or_init(|| {
            #[cfg(test)]
            DECODES.with(|d| d.set(d.get() + 1));
            let Some(raw) = self.msg.marshalled_properties.as_ref() else {
                return Props::Absent;
            };
            match PrimitiveMap::decode(raw) {
                Ok(Some(m)) => Props::Map(Arc::new(m)),
                Ok(None) => Props::Absent,
                Err(e) => {
                    tracing::warn!("cannot decode properties of message {}: {}", self.msg.message_id_text(), e);
                    Props::Undecodable
                }
            }
        })
    }

    /// Decoded application properties, computed once per message; `None` when the message
    /// has none or they cannot be decoded.
    pub fn properties(&self) -> Option<&Arc<PrimitiveMap>> {
        match self.props() {
            Props::Map(m) => Some(m),
            _ => None,
        }
    }

    /// True once the properties have been decoded (or found absent or undecodable).
    pub fn properties_decoded(&self) -> bool {
        self.meta.props.get().is_some()
    }

    /// The message as it must be dispatched, with the current redelivery counter.
    pub fn dispatch_message(&self) -> Arc<Message> {
        if self.msg.redelivery_counter == self.redelivery {
            self.msg.clone()
        } else {
            let mut m = (*self.msg).clone();
            m.redelivery_counter = self.redelivery;
            Arc::new(m)
        }
    }
}

/// Selector value of a property, with the Java class ActiveMQ unmarshals it to.
fn prop_to_sval(name: &str, v: &Value) -> SVal {
    match v {
        Value::Null => SVal::Null,
        Value::Bool(b) => SVal::Bool(*b),
        Value::Byte(b) => SVal::Byte(*b),
        Value::Short(s) => SVal::Short(*s),
        Value::Int(i) => SVal::Int(*i),
        Value::Long(l) => SVal::Long(*l),
        Value::Float(f) => SVal::Float(*f),
        Value::Double(d) => SVal::Double(*d),
        Value::String(s) => SVal::Str(s.clone()),
        Value::Char(c) => SVal::Char(*c),
        // A byte array is equal only to itself (Java identity); maps and lists by content.
        Value::Bytes(_) => SVal::Opaque(format!("[B@{name}").into()),
        Value::Map(_) | Value::List(_) => SVal::Opaque(format!("{v:?}").into()),
    }
}

fn opt_str(s: &Option<String>) -> SVal {
    s.as_ref().map(|v| SVal::Str(v.clone())).unwrap_or(SVal::Null)
}

/// `TransactionId.toString()`: `TX:<connectionId>:<value>` for local transactions,
/// `XID:[<formatId>,globalId=<hex>,branchId=<hex>]` for XA ones.
fn transaction_text(t: &TransactionId) -> String {
    match t {
        TransactionId::Local { value, connection_id } => match connection_id {
            Some(c) => format!("TX:{c}:{value}"),
            None => format!("TX:null:{value}"),
        },
        TransactionId::Xa { format_id, global_transaction_id, branch_qualifier } => {
            let hex = |b: &Option<bytes::Bytes>| {
                b.as_ref().map(|b| b.iter().map(|x| format!("{x:x}")).collect::<String>()).unwrap_or_default()
            };
            format!("XID:[{format_id},globalId={},branchId={}]", hex(global_transaction_id), hex(branch_qualifier))
        }
    }
}

/// `Arrays.toString(brokerPath)`.
fn broker_path_text(path: &Option<Vec<DataStructure>>) -> String {
    match path {
        None => "null".to_string(),
        Some(ids) => {
            let items: Vec<String> = ids
                .iter()
                .map(|d| match d {
                    DataStructure::BrokerId(b) => b.value.to_string(),
                    other => format!("{other:?}"),
                })
                .collect();
            format!("[{}]", items.join(", "))
        }
    }
}

impl MessageView for Entry {
    fn header(&self, h: Header) -> Result<SVal, EvalError> {
        let m = &self.msg;
        Ok(match h {
            Header::Destination => match m.original_destination.as_ref().or(m.destination.as_ref()) {
                Some(d) => SVal::Str(d.to_string()),
                None => SVal::Null,
            },
            Header::ReplyTo => m.reply_to.as_ref().map(|d| SVal::Str(d.to_string())).unwrap_or(SVal::Null),
            Header::Type => opt_str(&m.jms_type),
            Header::DeliveryMode => SVal::Str(if m.persistent { "PERSISTENT" } else { "NON_PERSISTENT" }.into()),
            Header::Priority => SVal::Int(m.priority as i32),
            Header::MessageId => m.message_id.as_ref().map(|id| SVal::Str(id.to_string())).unwrap_or(SVal::Null),
            Header::Timestamp => SVal::Long(m.timestamp),
            Header::CorrelationId => opt_str(&m.correlation_id),
            Header::Expiration => SVal::Long(m.expiration),
            Header::Redelivered => SVal::Bool(self.redelivery > 0),
            Header::DeliveryCount => SVal::Int(self.redelivery.wrapping_add(1)),
            Header::GroupId => opt_str(&m.group_id),
            Header::UserId => match &m.user_id {
                Some(u) => SVal::Str(u.clone()),
                None => return self.property("JMSXUserID"),
            },
            Header::GroupSeq => SVal::Int(m.group_sequence),
            Header::ProducerTxId => match m.original_transaction_id.as_ref().or(m.transaction_id.as_ref()) {
                Some(t) => SVal::Str(transaction_text(t)),
                None => SVal::Null,
            },
            Header::BrokerInTime => SVal::Long(m.broker_in_time),
            Header::BrokerOutTime => SVal::Long(m.broker_out_time),
            Header::BrokerPath => SVal::Str(broker_path_text(&m.broker_path)),
            Header::GroupFirstForConsumer => SVal::Bool(m.jmsx_group_first_for_consumer),
        })
    }

    fn property(&self, name: &str) -> Result<SVal, EvalError> {
        match self.props() {
            Props::Map(p) => Ok(p.get(name).map(|v| prop_to_sval(name, v)).unwrap_or(SVal::Null)),
            Props::Absent => Ok(SVal::Null),
            Props::Undecodable => Err(EvalError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openwire::types as t;
    use crate::selector::Selector;
    use bytes::Bytes;

    fn entry(props: Option<Bytes>) -> Entry {
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.correlation_id = Some("ORD-A".into());
        m.priority = 7;
        m.marshalled_properties = props;
        let meta = Meta::new(Arc::new(Memory::new(0)), &m);
        Entry { seq: 1, msg: Arc::new(m), meta, redelivery: 0 }
    }

    fn props(entries: &[(&str, Value)]) -> Bytes {
        let mut map = PrimitiveMap::new();
        for (k, v) in entries {
            map.set(k, v.clone());
        }
        map.encode()
    }

    fn decodes() -> usize {
        DECODES.with(|d| d.get())
    }

    #[test]
    fn header_only_selector_does_not_decode_properties() {
        let e = entry(Some(props(&[("color", Value::String("red".into()))])));
        let s = Selector::compile("JMSCorrelationID = 'ORD-A' AND JMSPriority > 3").unwrap().unwrap();
        let before = decodes();
        assert!(s.matches(&e));
        assert_eq!(decodes(), before);
        assert!(!e.properties_decoded());
        let p = Selector::compile("color = 'red'").unwrap().unwrap();
        assert!(p.matches(&e));
        assert!(e.properties_decoded());
    }

    #[test]
    fn properties_are_decoded_once_for_many_consumers() {
        let e = entry(Some(props(&[("color", Value::String("red".into())), ("n", Value::Int(5))])));
        let copies: Vec<Entry> = (0..5).map(|_| e.clone()).collect();
        let selectors: Vec<Selector> = ["color = 'red'", "n = 5", "n > 1 AND color LIKE 'r%'", "missing IS NULL", "n BETWEEN 1 AND 9"]
            .iter()
            .map(|s| Selector::compile(s).unwrap().unwrap())
            .collect();
        let before = decodes();
        for (s, c) in selectors.iter().zip(&copies) {
            assert!(s.matches(c), "{}", s.text());
        }
        assert_eq!(decodes() - before, 1);
    }

    #[test]
    fn undecodable_properties_make_the_selector_unknown() {
        // A truncated map: one entry announced, nothing follows.
        let e = entry(Some(Bytes::from_static(&[0, 0, 0, 1])));
        let before = decodes();
        for s in ["color = 'red'", "color IS NULL", "NOT (color = 'red')", "color IS NOT NULL"] {
            assert!(!Selector::compile(s).unwrap().unwrap().matches(&e), "{s}");
        }
        assert_eq!(decodes() - before, 1, "decoded (and logged) once");
        assert!(e.properties().is_none());
        // Header-only selectors keep working.
        assert!(Selector::compile("JMSPriority = 7").unwrap().unwrap().matches(&e));
    }

    #[test]
    fn property_types_keep_their_java_class() {
        let e = entry(Some(props(&[
            ("b", Value::Byte(7)),
            ("s", Value::Short(300)),
            ("i", Value::Int(5)),
            ("l", Value::Long(10_000_000_000)),
            ("f", Value::Float(1.5)),
            ("d", Value::Double(2.5)),
            ("c", Value::Char('x' as u16)),
            ("t", Value::Bool(true)),
            ("str", Value::String("v".into())),
            ("bytes", Value::Bytes(Bytes::from_static(b"zz"))),
        ])));
        let cases = [
            ("b = 7", true),
            ("7 = b", false),
            ("s = 300", true),
            ("300 = s", false),
            ("i = 5", true),
            ("l = 10000000000", true),
            ("f = 1.5", true),
            ("d = 2.5", true),
            ("c = 'x'", false),
            ("c = c", true),
            ("t", true),
            ("str = 'v'", true),
            ("bytes = bytes", true),
            ("bytes IS NOT NULL", true),
        ];
        for (s, expected) in cases {
            assert_eq!(Selector::compile(s).unwrap().unwrap().matches(&e), expected, "{s}");
        }
    }
}
