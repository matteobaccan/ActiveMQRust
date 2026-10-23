## Context

Release builds use `panic = "abort"`; the per-connection task model means most failures can be confined to one connection if they are returned as errors instead of panics.

## Goals / Non-Goals

**Goals:** no input from a client can stop the broker; memory stays bounded by default; the service comes back by itself after a failure.
**Non-Goals:** persistence of in-flight messages across a restart (the broker stays in-memory by design).

## Decisions

- **Three fuzz targets, chosen by what a client can reach**: `decode_frame` (the command decoder, the only part of the broker reachable before the login), `primitive_map` (the marshalled properties of a message, decoded when a selector asks for one) and `selector` (text the client writes). *Alternatives considered*: one target driving a whole connection, which would spend its runs in the socket and the handshake rather than in the parser; no target for `primitive_map`, which nests by construction and so is exactly where nesting limits are worth testing.
- **cargo-fuzz in a package of its own, replayed by the suite on stable**: `fuzz/` is outside the broker's build because libFuzzer and nightly are not available on the Windows release toolchain, and `tests/fuzz_seeds.rs` replays the committed seeds and sweeps seeded random and mutated input, so the same entry points are guarded on every platform CI builds. *Alternatives considered*: a nightly fuzzing job in `build.yml`, which would fuzz for seconds per run and only on the platform that matters least; `cargo-afl`, with the same nightly requirement and without the corpus tooling.
- **Nesting is bounded, not caught**: `panic = "abort"` means a stack overflow cannot be caught, so the decoder counts how deep it is and refuses a frame past 32 levels, the limit nested primitive maps already used. *Alternatives considered*: `panic = "unwind"` with `catch_unwind` around each connection task, which trades the release profile for the chance of carrying on from a corrupted state; a larger thread stack, which moves the threshold without removing it.

Still to be designed in 0.5.0: the memory default formula, the recovery-action values and options, and the launchd integration.

## Risks / Trade-offs

- [A default memory limit changes behaviour for users with very large backlogs] → Documented; `max_memory_mb = 0` keeps the current unlimited behaviour.
