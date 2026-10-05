// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Helpers shared by the criterion benchmarks.

#![allow(dead_code)]

pub mod payload;

use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;

use mqrust::broker::conn::{ConnHandle, Out};
use mqrust::broker::destination::{Dest, SubSpec};
use mqrust::broker::{now_ms, Broker};
use mqrust::config::{build, ConfigSource, FileConfig, Overrides};
use mqrust::openwire::model::*;
use mqrust::openwire::types as t;
use mqrust::selector::Selector;

/// A broker with the built-in defaults.
pub fn broker() -> Arc<Broker> {
    broker_with(|_| {})
}

pub fn broker_with(f: impl FnOnce(&mut FileConfig)) -> Arc<Broker> {
    let mut fc = FileConfig::default();
    f(&mut fc);
    Broker::new(Arc::new(build(fc, ConfigSource::Defaults, &Overrides::default()).unwrap()))
}

pub fn producer() -> ProducerId {
    ProducerId { connection_id: Arc::from("ID:bench-1-1-1:1"), session_id: 1, value: 1 }
}

/// An `ActiveMQTextMessage` whose content is `size` bytes (as the client stores it).
pub fn text_message(dest: &Destination, seq: i64, size: usize) -> Message {
    let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
    m.producer_id = Some(producer());
    m.destination = Some(dest.clone());
    m.message_id = Some(MessageId { text_view: None, producer_id: Some(producer()), producer_sequence_id: seq, broker_sequence_id: 0 });
    m.correlation_id = Some("ORD-A".into());
    m.timestamp = 1;
    let mut body = vec![b'x'; size.max(4)];
    body[..4].copy_from_slice(&((size.max(4) - 4) as i32).to_be_bytes());
    m.content = Some(Bytes::from(body));
    m
}

/// A consumer attached to `dest`, receiving through an in-memory channel.
pub struct Consumer {
    pub id: ConsumerId,
    pub rx: mpsc::UnboundedReceiver<Out>,
}

pub fn consumer(b: &Broker, d: &Dest, n: i64, prefetch: i32, selector: Option<&str>) -> Consumer {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = Arc::new(ConnHandle::new(b.new_conn_id(), "127.0.0.1:1".parse().unwrap(), tx));
    let id = ConsumerId { connection_id: Arc::from(format!("ID:bench-consumer-{n}")), session_id: 1, value: n };
    d.add_sub(
        SubSpec {
            id: id.clone(),
            conn: handle,
            prefetch,
            selector: selector.map(|s| Arc::new(Selector::compile(s).unwrap().unwrap())),
            no_local: false,
            browser: false,
        },
        now_ms(),
    );
    Consumer { id, rx }
}

/// Acknowledges every dispatch waiting for `c`; returns how many there were.
pub fn drain_and_ack(d: &Dest, c: &mut Consumer) -> usize {
    let mut n = 0;
    while let Ok(Out::Cmd(Command::MessageDispatch(md))) = c.rx.try_recv() {
        let Some(m) = md.message else { continue };
        let ack = MessageAck {
            header: Header::default(),
            destination: Some(d.dest.clone()),
            transaction_id: None,
            consumer_id: Some(c.id.clone()),
            ack_type: ack_type::STANDARD,
            first_message_id: None,
            last_message_id: m.message_id.clone(),
            message_count: 1,
            poison_cause: None,
        };
        let _ = d.ack(&ack, false, now_ms());
        n += 1;
    }
    n
}
