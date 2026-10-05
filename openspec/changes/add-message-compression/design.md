## Context

After `add-queue-messaging`, messages are stored as `Arc<StoredMessage>` with `content` and `marshalledProperties` kept as opaque `bytes::Bytes` taken from the frame, and the `compressed` flag is decoded but ignored. In OpenWire, compression is per message: a Java client with `useCompression=true` deflates `content` in `storeContent()` and sets `compressed=true`; any receiving client inflates it based on the flag. Each `ActiveMQ*Message` class has its own compressed layout (for example `ActiveMQBytesMessage` writes the original length before the deflate data). The project goals are lower RAM and higher speed than ActiveMQ with full client compatibility, and the executable must stay free of native dependencies.

## Goals / Non-Goals

**Goals:**
- Client-compressed bodies travel through the broker byte-for-byte unchanged and are accounted at their compressed size.
- Large uncompressed bodies are compressed once on entry, cheaply, in a format every Java client decodes.
- No effect on FIFO order and no blocking of the async runtime.
- Safe admin preview of compressed bodies.

**Non-Goals:**
- Decompressing for consumers that do not support compression (every OpenWire Java client does).
- Stream-level (transport) compression; `tcp://` has none.
- Compressing properties or headers.
- Recompressing client-compressed bodies at a higher ratio.

## Decisions

### D1. zlib format, level 1, `flate2` with `zlib-rs`
Level 1 gives most of the size reduction on text/XML/JSON at a fraction of the CPU of the default level. zlib (not raw deflate, not gzip) is what `java.util.zip.Inflater` reads by default. `zlib-rs` is pure Rust, so no native library enters the executable.
- *Alternatives:* `miniz_oxide` backend (pure Rust but slower); the C `zlib-ng` backend (fast, but a native build dependency against the single-exe goal); LZ4 or zstd (faster or better, but Java clients cannot read them through the `compressed` flag).

### D2. Threshold and minimum saving, decided once on entry
A body is a candidate when `content.len() > compress_threshold_kb * 1024` and `compressed=false`. The compressed result is kept only if `compressed_len <= original_len * (100 - compress_min_saving_pct) / 100`. The decision is never revisited, so dispatch and redelivery do no compression work.
- *Alternatives:* compressing every body (CPU wasted on small bodies where headers dominate); sampling the first bytes to predict compressibility (extra complexity; level 1 on incompressible data is fast enough and the result is discarded).

### D3. Per-type encoder module, verified against the Java sources
`src/broker/compression.rs` holds one encoder and one bounded decoder per supported data type (24 Bytes, 25 Map, 26 Object, 27 Stream, 28 Text). The uncompressed `content` of these types is already the serialized form, so the encoder applies zlib to it, adding the length prefix for `BytesMessage`. Each type has a flag in a compile-time table; a type that fails the Java integration check is set to "excluded" and is passed through as received. `ActiveMQMessage` (no body) and other types are never compressed.
- *Alternatives:* decoding and re-serializing bodies via the Java object model (requires full body decoding on the hot path, against §14.1); compressing all types with one format (would break `BytesMessage`, which has a different layout).

### D4. `spawn_blocking` above 1 MB, reader waits
The connection reader calls the compressor inline for bodies up to 1 MB. Above 1 MB it awaits a `spawn_blocking` task. Because the same reader handles the next frame only after the await, messages from one connection keep their order; `broker_seq` is assigned when the message enters the destination, after compression.
- *Alternatives:* always inline (a 50 MB body would stall a runtime worker and every connection on it); a separate compression pool with reordering buffers (more code for no gain, since the reader must preserve per-connection order anyway).

### D5. Accounting on the stored size
Memory accounting and the `max_memory_mb` check use the stored (possibly compressed) size, measured after compression. Statistics record `compressed` and the stored size.
- *Alternatives:* accounting the uncompressed size (overstates RAM use and rejects messages that would fit).

### D6. Bounded decompression only in the admin
Admin preview inflates with a streaming decoder into a buffer capped at 64 KB of output and stops there with a truncation notice. Inflate errors produce an error notice. The delivery path never inflates.
- *Alternatives:* full decompression for the preview (zip-bomb exposure: a small body can expand to gigabytes).

## Risks / Trade-offs

- [Per-type compressed layout differs from the client's expectations, so consumers fail to read broker-compressed messages] → Replicate `storeContent()` exactly from 5.18.x / 6.x sources; integration tests on all 5 types at threshold-1, threshold and threshold+1 with both driver profiles; any failing type is excluded from broker-side compression.
- [CPU cost on entry raises latency for large messages] → Level 1, a 32 KB threshold, and off-thread work above 1 MB; `criterion` benchmarks measure compression throughput.
- [Incompressible payloads waste CPU every time] → Minimum-saving rule keeps the original; the cost is one level-1 pass per message on entry.
- [Older or non-Java clients without inflate support] → Out of scope for the first version (A1 reference client is Java); `compress_threshold_kb = 0` disables broker-side compression.
- [Zip bomb in the admin] → 64 KB decompressed cap.

## Migration Plan

No data migration: the broker keeps no state across restarts. Existing clients need no change; consumers transparently inflate broker-compressed messages. If a compatibility problem appears, set `compress_threshold_kb = 0` to disable broker-side compression without redeploying. Rollback means deploying the previous `mqrust.exe` or pointing clients back to ActiveMQ.

## Open Questions

- Verify against `storeContent()` / `doCompress()` / `copy()` of `ActiveMQBytesMessage`, `ActiveMQTextMessage`, `ActiveMQMapMessage`, `ActiveMQObjectMessage` and `ActiveMQStreamMessage` in 5.18.x and 6.x the exact compressed layout of each type (length prefix width and position for `BytesMessage`; whether `TextMessage` compresses the `writeUTF8` form including its length).
- Verify how each class's `getContent`/`decompress` path detects the format (for example whether `BytesMessage` relies on the length prefix), so broker-compressed bodies decode on both driver versions.
- Verify whether `ActiveMQBlobMessage` and plain `ActiveMQMessage` ever carry compressible `content`, to confirm their exclusion.
- Decide after benchmarks whether 1 MB is the right inline/off-thread boundary on the target hardware.
