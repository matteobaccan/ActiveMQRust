// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! OpenWire data structure type codes (`org.apache.activemq.command.CommandTypes`).

pub const NULL: u8 = 0;
pub const WIREFORMAT_INFO: u8 = 1;
pub const BROKER_INFO: u8 = 2;
pub const CONNECTION_INFO: u8 = 3;
pub const SESSION_INFO: u8 = 4;
pub const CONSUMER_INFO: u8 = 5;
pub const PRODUCER_INFO: u8 = 6;
pub const TRANSACTION_INFO: u8 = 7;
pub const DESTINATION_INFO: u8 = 8;
pub const REMOVE_SUBSCRIPTION_INFO: u8 = 9;
pub const KEEP_ALIVE_INFO: u8 = 10;
pub const SHUTDOWN_INFO: u8 = 11;
pub const REMOVE_INFO: u8 = 12;
pub const CONTROL_COMMAND: u8 = 14;
pub const FLUSH_COMMAND: u8 = 15;
pub const CONNECTION_ERROR: u8 = 16;
pub const CONSUMER_CONTROL: u8 = 17;
pub const CONNECTION_CONTROL: u8 = 18;
pub const PRODUCER_ACK: u8 = 19;
pub const MESSAGE_PULL: u8 = 20;
pub const MESSAGE_DISPATCH: u8 = 21;
pub const MESSAGE_ACK: u8 = 22;
pub const ACTIVEMQ_MESSAGE: u8 = 23;
pub const ACTIVEMQ_BYTES_MESSAGE: u8 = 24;
pub const ACTIVEMQ_MAP_MESSAGE: u8 = 25;
pub const ACTIVEMQ_OBJECT_MESSAGE: u8 = 26;
pub const ACTIVEMQ_STREAM_MESSAGE: u8 = 27;
pub const ACTIVEMQ_TEXT_MESSAGE: u8 = 28;
pub const ACTIVEMQ_BLOB_MESSAGE: u8 = 29;
pub const RESPONSE: u8 = 30;
pub const EXCEPTION_RESPONSE: u8 = 31;
pub const DATA_RESPONSE: u8 = 32;
pub const DATA_ARRAY_RESPONSE: u8 = 33;
pub const INTEGER_RESPONSE: u8 = 34;
pub const MESSAGE_DISPATCH_NOTIFICATION: u8 = 90;
pub const BROKER_SUBSCRIPTION_INFO: u8 = 92;
pub const ACTIVEMQ_QUEUE: u8 = 100;
pub const ACTIVEMQ_TOPIC: u8 = 101;
pub const ACTIVEMQ_TEMP_QUEUE: u8 = 102;
pub const ACTIVEMQ_TEMP_TOPIC: u8 = 103;
pub const MESSAGE_ID: u8 = 110;
pub const ACTIVEMQ_LOCAL_TRANSACTION_ID: u8 = 111;
pub const ACTIVEMQ_XA_TRANSACTION_ID: u8 = 112;
pub const CONNECTION_ID: u8 = 120;
pub const SESSION_ID: u8 = 121;
pub const CONSUMER_ID: u8 = 122;
pub const PRODUCER_ID: u8 = 123;
pub const BROKER_ID: u8 = 124;

/// Lowest and highest OpenWire versions accepted by the broker.
pub const MIN_VERSION: i32 = 9;
pub const MAX_VERSION: i32 = 12;

pub fn is_message_type(t: u8) -> bool {
    (ACTIVEMQ_MESSAGE..=ACTIVEMQ_BLOB_MESSAGE).contains(&t)
}

pub fn command_name(t: u8) -> &'static str {
    match t {
        WIREFORMAT_INFO => "WireFormatInfo",
        BROKER_INFO => "BrokerInfo",
        CONNECTION_INFO => "ConnectionInfo",
        SESSION_INFO => "SessionInfo",
        CONSUMER_INFO => "ConsumerInfo",
        PRODUCER_INFO => "ProducerInfo",
        TRANSACTION_INFO => "TransactionInfo",
        DESTINATION_INFO => "DestinationInfo",
        REMOVE_SUBSCRIPTION_INFO => "RemoveSubscriptionInfo",
        KEEP_ALIVE_INFO => "KeepAliveInfo",
        SHUTDOWN_INFO => "ShutdownInfo",
        REMOVE_INFO => "RemoveInfo",
        CONTROL_COMMAND => "ControlCommand",
        FLUSH_COMMAND => "FlushCommand",
        CONNECTION_ERROR => "ConnectionError",
        CONSUMER_CONTROL => "ConsumerControl",
        CONNECTION_CONTROL => "ConnectionControl",
        PRODUCER_ACK => "ProducerAck",
        MESSAGE_PULL => "MessagePull",
        MESSAGE_DISPATCH => "MessageDispatch",
        MESSAGE_ACK => "MessageAck",
        ACTIVEMQ_MESSAGE => "ActiveMQMessage",
        ACTIVEMQ_BYTES_MESSAGE => "ActiveMQBytesMessage",
        ACTIVEMQ_MAP_MESSAGE => "ActiveMQMapMessage",
        ACTIVEMQ_OBJECT_MESSAGE => "ActiveMQObjectMessage",
        ACTIVEMQ_STREAM_MESSAGE => "ActiveMQStreamMessage",
        ACTIVEMQ_TEXT_MESSAGE => "ActiveMQTextMessage",
        ACTIVEMQ_BLOB_MESSAGE => "ActiveMQBlobMessage",
        RESPONSE => "Response",
        EXCEPTION_RESPONSE => "ExceptionResponse",
        DATA_RESPONSE => "DataResponse",
        DATA_ARRAY_RESPONSE => "DataArrayResponse",
        INTEGER_RESPONSE => "IntegerResponse",
        MESSAGE_DISPATCH_NOTIFICATION => "MessageDispatchNotification",
        BROKER_SUBSCRIPTION_INFO => "BrokerSubscriptionInfo",
        _ => "Unknown",
    }
}
