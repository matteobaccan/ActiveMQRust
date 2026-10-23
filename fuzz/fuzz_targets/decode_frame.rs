// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Feeds arbitrary bytes to the OpenWire frame decoder, the first thing a connection does
//! with what a client sends and the only part of the broker reachable before the login.

#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use mqrust::openwire::marshal::Decoder;

fuzz_target!(|data: &[u8]| {
    // The negotiated version changes which fields a command has, so both ends of the
    // supported range see the same bytes.
    for version in [9, 12] {
        let _ = Decoder::new(version).decode_frame(Bytes::copy_from_slice(data));
    }
});
