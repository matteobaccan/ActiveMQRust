## MODIFIED Requirements

### Requirement: Compression configuration keys
The `[broker]` section SHALL support:
- `compress_threshold_kb` (default 0): bodies larger than this many KB (1 KB = 1024 bytes) are candidates for broker-side compression; 0, the default, disables broker-side compression, so the broker gives its best throughput and latency out of the box; a positive value (for example 32 or 256) trades CPU and throughput for lower memory;
- `compress_min_saving_pct` (default 10): the compressed body is kept only if it saves at least this percentage of the original size.

`compress_threshold_kb` SHALL be a non-negative integer and `compress_min_saving_pct` an integer from 0 to 99; other values SHALL be reported with the field name and exit code 2. Without a configuration file the defaults SHALL apply.

#### Scenario: Defaults
- **WHEN** the broker starts without these keys and an uncompressed 1 MB `TextMessage` arrives
- **THEN** the broker stores and delivers it uncompressed, and the minimum saving used when compression is enabled is 10%

#### Scenario: Enabled by configuration
- **WHEN** the configuration sets `compress_threshold_kb = 32`
- **THEN** broker-side compression applies to bodies larger than 32 KB with a minimum saving of 10%

#### Scenario: Invalid percentage
- **WHEN** the configuration sets `compress_min_saving_pct = 150`
- **THEN** the broker reports `broker.compress_min_saving_pct` and exits with code 2

#### Scenario: Disabled
- **WHEN** `compress_threshold_kb = 0` and an uncompressed 1 MB message arrives
- **THEN** the broker stores and delivers it uncompressed

### Requirement: Broker-side compression of large bodies
When broker-side compression is enabled (`compress_threshold_kb` > 0) and an uncompressed message (`compressed=false`) of a supported type arrives with `content` strictly larger than `compress_threshold_kb * 1024` bytes, the broker SHALL compress it before queuing it, using deflate in zlib format (the format `java.util.zip.Inflater` reads by default) at level 2 with the `flate2` crate and its `zlib-rs` backend. Level 2 is the lowest level that saves at least 15% on XML carrying a base64 payload (the payload of the Java benchmark): level 1 of `zlib-rs` uses static Huffman codes only and saves under 1% on base64 text, while level 2 uses dynamic Huffman codes and saves about 25%; on plain text level 2 also produces about a third less output than level 1. The choice is based on compressed sizes only; its speed is confirmed by the compression benchmark. If compression saves at least `compress_min_saving_pct` percent of the original `content` size, the broker SHALL replace `content` with the compressed form and set `compressed=true`; otherwise it SHALL keep the original unchanged. The decision SHALL be made once, on entry. `MessageId`, all headers and `marshalledProperties` SHALL stay unchanged, so for the receiving client the message is identical to one sent with `useCompression=true`. Messages already compressed by the client SHALL never be compressed again.

#### Scenario: Large text body compressed
- **WHEN** `compress_threshold_kb = 32` and a producer without compression sends a 200 KB `TextMessage` of repetitive JSON
- **THEN** the broker stores it with `compressed=true` and a smaller body, the consumer reads the identical text, and `JMSMessageID` is unchanged

#### Scenario: Boundary sizes
- **WHEN** compressible bodies of exactly `threshold - 1`, `threshold` and `threshold + 1` bytes (threshold = 32 × 1024, with `compress_threshold_kb = 32`) are sent uncompressed
- **THEN** only the `threshold + 1` body is compressed by the broker, and all three are read correctly by the Java consumer

#### Scenario: Incompressible data
- **WHEN** broker-side compression is enabled and an uncompressed 500 KB `BytesMessage` containing random or already-compressed data (JPEG, ZIP) arrives
- **THEN** compression saves less than 10%, the broker keeps the original body with `compressed=false`, and the consumer reads it unchanged

#### Scenario: Level choice
- **WHEN** a test compresses 1 MB of XML with a base64 payload at levels 1 to 6
- **THEN** the broker's level is the lowest one whose output is at most 85% of the input

#### Scenario: Wasted compression is counted
- **WHEN** the broker compresses a body and then keeps the original because the saving is below `compress_min_saving_pct`
- **THEN** the broker statistics counter `compress_discarded` grows by one, while `compressed` counts the bodies the broker did replace

#### Scenario: Frame released after compression
- **WHEN** the broker replaces a body with its compressed form
- **THEN** the stored message no longer references the received frame (its properties are copied), so only the compressed size stays in memory
