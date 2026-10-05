// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! OpenWire data structures and commands used by the broker.

use bytes::Bytes;
use std::fmt;
use std::sync::Arc;

use super::types;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId {
    pub value: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId {
    pub connection_id: Arc<str>,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConsumerId {
    pub connection_id: Arc<str>,
    pub session_id: i64,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProducerId {
    pub connection_id: Arc<str>,
    pub session_id: i64,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BrokerId {
    pub value: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId {
    pub text_view: Option<Arc<str>>,
    pub producer_id: Option<ProducerId>,
    pub producer_sequence_id: i64,
    pub broker_sequence_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionId {
    Local {
        value: i64,
        connection_id: Option<ConnectionId>,
    },
    Xa {
        format_id: i32,
        global_transaction_id: Option<Bytes>,
        branch_qualifier: Option<Bytes>,
    },
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.connection_id, self.value)
    }
}

impl fmt::Display for ConsumerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.connection_id, self.session_id, self.value)
    }
}

impl fmt::Display for ProducerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.connection_id, self.session_id, self.value)
    }
}

impl fmt::Display for MessageId {
    /// Same text as `MessageId.toString()` in ActiveMQ.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(tv) = &self.text_view {
            if tv.starts_with("ID:") {
                f.write_str(tv)
            } else {
                write!(f, "ID:{tv}")
            }
        } else {
            match &self.producer_id {
                Some(p) => write!(f, "{}:{}", p, self.producer_sequence_id),
                None => write!(f, "null:{}", self.producer_sequence_id),
            }
        }
    }
}

impl SessionId {
    pub fn of_consumer(c: &ConsumerId) -> SessionId {
        SessionId {
            connection_id: c.connection_id.clone(),
            value: c.session_id,
        }
    }
    pub fn of_producer(p: &ProducerId) -> SessionId {
        SessionId {
            connection_id: p.connection_id.clone(),
            value: p.session_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Destinations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DestKind {
    Queue,
    Topic,
    TempQueue,
    TempTopic,
}

impl DestKind {
    pub fn type_code(self) -> u8 {
        match self {
            DestKind::Queue => types::ACTIVEMQ_QUEUE,
            DestKind::Topic => types::ACTIVEMQ_TOPIC,
            DestKind::TempQueue => types::ACTIVEMQ_TEMP_QUEUE,
            DestKind::TempTopic => types::ACTIVEMQ_TEMP_TOPIC,
        }
    }

    pub fn from_type_code(t: u8) -> Option<DestKind> {
        match t {
            types::ACTIVEMQ_QUEUE => Some(DestKind::Queue),
            types::ACTIVEMQ_TOPIC => Some(DestKind::Topic),
            types::ACTIVEMQ_TEMP_QUEUE => Some(DestKind::TempQueue),
            types::ACTIVEMQ_TEMP_TOPIC => Some(DestKind::TempTopic),
            _ => None,
        }
    }

    pub fn is_queue(self) -> bool {
        matches!(self, DestKind::Queue | DestKind::TempQueue)
    }

    pub fn is_topic(self) -> bool {
        !self.is_queue()
    }

    pub fn is_temporary(self) -> bool {
        matches!(self, DestKind::TempQueue | DestKind::TempTopic)
    }

    pub fn prefix(self) -> &'static str {
        match self {
            DestKind::Queue => "queue://",
            DestKind::Topic => "topic://",
            DestKind::TempQueue => "temp-queue://",
            DestKind::TempTopic => "temp-topic://",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Destination {
    pub kind: DestKind,
    pub name: Arc<str>,
}

impl Destination {
    pub fn new(kind: DestKind, name: &str) -> Destination {
        Destination {
            kind,
            name: Arc::from(name),
        }
    }

    pub fn queue(name: &str) -> Destination {
        Destination::new(DestKind::Queue, name)
    }

    pub fn is_composite(&self) -> bool {
        self.name.contains(',')
    }

    pub fn is_wildcard(&self) -> bool {
        self.name.contains('*') || self.name.contains('>')
    }

    pub fn is_advisory(&self) -> bool {
        self.kind.is_topic() && self.name.starts_with("ActiveMQ.Advisory.")
    }
}

impl fmt::Display for Destination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.kind.prefix(), self.name)
    }
}

// ---------------------------------------------------------------------------
// Generic nested data structures
// ---------------------------------------------------------------------------

/// A nested object whose type is only known at run time.
#[derive(Debug, Clone, PartialEq)]
pub enum DataStructure {
    Destination(Destination),
    ConnectionId(ConnectionId),
    SessionId(SessionId),
    ConsumerId(ConsumerId),
    ProducerId(ProducerId),
    MessageId(MessageId),
    TransactionId(TransactionId),
    BrokerId(BrokerId),
    BrokerInfo(Box<BrokerInfo>),
    /// Carried by temporary-destination advisory messages.
    DestinationInfo(Box<DestinationInfo>),
}

/// A Java exception carried on the wire (class name and message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Throwable {
    pub class_name: String,
    pub message: Option<String>,
}

impl Throwable {
    pub fn new(class_name: &str, message: impl Into<String>) -> Throwable {
        Throwable {
            class_name: class_name.to_string(),
            message: Some(message.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Header {
    pub command_id: i32,
    pub response_required: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireFormatInfo {
    pub magic: [u8; 8],
    pub version: i32,
    pub properties: super::props::PrimitiveMap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BrokerInfo {
    pub header: Header,
    pub broker_id: Option<BrokerId>,
    pub broker_url: Option<String>,
    pub peer_broker_infos: Option<Vec<DataStructure>>,
    pub broker_name: Option<String>,
    pub slave_broker: bool,
    pub master_broker: bool,
    pub fault_tolerant_configuration: bool,
    pub duplex_connection: bool,
    pub network_connection: bool,
    pub connection_id: i64,
    pub broker_upload_url: Option<String>,
    pub network_properties: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionInfo {
    pub header: Header,
    pub connection_id: Option<ConnectionId>,
    pub client_id: Option<String>,
    pub password: Option<String>,
    pub user_name: Option<String>,
    pub broker_path: Option<Vec<DataStructure>>,
    pub broker_master_connector: bool,
    pub manageable: bool,
    pub client_master: bool,
    pub fault_tolerant: bool,
    pub failover_reconnect: bool,
    pub client_ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub header: Header,
    pub session_id: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumerInfo {
    pub header: Header,
    pub consumer_id: Option<ConsumerId>,
    pub browser: bool,
    pub destination: Option<Destination>,
    pub prefetch_size: i32,
    pub maximum_pending_message_limit: i32,
    pub dispatch_async: bool,
    pub selector: Option<String>,
    pub client_id: Option<String>,
    pub subscription_name: Option<String>,
    pub no_local: bool,
    pub exclusive: bool,
    pub retroactive: bool,
    pub priority: u8,
    pub broker_path: Option<Vec<DataStructure>>,
    pub additional_predicate: Option<DataStructure>,
    pub network_subscription: bool,
    pub optimized_acknowledge: bool,
    pub no_range_acks: bool,
    pub network_consumer_path: Option<Vec<DataStructure>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProducerInfo {
    pub header: Header,
    pub producer_id: Option<ProducerId>,
    pub destination: Option<Destination>,
    pub broker_path: Option<Vec<DataStructure>>,
    pub dispatch_async: bool,
    pub window_size: i32,
}

pub mod tx_type {
    pub const BEGIN: u8 = 0;
    pub const PREPARE: u8 = 1;
    pub const COMMIT_ONE_PHASE: u8 = 2;
    pub const COMMIT_TWO_PHASE: u8 = 3;
    pub const ROLLBACK: u8 = 4;
    pub const RECOVER: u8 = 5;
    pub const FORGET: u8 = 6;
    pub const END: u8 = 7;
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransactionInfo {
    pub header: Header,
    pub connection_id: Option<ConnectionId>,
    pub transaction_id: Option<TransactionId>,
    pub tx_type: u8,
}

pub mod dest_op {
    pub const ADD: u8 = 0;
    pub const REMOVE: u8 = 1;
}

#[derive(Debug, Clone, PartialEq)]
pub struct DestinationInfo {
    pub header: Header,
    pub connection_id: Option<ConnectionId>,
    pub destination: Option<Destination>,
    pub operation_type: u8,
    pub timeout: i64,
    pub broker_path: Option<Vec<DataStructure>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoveSubscriptionInfo {
    pub header: Header,
    pub connection_id: Option<ConnectionId>,
    pub subscription_name: Option<String>,
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoveInfo {
    pub header: Header,
    pub object_id: Option<DataStructure>,
    pub last_delivered_sequence_id: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsumerControl {
    pub header: Header,
    pub destination: Option<Destination>,
    pub close: bool,
    pub consumer_id: Option<ConsumerId>,
    pub prefetch: i32,
    pub flush: bool,
    pub start: bool,
    pub stop: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionControl {
    pub header: Header,
    pub close: bool,
    pub exit: bool,
    pub fault_tolerant: bool,
    pub resume: bool,
    pub suspend: bool,
    pub connected_brokers: Option<String>,
    pub reconnect_to: Option<String>,
    pub rebalance_connection: bool,
    pub token: Option<Bytes>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProducerAck {
    pub header: Header,
    pub producer_id: Option<ProducerId>,
    pub size: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessagePull {
    pub header: Header,
    pub consumer_id: Option<ConsumerId>,
    pub destination: Option<Destination>,
    pub timeout: i64,
    pub correlation_id: Option<String>,
    pub message_id: Option<MessageId>,
}

#[derive(Debug, Clone)]
pub struct MessageDispatch {
    pub header: Header,
    pub consumer_id: Option<ConsumerId>,
    pub destination: Option<Destination>,
    pub message: Option<Arc<Message>>,
    pub redelivery_counter: i32,
}

pub mod ack_type {
    pub const DELIVERED: u8 = 0;
    pub const POISON: u8 = 1;
    pub const STANDARD: u8 = 2;
    pub const REDELIVERED: u8 = 3;
    pub const INDIVIDUAL: u8 = 4;
    pub const UNMATCHED: u8 = 5;
    pub const EXPIRED: u8 = 6;
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageAck {
    pub header: Header,
    pub destination: Option<Destination>,
    pub transaction_id: Option<TransactionId>,
    pub consumer_id: Option<ConsumerId>,
    pub ack_type: u8,
    pub first_message_id: Option<MessageId>,
    pub last_message_id: Option<MessageId>,
    pub message_count: i32,
    pub poison_cause: Option<Throwable>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControlCommand {
    pub header: Header,
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionError {
    pub header: Header,
    pub exception: Option<Throwable>,
    pub connection_id: Option<ConnectionId>,
}

/// An ActiveMQ message of any JMS type (`msg_type` 23..29).
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub msg_type: u8,
    pub header: Header,
    pub producer_id: Option<ProducerId>,
    pub destination: Option<Destination>,
    pub transaction_id: Option<TransactionId>,
    pub original_destination: Option<Destination>,
    pub message_id: Option<MessageId>,
    pub original_transaction_id: Option<TransactionId>,
    pub group_id: Option<String>,
    pub group_sequence: i32,
    pub correlation_id: Option<String>,
    pub persistent: bool,
    pub expiration: i64,
    pub priority: u8,
    pub reply_to: Option<Destination>,
    pub timestamp: i64,
    pub jms_type: Option<String>,
    pub content: Option<Bytes>,
    pub marshalled_properties: Option<Bytes>,
    pub data_structure: Option<DataStructure>,
    pub target_consumer_id: Option<ConsumerId>,
    pub compressed: bool,
    pub redelivery_counter: i32,
    pub broker_path: Option<Vec<DataStructure>>,
    pub arrival: i64,
    pub user_id: Option<String>,
    pub recieved_by_df_bridge: bool,
    pub droppable: bool,
    pub cluster: Option<Vec<DataStructure>>,
    pub broker_in_time: i64,
    pub broker_out_time: i64,
    pub jmsx_group_first_for_consumer: bool,
}

impl Message {
    pub fn new(msg_type: u8) -> Message {
        Message {
            msg_type,
            header: Header::default(),
            producer_id: None,
            destination: None,
            transaction_id: None,
            original_destination: None,
            message_id: None,
            original_transaction_id: None,
            group_id: None,
            group_sequence: 0,
            correlation_id: None,
            persistent: false,
            expiration: 0,
            priority: 4,
            reply_to: None,
            timestamp: 0,
            jms_type: None,
            content: None,
            marshalled_properties: None,
            data_structure: None,
            target_consumer_id: None,
            compressed: false,
            redelivery_counter: 0,
            broker_path: None,
            arrival: 0,
            user_id: None,
            recieved_by_df_bridge: false,
            droppable: false,
            cluster: None,
            broker_in_time: 0,
            broker_out_time: 0,
            jmsx_group_first_for_consumer: false,
        }
    }

    pub fn content_len(&self) -> usize {
        self.content.as_ref().map_or(0, |c| c.len())
    }

    pub fn properties_len(&self) -> usize {
        self.marshalled_properties.as_ref().map_or(0, |c| c.len())
    }

    pub fn message_id_text(&self) -> String {
        self.message_id.as_ref().map(|m| m.to_string()).unwrap_or_default()
    }

    pub fn type_name(&self) -> &'static str {
        match self.msg_type {
            types::ACTIVEMQ_TEXT_MESSAGE => "TextMessage",
            types::ACTIVEMQ_BYTES_MESSAGE => "BytesMessage",
            types::ACTIVEMQ_MAP_MESSAGE => "MapMessage",
            types::ACTIVEMQ_OBJECT_MESSAGE => "ObjectMessage",
            types::ACTIVEMQ_STREAM_MESSAGE => "StreamMessage",
            types::ACTIVEMQ_BLOB_MESSAGE => "BlobMessage",
            _ => "Message",
        }
    }
}

/// Every command the broker can receive or send.
#[derive(Debug, Clone)]
pub enum Command {
    WireFormatInfo(WireFormatInfo),
    BrokerInfo(BrokerInfo),
    ConnectionInfo(ConnectionInfo),
    SessionInfo(SessionInfo),
    ConsumerInfo(ConsumerInfo),
    ProducerInfo(ProducerInfo),
    TransactionInfo(TransactionInfo),
    DestinationInfo(DestinationInfo),
    RemoveSubscriptionInfo(RemoveSubscriptionInfo),
    KeepAliveInfo(Header),
    ShutdownInfo(Header),
    RemoveInfo(RemoveInfo),
    ControlCommand(ControlCommand),
    FlushCommand(Header),
    ConnectionError(ConnectionError),
    ConsumerControl(ConsumerControl),
    ConnectionControl(ConnectionControl),
    ProducerAck(ProducerAck),
    MessagePull(MessagePull),
    MessageDispatch(MessageDispatch),
    MessageAck(MessageAck),
    Message(Box<Message>),
    Response {
        header: Header,
        correlation_id: i32,
    },
    ExceptionResponse {
        header: Header,
        correlation_id: i32,
        exception: Option<Throwable>,
    },
    IntegerResponse {
        header: Header,
        correlation_id: i32,
        result: i32,
    },
    /// A command the broker decodes only to answer it (type code and header).
    Unsupported {
        type_code: u8,
        header: Header,
    },
}

impl Command {
    pub fn header(&self) -> Header {
        match self {
            Command::WireFormatInfo(_) => Header::default(),
            Command::BrokerInfo(c) => c.header,
            Command::ConnectionInfo(c) => c.header,
            Command::SessionInfo(c) => c.header,
            Command::ConsumerInfo(c) => c.header,
            Command::ProducerInfo(c) => c.header,
            Command::TransactionInfo(c) => c.header,
            Command::DestinationInfo(c) => c.header,
            Command::RemoveSubscriptionInfo(c) => c.header,
            Command::KeepAliveInfo(h) => *h,
            Command::ShutdownInfo(h) => *h,
            Command::RemoveInfo(c) => c.header,
            Command::ControlCommand(c) => c.header,
            Command::FlushCommand(h) => *h,
            Command::ConnectionError(c) => c.header,
            Command::ConsumerControl(c) => c.header,
            Command::ConnectionControl(c) => c.header,
            Command::ProducerAck(c) => c.header,
            Command::MessagePull(c) => c.header,
            Command::MessageDispatch(c) => c.header,
            Command::MessageAck(c) => c.header,
            Command::Message(m) => m.header,
            Command::Response { header, .. } => *header,
            Command::ExceptionResponse { header, .. } => *header,
            Command::IntegerResponse { header, .. } => *header,
            Command::Unsupported { header, .. } => *header,
        }
    }

    pub fn type_code(&self) -> u8 {
        match self {
            Command::WireFormatInfo(_) => types::WIREFORMAT_INFO,
            Command::BrokerInfo(_) => types::BROKER_INFO,
            Command::ConnectionInfo(_) => types::CONNECTION_INFO,
            Command::SessionInfo(_) => types::SESSION_INFO,
            Command::ConsumerInfo(_) => types::CONSUMER_INFO,
            Command::ProducerInfo(_) => types::PRODUCER_INFO,
            Command::TransactionInfo(_) => types::TRANSACTION_INFO,
            Command::DestinationInfo(_) => types::DESTINATION_INFO,
            Command::RemoveSubscriptionInfo(_) => types::REMOVE_SUBSCRIPTION_INFO,
            Command::KeepAliveInfo(_) => types::KEEP_ALIVE_INFO,
            Command::ShutdownInfo(_) => types::SHUTDOWN_INFO,
            Command::RemoveInfo(_) => types::REMOVE_INFO,
            Command::ControlCommand(_) => types::CONTROL_COMMAND,
            Command::FlushCommand(_) => types::FLUSH_COMMAND,
            Command::ConnectionError(_) => types::CONNECTION_ERROR,
            Command::ConsumerControl(_) => types::CONSUMER_CONTROL,
            Command::ConnectionControl(_) => types::CONNECTION_CONTROL,
            Command::ProducerAck(_) => types::PRODUCER_ACK,
            Command::MessagePull(_) => types::MESSAGE_PULL,
            Command::MessageDispatch(_) => types::MESSAGE_DISPATCH,
            Command::MessageAck(_) => types::MESSAGE_ACK,
            Command::Message(m) => m.msg_type,
            Command::Response { .. } => types::RESPONSE,
            Command::ExceptionResponse { .. } => types::EXCEPTION_RESPONSE,
            Command::IntegerResponse { .. } => types::INTEGER_RESPONSE,
            Command::Unsupported { type_code, .. } => *type_code,
        }
    }

    pub fn response(correlation_id: i32) -> Command {
        Command::Response {
            header: Header::default(),
            correlation_id,
        }
    }

    pub fn exception(correlation_id: i32, class_name: &str, message: impl Into<String>) -> Command {
        Command::ExceptionResponse {
            header: Header::default(),
            correlation_id,
            exception: Some(Throwable::new(class_name, message)),
        }
    }
}
