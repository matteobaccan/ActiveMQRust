// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! Golden vectors: byte streams recorded from the real ActiveMQ Java client
//! (scripts/capture-frames.py), and streams written by the Java client's OpenWire marshaller for
//! every version from 9 to 12 (tests/data/golden/broker, written by `GoldenVectors` in tests/java-it),
//! including the commands a broker sends. Every frame must decode, and re-encode to the same bytes.

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
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|r| {
            r.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "bin"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    assert!(
        !files.is_empty(),
        "no golden vectors in {}: run scripts/capture-frames.py",
        dir.display()
    );
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
            assert_eq!(
                &again[4..],
                &body[..],
                "{} frame {n} ({}) re-encodes differently",
                f.display(),
                t::command_name(cmd.type_code())
            );
            checked += 1;
        }
    }
    println!("golden frames checked: {checked}; command types: {kinds:?}");
    assert!(checked > 0);
}

/// Streams written by the ActiveMQ client's own OpenWire marshaller for versions 9 to 12
/// (`GoldenVectors` in tests/java-it): the same commands, in the same order, in every file.
fn marshaller_vectors() -> Vec<(i32, Vec<Bytes>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("golden")
        .join("broker");
    (9..=12)
        .map(|v| {
            let path = dir.join(format!("v{v:02}.bin"));
            let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            (v, frames(&data))
        })
        .collect()
}

fn decode(version: i32, body: &Bytes) -> Command {
    Decoder::new(version)
        .decode_frame(body.clone())
        .unwrap()
        .expect("non-null command")
}

#[test]
fn marshaller_vectors_round_trip_for_every_version() {
    for (version, all) in marshaller_vectors() {
        // The broker's WireFormatInfo: version independent, properties kept in order.
        match decode(t::MAX_VERSION, &all[0]) {
            Command::WireFormatInfo(wf) => {
                assert_eq!(wf.version, version);
                let again = Encoder::new(version).frame(&Command::WireFormatInfo(wf));
                assert_eq!(
                    &again[4..],
                    &all[0][..],
                    "v{version} WireFormatInfo re-encodes differently"
                );
            }
            other => panic!("v{version}: first frame is not WireFormatInfo: {other:?}"),
        }
        let mut kinds = std::collections::BTreeSet::new();
        for (n, body) in all.iter().enumerate().skip(1) {
            let cmd = decode(version, body);
            assert!(
                !matches!(cmd, Command::Unsupported { .. }),
                "v{version} frame {n}: unsupported"
            );
            kinds.insert(t::command_name(cmd.type_code()));
            let again = Encoder::new(version).frame(&cmd);
            assert_eq!(
                &again[4..],
                &body[..],
                "v{version} frame {n} ({}) re-encodes differently",
                t::command_name(cmd.type_code())
            );
        }
        for k in [
            "BrokerInfo",
            "MessageDispatch",
            "ProducerAck",
            "Response",
            "ExceptionResponse",
            "MessageAck",
            "TransactionInfo",
            "ConsumerInfo",
            "RemoveSubscriptionInfo",
            "DestinationInfo",
            "MessagePull",
        ] {
            assert!(kinds.contains(k), "v{version}: no {k} in {kinds:?}");
        }
    }
}

#[test]
fn commands_re_encoded_for_another_version_match_the_java_marshaller() {
    let vectors = marshaller_vectors();
    let (_, v12) = &vectors[3];
    for (version, frames) in &vectors {
        assert_eq!(frames.len(), v12.len());
        for n in 1..frames.len() {
            // Decoded at version 12, encoded at `version`: identical to Java's bytes for that version.
            let cmd = decode(12, &v12[n]);
            let again = Encoder::new(*version).frame(&cmd);
            assert_eq!(
                &again[4..],
                &frames[n][..],
                "frame {n} ({}) 12 -> {version}",
                t::command_name(cmd.type_code())
            );
        }
    }
}

#[test]
fn marshaller_vectors_decode_to_the_expected_fields() {
    use mqrust::openwire::model::*;
    for (version, all) in marshaller_vectors() {
        let cmds: Vec<Command> = all.iter().skip(1).map(|b| decode(version, b)).collect();
        // MessageId: textView only from version 10, same text form everywhere.
        let dispatch = cmds
            .iter()
            .find_map(|c| match c {
                Command::MessageDispatch(md) if md.message.is_some() => Some(md.clone()),
                _ => None,
            })
            .unwrap();
        let m = dispatch.message.as_ref().unwrap();
        let id = m.message_id.as_ref().unwrap();
        assert_eq!(id.to_string(), "ID:golden-51234-1759672800000-1:1:1:2:42");
        assert_eq!(id.text_view.is_some(), version >= 10, "v{version} textView");
        assert_eq!(id.broker_sequence_id, 1001);
        assert_eq!(dispatch.redelivery_counter, 2);
        assert_eq!(m.correlation_id.as_deref(), Some("ORD-A"));
        assert!(m.persistent);
        // Null dispatch (end of browse / pull timeout).
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::MessageDispatch(md) if md.message.is_none())));
        // Transactions: local and XA identifiers in TransactionInfo, messages and acks.
        let local = TransactionId::Local {
            value: 5,
            connection_id: Some(ConnectionId {
                value: "ID:golden-51234-1759672800000-1:1".into(),
            }),
        };
        assert!(cmds.iter().any(|c| matches!(c, Command::TransactionInfo(ti) if ti.transaction_id.as_ref() == Some(&local) && ti.tx_type == tx_type::COMMIT_ONE_PHASE)));
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::Message(m) if m.transaction_id.as_ref() == Some(&local))));
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::MessageAck(a) if a.transaction_id.as_ref() == Some(&local))));
        let xa = |t: &Option<TransactionId>| matches!(t, Some(TransactionId::Xa { format_id: 0x1234, global_transaction_id: Some(g), branch_qualifier: Some(b) }) if &g[..] == b"global" && &b[..] == b"branch");
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::TransactionInfo(ti) if xa(&ti.transaction_id))));
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::MessageAck(a) if xa(&a.transaction_id))));
        // Durable subscription fields and RemoveSubscriptionInfo.
        let durable = cmds
            .iter()
            .find_map(|c| match c {
                Command::ConsumerInfo(ci) if ci.subscription_name.is_some() => Some(ci.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(durable.subscription_name.as_deref(), Some("golden-durable"));
        assert!(durable.no_local);
        assert_eq!(durable.selector.as_deref(), Some("color = 'blue' AND count > 3"));
        assert_eq!(
            durable.client_id.is_some(),
            version >= 10,
            "v{version} ConsumerInfo.clientId"
        );
        assert!(cmds.iter().any(|c| matches!(c, Command::RemoveSubscriptionInfo(r)
            if r.subscription_name.as_deref() == Some("golden-durable") && r.client_id.as_deref() == Some("golden-client"))));
        // Broker-side commands.
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::ProducerAck(p) if p.size == 1056)));
        assert!(cmds
            .iter()
            .any(|c| matches!(c, Command::BrokerInfo(b) if b.broker_name.as_deref() == Some("ActiveMQRust"))));
    }
}
