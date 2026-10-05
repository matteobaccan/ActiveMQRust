// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! A stored message: the shared message, its memory ticket and cached properties.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use crate::openwire::model::Message;
use crate::openwire::props::{PrimitiveMap, Value};
use crate::selector::{MessageView, SVal};

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

/// Data shared by every copy of one stored message.
pub struct Meta {
    pub size: u64,
    _ticket: MemTicket,
    props: OnceLock<Option<Arc<PrimitiveMap>>>,
}

impl Meta {
    pub fn new(memory: Arc<Memory>, msg: &Message) -> Arc<Meta> {
        let size = msg.content_len() as u64 + msg.properties_len() as u64 + ENTRY_OVERHEAD;
        Arc::new(Meta { size, _ticket: MemTicket::new(memory, size), props: OnceLock::new() })
    }
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

    /// Decoded application properties, computed once per message.
    pub fn properties(&self) -> Option<&Arc<PrimitiveMap>> {
        self.meta
            .props
            .get_or_init(|| {
                let raw = self.msg.marshalled_properties.as_ref()?;
                match PrimitiveMap::decode(raw) {
                    Ok(m) => m.map(Arc::new),
                    Err(e) => {
                        tracing::warn!("cannot decode properties of message {}: {}", self.msg.message_id_text(), e);
                        None
                    }
                }
            })
            .as_ref()
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

fn prop_to_sval(v: &Value) -> SVal {
    match v {
        Value::Null => SVal::Null,
        Value::Bool(b) => SVal::Bool(*b),
        Value::Byte(b) => SVal::Int(*b as i64),
        Value::Short(s) => SVal::Int(*s as i64),
        Value::Int(i) => SVal::Int(*i as i64),
        Value::Long(l) => SVal::Int(*l),
        Value::Float(f) => SVal::Float(*f as f64),
        Value::Double(d) => SVal::Float(*d),
        Value::String(s) => SVal::Str(s.clone()),
        Value::Char(c) => SVal::Str(char::from_u32(*c as u32).map(|c| c.to_string()).unwrap_or_default()),
        Value::Bytes(_) | Value::Map(_) | Value::List(_) => SVal::Opaque,
    }
}

fn opt_str(s: &Option<String>) -> SVal {
    s.as_ref().map(|v| SVal::Str(v.clone())).unwrap_or(SVal::Null)
}

impl MessageView for Entry {
    fn header(&self, name: &str) -> Option<SVal> {
        let m = &self.msg;
        Some(match name {
            "JMSDeliveryMode" => SVal::Str(if m.persistent { "PERSISTENT" } else { "NON_PERSISTENT" }.into()),
            "JMSPriority" => SVal::Int(m.priority as i64),
            "JMSMessageID" => match &m.message_id {
                Some(id) => SVal::Str(id.to_string()),
                None => SVal::Null,
            },
            "JMSTimestamp" => SVal::Int(m.timestamp),
            "JMSCorrelationID" => opt_str(&m.correlation_id),
            "JMSType" => opt_str(&m.jms_type),
            "JMSExpiration" => SVal::Int(m.expiration),
            "JMSRedelivered" => SVal::Bool(self.redelivery > 0),
            "JMSXDeliveryCount" => SVal::Int(self.redelivery as i64 + 1),
            "JMSXGroupID" => opt_str(&m.group_id),
            "JMSXGroupSeq" => SVal::Int(m.group_sequence as i64),
            "JMSXUserID" => opt_str(&m.user_id),
            "JMSXGroupFirstForConsumer" => SVal::Bool(m.jmsx_group_first_for_consumer),
            "JMSActiveMQBrokerInTime" => SVal::Int(m.broker_in_time),
            "JMSActiveMQBrokerOutTime" => SVal::Int(m.broker_out_time),
            "JMSDestination" | "JMSReplyTo" | "JMSXProducerTXID" | "JMSActiveMQBrokerPath" => {
                let present = match name {
                    "JMSDestination" => m.destination.is_some(),
                    "JMSReplyTo" => m.reply_to.is_some(),
                    "JMSXProducerTXID" => m.transaction_id.is_some(),
                    _ => m.broker_path.is_some(),
                };
                if present {
                    SVal::Opaque
                } else {
                    SVal::Null
                }
            }
            _ => return None,
        })
    }

    fn property(&self, name: &str) -> SVal {
        match self.properties() {
            Some(p) => p.get(name).map(prop_to_sval).unwrap_or(SVal::Null),
            None => SVal::Null,
        }
    }
}
