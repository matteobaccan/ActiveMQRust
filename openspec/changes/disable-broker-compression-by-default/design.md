## Context

Broker-side compression (zlib level 2 through `flate2`/`zlib-rs`) is applied on entry to uncompressed bodies larger than `compress_threshold_kb`, and compressed bodies are delivered compressed to consumers, who inflate them in the Java client. The default threshold was 32 KB.

## Goals / Non-Goals

**Goals:**
- Best throughput and latency with the default configuration.
- Keep broker-side compression available, unchanged, for deployments that prefer RAM savings.

**Non-Goals:**
- A faster algorithm (LZ4, zstd): consumers receive broker-compressed bodies as they are stored, and the ActiveMQ Java client can only inflate zlib, so another algorithm would require decompressing before every dispatch.
- Adaptive compression (switching on under memory pressure): possible later, not needed for this change.

## Decisions

1. **Default `compress_threshold_kb = 0`.** Measured at full speed with 50 KB and 300 KB bodies, compression lowered throughput by about 3.5× and raised CPU per message by 15×; with no compression ActiveMQRust matched or beat ActiveMQ in throughput and used 5–8× less memory. *Alternative:* threshold 256 KB — rejected, the 300 KB run shows the same penalty.
2. **Behaviour with a threshold set is unchanged**, so existing configurations that set the key keep working.
3. **Tests set the threshold explicitly** where they exercise compression; default-configuration tests assert that nothing is compressed.
4. **The comparison benchmark keeps measuring broker compression** in measurement (e) by setting 32 in `mqrust-bench.toml`, and the "nocompress" setup now equals the default.

## Risks / Trade-offs

- [Higher RAM with large bodies at a steady rate for users who relied on the default] → Documented in README and template with the measured trade-off; one line of configuration restores it.
