## 1. Research against the Java sources

- [ ] 1.1 Extract the compressed `content` layout of `ActiveMQTextMessage`, `ActiveMQBytesMessage`, `ActiveMQMapMessage`, `ActiveMQObjectMessage` and `ActiveMQStreamMessage` from `storeContent()` and the decompression code in 5.18.x and 6.x
- [ ] 1.2 Capture golden vectors: the same bodies sent with `useCompression=true` and `false` by a real Java client, saved in `tests/data/`

## 2. Configuration and dependency

- [ ] 2.1 Add `flate2` with the `zlib-rs` backend and confirm `scripts/check-deps.cmd` still passes
- [ ] 2.2 Add `[broker] compress_threshold_kb = 32` and `compress_min_saving_pct = 10` with validation (non-negative threshold, percentage 0–99, field-named errors, exit code 2), `init-config` and `mqrust.example.toml`
- [ ] 2.3 Unit tests for defaults, threshold 0 and invalid values

## 3. Compression module

- [ ] 3.1 Implement `src/broker/compression.rs`: candidate check, zlib level 1 compression, minimum-saving decision
- [ ] 3.2 Implement per-type encoders (length prefix for `BytesMessage`, zlib stream for the other four types) and the per-type exclusion table
- [ ] 3.3 Implement the bounded per-type decoder (64 KB output cap, truncation flag, error result)
- [ ] 3.4 Unit tests: golden vectors decode identically to the client's uncompressed form; boundary sizes threshold-1, threshold, threshold+1; incompressible data kept; decoder stops at 64 KB on a zip bomb

## 4. Broker integration

- [ ] 4.1 Pass client-compressed messages through unchanged on the entry and delivery paths
- [ ] 4.2 Compress candidates on entry, inline up to 1 MB and in `spawn_blocking` above, with the reader awaiting the result before the next frame
- [ ] 4.3 Account memory and statistics on the stored size and record the compressed flag
- [ ] 4.4 Use the bounded decoder for the admin preview with truncation and error notices (rendered by `add-admin-console`)
- [ ] 4.5 Semantics tests: FIFO with a 5 MB message followed by small ones; other connections progress during a 50 MB compression; headers and `MessageId` unchanged

## 5. Verification

- [ ] 5.1 All unit and semantics tests pass (`cargo test`)
- [ ] 5.2 Java integration tests (§11 item 6) with `amq5` and `amq6`: a client-compressed body reaches the consumer byte-for-byte identical; large uncompressed bodies of each of the 5 types, at threshold-1, threshold and threshold+1, are read correctly; any failing type is marked excluded and retested
- [ ] 5.3 Acceptance scenarios 1, 2 and 3 still pass against `mqrust.exe`
- [ ] 5.4 Earlier Java integration tests still pass
- [ ] 5.5 Add the `criterion` compression benchmark (§14.3) and record throughput for 32 KB, 1 MB and 10 MB bodies
