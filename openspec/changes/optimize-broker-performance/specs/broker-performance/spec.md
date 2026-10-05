## ADDED Requirements

### Requirement: Opaque bodies on the hot path
The broker SHALL NOT decode the message body on the send, dispatch or acknowledge path. `content` and `marshalledProperties` SHALL be kept as opaque byte buffers that are slices of the received frame, without copying them out of it. The header fields of a message (IDs, destinations, strings and numbers, but not `content` and `marshalledProperties`) SHALL all be decoded: every connection re-encodes the whole `MessageDispatch` in its own negotiated version, so every header field must be available, and decoding them costs little next to the body (only the ID and destination strings allocate). Properties SHALL be decoded only when a selector must evaluate the message, at most once per message; a process-wide counter of decoded property maps (`openwire::props::decode_count()`) SHALL be exposed to tests.

Each connection SHALL read through one reusable buffer of 64 KB. A frame that is not a message is cut from that buffer without allocating; a message frame up to 64 KB is copied once into its own exactly sized allocation, so that a stored message never keeps the shared read buffer and its neighbouring frames alive; a larger frame is read straight from the socket into its own allocation, without passing through the buffer.

#### Scenario: Undecodable body is delivered intact
- **WHEN** a producer sends an `ActiveMQMapMessage` whose `content` bytes are not a valid marshalled map
- **THEN** the broker accepts it and the consumer receives a `content` byte-for-byte identical to the one sent

#### Scenario: No property decoding without selectors
- **WHEN** 10,000 messages with properties flow through a queue whose consumers have no selector
- **THEN** the broker's property-decode counter (exposed to tests) is 0

#### Scenario: Body bytes are not copied
- **WHEN** a 1 MB message is received and stored
- **THEN** the stored `content` buffer points into the received frame's memory (checked by a unit test comparing buffer addresses)

#### Scenario: Many frames in one read
- **WHEN** several frames arrive in pieces of 1,000 bytes, including a 1 MB message
- **THEN** the reader returns each frame whole and in order, and each message's `content` points into its own frame

### Requirement: One shared allocation per message
A stored message SHALL be held in a single `Arc<StoredMessage>`. Deliveries to several topic subscribers and copies sent to queue browsers SHALL share that allocation and SHALL NOT copy the body. The accounted message memory SHALL count a shared body once.

#### Scenario: Topic fan-out shares the body
- **WHEN** topic messaging is present and a 1 MB message is published to a topic with 3 subscribers
- **THEN** the accounted message memory grows by about 1 MB, not 3 MB

#### Scenario: Browser shares the body
- **WHEN** a `QueueBrowser` browses a queue holding a 1 MB message
- **THEN** the accounted message memory does not grow during browsing

### Requirement: Header-only re-encoding with vectored writes
Each connection SHALL encode outgoing commands in its own negotiated OpenWire version. For a `MessageDispatch`, only the headers SHALL be encoded; a body of 2048 bytes or more SHALL be written from the stored buffer with a vectored write, without copying it into the output buffer. A smaller body SHALL be copied into the header buffer: for a few hundred bytes a copy is cheaper than an extra slice per write, and small frames then coalesce into one buffer.

#### Scenario: Different client versions
- **WHEN** a producer with OpenWire version 12 sends a message and a consumer with version 9 receives it
- **THEN** the consumer decodes it correctly and its body bytes are identical to those sent

#### Scenario: No body copy on dispatch
- **WHEN** a 1 MB message is dispatched to a consumer
- **THEN** the writer submits the body as a separate slice of the vectored write (checked by a unit test with a recording writer)

### Requirement: Write batching
The connection writer SHALL take every command already queued for the connection, until 256 KB of encoded data are gathered, and write them before taking more. The batch SHALL be written with vectored writes of at most 64 slices each; small frames are coalesced into one slice, so a batch of small frames is one write call. The writer SHALL NOT wait for more frames when the queue is empty. `TCP_NODELAY` SHALL be enabled on every OpenWire socket.

#### Scenario: Batching under load
- **WHEN** 1,000 dispatches are queued for one connection before the writer runs
- **THEN** they are written with at most 16 write calls (checked with a recording writer)

#### Scenario: No delay at low load
- **WHEN** a single dispatch is queued for an idle connection
- **THEN** it is written immediately, without any timer delay

#### Scenario: TCP_NODELAY
- **WHEN** a client connects
- **THEN** the accepted socket has `TCP_NODELAY` enabled

### Requirement: Socket buffers sized for large dispatches
The OpenWire listening socket SHALL have its TCP send and receive buffers set to `broker.socket_buffer_kb` (default 1024; 0 keeps the operating system default; at most 65536) before it listens, so that every accepted socket inherits them. With the small Windows default, writes of large messages stall between socket writes and consumers wait for data while the broker is idle.

#### Scenario: Default buffer
- **WHEN** the broker starts without `socket_buffer_kb`
- **THEN** accepted OpenWire sockets have 1 MB send and receive buffers

#### Scenario: Large messages are not throttled
- **WHEN** 10,000 messages of 50 KB held in a queue are consumed with prefetch 1000 on loopback
- **THEN** the consume time is not worse than ActiveMQ 5.18 tuned on the same machine

#### Scenario: Invalid value
- **WHEN** `socket_buffer_kb` is negative or above 65536
- **THEN** the configuration is rejected with an error that names the key

### Requirement: Processor count like the JVM
The broker SHALL compute the processors it may use like the JVM's `Runtime.availableProcessors()`: on Windows, the processors in the process affinity mask (falling back to the system count with more than 64 processors). `broker.processors` or `--processors` SHALL override it, like `-XX:ActiveProcessorCount` (0 = automatic, at most 1024). The runtime SHALL use one I/O worker per processor and run CPU-heavy work (compression) on at most `processors - 1` threads (at least 1), like the `ForkJoinPool.commonPool()` parallelism. The count in use SHALL be logged at start-up.

#### Scenario: Affinity is honoured
- **WHEN** the broker is started with `start /affinity 3` on a machine with 8 logical processors
- **THEN** it logs 2 processors and starts 2 I/O workers

#### Scenario: Override
- **WHEN** the broker is started with `--processors 3`
- **THEN** it uses 3 I/O workers and at most 2 compression threads, whatever the affinity

### Requirement: Short and partitioned locks
The destination registry SHALL be a map split into 64 shards, each a `parking_lot::RwLock<HashMap<Destination, Arc<Dest>>>` chosen by the hash of the destination, so lookups of different destinations rarely contend on one lock; the key is the `Destination` (kind and name), so a queue and a topic with the same name are different entries. Each destination SHALL have its own lock, held only for in-memory work and never during network I/O, encoding or compression: commands for consumers are queued on their connection's channel and encoded by its writer task. Under the destination lock the broker SHALL do the dispatch decisions, which include evaluating selectors (decoding a message's properties the first time a selector needs them) and, for a consumer with a selector, scanning the pending messages in order until one matches (O(n) in the worst case). Operations on two different queues SHALL never wait for the same lock.

#### Scenario: Independent queues
- **WHEN** a test holds the lock of queue `A` and a producer sends to queue `B`
- **THEN** the send to `B` completes without waiting for `A`'s lock to be released

#### Scenario: No lock during I/O
- **WHEN** a consumer's socket is blocked because the client does not read (its writer task never completes a write)
- **THEN** producers and other consumers of the same queue continue to send and receive

### Requirement: Bounded allocations per message
In steady state, the number of heap allocations per message on the receive, store, dispatch and encode path SHALL be a constant that does not depend on body size, and at most 9 on average (as measured): receiving takes the message frame and its promotion to a shared buffer when the body is sliced (2), the decoded message box (1) and the producer, message-id and destination strings (3); storing takes the shared `Arc<Message>` (1) and the shared accounting record (1); dispatching takes on average 1 for the writer's output buffer. Read buffers and encode buffers SHALL be reused per connection. IDs SHALL be `Arc<str>` values shared by every copy of a decoded command, but they are not interned across commands: each decoded message allocates its own strings, except that the message ID reuses the producer's connection-id string when they are equal; ASCII strings are decoded and encoded without intermediate buffers.

#### Scenario: Allocation count
- **WHEN** a test with a counting allocator (counting on the test thread only) reads, decodes, stores, dispatches and encodes 10,000 messages of 1 KB after a warm-up of 1,000 messages (acknowledgements are not counted)
- **THEN** the allocations counted divided by 10,000 are at most 9 (plus 0.1 for the occasional growth of queues and maps)

#### Scenario: Independent of size
- **WHEN** the same test runs with 100 KB messages
- **THEN** the allocations per message are the same as with 1 KB messages

### Requirement: Immediate response to synchronous sends
For a send with `responseRequired=true`, the broker SHALL queue the `Response` as soon as the message is stored in the destination. Storing includes the dispatch decision, which only places the message on the outgoing queues of the chosen consumers' connections; the `Response` SHALL NOT wait for any message to be written to a consumer socket, received or acknowledged.

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
- **WHEN** a test serves a connection with `connection::serve_with_codec` and a wrapper codec that implements `WireCodec` and counts calls
- **THEN** a full send and receive round trip works and every frame after the client's `WireFormatInfo` passes through the wrapper

#### Scenario: Tight encoding not negotiated
- **WHEN** a Java client that announces `TightEncodingEnabled=true` connects
- **THEN** the session uses loose encoding

### Requirement: Criterion micro-benchmarks
The repository SHALL contain `criterion` benchmarks, run with `cargo bench`, each file using `mimalloc` as its global allocator like the broker: `benches/codec.rs` (decoding and encoding an `ActiveMQTextMessage` of 1 KB with OpenWire version 12 and with version 9, the oldest version the broker accepts, plus zero-copy encoding of a 50 KB body); `benches/dispatch.rs` (enqueue, dispatch and acknowledge on a queue with 1 consumer and with 10 consumers); `benches/compression.rs` (broker-side compression of 32 KB, 64 KB, 1 MB and 10 MB bodies, XML with a base64 payload and plain text); `benches/selectors.rs` (a header selector, and dispatch to 10 selective consumers of a queue holding 100,000 messages); `benches/expiry.rs` (a sweep pass over 1,000,000 messages with random TTLs, and enqueue plus dispatch with and without the sweeper running in the background, whose results must not differ measurably). The baseline for later comparisons is the code of version 0.2.0, saved with `cargo bench -- --save-baseline v0.2.0`.

#### Scenario: Benchmarks build
- **WHEN** `cargo bench --no-run` is run
- **THEN** every benchmark file compiles

#### Scenario: Benchmarks run
- **WHEN** `cargo bench` is run on the development machine
- **THEN** every listed benchmark runs and reports its time per iteration

#### Scenario: Regression comparison
- **WHEN** `cargo bench -- --baseline v0.2.0` is run after a change
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
