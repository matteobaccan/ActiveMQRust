## Context

After `add-queue-messaging` the broker sends, dispatches and acknowledges queue messages correctly. Its speed and memory use are what make it worth deploying instead of ActiveMQ. Because there is no persistence, there is no fsync and no disk I/O: the cost of a message is CPU for decoding and encoding, memory copies, allocations, lock contention and syscalls. The Java client on the same machine or LAN is fast enough that the broker is the bottleneck.

Design §14 lists eight principles, the reasons for keeping loose encoding, the measurable targets and the benchmarks. This change applies those principles where `add-queue-messaging` took simpler shortcuts, and adds the tools that prove the result.

## Goals / Non-Goals

**Goals:**
- A hot path with no body decoding, no body copies, a bounded number of allocations per message and one syscall for many dispatches under load.
- Reproducible micro-benchmarks (`criterion`) and an end-to-end benchmark with the real Java driver.
- Published numbers against the §14.3 targets, including misses.

**Non-Goals:**
- Tight encoding or the marshalling cache (they stay disabled; see D2).
- Comparison with real ActiveMQ: done by `add-activemq-comparison-benchmark`, which reuses the `bench` mode built here.
- Persistence, kernel-bypass networking, or platform-specific I/O such as registered I/O (RIO).

## Decisions

### D1. Zero-copy opaque bodies
The reader reads into a reusable `BytesMut`; each complete frame is split off as `Bytes`, and `content` and `marshalledProperties` are kept as `Bytes` slices of that frame. The broker decodes only destination, message ID, expiration, persistence, `compressed` and `redeliveryCounter`. Properties are decoded only when a selector needs them, once per message.
- *Alternatives:* decode the whole message into Rust structs (simple, but costs CPU and allocations for data the broker never reads); copy the body into its own `Vec<u8>` (one extra copy per message and a second allocation).

### D2. Keep loose encoding, behind the `WireCodec` trait
Tight encoding saves a few bytes per command but needs a `BooleanStream` and two marshalling passes. On a LAN or loopback the limit is CPU per message, not bandwidth, so loose encoding is also the faster choice. All encoding and decoding go through the `WireCodec` trait so that a tight codec can be added later as `broker.tight_encoding` if a benchmark shows a real gain. This change does not add that key.
- *Alternatives:* implement tight encoding now (about three times the codec code, more compatibility risk, and no measured benefit yet); remove the trait (saves an indirection that monomorphization already removes, but closes the door to a later option).

### D3. Header-only re-encoding with vectored writes
Every connection encodes in its own negotiated version, so a `MessageDispatch` is re-encoded per consumer. Only the headers are encoded into a small buffer; the body `Bytes` is passed to `write_vectored` as a separate slice. The size prefix is computed from header length plus body length.
- *Alternatives:* re-send the original frame bytes (wrong when producer and consumer negotiate different versions, and the dispatch wrapper differs anyway); copy header and body into one buffer (one body copy per delivery, which dominates for large messages).

### D4. Write batching in the writer task
The writer task waits for the first frame on its channel, then drains everything already queued without waiting (`try_recv`) into one vectored write, up to a cap of 64 frames or 256 KB, and only then flushes. There is no timer: under low load a single frame is written at once, so latency is not delayed; under load many dispatches share one syscall. `TCP_NODELAY` is set on every socket.
- *Alternatives:* flush after every frame (one syscall per message, the main cost at high rates); Nagle-style delay or a timer (adds latency at low rates, against the p99 target).

### D5. Sharded destination registry and short per-destination locks
The registry is an in-house map split into 64 shards, each a `parking_lot::RwLock<HashMap<Arc<str>, Arc<Destination>>>`, selected by the name hash. Lookups take a shard read lock for a hash lookup and an `Arc` clone. Each destination has its own `parking_lot::Mutex`, held only for O(1) or O(log n) in-memory operations, never during I/O or encoding. Dispatch decisions are made under the lock; encoding and sending happen after it is released, through the consumer connection's channel.
- *Alternatives:* `dashmap` (similar design, one more dependency, less control over shard count); one global `RwLock` (every lookup contends on one cache line); an actor per destination (no locks, but channel hops add latency per message).

### D6. Allocation discipline
Read buffers are reused per connection; connection, session, producer and consumer IDs are `Arc<str>` shared by every command of the connection; header encode buffers come from a per-connection reusable buffer. The steady-state target is a constant number of heap allocations per message that does not depend on body size: the `Arc<StoredMessage>` and at most one more. A counting global allocator in a test measures it.
- *Alternatives:* an object pool for `StoredMessage` (complex lifetime handling with `Arc`; `mimalloc` already makes small allocations cheap); arena allocation per batch (messages live for different times, so arenas fragment).

### D7. Immediate response for synchronous sends
For a synchronous send, the `Response` is queued on the producer's channel as soon as the message is in `pending`, before dispatch to consumers. There is no fsync to wait for.
- *Alternative:* respond after dispatch (adds dispatch time to every synchronous send without any durability gain).

### D8. Release profile and allocator
`Cargo.toml` keeps `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `opt-level = 3` and `mimalloc` as the global allocator (set by `bootstrap-broker-foundation`). This change verifies them and measures the effect of `mimalloc` against the system allocator once, to document the choice.
- *Alternatives:* system allocator (Windows heap; slower for many small allocations in measurements of comparable servers); `jemalloc` (no good Windows MSVC support).

### D9. Two benchmark layers
`criterion` benchmarks measure the codec and broker core in-process, with no network noise, and are used to catch regressions. The Java `bench` mode measures the whole system with the real `activemq-client`, which is what users experience and what the §14.3 targets refer to. Results are published as Markdown in `docs/benchmarks/` with machine details.
- *Alternatives:* only Java end-to-end (cannot isolate a codec regression); a Rust OpenWire client for load (would not prove anything about the real Java driver's behaviour, such as async send and prefetch).

### D10. How "broker-side p99 < 1 ms" is measured
The Java `bench` mode measures end-to-end latency on loopback: the producer stores `System.nanoTime()` in a long property and the consumer, in the same JVM, computes the difference on receipt. End-to-end latency includes both client stacks, so it is an upper bound of the broker-side latency. The target is met if the end-to-end p99 at 1,000 msg/s is below 1 ms.
- *Alternative:* broker-internal timestamps exported through a debug endpoint (exact, but needs extra hot-path code and does not reflect what applications see).

## Risks / Trade-offs

- [Benchmarks on a busy desktop machine are noisy] → Warm-up, at least 3 measured runs with the median reported, power plan "High performance", and machine details in every report.
- [Batching could add latency] → Batching never waits for more frames; a latency test at 1,000 msg/s guards the p99 target.
- [Holding body slices of the read buffer could keep large buffers alive] → Each frame is split into its own `Bytes`, so a stored message keeps alive only its own frame, not neighbouring frames; covered by a memory test with mixed sizes.
- [Targets may be missed on some hardware] → Results are published as measured, with "met" or "not met" per target; a miss opens a follow-up task instead of being hidden.
- [Micro-optimizations reduce readability] → Each principle has a test that guards it, so code can be simplified safely when a test shows no loss.

## Migration Plan

Internal changes with no configuration or protocol change. Clients see the same behaviour, only faster. Rollback means running the previous executable.

## Open Questions

- The definition of "scales almost linearly with cores" for 10 producers → 10 consumers on 10 queues: this change uses a scaling efficiency of at least 70% of linear, up to the number of physical cores, as the pass threshold; confirm or adjust after the first measurements.
- Whether the LAN measurements need a dedicated second machine in the repository documentation, or are reported only when one is available.
