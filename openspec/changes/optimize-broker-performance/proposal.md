## Why

ActiveMQRust exists to be **compatible** with ActiveMQ while using **less RAM** and being **faster** than it (requirement R13). Messages are processed only in RAM, with no storage, so the broker has no disk latency to hide behind: its speed is decided entirely by CPU per message, copies, allocations, locks and syscalls. `add-queue-messaging` makes messaging correct; this change makes the hot path follow the performance principles of the design (§14), adds the benchmarks that measure them, and turns the targets into numbers that are checked and published.

## What Changes

- Hot path rules applied and verified: bodies and properties kept as opaque `bytes::Bytes` slices of the read buffer; one shared `Arc<StoredMessage>` per message; only headers re-encoded per connection, with bodies written through vectored writes; write batching with `TCP_NODELAY`; short, partitioned locks with a sharded destination registry; reused read buffers and `Arc<str>` IDs; immediate `Response` for synchronous sends; the release profile (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `opt-level = 3`, `mimalloc`).
- The codec stays loose-encoding only, behind the `WireCodec` trait; tight encoding is added later only if benchmarks show a real advantage.
- `criterion` benchmarks for decoding and encoding `ActiveMQTextMessage`, enqueue and dispatch, and, when those features are present, compression, selector dispatch and the expiry sweeper.
- A `bench` mode in the Java program of `tests/java-it` that measures end-to-end throughput and latency with the real ActiveMQ driver.
- Measurable targets (≥ 100,000 msg/s async 1 KB, p99 < 1 ms at 1,000 msg/s, near-linear scaling on 10 queues, < 20 MB RSS idle) measured and published in `docs/benchmarks/` and the README.

## Capabilities

### New Capabilities

- `broker-performance`: hot-path principles, wire format and `WireCodec` rationale, release profile, measurable targets, `criterion` benchmarks and the Java end-to-end benchmark mode.

### Modified Capabilities

None.

## Impact

- Code: `src/openwire/frame.rs`, `marshal.rs` (zero-copy body slices, header-only re-encoding), `src/connection.rs` (reader buffer reuse, writer batching, vectored writes), `src/broker/mod.rs` and `destination.rs` (sharded registry, lock scope), `Cargo.toml` release profile.
- New `benches/` directory (`codec.rs`, `dispatch.rs`, and `compression.rs`, `selectors.rs`, `expiry.rs` when those features exist).
- `tests/java-it/`: new `bench` mode in `mqrust-acceptance.jar` and `run-bench.cmd`.
- New documentation: `docs/benchmarks/performance-<date>.md` and a performance section in the README.
- Dependencies: `criterion` (dev only). `bytes`, `parking_lot` and `mimalloc` already exist.
- Depends on `add-queue-messaging`. `add-activemq-comparison-benchmark` depends on this change and reuses its `bench` mode.
