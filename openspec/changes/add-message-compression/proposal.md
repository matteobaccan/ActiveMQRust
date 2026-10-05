## Why

ActiveMQRust keeps every message in RAM, so the size of large bodies directly drives memory use; the project aims to **use less RAM** and be **faster** than ActiveMQ while staying **compatible** with it. Java applications that enable `useCompression=true` on `ActiveMQConnectionFactory` send deflate-compressed bodies, and the broker must deliver them unchanged to any consumer. Many applications do not enable compression even for large XML or JSON payloads: compressing those bodies once on entry, quickly, reduces queue memory and the bandwidth towards consumers, with no change on the client side because the receiving client already decompresses any message marked `compressed`.

## What Changes

- Client-compressed messages (`compressed=true`) are kept and delivered byte-for-byte as received: never decompressed or recompressed on the delivery path. Memory accounting and statistics use the compressed size and record that the message is compressed.
- Broker-side compression: an uncompressed message whose `content` is larger than `compress_threshold_kb` (default 32 KB) is compressed on entry with deflate in zlib format, level 2 (the lowest `zlib-rs` level with dynamic Huffman codes, which level 1 lacks), using `flate2` with the `zlib-rs` backend. The compressed form is kept only if it saves at least `compress_min_saving_pct` (default 10%). `MessageId` and all headers are unchanged; `compressed=true` is set.
- Bodies over 1 MB are compressed in `spawn_blocking` without breaking FIFO: the connection reader waits for the result before reading the next frame.
- The compressed `content` format reproduces exactly the `storeContent()` of each `ActiveMQ*Message` class (5.18.x / 6.x) for the 5 JMS body types; a type that fails verification is excluded from broker-side compression.
- Admin preview decompresses bodies with a 64 KB decompressed cap to guard against zip bombs.
- New `[broker]` configuration keys `compress_threshold_kb` (32; 0 disables) and `compress_min_saving_pct` (10).

## Capabilities

### New Capabilities

- `message-compression`: pass-through of client-compressed bodies, broker-side compression rules and thresholds, per-message-type compressed format, FIFO with off-thread compression, memory accounting, admin preview decompression, configuration keys.

### Modified Capabilities

None.

## Impact

- Code: new `src/broker/compress.rs` (decision, per-type format, compression and bounded decompression), message entry path in `src/connection.rs`, accounting in `src/broker/entry.rs`, statistics in `src/broker/mod.rs`, preview in `src/admin/body.rs`, `[broker]` keys in `src/config.rs`.
- New crate compiled into the executable: `flate2` with the pure-Rust `zlib-rs` backend (no native library, keeps `mqrust.exe` dependency-free).
- Depends on `add-queue-messaging` (message entry, storage and dispatch). The admin preview is rendered by `add-admin-console`.
- The Java acceptance scenarios 1, 2 and 3 keep passing.
