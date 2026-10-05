// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Loose-encoding marshalling of OpenWire commands, versions 9 to 12.
//! Field order follows `org.apache.activemq.openwire.vN.*Marshaller.looseMarshal`.

use bytes::{BufMut, Bytes, BytesMut};
use std::sync::Arc;

use super::codec::{err, CodecResult, Reader, Writer};
use super::model::*;
use super::props::PrimitiveMap;
use super::types as t;

/// A wire encoding. The broker negotiates loose encoding; a tight encoding could be added
/// behind this trait if benchmarks show it is worth it.
pub trait WireCodec: Send + Sync {
    /// Decodes a frame body (without the size prefix).
    fn decode(&self, body: Bytes) -> CodecResult<Option<Command>>;
    /// Appends a complete frame, size prefix included, without copying large bodies.
    fn encode(&self, cmd: &Command, out: &mut ChunkBuf);
}

/// Loose encoding for one negotiated version.
pub struct LooseCodec {
    pub decoder: Decoder,
    pub encoder: Encoder,
}

impl LooseCodec {
    pub fn new(version: i32) -> Self {
        LooseCodec {
            decoder: Decoder::new(version),
            encoder: Encoder::new(version),
        }
    }
}

impl WireCodec for LooseCodec {
    fn decode(&self, body: Bytes) -> CodecResult<Option<Command>> {
        self.decoder.decode_frame(body)
    }

    fn encode(&self, cmd: &Command, out: &mut ChunkBuf) {
        self.encoder.encode_frame_chunks(cmd, out)
    }
}

/// Version-aware decoder for one frame body.
pub struct Decoder {
    pub version: i32,
    pub stack_trace: bool,
}

impl Decoder {
    pub fn new(version: i32) -> Self {
        Decoder {
            version,
            stack_trace: false,
        }
    }

    /// Decodes a frame body (data type byte followed by the command).
    pub fn decode_frame(&self, body: Bytes) -> CodecResult<Option<Command>> {
        let mut r = Reader::new(body);
        let type_code = r.u8()?;
        if type_code == t::NULL {
            return Ok(None);
        }
        self.command(type_code, &mut r).map(Some)
    }

    fn header(&self, r: &mut Reader) -> CodecResult<Header> {
        Ok(Header {
            command_id: r.i32()?,
            response_required: r.bool()?,
        })
    }

    fn command(&self, type_code: u8, r: &mut Reader) -> CodecResult<Command> {
        Ok(match type_code {
            t::WIREFORMAT_INFO => {
                let raw = r.take(8)?;
                let mut magic = [0u8; 8];
                magic.copy_from_slice(&raw);
                let version = r.i32()?;
                let properties = match r.opt_bytes()? {
                    Some(b) => PrimitiveMap::decode(&b)?.unwrap_or_default(),
                    None => PrimitiveMap::new(),
                };
                Command::WireFormatInfo(WireFormatInfo {
                    magic,
                    version,
                    properties,
                })
            }
            t::BROKER_INFO => Command::BrokerInfo(self.broker_info(r)?),
            t::CONNECTION_INFO => {
                let header = self.header(r)?;
                Command::ConnectionInfo(ConnectionInfo {
                    header,
                    connection_id: self.opt_connection_id(r)?,
                    client_id: r.opt_string()?,
                    password: r.opt_string()?,
                    user_name: r.opt_string()?,
                    broker_path: self.object_array(r)?,
                    broker_master_connector: r.bool()?,
                    manageable: r.bool()?,
                    client_master: r.bool()?,
                    fault_tolerant: r.bool()?,
                    failover_reconnect: r.bool()?,
                    client_ip: r.opt_string()?,
                })
            }
            t::SESSION_INFO => {
                let header = self.header(r)?;
                Command::SessionInfo(SessionInfo {
                    header,
                    session_id: self.opt_session_id(r)?,
                })
            }
            t::CONSUMER_INFO => {
                let header = self.header(r)?;
                let consumer_id = self.opt_consumer_id(r)?;
                let browser = r.bool()?;
                let destination = self.opt_destination(r)?;
                let prefetch_size = r.i32()?;
                let maximum_pending_message_limit = r.i32()?;
                let dispatch_async = r.bool()?;
                let selector = r.opt_string()?;
                let client_id = if self.version >= 10 { r.opt_string()? } else { None };
                Command::ConsumerInfo(ConsumerInfo {
                    header,
                    consumer_id,
                    browser,
                    destination,
                    prefetch_size,
                    maximum_pending_message_limit,
                    dispatch_async,
                    selector,
                    client_id,
                    subscription_name: r.opt_string()?,
                    no_local: r.bool()?,
                    exclusive: r.bool()?,
                    retroactive: r.bool()?,
                    priority: r.u8()?,
                    broker_path: self.object_array(r)?,
                    additional_predicate: self.nested(r)?,
                    network_subscription: r.bool()?,
                    optimized_acknowledge: r.bool()?,
                    no_range_acks: r.bool()?,
                    network_consumer_path: self.object_array(r)?,
                })
            }
            t::PRODUCER_INFO => {
                let header = self.header(r)?;
                Command::ProducerInfo(ProducerInfo {
                    header,
                    producer_id: self.opt_producer_id(r)?,
                    destination: self.opt_destination(r)?,
                    broker_path: self.object_array(r)?,
                    dispatch_async: r.bool()?,
                    window_size: r.i32()?,
                })
            }
            t::TRANSACTION_INFO => {
                let header = self.header(r)?;
                Command::TransactionInfo(TransactionInfo {
                    header,
                    connection_id: self.opt_connection_id(r)?,
                    transaction_id: self.opt_transaction_id(r)?,
                    tx_type: r.u8()?,
                })
            }
            t::DESTINATION_INFO => {
                let header = self.header(r)?;
                Command::DestinationInfo(DestinationInfo {
                    header,
                    connection_id: self.opt_connection_id(r)?,
                    destination: self.opt_destination(r)?,
                    operation_type: r.u8()?,
                    timeout: r.i64()?,
                    broker_path: self.object_array(r)?,
                })
            }
            t::REMOVE_SUBSCRIPTION_INFO => {
                let header = self.header(r)?;
                Command::RemoveSubscriptionInfo(RemoveSubscriptionInfo {
                    header,
                    connection_id: self.opt_connection_id(r)?,
                    subscription_name: r.opt_string()?,
                    client_id: r.opt_string()?,
                })
            }
            t::KEEP_ALIVE_INFO => Command::KeepAliveInfo(self.header(r)?),
            t::SHUTDOWN_INFO => Command::ShutdownInfo(self.header(r)?),
            t::FLUSH_COMMAND => Command::FlushCommand(self.header(r)?),
            t::REMOVE_INFO => {
                let header = self.header(r)?;
                Command::RemoveInfo(RemoveInfo {
                    header,
                    object_id: self.nested(r)?,
                    last_delivered_sequence_id: r.i64()?,
                })
            }
            t::CONTROL_COMMAND => {
                let header = self.header(r)?;
                Command::ControlCommand(ControlCommand {
                    header,
                    command: r.opt_string()?,
                })
            }
            t::CONNECTION_ERROR => {
                let header = self.header(r)?;
                Command::ConnectionError(ConnectionError {
                    header,
                    exception: self.throwable(r)?,
                    connection_id: self.opt_connection_id(r)?,
                })
            }
            t::CONSUMER_CONTROL => {
                let header = self.header(r)?;
                Command::ConsumerControl(ConsumerControl {
                    header,
                    destination: self.opt_destination(r)?,
                    close: r.bool()?,
                    consumer_id: self.opt_consumer_id(r)?,
                    prefetch: r.i32()?,
                    flush: r.bool()?,
                    start: r.bool()?,
                    stop: r.bool()?,
                })
            }
            t::CONNECTION_CONTROL => {
                let header = self.header(r)?;
                Command::ConnectionControl(ConnectionControl {
                    header,
                    close: r.bool()?,
                    exit: r.bool()?,
                    fault_tolerant: r.bool()?,
                    resume: r.bool()?,
                    suspend: r.bool()?,
                    connected_brokers: r.opt_string()?,
                    reconnect_to: r.opt_string()?,
                    rebalance_connection: r.bool()?,
                    token: r.opt_bytes()?,
                })
            }
            t::PRODUCER_ACK => {
                let header = self.header(r)?;
                Command::ProducerAck(ProducerAck {
                    header,
                    producer_id: self.opt_producer_id(r)?,
                    size: r.i32()?,
                })
            }
            t::MESSAGE_PULL => {
                let header = self.header(r)?;
                Command::MessagePull(MessagePull {
                    header,
                    consumer_id: self.opt_consumer_id(r)?,
                    destination: self.opt_destination(r)?,
                    timeout: r.i64()?,
                    correlation_id: r.opt_string()?,
                    message_id: self.opt_message_id(r)?,
                })
            }
            t::MESSAGE_DISPATCH => {
                let header = self.header(r)?;
                let consumer_id = self.opt_consumer_id(r)?;
                let destination = self.opt_destination(r)?;
                let message = if r.bool()? {
                    let mt = r.u8()?;
                    if !t::is_message_type(mt) {
                        return err(format!("MessageDispatch carries non-message type {mt}"));
                    }
                    Some(Arc::new(self.message(mt, r)?))
                } else {
                    None
                };
                Command::MessageDispatch(MessageDispatch {
                    header,
                    consumer_id,
                    destination,
                    message,
                    redelivery_counter: r.i32()?,
                })
            }
            t::MESSAGE_ACK => {
                let header = self.header(r)?;
                Command::MessageAck(MessageAck {
                    header,
                    destination: self.opt_destination(r)?,
                    transaction_id: self.opt_transaction_id(r)?,
                    consumer_id: self.opt_consumer_id(r)?,
                    ack_type: r.u8()?,
                    first_message_id: self.opt_message_id(r)?,
                    last_message_id: self.opt_message_id(r)?,
                    message_count: r.i32()?,
                    poison_cause: self.throwable(r)?,
                })
            }
            mt if t::is_message_type(mt) => Command::Message(Box::new(self.message(mt, r)?)),
            t::RESPONSE => {
                let header = self.header(r)?;
                Command::Response {
                    header,
                    correlation_id: r.i32()?,
                }
            }
            t::EXCEPTION_RESPONSE => {
                let header = self.header(r)?;
                let correlation_id = r.i32()?;
                Command::ExceptionResponse {
                    header,
                    correlation_id,
                    exception: self.throwable(r)?,
                }
            }
            t::INTEGER_RESPONSE => {
                let header = self.header(r)?;
                let correlation_id = r.i32()?;
                Command::IntegerResponse {
                    header,
                    correlation_id,
                    result: r.i32()?,
                }
            }
            other => {
                // Unknown or unsupported command: only the BaseCommand header can be trusted.
                let header = if r.remaining() >= 5 {
                    self.header(r)?
                } else {
                    Header::default()
                };
                Command::Unsupported {
                    type_code: other,
                    header,
                }
            }
        })
    }

    fn broker_info(&self, r: &mut Reader) -> CodecResult<BrokerInfo> {
        let header = self.header(r)?;
        let broker_id = match self.nested(r)? {
            Some(DataStructure::BrokerId(b)) => Some(b),
            Some(_) => return err("BrokerInfo.brokerId has an unexpected type"),
            None => None,
        };
        Ok(BrokerInfo {
            header,
            broker_id,
            broker_url: r.opt_string()?,
            peer_broker_infos: self.object_array(r)?,
            broker_name: r.opt_string()?,
            slave_broker: r.bool()?,
            master_broker: r.bool()?,
            fault_tolerant_configuration: r.bool()?,
            duplex_connection: r.bool()?,
            network_connection: r.bool()?,
            connection_id: r.i64()?,
            broker_upload_url: r.opt_string()?,
            network_properties: r.opt_string()?,
        })
    }

    pub fn message(&self, msg_type: u8, r: &mut Reader) -> CodecResult<Message> {
        let header = self.header(r)?;
        let mut m = Message::new(msg_type);
        m.header = header;
        m.producer_id = self.opt_producer_id(r)?;
        m.destination = self.opt_destination(r)?;
        m.transaction_id = self.opt_transaction_id(r)?;
        m.original_destination = self.opt_destination(r)?;
        m.message_id = self.opt_message_id(r)?;
        // The message id normally repeats the producer's connection id: share one string.
        if let (
            Some(p),
            Some(MessageId {
                producer_id: Some(mp), ..
            }),
        ) = (&m.producer_id, &mut m.message_id)
        {
            if mp.connection_id == p.connection_id {
                mp.connection_id = p.connection_id.clone();
            }
        }
        m.original_transaction_id = self.opt_transaction_id(r)?;
        m.group_id = r.opt_string()?;
        m.group_sequence = r.i32()?;
        m.correlation_id = r.opt_string()?;
        m.persistent = r.bool()?;
        m.expiration = r.i64()?;
        m.priority = r.u8()?;
        m.reply_to = self.opt_destination(r)?;
        m.timestamp = r.i64()?;
        m.jms_type = r.opt_string()?;
        m.content = r.opt_bytes()?;
        m.marshalled_properties = r.opt_bytes()?;
        m.data_structure = self.nested(r)?;
        m.target_consumer_id = self.opt_consumer_id(r)?;
        m.compressed = r.bool()?;
        m.redelivery_counter = r.i32()?;
        m.broker_path = self.object_array(r)?;
        m.arrival = r.i64()?;
        m.user_id = r.opt_string()?;
        m.recieved_by_df_bridge = r.bool()?;
        m.droppable = r.bool()?;
        m.cluster = self.object_array(r)?;
        m.broker_in_time = r.i64()?;
        m.broker_out_time = r.i64()?;
        if self.version >= 10 {
            m.jmsx_group_first_for_consumer = r.bool()?;
        }
        Ok(m)
    }

    fn throwable(&self, r: &mut Reader) -> CodecResult<Option<Throwable>> {
        if !r.bool()? {
            return Ok(None);
        }
        let class_name = r.opt_string()?.unwrap_or_default();
        let message = r.opt_string()?;
        if self.stack_trace {
            let n = r.i16()?;
            for _ in 0..n.max(0) {
                r.opt_string()?;
                r.opt_string()?;
                r.opt_string()?;
                r.i32()?;
            }
            self.throwable(r)?;
        }
        Ok(Some(Throwable { class_name, message }))
    }

    fn object_array(&self, r: &mut Reader) -> CodecResult<Option<Vec<DataStructure>>> {
        if !r.bool()? {
            return Ok(None);
        }
        let n = r.i16()?;
        let mut v = Vec::with_capacity(n.max(0) as usize);
        for _ in 0..n.max(0) {
            if let Some(d) = self.nested(r)? {
                v.push(d);
            }
        }
        Ok(Some(v))
    }

    /// Loose nested (and, with the cache disabled, cached) object: flag, type, body.
    pub fn nested(&self, r: &mut Reader) -> CodecResult<Option<DataStructure>> {
        if !r.bool()? {
            return Ok(None);
        }
        let type_code = r.u8()?;
        self.data_structure(type_code, r).map(Some)
    }

    fn data_structure(&self, type_code: u8, r: &mut Reader) -> CodecResult<DataStructure> {
        Ok(match type_code {
            t::ACTIVEMQ_QUEUE | t::ACTIVEMQ_TOPIC | t::ACTIVEMQ_TEMP_QUEUE | t::ACTIVEMQ_TEMP_TOPIC => {
                let kind = DestKind::from_type_code(type_code).unwrap();
                let name = r.opt_arc_str()?.unwrap_or_else(|| Arc::from(""));
                DataStructure::Destination(Destination { kind, name })
            }
            t::CONNECTION_ID => DataStructure::ConnectionId(ConnectionId { value: self.arc(r)? }),
            t::SESSION_ID => DataStructure::SessionId(SessionId {
                connection_id: self.arc(r)?,
                value: r.i64()?,
            }),
            t::CONSUMER_ID => DataStructure::ConsumerId(ConsumerId {
                connection_id: self.arc(r)?,
                session_id: r.i64()?,
                value: r.i64()?,
            }),
            t::PRODUCER_ID => {
                // Wire order is connectionId, value, sessionId.
                let connection_id = self.arc(r)?;
                let value = r.i64()?;
                let session_id = r.i64()?;
                DataStructure::ProducerId(ProducerId {
                    connection_id,
                    session_id,
                    value,
                })
            }
            t::BROKER_ID => DataStructure::BrokerId(BrokerId { value: self.arc(r)? }),
            t::MESSAGE_ID => {
                let text_view = if self.version >= 10 { r.opt_arc_str()? } else { None };
                let producer_id = match self.nested(r)? {
                    Some(DataStructure::ProducerId(p)) => Some(p),
                    Some(_) => return err("MessageId.producerId has an unexpected type"),
                    None => None,
                };
                DataStructure::MessageId(MessageId {
                    text_view,
                    producer_id,
                    producer_sequence_id: r.i64()?,
                    broker_sequence_id: r.i64()?,
                })
            }
            t::ACTIVEMQ_LOCAL_TRANSACTION_ID => {
                let value = r.i64()?;
                let connection_id = match self.nested(r)? {
                    Some(DataStructure::ConnectionId(c)) => Some(c),
                    Some(_) => return err("LocalTransactionId.connectionId has an unexpected type"),
                    None => None,
                };
                DataStructure::TransactionId(TransactionId::Local { value, connection_id })
            }
            t::ACTIVEMQ_XA_TRANSACTION_ID => DataStructure::TransactionId(TransactionId::Xa {
                format_id: r.i32()?,
                global_transaction_id: r.opt_bytes()?,
                branch_qualifier: r.opt_bytes()?,
            }),
            t::BROKER_INFO => DataStructure::BrokerInfo(Box::new(self.broker_info(r)?)),
            t::DESTINATION_INFO => match self.command(t::DESTINATION_INFO, r)? {
                Command::DestinationInfo(d) => DataStructure::DestinationInfo(Box::new(d)),
                _ => return err("bad nested DestinationInfo"),
            },
            other => return err(format!("unsupported nested data structure type {other}")),
        })
    }

    fn arc(&self, r: &mut Reader) -> CodecResult<Arc<str>> {
        Ok(r.opt_arc_str()?.unwrap_or_else(|| Arc::from("")))
    }

    fn opt_destination(&self, r: &mut Reader) -> CodecResult<Option<Destination>> {
        match self.nested(r)? {
            Some(DataStructure::Destination(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a destination"),
        }
    }

    fn opt_connection_id(&self, r: &mut Reader) -> CodecResult<Option<ConnectionId>> {
        match self.nested(r)? {
            Some(DataStructure::ConnectionId(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a ConnectionId"),
        }
    }

    fn opt_session_id(&self, r: &mut Reader) -> CodecResult<Option<SessionId>> {
        match self.nested(r)? {
            Some(DataStructure::SessionId(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a SessionId"),
        }
    }

    fn opt_consumer_id(&self, r: &mut Reader) -> CodecResult<Option<ConsumerId>> {
        match self.nested(r)? {
            Some(DataStructure::ConsumerId(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a ConsumerId"),
        }
    }

    fn opt_producer_id(&self, r: &mut Reader) -> CodecResult<Option<ProducerId>> {
        match self.nested(r)? {
            Some(DataStructure::ProducerId(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a ProducerId"),
        }
    }

    fn opt_message_id(&self, r: &mut Reader) -> CodecResult<Option<MessageId>> {
        match self.nested(r)? {
            Some(DataStructure::MessageId(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a MessageId"),
        }
    }

    fn opt_transaction_id(&self, r: &mut Reader) -> CodecResult<Option<TransactionId>> {
        match self.nested(r)? {
            Some(DataStructure::TransactionId(d)) => Ok(Some(d)),
            None => Ok(None),
            Some(_) => err("expected a TransactionId"),
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Version-aware encoder producing complete frames (size prefix included).
#[derive(Clone, Copy)]
pub struct Encoder {
    pub version: i32,
}

impl Encoder {
    pub fn new(version: i32) -> Self {
        Encoder { version }
    }

    /// Appends a complete frame for `cmd` to `out`.
    pub fn encode_frame(&self, cmd: &Command, out: &mut BytesMut) {
        let start = out.len();
        out.put_i32(0);
        {
            let mut w = Writer::new(out);
            w.u8(cmd.type_code());
            self.command(cmd, &mut w);
        }
        let size = (out.len() - start - 4) as i32;
        out[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }

    /// Appends a frame to `out` without copying large message bodies.
    pub fn encode_frame_chunks(&self, cmd: &Command, out: &mut ChunkBuf) {
        let mut parts = std::mem::take(&mut out.parts);
        {
            let mut w = Writer::with_sink(&mut out.scratch, &mut parts);
            w.u8(cmd.type_code());
            self.command(cmd, &mut w);
        }
        if parts.is_empty() {
            // No large body: the frame is copied from the scratch buffer, which stays reusable.
            out.small.put_i32(out.scratch.len() as i32);
            out.small.put_slice(&out.scratch);
            out.scratch.clear();
            out.parts = parts;
            return;
        }
        parts.push(out.scratch.split().freeze());
        let size: usize = parts.iter().map(|p| p.len()).sum();
        out.small.put_i32(size as i32);
        for p in parts.drain(..) {
            if p.len() >= crate::openwire::codec::ZERO_COPY_MIN {
                out.push_big(p);
            } else {
                out.small.put_slice(&p);
            }
        }
        out.parts = parts;
    }

    pub fn frame(&self, cmd: &Command) -> Bytes {
        let mut out = BytesMut::with_capacity(128);
        self.encode_frame(cmd, &mut out);
        out.freeze()
    }

    fn header(&self, h: &Header, w: &mut Writer) {
        w.i32(h.command_id);
        w.bool(h.response_required);
    }

    fn command(&self, cmd: &Command, w: &mut Writer) {
        match cmd {
            Command::WireFormatInfo(c) => {
                w.raw(&c.magic);
                w.i32(c.version);
                let props = c.properties.encode();
                w.opt_bytes(Some(&props));
            }
            Command::BrokerInfo(c) => self.broker_info(c, w),
            Command::ConnectionInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.connection_id.as_ref().map(DsRef::ConnectionId), w);
                w.opt_string(c.client_id.as_deref());
                w.opt_string(c.password.as_deref());
                w.opt_string(c.user_name.as_deref());
                self.object_array(c.broker_path.as_deref(), w);
                w.bool(c.broker_master_connector);
                w.bool(c.manageable);
                w.bool(c.client_master);
                w.bool(c.fault_tolerant);
                w.bool(c.failover_reconnect);
                w.opt_string(c.client_ip.as_deref());
            }
            Command::SessionInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.session_id.as_ref().map(DsRef::SessionId), w);
            }
            Command::ConsumerInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.consumer_id.as_ref().map(DsRef::ConsumerId), w);
                w.bool(c.browser);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                w.i32(c.prefetch_size);
                w.i32(c.maximum_pending_message_limit);
                w.bool(c.dispatch_async);
                w.opt_string(c.selector.as_deref());
                if self.version >= 10 {
                    w.opt_string(c.client_id.as_deref());
                }
                w.opt_string(c.subscription_name.as_deref());
                w.bool(c.no_local);
                w.bool(c.exclusive);
                w.bool(c.retroactive);
                w.u8(c.priority);
                self.object_array(c.broker_path.as_deref(), w);
                self.opt(c.additional_predicate.as_ref().map(DsRef::Any), w);
                w.bool(c.network_subscription);
                w.bool(c.optimized_acknowledge);
                w.bool(c.no_range_acks);
                self.object_array(c.network_consumer_path.as_deref(), w);
            }
            Command::ProducerInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.producer_id.as_ref().map(DsRef::ProducerId), w);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                self.object_array(c.broker_path.as_deref(), w);
                w.bool(c.dispatch_async);
                w.i32(c.window_size);
            }
            Command::TransactionInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.connection_id.as_ref().map(DsRef::ConnectionId), w);
                self.opt(c.transaction_id.as_ref().map(DsRef::TransactionId), w);
                w.u8(c.tx_type);
            }
            Command::DestinationInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.connection_id.as_ref().map(DsRef::ConnectionId), w);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                w.u8(c.operation_type);
                w.i64(c.timeout);
                self.object_array(c.broker_path.as_deref(), w);
            }
            Command::RemoveSubscriptionInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.connection_id.as_ref().map(DsRef::ConnectionId), w);
                w.opt_string(c.subscription_name.as_deref());
                w.opt_string(c.client_id.as_deref());
            }
            Command::KeepAliveInfo(h) | Command::ShutdownInfo(h) | Command::FlushCommand(h) => self.header(h, w),
            Command::RemoveInfo(c) => {
                self.header(&c.header, w);
                self.opt(c.object_id.as_ref().map(DsRef::Any), w);
                w.i64(c.last_delivered_sequence_id);
            }
            Command::ControlCommand(c) => {
                self.header(&c.header, w);
                w.opt_string(c.command.as_deref());
            }
            Command::ConnectionError(c) => {
                self.header(&c.header, w);
                self.throwable(c.exception.as_ref(), w);
                self.opt(c.connection_id.as_ref().map(DsRef::ConnectionId), w);
            }
            Command::ConsumerControl(c) => {
                self.header(&c.header, w);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                w.bool(c.close);
                self.opt(c.consumer_id.as_ref().map(DsRef::ConsumerId), w);
                w.i32(c.prefetch);
                w.bool(c.flush);
                w.bool(c.start);
                w.bool(c.stop);
            }
            Command::ConnectionControl(c) => {
                self.header(&c.header, w);
                w.bool(c.close);
                w.bool(c.exit);
                w.bool(c.fault_tolerant);
                w.bool(c.resume);
                w.bool(c.suspend);
                w.opt_string(c.connected_brokers.as_deref());
                w.opt_string(c.reconnect_to.as_deref());
                w.bool(c.rebalance_connection);
                w.opt_bytes(c.token.as_deref());
            }
            Command::ProducerAck(c) => {
                self.header(&c.header, w);
                self.opt(c.producer_id.as_ref().map(DsRef::ProducerId), w);
                w.i32(c.size);
            }
            Command::MessagePull(c) => {
                self.header(&c.header, w);
                self.opt(c.consumer_id.as_ref().map(DsRef::ConsumerId), w);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                w.i64(c.timeout);
                w.opt_string(c.correlation_id.as_deref());
                self.opt(c.message_id.as_ref().map(DsRef::MessageId), w);
            }
            Command::MessageDispatch(c) => {
                self.header(&c.header, w);
                self.opt(c.consumer_id.as_ref().map(DsRef::ConsumerId), w);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                match &c.message {
                    Some(m) => {
                        w.bool(true);
                        w.u8(m.msg_type);
                        self.message(m, w);
                    }
                    None => w.bool(false),
                }
                w.i32(c.redelivery_counter);
            }
            Command::MessageAck(c) => {
                self.header(&c.header, w);
                self.opt(c.destination.as_ref().map(DsRef::Destination), w);
                self.opt(c.transaction_id.as_ref().map(DsRef::TransactionId), w);
                self.opt(c.consumer_id.as_ref().map(DsRef::ConsumerId), w);
                w.u8(c.ack_type);
                self.opt(c.first_message_id.as_ref().map(DsRef::MessageId), w);
                self.opt(c.last_message_id.as_ref().map(DsRef::MessageId), w);
                w.i32(c.message_count);
                self.throwable(c.poison_cause.as_ref(), w);
            }
            Command::Message(m) => self.message(m, w),
            Command::Response { header, correlation_id } => {
                self.header(header, w);
                w.i32(*correlation_id);
            }
            Command::ExceptionResponse {
                header,
                correlation_id,
                exception,
            } => {
                self.header(header, w);
                w.i32(*correlation_id);
                self.throwable(exception.as_ref(), w);
            }
            Command::IntegerResponse {
                header,
                correlation_id,
                result,
            } => {
                self.header(header, w);
                w.i32(*correlation_id);
                w.i32(*result);
            }
            Command::Unsupported { header, .. } => self.header(header, w),
        }
    }

    fn broker_info(&self, c: &BrokerInfo, w: &mut Writer) {
        self.header(&c.header, w);
        self.opt(c.broker_id.as_ref().map(DsRef::BrokerId), w);
        w.opt_string(c.broker_url.as_deref());
        self.object_array(c.peer_broker_infos.as_deref(), w);
        w.opt_string(c.broker_name.as_deref());
        w.bool(c.slave_broker);
        w.bool(c.master_broker);
        w.bool(c.fault_tolerant_configuration);
        w.bool(c.duplex_connection);
        w.bool(c.network_connection);
        w.i64(c.connection_id);
        w.opt_string(c.broker_upload_url.as_deref());
        w.opt_string(c.network_properties.as_deref());
    }

    /// Writes a message body (without the leading type byte).
    pub fn message(&self, m: &Message, w: &mut Writer) {
        self.header(&m.header, w);
        self.opt(m.producer_id.as_ref().map(DsRef::ProducerId), w);
        self.opt(m.destination.as_ref().map(DsRef::Destination), w);
        self.opt(m.transaction_id.as_ref().map(DsRef::TransactionId), w);
        self.opt(m.original_destination.as_ref().map(DsRef::Destination), w);
        self.opt(m.message_id.as_ref().map(DsRef::MessageId), w);
        self.opt(m.original_transaction_id.as_ref().map(DsRef::TransactionId), w);
        w.opt_string(m.group_id.as_deref());
        w.i32(m.group_sequence);
        w.opt_string(m.correlation_id.as_deref());
        w.bool(m.persistent);
        w.i64(m.expiration);
        w.u8(m.priority);
        self.opt(m.reply_to.as_ref().map(DsRef::Destination), w);
        w.i64(m.timestamp);
        w.opt_string(m.jms_type.as_deref());
        w.shared_bytes(m.content.as_ref());
        w.shared_bytes(m.marshalled_properties.as_ref());
        self.opt(m.data_structure.as_ref().map(DsRef::Any), w);
        self.opt(m.target_consumer_id.as_ref().map(DsRef::ConsumerId), w);
        w.bool(m.compressed);
        w.i32(m.redelivery_counter);
        self.object_array(m.broker_path.as_deref(), w);
        w.i64(m.arrival);
        w.opt_string(m.user_id.as_deref());
        w.bool(m.recieved_by_df_bridge);
        w.bool(m.droppable);
        self.object_array(m.cluster.as_deref(), w);
        w.i64(m.broker_in_time);
        w.i64(m.broker_out_time);
        if self.version >= 10 {
            w.bool(m.jmsx_group_first_for_consumer);
        }
    }

    fn throwable(&self, t: Option<&Throwable>, w: &mut Writer) {
        match t {
            Some(t) => {
                w.bool(true);
                w.opt_string(Some(&t.class_name));
                w.opt_string(t.message.as_deref());
                // Stack traces are never negotiated by this broker.
            }
            None => w.bool(false),
        }
    }

    fn object_array(&self, objects: Option<&[DataStructure]>, w: &mut Writer) {
        match objects {
            Some(list) => {
                w.bool(true);
                w.u16(list.len() as u16);
                for o in list {
                    self.opt(Some(DsRef::Any(o)), w);
                }
            }
            None => w.bool(false),
        }
    }

    fn opt(&self, d: Option<DsRef>, w: &mut Writer) {
        match d {
            Some(d) => {
                w.bool(true);
                self.body(d, w);
            }
            None => w.bool(false),
        }
    }

    /// Writes the type byte and body of a nested structure.
    fn body(&self, d: DsRef, w: &mut Writer) {
        match d {
            DsRef::Destination(dest) => {
                w.u8(dest.kind.type_code());
                w.opt_string(Some(&dest.name));
            }
            DsRef::ConnectionId(c) => {
                w.u8(t::CONNECTION_ID);
                w.opt_string(Some(&c.value));
            }
            DsRef::SessionId(s) => {
                w.u8(t::SESSION_ID);
                w.opt_string(Some(&s.connection_id));
                w.i64(s.value);
            }
            DsRef::ConsumerId(c) => {
                w.u8(t::CONSUMER_ID);
                w.opt_string(Some(&c.connection_id));
                w.i64(c.session_id);
                w.i64(c.value);
            }
            DsRef::ProducerId(p) => {
                // Wire order is connectionId, value, sessionId.
                w.u8(t::PRODUCER_ID);
                w.opt_string(Some(&p.connection_id));
                w.i64(p.value);
                w.i64(p.session_id);
            }
            DsRef::BrokerId(b) => {
                w.u8(t::BROKER_ID);
                w.opt_string(Some(&b.value));
            }
            DsRef::MessageId(m) => {
                w.u8(t::MESSAGE_ID);
                if self.version >= 10 {
                    w.opt_string(m.text_view.as_deref());
                }
                self.opt(m.producer_id.as_ref().map(DsRef::ProducerId), w);
                w.i64(m.producer_sequence_id);
                w.i64(m.broker_sequence_id);
            }
            DsRef::TransactionId(TransactionId::Local { value, connection_id }) => {
                w.u8(t::ACTIVEMQ_LOCAL_TRANSACTION_ID);
                w.i64(*value);
                self.opt(connection_id.as_ref().map(DsRef::ConnectionId), w);
            }
            DsRef::TransactionId(TransactionId::Xa {
                format_id,
                global_transaction_id,
                branch_qualifier,
            }) => {
                w.u8(t::ACTIVEMQ_XA_TRANSACTION_ID);
                w.i32(*format_id);
                w.opt_bytes(global_transaction_id.as_deref());
                w.opt_bytes(branch_qualifier.as_deref());
            }
            DsRef::Any(any) => match any {
                DataStructure::Destination(d) => self.body(DsRef::Destination(d), w),
                DataStructure::ConnectionId(d) => self.body(DsRef::ConnectionId(d), w),
                DataStructure::SessionId(d) => self.body(DsRef::SessionId(d), w),
                DataStructure::ConsumerId(d) => self.body(DsRef::ConsumerId(d), w),
                DataStructure::ProducerId(d) => self.body(DsRef::ProducerId(d), w),
                DataStructure::MessageId(d) => self.body(DsRef::MessageId(d), w),
                DataStructure::TransactionId(d) => self.body(DsRef::TransactionId(d), w),
                DataStructure::BrokerId(d) => self.body(DsRef::BrokerId(d), w),
                DataStructure::BrokerInfo(b) => {
                    w.u8(t::BROKER_INFO);
                    self.broker_info(b, w);
                }
                DataStructure::DestinationInfo(d) => {
                    w.u8(t::DESTINATION_INFO);
                    self.command(&Command::DestinationInfo((**d).clone()), w);
                }
            },
        }
    }
}

/// Output of zero-copy encoding: small pieces are coalesced, large bodies are referenced.
#[derive(Default)]
pub struct ChunkBuf {
    small: BytesMut,
    scratch: BytesMut,
    /// Reusable list of the pieces of one frame.
    parts: Vec<Bytes>,
    chunks: std::collections::VecDeque<Bytes>,
}

impl ChunkBuf {
    pub fn new() -> Self {
        ChunkBuf {
            small: BytesMut::with_capacity(64 * 1024),
            scratch: BytesMut::with_capacity(4096),
            parts: Vec::new(),
            chunks: Default::default(),
        }
    }

    fn push_big(&mut self, b: Bytes) {
        if !self.small.is_empty() {
            self.chunks.push_back(self.small.split().freeze());
        }
        self.chunks.push_back(b);
    }

    /// Bytes queued so far.
    pub fn len(&self) -> usize {
        self.small.len() + self.chunks.iter().map(|c| c.len()).sum::<usize>()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes everything queued, in order.
    pub fn take(&mut self) -> std::collections::VecDeque<Bytes> {
        if !self.small.is_empty() {
            self.chunks.push_back(self.small.split().freeze());
        }
        std::mem::take(&mut self.chunks)
    }
}

/// Borrowed view of a nested structure to encode.
enum DsRef<'a> {
    Destination(&'a Destination),
    ConnectionId(&'a ConnectionId),
    SessionId(&'a SessionId),
    ConsumerId(&'a ConsumerId),
    ProducerId(&'a ProducerId),
    BrokerId(&'a BrokerId),
    MessageId(&'a MessageId),
    TransactionId(&'a TransactionId),
    Any(&'a DataStructure),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openwire::props::Value;

    fn round_trip(cmd: &Command, version: i32) -> Command {
        let frame = Encoder::new(version).frame(cmd);
        let size = i32::from_be_bytes(frame[0..4].try_into().unwrap()) as usize;
        assert_eq!(size, frame.len() - 4);
        Decoder::new(version).decode_frame(frame.slice(4..)).unwrap().unwrap()
    }

    fn sample_message() -> Message {
        let pid = ProducerId {
            connection_id: Arc::from("ID:host-1-2-1:1"),
            session_id: 1,
            value: 3,
        };
        let mut m = Message::new(t::ACTIVEMQ_TEXT_MESSAGE);
        m.header = Header {
            command_id: 7,
            response_required: true,
        };
        m.producer_id = Some(pid.clone());
        m.destination = Some(Destination::queue("TEST.Q"));
        m.message_id = Some(MessageId {
            text_view: None,
            producer_id: Some(pid),
            producer_sequence_id: 42,
            broker_sequence_id: 0,
        });
        m.correlation_id = Some("ORD-A".into());
        m.persistent = true;
        m.expiration = 123;
        m.timestamp = 456;
        m.content = Some(Bytes::from_static(b"\x00\x00\x00\x05hello"));
        let mut props = PrimitiveMap::new();
        props.set("seq", Value::Int(1));
        m.marshalled_properties = Some(props.encode());
        m.jmsx_group_first_for_consumer = true;
        m
    }

    #[test]
    fn message_round_trip_v12_and_v9() {
        let m = sample_message();
        match round_trip(&Command::Message(Box::new(m.clone())), 12) {
            Command::Message(back) => assert_eq!(*back, m),
            other => panic!("unexpected {other:?}"),
        }
        let mut m9 = m.clone();
        m9.jmsx_group_first_for_consumer = false;
        match round_trip(&Command::Message(Box::new(m9.clone())), 9) {
            Command::Message(back) => assert_eq!(*back, m9),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn zero_copy_frames_match_copied_frames() {
        let mut m = sample_message();
        m.content = Some(Bytes::from(vec![9u8; 50_000]));
        let cmd = Command::Message(Box::new(m));
        let enc = Encoder::new(12);
        let mut cb = ChunkBuf::new();
        enc.encode_frame_chunks(&cmd, &mut cb);
        enc.encode_frame_chunks(&Command::response(3), &mut cb);
        let joined: Vec<u8> = cb.take().into_iter().flat_map(|b| b.to_vec()).collect();
        let mut expected = enc.frame(&cmd).to_vec();
        expected.extend_from_slice(&enc.frame(&Command::response(3)));
        assert_eq!(joined, expected);
    }

    #[test]
    fn message_id_text_matches_activemq() {
        let m = sample_message();
        assert_eq!(m.message_id_text(), "ID:host-1-2-1:1:1:3:42");
    }

    #[test]
    fn consumer_info_round_trip() {
        let ci = ConsumerInfo {
            header: Header {
                command_id: 3,
                response_required: true,
            },
            consumer_id: Some(ConsumerId {
                connection_id: Arc::from("c"),
                session_id: 1,
                value: 2,
            }),
            browser: false,
            destination: Some(Destination::queue("Q")),
            prefetch_size: 1000,
            maximum_pending_message_limit: 0,
            dispatch_async: true,
            selector: Some("a = 1".into()),
            client_id: None,
            subscription_name: None,
            no_local: false,
            exclusive: false,
            retroactive: false,
            priority: 0,
            broker_path: None,
            additional_predicate: None,
            network_subscription: false,
            optimized_acknowledge: false,
            no_range_acks: false,
            network_consumer_path: None,
        };
        match round_trip(&Command::ConsumerInfo(ci.clone()), 12) {
            Command::ConsumerInfo(back) => assert_eq!(back, ci),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn exception_response_round_trip() {
        let cmd = Command::exception(5, "java.lang.SecurityException", "bad");
        match round_trip(&cmd, 12) {
            Command::ExceptionResponse {
                correlation_id,
                exception,
                ..
            } => {
                assert_eq!(correlation_id, 5);
                assert_eq!(exception.unwrap().class_name, "java.lang.SecurityException");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
