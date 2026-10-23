// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! What the fuzzer feeds the broker, replayed on the stable toolchain and on every platform
//! CI builds: the seeds kept under `fuzz/seeds`, and a deterministic sweep of random and
//! mutated input over the same entry points. The fuzz targets themselves
//! (`fuzz/fuzz_targets`) need a nightly toolchain and libFuzzer, so they run by hand; this
//! file is what guards those entry points on every run of the suite. Each test calls the
//! same public function as the target of the same name.
//!
//! The sweeps seed their generator from the iteration number, so a failing run reproduces
//! exactly: re-run it, print the input of that iteration and write those bytes to
//! `fuzz/seeds/<target>/<name>`. A crash the fuzzer finds belongs there too, by the name of
//! what it is: the working corpus under `fuzz/corpus` is regenerated and not kept.

use bytes::Bytes;
use std::path::PathBuf;

use mqrust::openwire::marshal::Decoder;
use mqrust::openwire::props::PrimitiveMap;
use mqrust::selector::{EvalError, Header, MessageView, SVal, Selector};

/// Versions a client can negotiate; each decodes the same bytes differently.
const VERSIONS: [i32; 2] = [9, 12];

fn repo(parts: &[&str]) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    parts.iter().for_each(|x| p.push(x));
    p
}

/// Every seed of a target, sorted, so a failure names the same file everywhere.
fn seeds(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = repo(&["fuzz", "seeds", target]);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no seeds in {}", dir.display());
    files
        .iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(p).unwrap())
        })
        .collect()
}

/// Splits a recorded stream into frame bodies, the size prefix removed.
fn frames(data: &[u8]) -> Vec<Bytes> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        let size = i32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        if i + 4 + size > data.len() {
            break;
        }
        out.push(Bytes::copy_from_slice(&data[i + 4..i + 4 + size]));
        i += 4 + size;
    }
    out
}

/// The frames recorded from the real ActiveMQ client, the seeds of `decode_frame`.
fn golden_frames() -> Vec<Bytes> {
    let dir = repo(&["tests", "data", "golden"]);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "bin"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        out.extend(frames(&std::fs::read(f).unwrap()));
    }
    assert!(!out.is_empty(), "no golden frames in {}", dir.display());
    out
}

/// xorshift64*: every sweep below seeds it from the iteration number, so a failure
/// reproduces on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// What `fuzz_targets/decode_frame.rs` does with one input.
fn decode_frame(data: &[u8]) {
    for version in VERSIONS {
        let _ = Decoder::new(version).decode_frame(Bytes::copy_from_slice(data));
    }
}

/// What `fuzz_targets/primitive_map.rs` does with one input.
fn decode_primitive_map(data: &[u8]) {
    let _ = PrimitiveMap::decode(&Bytes::copy_from_slice(data));
}

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

/// What `fuzz_targets/selector.rs` does with one input.
fn compile_selector(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(Some(sel)) = Selector::compile(text) {
        let _ = sel.matches(&View);
        let _ = sel.evaluate(&View);
    }
}

#[test]
fn the_decode_frame_seeds_are_refused_without_a_crash() {
    for (name, data) in seeds("decode_frame") {
        println!("decode_frame/{name} ({} bytes)", data.len());
        decode_frame(&data);
    }
}

#[test]
fn the_primitive_map_seeds_are_refused_without_a_crash() {
    for (name, data) in seeds("primitive_map") {
        println!("primitive_map/{name} ({} bytes)", data.len());
        decode_primitive_map(&data);
    }
}

#[test]
fn the_selector_seeds_are_refused_without_a_crash() {
    for (name, data) in seeds("selector") {
        println!("selector/{name} ({} bytes)", data.len());
        compile_selector(&data);
    }
}

#[test]
fn random_bytes_never_decode_into_a_crash() {
    let mut buf = Vec::with_capacity(512);
    for i in 0..20_000u64 {
        let mut rng = Rng(0x5eed_0f0b_ad1d_ea01 ^ i);
        buf.clear();
        let len = rng.below(512);
        for _ in 0..len {
            buf.push(rng.next() as u8);
        }
        decode_frame(&buf);
        decode_primitive_map(&buf);
    }
}

/// The frames a real client sends, with bytes flipped and tails cut: the shapes a decoder
/// is most likely to trust, in states it never meets while things go well.
#[test]
fn mutated_client_frames_never_decode_into_a_crash() {
    let golden = golden_frames();
    for i in 0..20_000u64 {
        let mut rng = Rng(0x5eed_0f0b_ad1d_ea02 ^ i);
        let mut body = golden[rng.below(golden.len())].to_vec();
        if body.is_empty() {
            continue;
        }
        match rng.below(3) {
            0 => {
                // Flip up to four bytes.
                for _ in 0..1 + rng.below(4) {
                    let at = rng.below(body.len());
                    body[at] = rng.next() as u8;
                }
            }
            1 => {
                // Cut the frame short, the way a connection dies mid-command.
                let keep = rng.below(body.len());
                body.truncate(keep);
            }
            _ => {
                // Grow a length field: claim more than the frame holds.
                let at = rng.below(body.len());
                body[at] = 0xff;
            }
        }
        decode_frame(&body);
    }
}

/// Selector text assembled from the pieces of the grammar, including the ones that nest.
#[test]
fn random_selector_text_never_compiles_into_a_crash() {
    // Pieces of the selector grammar, separated by a vertical bar (which none of them
    // contains). The last one is an identifier long enough to leave the lexer's short path.
    const PIECES: &str = "(|)| AND | OR | NOT |=|<>|<|>|<=|>=|+|-|*|/| LIKE | ESCAPE | BETWEEN \
        | IN | IS NULL | IS NOT NULL |'red'|'%a_b'|'\\'|color|size|weight|flag|JMSPriority\
        |JMSType|JMSTimestamp|TRUE|FALSE|NULL|0|1|-1|9223372036854775807|1.5E300|0.0|,|XPATH \
        |aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let pieces: Vec<&str> = PIECES.split('|').collect();
    for i in 0..20_000u64 {
        let mut rng = Rng(0x5eed_0f0b_ad1d_ea03 ^ i);
        let mut text = String::new();
        for _ in 0..1 + rng.below(24) {
            text.push_str(pieces[rng.below(pieces.len())]);
        }
        compile_selector(text.as_bytes());
    }
}

/// Nesting is the one input that costs stack rather than time: the decoder counts it, and
/// `fuzz/seeds/decode_frame/nested-destination-info` keeps the shape that proved it.
#[test]
fn nesting_is_bounded_whatever_the_frame_claims() {
    let mut body: Vec<u8> = vec![8]; // DestinationInfo
    for _ in 0..100_000 {
        body.extend_from_slice(&1i32.to_be_bytes());
        body.extend_from_slice(&[0, 0, 0, 0]);
        body.extend_from_slice(&0i64.to_be_bytes());
        body.push(1);
        body.extend_from_slice(&1i16.to_be_bytes());
        body.extend_from_slice(&[1, 8]);
    }
    let e = Decoder::new(12)
        .decode_frame(Bytes::from(body))
        .expect_err("a frame nested 100000 deep must be refused");
    assert!(e.0.contains("nested more than"), "{}", e.0);
}

/// The same for a selector: the parser, the evaluator and the drop of the tree each
/// recurse once per level, and `fuzz/seeds/selector/nested-prefix-operators` keeps the
/// shape the fuzzer found.
#[test]
fn a_selector_nests_no_deeper_than_the_parser_allows() {
    for text in [&"+".repeat(100_000), &"(".repeat(100_000)] {
        let e = Selector::compile(text).expect_err("a selector nested 100000 deep");
        assert!(e.message.contains("nested more than"), "{}", e.message);
    }
}

#[test]
fn every_fuzz_target_has_seeds() {
    let dir = repo(&["fuzz", "fuzz_targets"]);
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            let target = p.file_stem().unwrap().to_string_lossy().into_owned();
            let c = repo(&["fuzz", "seeds", &target]);
            assert!(c.is_dir(), "fuzz target {target} has no seeds at {}", c.display());
        }
    }
}
