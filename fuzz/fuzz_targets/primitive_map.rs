// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Feeds arbitrary bytes to the decoder of the marshalled properties of a message, which
//! the broker reads when a selector asks for a property.

#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use mqrust::openwire::props::PrimitiveMap;

fuzz_target!(|data: &[u8]| {
    let _ = PrimitiveMap::decode(&Bytes::copy_from_slice(data));
});
