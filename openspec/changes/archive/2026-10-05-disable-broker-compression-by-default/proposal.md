## Why

Load tests with 20 producers and 20 consumers sending as fast as they can for one minute (same Java client for every broker) showed that broker-side compression makes ActiveMQRust slower than ActiveMQ whenever it applies:

| Message size | ActiveMQRust, compression on | ActiveMQRust, compression off | ActiveMQ 5.18.7 | ActiveMQ 6.3.2 |
|---|---|---|---|---|
| 50 KB (default threshold 32 KB) | 155 MB/s | 529 MB/s | 404 MB/s | 437 MB/s |
| 300 KB (threshold 256 KB) | 148 MB/s | 547 MB/s | 545 MB/s | 544 MB/s |

With compression the broker spends 4–5 cores in zlib, the backlog grows, and at full speed it even uses more memory (542 MB) than without compression (202 MB). Users who try the broker with its defaults must see its best performance first; saving RAM by compressing bodies stays available as an explicit choice.

## What Changes

- **BREAKING (default only)**: `broker.compress_threshold_kb` defaults to `0`, so the broker does not compress bodies unless configured. Setting a value (for example 32 or 256) restores broker-side compression above that size, unchanged in behaviour.
- `init-config`, `mqrust.example.toml`, the README and the help describe the trade-off: compression saves RAM at a steady rate, costs CPU and throughput at full speed.
- Tests that exercise broker-side compression set the threshold explicitly; the Java compression checks take the threshold from an option and verify that nothing is compressed when it is 0.
- Client-compressed messages (`useCompression=true`) are unaffected: they are still passed through unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `message-compression`: "Compression configuration keys" (default 0 instead of 32) and "Broker-side compression of large bodies" (scenarios state the configured threshold instead of the 32 KB default).

## Impact

- `src/config.rs` (default and template), `mqrust.example.toml`, `scripts/activemq-bench/mqrust-bench.toml` (sets 32 explicitly so the comparison keeps measuring broker compression in measurement (e)).
- Tests: `tests/broker_semantics.rs`, `tests/compression.rs`, `src/config.rs`/`src/setup.rs` unit tests, `tests/cli_setup.rs`, Java `Integration.brokerCompression` and `CompressionChecks`.
- README: features list, configuration, results.
