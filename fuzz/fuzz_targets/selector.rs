// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Compiles arbitrary text as a JMS selector and evaluates what compiles. A client chooses
//! the text, and the broker compiles it when a consumer subscribes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mqrust::selector::{EvalError, Header, MessageView, SVal, Selector};

/// A message with one value of every type a selector can meet.
struct View;

impl MessageView for View {
    fn header(&self, h: Header) -> Result<SVal, EvalError> {
        Ok(match h {
            Header::CorrelationId => SVal::Str("ORD-A-100".into()),
            Header::Type => SVal::Str("order".into()),
            Header::Priority => SVal::Int(4),
            Header::Timestamp => SVal::Long(1_700_000_000_000),
            Header::Redelivered => SVal::Bool(false),
            _ => SVal::Null,
        })
    }

    fn property(&self, name: &str) -> Result<SVal, EvalError> {
        Ok(match name {
            "color" => SVal::Str("red".into()),
            "size" => SVal::Int(3),
            "weight" => SVal::Double(2.5),
            "flag" => SVal::Bool(true),
            "broken" => return Err(EvalError),
            _ => SVal::Null,
        })
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // Compiling is half the surface: evaluation walks the tree the parser built.
    if let Ok(Some(sel)) = Selector::compile(text) {
        let _ = sel.matches(&View);
        let _ = sel.evaluate(&View);
    }
});
