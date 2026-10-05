// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! `WireFormatInfo` negotiation (see `OpenWireFormat.renegotiateWireFormat`).

use super::model::WireFormatInfo;
use super::props::{PrimitiveMap, Value};
use super::types::{MAX_VERSION, MIN_VERSION};

pub const MAGIC: [u8; 8] = *b"ActiveMQ";
pub const PROVIDER_NAME: &str = "ActiveMQRust";
pub const PROVIDER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Settings agreed with one client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Negotiated {
    pub version: i32,
    pub max_inactivity_ms: i64,
    pub max_inactivity_initial_delay_ms: i64,
    pub max_frame_size: i64,
    pub tcp_no_delay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationError {
    BadMagic,
    VersionTooOld(i32),
}

/// Default inactivity settings of the ActiveMQ client, used when the client omits them.
const DEFAULT_INACTIVITY_MS: i64 = 30_000;
const DEFAULT_INITIAL_DELAY_MS: i64 = 10_000;

pub fn platform_details() -> String {
    format!("Rust, {} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Builds the broker's `WireFormatInfo`, echoing the client's inactivity settings.
pub fn broker_wire_format(client: &WireFormatInfo, max_frame_size: i64) -> WireFormatInfo {
    let inactivity = client.properties.get_long("MaxInactivityDuration").unwrap_or(DEFAULT_INACTIVITY_MS);
    let initial = client
        .properties
        .get_long("MaxInactivityDurationInitalDelay")
        .unwrap_or(DEFAULT_INITIAL_DELAY_MS);
    let mut p = PrimitiveMap::new();
    p.set("TightEncodingEnabled", Value::Bool(false));
    p.set("CacheEnabled", Value::Bool(false));
    p.set("SizePrefixDisabled", Value::Bool(false));
    p.set("StackTraceEnabled", Value::Bool(false));
    p.set("TcpNoDelayEnabled", Value::Bool(true));
    p.set("MaxInactivityDuration", Value::Long(inactivity));
    p.set("MaxInactivityDurationInitalDelay", Value::Long(initial));
    p.set("MaxFrameSize", Value::Long(max_frame_size));
    p.set("MaxFrameSizeEnabled", Value::Bool(true));
    p.set("CacheSize", Value::Int(0));
    p.set("ProviderName", Value::String(PROVIDER_NAME.into()));
    p.set("ProviderVersion", Value::String(PROVIDER_VERSION.into()));
    p.set("PlatformDetails", Value::String(platform_details()));
    WireFormatInfo { magic: MAGIC, version: MAX_VERSION, properties: p }
}

/// Computes the settings both sides will use after the exchange.
pub fn negotiate(client: &WireFormatInfo, max_frame_size: i64) -> Result<Negotiated, NegotiationError> {
    if client.magic != MAGIC {
        return Err(NegotiationError::BadMagic);
    }
    if client.version < MIN_VERSION {
        return Err(NegotiationError::VersionTooOld(client.version));
    }
    let props = &client.properties;
    let client_frame = props.get_long("MaxFrameSize").unwrap_or(i64::MAX);
    Ok(Negotiated {
        version: client.version.min(MAX_VERSION),
        max_inactivity_ms: props.get_long("MaxInactivityDuration").unwrap_or(DEFAULT_INACTIVITY_MS),
        max_inactivity_initial_delay_ms: props
            .get_long("MaxInactivityDurationInitalDelay")
            .unwrap_or(DEFAULT_INITIAL_DELAY_MS),
        max_frame_size: client_frame.min(max_frame_size),
        tcp_no_delay: props.get_bool("TcpNoDelayEnabled").unwrap_or(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(version: i32) -> WireFormatInfo {
        let mut p = PrimitiveMap::new();
        p.set("TightEncodingEnabled", Value::Bool(true));
        p.set("CacheEnabled", Value::Bool(true));
        p.set("MaxInactivityDuration", Value::Long(30000));
        p.set("MaxInactivityDurationInitalDelay", Value::Long(10000));
        WireFormatInfo { magic: MAGIC, version, properties: p }
    }

    #[test]
    fn negotiates_min_version() {
        let n = negotiate(&client(9), 1000).unwrap();
        assert_eq!(n.version, 9);
        assert_eq!(n.max_frame_size, 1000);
        assert_eq!(negotiate(&client(13), 1000).unwrap().version, 12);
    }

    #[test]
    fn rejects_old_versions() {
        assert_eq!(negotiate(&client(5), 1000), Err(NegotiationError::VersionTooOld(5)));
    }

    #[test]
    fn broker_disables_tight_and_cache() {
        let b = broker_wire_format(&client(12), 1000);
        assert_eq!(b.properties.get_bool("TightEncodingEnabled"), Some(false));
        assert_eq!(b.properties.get_bool("CacheEnabled"), Some(false));
        assert_eq!(b.properties.get_string("ProviderName"), Some("ActiveMQRust"));
        assert_eq!(b.properties.get_long("MaxInactivityDuration"), Some(30000));
    }
}
