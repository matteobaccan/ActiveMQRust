## 1. Baseline measurement

- [ ] 1.1 Add `criterion` as a dev-dependency and create `benches/codec.rs` (encode/decode 1 KB `ActiveMQTextMessage`, versions 12 and 6) and `benches/dispatch.rs` (enqueue and dispatch, 1 and 10 consumers)
- [ ] 1.2 Implement the `bench` mode in `tests/java-it` (arguments, `throughput`, `latency` and `scale` scenarios, async/sync send, warm-up, `RESULT` line, missing-message check) and `run-bench.cmd`
- [ ] 1.3 Run both layers on the code from `add-queue-messaging` and save the criterion baseline `pre-optimization` and the Java results

## 2. Zero-copy receive path

- [ ] 2.1 Reuse one `BytesMut` read buffer per connection and split each frame into its own `Bytes`
- [ ] 2.2 Keep `content` and `marshalledProperties` as `Bytes` slices; decode only the needed headers
- [ ] 2.3 Add the test-only property-decode counter and tests for undecodable bodies and buffer addresses

## 3. Shared storage and send path

- [ ] 3.1 Store messages as one `Arc<StoredMessage>` shared by browsers (and topic subscribers when present); count shared bodies once in memory accounting
- [ ] 3.2 Send the `Response` for synchronous sends right after enqueue
- [ ] 3.3 Share connection, session, producer and consumer IDs as `Arc<str>`
- [ ] 3.4 Add the counting-allocator test (≤ 3 allocations per message, same for 1 KB and 100 KB)

## 4. Write path

- [ ] 4.1 Encode only `MessageDispatch` headers per connection version into a reusable buffer
- [ ] 4.2 Write header and body slices with `write_vectored`; batch queued frames up to 64 frames or 256 KB per write, with no timer
- [ ] 4.3 Enable `TCP_NODELAY` on accepted sockets
- [ ] 4.4 Tests with a recording writer: separate body slice, ≤ 16 writes for 1,000 queued dispatches, immediate write for a single frame; cross-version test (producer v12, consumer v9)

## 5. Locking

- [ ] 5.1 Replace the destination registry with a 64-shard `RwLock<HashMap>` map
- [ ] 5.2 Audit every destination lock scope: no I/O, encoding or compression while held; move work outside where needed
- [ ] 5.3 Tests: a held lock on queue A does not block sends to queue B; a blocked consumer socket does not stall the queue

## 6. Codec and build settings

- [ ] 6.1 Confirm every encode/decode path goes through `WireCodec`; add the counting-wrapper codec test
- [ ] 6.2 Confirm the release profile settings and `mimalloc`; measure once against the system allocator and record the result in the design notes of the report
- [ ] 6.3 Add `benches/compression.rs`, `benches/selectors.rs` and `benches/expiry.rs` for the features already applied

## 7. Measurement and publication

- [ ] 7.1 Run every target scenario on loopback (warm-up plus at least 3 runs, median) and over a LAN when a second machine is available
- [ ] 7.2 Measure idle Working Set and Working Set with 100,000 messages of 1 KB
- [ ] 7.3 Write `docs/benchmarks/performance-<date>.md` with machine details, commands, runs, medians and met/not met per target; add the README summary
- [ ] 7.4 Open a follow-up task for every target not met

## 8. Verification

- [ ] 8.1 All unit, semantics and Java acceptance tests still pass
- [ ] 8.2 `cargo bench` runs every benchmark; criterion shows no regression against `pre-optimization`
- [ ] 8.3 The async throughput, latency and idle-memory targets are met, or reported as not met with a follow-up task
- [ ] 8.4 `openspec validate optimize-broker-performance` passes
