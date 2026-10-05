// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Golden vectors: byte streams recorded from the real ActiveMQ Java client
//! (scripts/capture-frames.py). Every frame must decode, and re-encode to the same bytes.

use bytes::Bytes;
use std::path::Path;

use mqrust::openwire::marshal::{Decoder, Encoder};
use mqrust::openwire::model::Command;
use mqrust::openwire::types as t;

fn frames(data: &[u8]) -> Vec<Bytes> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        let size = i32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        if i + 4 + size > data.len() {
            break; // connection cut in the middle of a frame
        }
        out.push(Bytes::copy_from_slice(&data[i + 4..i + 4 + size]));
        i += 4 + size;
    }
    out
}

#[test]
fn recorded_client_frames_round_trip() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join("golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "bin")).collect())
        .unwrap_or_default();
    files.sort();
    assert!(!files.is_empty(), "no golden vectors in {}: run scripts/capture-frames.py", dir.display());
    let mut checked = 0;
    let mut kinds = std::collections::BTreeSet::new();
    for f in &files {
        let data = std::fs::read(f).unwrap();
        let all = frames(&data);
        if all.is_empty() {
            continue;
        }
        let version = match Decoder::new(t::MAX_VERSION).decode_frame(all[0].clone()).unwrap() {
            Some(Command::WireFormatInfo(wf)) => wf.version.min(t::MAX_VERSION),
            other => panic!("{}: first frame is not WireFormatInfo: {other:?}", f.display()),
        };
        let dec = Decoder::new(version);
        let enc = Encoder::new(version);
        for (n, body) in all.iter().enumerate().skip(1) {
            let cmd = dec
                .decode_frame(body.clone())
                .unwrap_or_else(|e| panic!("{} frame {n}: {e}", f.display()))
                .expect("non-null command");
            if let Command::Unsupported { type_code, .. } = cmd {
                panic!("{} frame {n}: unsupported type {type_code}", f.display());
            }
            kinds.insert(t::command_name(cmd.type_code()));
            let again = enc.frame(&cmd);
            assert_eq!(&again[4..], &body[..], "{} frame {n} ({}) re-encodes differently", f.display(), t::command_name(cmd.type_code()));
            checked += 1;
        }
    }
    println!("golden frames checked: {checked}; command types: {kinds:?}");
    assert!(checked > 0);
}
