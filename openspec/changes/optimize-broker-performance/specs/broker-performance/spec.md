## ADDED Requirements

### Requirement: Opaque bodies on the hot path
The broker SHALL NOT decode the message body on the send, dispatch or acknowledge path. `content` and `marshalledProperties` SHALL be kept as opaque byte buffers taken from the received frame without copying. The broker SHALL read only the headers it needs: destination, message ID, expiration, persistence, `compressed` flag and `redeliveryCounter`. Properties SHALL be decoded only when a selector must evaluate the message, at most once per message.

#### Scenario: Undecodable body is delivered intact
- **WHEN** a producer sends an `ActiveMQMapMessage` whose `content` bytes are not a valid marshalled map
- **THEN** the broker accepts it and the consumer receives a `content` byte-for-byte identical to the one sent

#### Scenario: No property decoding without selectors
- **WHEN** 10,000 messages with properties flow through a queue whose consumers have no selector
- **THEN** the broker's property-decode counter (exposed to tests) is 0

#### Scenario: Body bytes are not copied
- **WHEN** a 1 MB message is received and stored
- **THEN** the stored `content` buffer points into the received frame's memory (checked by a unit test comparing buffer addresses)

### Requirement: One shared allocation per message
A stored message SHALL be held in a single `Arc<StoredMessage>`. Deliveries to several topic subscribers and copies sent to queue browsers SHALL share that allocation and SHALL NOT copy the body. The accounted message memory SHALL count a shared body once.

#### Scenario: Topic fan-out shares the body
- **WHEN** topic messaging is present and a 1 MB message is published to a topic with 3 subscribers
- **THEN** the accounted message memory grows by about 1 MB, not 3 MB

#### Scenario: Browser shares the body
- **WHEN** a `QueueBrowser` browses a queue holding a 1 MB message
- **THEN** the accounted message memory does not grow during browsing

### Requirement: Header-only re-encoding with vectored writes
Each connection SHALL encode outgoing commands in its own negotiated OpenWire version. For a `MessageDispatch`, only the headers SHALL be encoded; the body SHALL be written from the stored buffer with a vectored write, without copying it into the output buffer.

#### Scenario: Different client versions
- **WHEN** a producer with OpenWire version 12 sends a message and a consumer with version 9 receives it
- **THEN** the consumer decodes it correctly and its body bytes are identical to those sent

#### Scenario: No body copy on dispatch
- **WHEN** a 1 MB message is dispatched to a consumer
- **THEN** the writer submits the body as a separate slice of the vectored write (checked by a unit test with a recording writer)

### Requirement: Write batching
The connection writer SHALL write everything already queued for the connection, up to 64 frames or 256 KB, in one vectored write before flushing. It SHALL NOT wait for more frames when the queue is empty. `TCP_NODELAY` SHALL be enabled on every OpenWire socket.

#### Scenario: Batching under load
- **WHEN** 1,000 dispatches are queued for one connection before the writer runs
- **THEN** they are written with at most 16 write calls (checked with a recording writer)

#### Scenario: No delay at low load
- **WHEN** a single dispatch is queued for an idle connection
- **THEN** it is written immediately, without any timer delay

#### Scenario: TCP_NODELAY
- **WHEN** a client connects
- **THEN** the accepted socket has `TCP_NODELAY` enabled

### Requirement: Short and partitioned locks
The destination registry SHALL be a partitioned concurrent map, so lookups of different destinations do not contend on a single lock. Each destination SHALL have its own lock, held only for in-memory operations of O(1) or O(log n), and never during network I/O, encoding or compression. Operations on two different queues SHALL never wait for the same lock.

#### Scenario: Independent queues
- **WHEN** a test holds the lock of queue `A` and a producer sends to queue `B`
- **THEN** the send to `B` completes without waiting for `A`'s lock to be released

#### Scenario: No lock during I/O
- **WHEN** a consumer's socket is blocked because the client does not read
- **THEN** producers and other consumers of the same queue continue to send and receive

### Requirement: Bounded allocations per message
In steady state, the number of heap allocations per message on the send-and-dispatch path SHALL be a constant that does not depend on body size, and at most 2 per message received and 1 per message dispatched. Read buffers SHALL be reused per connection, and connection, session, producer and consumer IDs SHALL be shared `Arc<str>` values.

#### Scenario: Allocation count
- **WHEN** a test with a counting global allocator sends and dispatches 10,000 messages of 1 KB after a warm-up of 1,000 messages
- **THEN** the allocations counted divided by 10,000 are at most 3

#### Scenario: Independent of size
- **WHEN** the same test runs with 100 KB messages
- **THEN** the allocations per message are the same as with 1 KB messages

### Requirement: Immediate response to synchronous sends
For a send with `responseRequired=true`, the broker SHALL send the `Response` as soon as the message is added to the destination, without waiting for dispatch to consumers.

#### Scenario: Synchronous send without consumers
- **WHEN** a producer with `alwaysSyncSend=true` sends 1,000 messages to a queue with no consumer
- **THEN** every send returns, and each `Response` is sent before any consumer exists

### Requirement: Release profile
The release build SHALL use `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `opt-level = 3` and `mimalloc` as the global allocator.

#### Scenario: Profile check
- **WHEN** `Cargo.toml` is inspected and `cargo build --release` is run
- **THEN** the release profile has those four settings, the build succeeds, and the binary uses `mimalloc` as its global allocator

### Requirement: Loose encoding behind the WireCodec trait
The broker SHALL continue to negotiate loose encoding without marshalling cache (`TightEncodingEnabled=false`, `CacheEnabled=false`). All encoding and decoding SHALL go through the `WireCodec` trait, so that an alternative codec can be used by a connection without changing connection or broker code. A `broker.tight_encoding` option SHALL NOT be added unless a benchmark shows a real advantage; this change adds no configuration key.

#### Scenario: Codec is pluggable
- **WHEN** a test builds a connection with a wrapper codec that implements `WireCodec` and counts calls
- **THEN** a full send and receive round trip works and every frame passes through the wrapper

#### Scenario: Tight encoding not negotiated
- **WHEN** a Java client that announces `TightEncodingEnabled=true` connects
- **THEN** the session uses loose encoding

### Requirement: Criterion micro-benchmarks
The repository SHALL contain `criterion` benchmarks, run with `cargo bench`, for: decoding and encoding an `ActiveMQTextMessage` of 1 KB (OpenWire versions 12 and 6); enqueue and dispatch on a queue with 1 consumer and with 10 consumers. When the features are present, it SHALL also contain: broker-side compression of a 64 KB body; dispatch with 10 selective consumers on a queue of 100,000 messages; and the expiry sweeper with 1,000,000 messages with random TTLs, measuring that the sweeper does not measurably increase p99 dispatch latency.

#### Scenario: Benchmarks run
- **WHEN** `cargo bench` is run on the development machine
- **THEN** every listed benchmark for the features present runs and reports its time per iteration

#### Scenario: Regression comparison
- **WHEN** `cargo bench -- --baseline <name>` is run after a change
- **THEN** criterion reports the change of each benchmark against the saved baseline

### Requirement: Java end-to-end benchmark mode
The Java program in `tests/java-it` SHALL offer a `bench` mode, run as `java -jar mqrust-acceptance.jar bench --url <url> --user <u> --password <p> --scenario <name>` and through `tests\java-it\run-bench.cmd`, using the real `activemq-client` (profiles `amq5` and `amq6`). It SHALL support at least: `--messages <n>`, `--size <bytes>`, `--send async|sync` (async sets `jms.useAsyncSend=true`, sync sets `jms.alwaysSyncSend=true`), `--producers <n>`, `--consumers <n>`, `--rate <msg/s>` and `--warmup <n>`. Messages SHALL be `NON_PERSISTENT`, sessions `AUTO_ACKNOWLEDGE`. Warm-up messages SHALL be excluded from the results. For each run it SHALL print one `RESULT` line with scenario, parameters, elapsed time, msgs/s, MB/s (1 MB = 1,048,576 bytes of payload) and, for latency runs, p50, p99 and max latency in microseconds. It SHALL check that every message was received, and report the run as failed otherwise.

#### Scenario: Throughput scenario
- **WHEN** `bench --scenario throughput --messages 1000000 --size 1024 --send async` runs against the broker
- **THEN** one producer and one consumer exchange the messages on a new queue and a `RESULT` line reports elapsed time, msgs/s and MB/s

#### Scenario: Missing messages
- **WHEN** fewer messages than sent are received within the timeout
- **THEN** the `RESULT` line is marked failed and the exit code is 1

#### Scenario: Latency scenario
- **WHEN** `bench --scenario latency --rate 1000 --messages 60000 --size 1024` runs
- **THEN** the producer sends at 1,000 msg/s, each message carries its send time from `System.nanoTime()`, and the `RESULT` line reports p50, p99 and max end-to-end latency

### Requirement: Measurable targets
On the development machine, with broker and client on the same machine and then over a LAN when available, with `NON_PERSISTENT` 1 KB messages, the broker SHALL meet these targets, measured as the median of at least 3 runs after a warm-up:

| Scenario | Target |
|---|---|
| 1 producer → 1 consumer, async send | ≥ 100,000 msg/s |
| 1 producer → 1 consumer, sync send | limited only by the network round trip: at least 80% of `1 / round-trip time`, with the round trip measured by a TCP ping-pong on the same path |
| 10 producers → 10 consumers on 10 queues | scaling efficiency ≥ 70% of linear, up to the number of physical cores |
| p99 end-to-end latency at 1,000 msg/s | < 1 ms |
| Idle broker after startup | < 20 MB Working Set |
| Comparison with ActiveMQ 5.18 / 6.x | throughput ≥ ActiveMQ, memory ≤ 1/5 (measured by `add-activemq-comparison-benchmark`) |

#### Scenario: Async throughput target
- **WHEN** the throughput scenario with async send and 1 KB messages runs on loopback
- **THEN** the median result is at least 100,000 msg/s

#### Scenario: Latency target
- **WHEN** the latency scenario runs at 1,000 msg/s on loopback
- **THEN** the median p99 end-to-end latency is below 1 ms

#### Scenario: Scaling target
- **WHEN** the scale scenario runs with 10 producers and 10 consumers on 10 queues on a machine with C physical cores
- **THEN** the aggregate msgs/s is at least 0.7 × min(10, C) × the single-pair async result

#### Scenario: Idle memory target
- **WHEN** `mqrust.exe` has started with no configuration and has been idle for 10 seconds
- **THEN** its Working Set is below 20 MB

### Requirement: Published performance results
The results of the end-to-end benchmarks SHALL be written to `docs/benchmarks/performance-<yyyy-MM-dd>.md`, with the machine details (CPU, cores, RAM, Windows version, JDK version, ActiveMQRust version, client profile), the command lines used, the median and the individual runs, and "met" or "not met" for each target. The report SHALL also give the broker's Working Set when idle and when holding 100,000 messages of 1 KB. The README SHALL summarize the latest results and link to the report. A missed target SHALL be reported as not met, never omitted.

#### Scenario: Report content
- **WHEN** the benchmarks have been run and the report is written
- **THEN** it contains the machine details, every target with its measured value and met/not met status, and the idle and 100,000 × 1 KB memory figures

#### Scenario: Missed target
- **WHEN** a measured value misses its target
- **THEN** the report shows it as "not met" with the measured value
