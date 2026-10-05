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
The reader (`connection::FrameReader`) reads into a reusable 64 KB `BytesMut`, so one `recv` brings many small frames. A frame that is not a message is split off the buffer (`split_to().freeze()`) without allocating and is dropped as soon as it is handled, so the buffer space is reclaimed in place. A message frame up to 64 KB is copied once into its own exactly sized allocation: the broker may keep the message for a long time, and a slice of the shared buffer would keep the whole buffer (and the neighbouring frames) alive while memory accounting counts only the message. A frame larger than 64 KB is read straight from the socket into its own allocation. `content` and `marshalledProperties` are then `Bytes` slices of the message's frame. All header fields are decoded, because every consumer connection re-encodes the whole `MessageDispatch` in its own version; only the ID and destination strings allocate, and ASCII strings are decoded without an intermediate `String`. Properties are decoded only when a selector needs them, once per message (`openwire::props::decode_count()` lets tests check it). When the broker compresses a body, the properties are copied out of the frame so the uncompressed frame is freed.
- *Alternatives:* decode only a few header fields lazily (needs a second, partial decoder and a re-encoder that copies the undecoded header bytes, which differ between versions); slice every frame from the shared buffer (no copy, but a small stored message pins a 64 KB buffer); copy the body into its own `Vec<u8>` (one extra copy per message and a second allocation).

### D2. Keep loose encoding, behind the `WireCodec` trait
Tight encoding saves a few bytes per command but needs a `BooleanStream` and two marshalling passes. On a LAN or loopback the limit is CPU per message, not bandwidth, so loose encoding is also the faster choice. All encoding and decoding go through the `WireCodec` trait so that a tight codec can be added later as `broker.tight_encoding` if a benchmark shows a real gain. This change does not add that key.
- *Alternatives:* implement tight encoding now (about three times the codec code, more compatibility risk, and no measured benefit yet); remove the trait (saves an indirection that monomorphization already removes, but closes the door to a later option).

### D3. Header-only re-encoding with vectored writes
Every connection encodes in its own negotiated version, so a `MessageDispatch` is re-encoded per consumer. Only the headers are encoded into a small buffer; the body `Bytes` is passed to `write_vectored` as a separate slice. The size prefix is computed from header length plus body length.
- *Alternatives:* re-send the original frame bytes (wrong when producer and consumer negotiate different versions, and the dispatch wrapper differs anyway); copy header and body into one buffer (one body copy per delivery, which dominates for large messages).

### D4. Write batching in the writer task
The writer task waits for the first frame on its channel, then drains everything already queued without waiting (`try_recv`) until 256 KB of encoded data are gathered, and writes the batch with vectored writes of at most 64 slices each. Small frames are coalesced into one buffer, so 1,000 queued 1 KB dispatches take a handful of write calls (a test with a recording writer checks at most 16); a body of 2048 bytes or more is a slice of its own, written from the stored buffer. There is no timer: under low load a single frame is written at once, so latency is not delayed; under load many dispatches share one syscall. `TCP_NODELAY` is set on every socket.
- *Alternatives:* flush after every frame (one syscall per message, the main cost at high rates); Nagle-style delay or a timer (adds latency at low rates, against the p99 target).

### D5. Sharded destination registry and short per-destination locks
The registry is an in-house map split into 64 shards, each a `parking_lot::RwLock<HashMap<Destination, Arc<Dest>>>`, selected by the hash of the destination (kind and name, so a queue and a topic with the same name are separate). Lookups take a shard read lock for a hash lookup and an `Arc` clone. Each destination has its own `parking_lot::Mutex`, held only for in-memory work, never during I/O, encoding or compression. Dispatch decisions are made under the lock, including selector evaluation (with the one-time property decode of a message) and, for a selective consumer, an in-order scan of the pending messages until one matches, which is O(n) in the worst case; encoding and sending happen after it is released, through the consumer connection's channel and writer task.
- *Alternatives:* `dashmap` (similar design, one more dependency, less control over shard count); one global `RwLock` (every lookup contends on one cache line); an actor per destination (no locks, but channel hops add latency per message).

### D6. Allocation discipline
Read buffers are reused per connection; header encode buffers come from a per-connection reusable buffer; ASCII strings are encoded and decoded without intermediate buffers. IDs are `Arc<str>` shared by every copy of a decoded command, but they are not interned across commands (an interning table would need a lock or a per-connection cache with eviction); the message ID reuses the producer's connection-id string when they are equal. A counting allocator in `tests/hot_path.rs` measures 9 allocations per message on average, the same for 1 KB and 100 KB bodies: the message frame and its promotion to a shared buffer (2), the message box (1), three ID and destination strings (3), `Arc<Message>` and the accounting record (2), and on average one for the writer's output buffer. The original target of at most 3 would need interned IDs and a decoder that builds the stored message in place; it is left to a later change if the benchmarks show the allocations matter.
- *Alternatives:* an object pool for `StoredMessage` (complex lifetime handling with `Arc`; `mimalloc` already makes small allocations cheap); arena allocation per batch (messages live for different times, so arenas fragment).

### D7. Immediate response for synchronous sends
For a synchronous send, the `Response` is queued on the producer's channel as soon as the message is in `pending`, before dispatch to consumers. There is no fsync to wait for.
- *Alternative:* respond after dispatch (adds dispatch time to every synchronous send without any durability gain).

### D8. Release profile and allocator
`Cargo.toml` keeps `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `opt-level = 3` and `mimalloc` as the global allocator (set by `bootstrap-broker-foundation`). This change verifies them. The criterion benchmarks also use `mimalloc`. The comparison of `mimalloc` against the system allocator is not measured yet (no benchmark runs are made while the change is implemented); it will be measured with the benchmarks and recorded here.
- *Alternatives:* system allocator (Windows heap; slower for many small allocations in measurements of comparable servers); `jemalloc` (no good Windows MSVC support).

### D9. Two benchmark layers
`criterion` benchmarks measure the codec and broker core in-process, with no network noise, and are used to catch regressions. The Java `bench` mode measures the whole system with the real `activemq-client`, which is what users experience and what the §14.3 targets refer to. Results are published as Markdown in `docs/benchmarks/` with machine details. The criterion files are `benches/codec.rs`, `dispatch.rs`, `compression.rs`, `selectors.rs` and `expiry.rs`, with shared helpers in `benches/common/`. The optimizations were implemented before a baseline was saved, so the reference for later regressions is the code of version 0.2.0, saved with `cargo bench -- --save-baseline v0.2.0` and compared with `cargo bench -- --baseline v0.2.0`.
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
