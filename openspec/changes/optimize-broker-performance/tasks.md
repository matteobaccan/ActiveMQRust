## 1. Baseline measurement

- [x] 1.1 Add `criterion` as a dev-dependency and create `benches/codec.rs` (encode/decode 1 KB `ActiveMQTextMessage`, versions 12 and 9) and `benches/dispatch.rs` (enqueue and dispatch, 1 and 10 consumers)
- [x] 1.2 Implement the `bench` mode in `tests/java-it` (arguments, `throughput`, `latency` and `scale` scenarios, async/sync send, warm-up, `RESULT` line, missing-message check) and `run-bench.cmd`
- [ ] 1.3 Save the criterion baseline `v0.2.0` from the current code (`cargo bench -- --save-baseline v0.2.0`; the optimizations were implemented before a baseline was taken) and run the Java benchmark on the same version

## 2. Zero-copy receive path

- [x] 2.1 Reuse one `BytesMut` read buffer per connection and split each frame into its own `Bytes`
- [x] 2.2 Keep `content` and `marshalledProperties` as `Bytes` slices of the frame; decode the header fields only (bodies and properties stay opaque)
- [x] 2.3 Add the test-only property-decode counter and tests for undecodable bodies and buffer addresses

## 3. Shared storage and send path

- [x] 3.1 Store messages as one `Arc<StoredMessage>` shared by browsers (and topic subscribers when present); count shared bodies once in memory accounting
- [x] 3.2 Send the `Response` for synchronous sends right after enqueue
- [x] 3.3 Share connection, session, producer and consumer IDs as `Arc<str>`
- [x] 3.4 Add the counting-allocator test (measured 9 allocations per message, same for 1 KB and 100 KB)

## 4. Write path

- [x] 4.1 Encode only `MessageDispatch` headers per connection version into a reusable buffer
- [x] 4.2 Write header and body slices with `write_vectored`; batch queued frames up to 256 KB, at most 64 slices per write call, with no timer
- [x] 4.3 Enable `TCP_NODELAY` on accepted sockets
- [x] 4.4 Tests with a recording writer: separate body slice, ≤ 16 writes for 1,000 queued dispatches, immediate write for a single frame; cross-version test (producer v12, consumer v9)
- [x] 4.5 Set accepted socket buffers from `broker.socket_buffer_kb` (default 1 MB); validate the key
- [x] 4.6 Size the runtime like the JVM: processors from the process affinity, `broker.processors` / `--processors` override, `processors - 1` compression threads; log the count

## 5. Locking

- [x] 5.1 Replace the destination registry with a 64-shard `RwLock<HashMap<Destination, Arc<Dest>>>` map
- [x] 5.2 Audit every destination lock scope: no I/O, encoding or compression while held; move work outside where needed
- [x] 5.3 Tests: a held lock on queue A does not block sends to queue B; a blocked consumer socket does not stall the queue

## 6. Codec and build settings

- [x] 6.1 Confirm every encode/decode path goes through `WireCodec`; add the counting-wrapper codec test
- [ ] 6.2 Confirm the release profile settings and `mimalloc`; measure once against the system allocator and record the result in the design notes of the report
- [x] 6.3 Add `benches/compression.rs`, `benches/selectors.rs` and `benches/expiry.rs` for the features already applied

## 7. Measurement and publication

- [ ] 7.1 Run every target scenario on loopback (warm-up plus at least 3 runs, median) and over a LAN when a second machine is available
- [ ] 7.2 Measure idle Working Set and Working Set with 100,000 messages of 1 KB
- [ ] 7.3 Write the results in the README (machine details, commands, runs, medians and met/not met per target) and update "Performance at a glance"
- [ ] 7.4 Open a follow-up task for every target not met

## 8. Verification

- [x] 8.1 All unit, semantics and Java acceptance tests still pass
- [ ] 8.2 `cargo bench` runs every benchmark; criterion shows no regression against `v0.2.0`
- [ ] 8.3 The async throughput, latency and idle-memory targets are met, or reported as not met with a follow-up task
- [x] 8.4 `openspec validate optimize-broker-performance` passes
