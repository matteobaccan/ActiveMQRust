# message-compression Specification

## Purpose
Defines how the broker stores, accounts for, optionally compresses and previews message bodies (client-compressed or broker-compressed) without breaking FIFO order, selectors or OpenWire compatibility.
## Requirements
### Requirement: Client-compressed messages passed through unchanged
When a message arrives with `compressed=true` (sent by a client with `useCompression=true`), the broker SHALL keep its `content` exactly as received, SHALL NOT decompress or recompress it on the delivery path, and SHALL deliver it intact with `compressed=true` to every consumer. The receiving Java client decompresses it transparently based on the `compressed` flag, whatever its own `useCompression` setting. Compression in OpenWire is per message: `marshalledProperties` are never compressed and the `tcp://` transport has no stream compression.

#### Scenario: Byte-identical delivery
- **WHEN** a producer with `useCompression=true` sends a message and a consumer receives it
- **THEN** the `content` bytes delivered by the broker are identical to the bytes received from the producer, and the consumer reads the original body

#### Scenario: Consumer without compression enabled
- **WHEN** a producer with `useCompression=true` sends a `TextMessage` and a consumer on a connection with `useCompression=false` receives it
- **THEN** the consumer reads the original text

### Requirement: Memory accounting and statistics for compressed messages
For every compressed message, whether compressed by the client or by the broker, the broker SHALL account the compressed `content` size (the size actually held in RAM) toward message memory and `max_memory_mb`, and SHALL record in the message statistics that the message is compressed together with its compressed size.

#### Scenario: Accounted size
- **WHEN** a client-compressed message with a 10 KB compressed body (100 KB uncompressed) is queued
- **THEN** the accounted message memory increases by the 10 KB body plus properties and overhead, not by 100 KB

#### Scenario: Compressed flag in statistics
- **WHEN** the admin JSON API returns a compressed message
- **THEN** it reports the message as compressed with its compressed size

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

### Requirement: Compressed format per message type
For each message type, the compressed `content` produced by the broker SHALL be exactly the format produced by the `storeContent()` (and related compression code) of the corresponding `ActiveMQ*Message` class in activemq-client 5.18.x / 6.x, so the Java client decodes it as if the producer had compressed it. In particular `ActiveMQBytesMessage` places the original length before the deflate data, while `ActiveMQTextMessage`, `ActiveMQMapMessage`, `ActiveMQObjectMessage` and `ActiveMQStreamMessage` apply a `DeflaterOutputStream` to their serialized content. Each of the 5 types SHALL be verified by Java integration tests and by golden vectors marshalled by the real client (`tests/data/compression/<type>.bin`: the frame of a client with `useCompression=true` followed by the frame of the same body without compression). The supported types are listed in `compressible()` of `src/broker/compress.rs`, which is the exclusion table: a type that fails verification SHALL be removed from it and is then always stored as received. Messages without a body (`ActiveMQMessage`) and any other message type SHALL never be compressed by the broker.

#### Scenario: All five types
- **WHEN** a producer without compression sends a large compressible `TextMessage`, `BytesMessage`, `MapMessage`, `ObjectMessage` and `StreamMessage`
- **THEN** the broker compresses each supported type and the Java consumer reads every body exactly as sent

#### Scenario: Golden vectors
- **WHEN** the compressed golden frame of each type is decoded
- **THEN** its inflated `content` is byte-for-byte the `content` of the uncompressed golden frame, the broker's own compressed form has the same layout (length prefix for `BytesMessage`, zlib header), and the compressed frame passes through the broker with an identical `content`

#### Scenario: Excluded type
- **WHEN** a message type has been excluded from broker-side compression because it failed verification, and a large uncompressed message of that type arrives
- **THEN** the broker stores and delivers it uncompressed

### Requirement: Off-thread compression without breaking FIFO
For bodies larger than 1 MB (1,048,576 bytes), the broker SHALL run compression in `spawn_blocking` so that it does not block the async runtime. The connection reader SHALL wait for compression to finish before processing the next frame of the same connection, and the message SHALL get its `broker_seq` only when it enters the destination, so FIFO order SHALL NOT change. Compression of smaller bodies SHALL run inline.

#### Scenario: Order with mixed sizes
- **WHEN** one producer sends a 5 MB compressible message followed by ten 1 KB messages to a queue
- **THEN** the consumer receives the 5 MB message first and then the ten small messages in send order

#### Scenario: Runtime not blocked
- **WHEN** one connection sends a 50 MB compressible message while another connection exchanges small messages
- **THEN** the other connection keeps receiving and sending messages while the large body is being compressed

### Requirement: Admin preview with bounded decompression
The broker SHALL decompress a compressed body only to render the admin preview, never on the delivery path, using the per-type compressed format. Decompression SHALL stop after 64 KB of decompressed output to guard against "zip bombs", and the preview SHALL show a truncation notice when the limit is reached. The decoder (`decompress_content` in `src/broker/compress.rs`) takes an output limit and returns at most that many bytes; the preview asks for one byte more than it shows and reports truncation when it gets it, so the decoder needs no separate truncation flag. A body that cannot be decompressed SHALL be shown with an error notice instead of a preview, without affecting the message.

#### Scenario: Large compressed body preview
- **WHEN** the admin opens the detail of a compressed `TextMessage` whose decompressed text is 10 MB
- **THEN** the broker decompresses at most 64 KB, shows that text with a truncation notice, and the stored message is unchanged

#### Scenario: Corrupt compressed body
- **WHEN** the admin opens the detail of a message with `compressed=true` whose body is not valid deflate data
- **THEN** the page shows an error notice for the body and the message stays deliverable as received

### Requirement: Selectors unaffected by compression
Compression SHALL apply only to `content`. Properties SHALL never be compressed, so selector evaluation SHALL never require decompression.

#### Scenario: Selector on a compressed message
- **WHEN** a broker-compressed message with property `k = 1` is evaluated by a consumer with selector `k = 1`
- **THEN** the message is selected without its body being decompressed

