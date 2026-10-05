## ADDED Requirements

### Requirement: Same benchmark client for both brokers
The comparison SHALL use the `bench` mode of `mqrust-acceptance.jar` from `tests/java-it` as the only client, against both ActiveMQRust and ActiveMQ, on the same machine, over `tcp://127.0.0.1:<port>` (the script's `-Port`, default 61616, used for every broker), with the same JDK, the same JVM options for the client (`-Xmx3g`), the same connection URL options and the same credentials (`admin` / `admin`, configured for both brokers in the benchmark only). ActiveMQ 5.18.x SHALL be compared with ActiveMQRust using the `amq5` build of the jar, and ActiveMQ 6.x with ActiveMQRust using the `amq6` build. Messages SHALL be `NON_PERSISTENT`, sessions `AUTO_ACKNOWLEDGE`, the queue prefetch SHALL be 1000 on both (`jms.prefetchPolicy.queuePrefetch=1000`), and client compression SHALL be off (`jms.useCompression=false`). Only one broker SHALL run at a time.

#### Scenario: Identical client invocation
- **WHEN** the comparison script runs the same workload against ActiveMQ 6.x and against ActiveMQRust
- **THEN** both runs use the same `amq6` jar file, the same `java` executable and an identical client command line, and the report lists that command line once

#### Scenario: Port free before start
- **WHEN** the script starts and the OpenWire port or the admin port it was given is already in use
- **THEN** it stops with an error naming the port and runs nothing

### Requirement: XML TextMessage payload
Every benchmark message, including warm-up messages, SHALL be a JMS `TextMessage` whose text is an XML document of the form `<message><id>…</id><field01>…</field01> … <field20>…</field20><payload encoding="base64">…</payload></message>`, with an int property `seq` equal to `<id>`. The field names SHALL be fixed (`field01` to `field20`) and their values random, of fixed kinds by position: `field01`–`field04` alphanumeric strings of 8 to 24 characters; `field05`–`field08` integers; `field09`–`field12` decimals with 2 to 6 fractional digits; `field13`–`field16` ISO-8601 UTC timestamps with milliseconds; `field17`–`field20` booleans. All characters SHALL be ASCII. The `payload` element SHALL contain padded standard base64 of random bytes, with the largest length that is a multiple of 4 and fits; 0 to 3 space characters before `</message>` SHALL make the UTF-8 length of the whole document equal the target size exactly: 1,024 bytes (1 KB), 10,240 bytes (10 KB), 12,288 bytes (12 KB) or 51,200 bytes (50 KB).

#### Scenario: Exact size
- **WHEN** the generator produces documents for targets 1,024, 10,240, 12,288 and 51,200
- **THEN** every document's UTF-8 byte length equals its target exactly

#### Scenario: Well-formed and complete
- **WHEN** a generated document is parsed with a standard XML parser
- **THEN** it is well-formed, its root `message` contains `id`, `field01` … `field20` and `payload` in that order, every field value matches its kind, and `payload` decodes as base64

#### Scenario: Size check before sending
- **WHEN** a generated document does not have the exact target length
- **THEN** the bench aborts the run before sending anything and reports the failure

### Requirement: Deterministic generation outside the timed section
Messages SHALL be generated from a fixed seed per scenario (a different fixed seed for warm-up messages), so that both brokers receive identical message sets. The warm-up phase SHALL generate at most 1,000 distinct documents and send them in cycles. The full message set of a run SHALL be generated and size-checked before the timed section starts, and generation SHALL NOT be included in produce time.

#### Scenario: Same messages for both brokers
- **WHEN** the 10 KB hold workload is generated for an ActiveMQ run and for an ActiveMQRust run
- **THEN** the two message sets are identical, text for text, in the same order

#### Scenario: Generation not timed
- **WHEN** a hold run prints its `PHASE` lines
- **THEN** the generation of all messages finishes before `produce-start`

### Requirement: Hold workload with separate produce and consume timing
The `bench` mode SHALL provide a `hold` workload that, on a new queue: produces N messages with no consumer attached; waits a hold window (default 10 s, option `--hold-seconds`); then creates one consumer and consumes all N messages. Produce time SHALL run from the first `send` until the broker has confirmed all N messages; with async send, the N-th message SHALL be sent synchronously through a second producer on the same session with a send timeout of 120 s, so that its response confirms all earlier messages. Consume time SHALL run from the creation of the consumer to the receipt of message N. For each phase the bench SHALL report elapsed time in ms, msgs/s and MB/s, where MB/s is N × target size / 1,048,576 per second. The bench SHALL print timestamped `PHASE` lines (`warmup-start`, `warmup-end`, `produce-start`, `produce-end`, `hold-end`, `consume-start`, `consume-end`) and one `RESULT` line.

#### Scenario: Hold run output
- **WHEN** `bench --scenario hold --messages 100000 --size 10240` runs against a broker
- **THEN** the output contains the `PHASE` lines in order and a `RESULT` line with produce ms, consume ms, produce msgs/s, consume msgs/s, produce MB/s and consume MB/s

#### Scenario: No consumer during produce
- **WHEN** the produce phase of a hold run is in progress
- **THEN** the queue has no consumer, so every message is held by the broker until the consume phase

### Requirement: Consumer verification
In the `hold` and `throughput` workloads, during the timed consume loop the bench SHALL check, for every received message, only that the text length equals the target size and that `seq` is exactly one more than the previous message's `seq` from the same producer (only increasing when several consumers share a queue); these O(1) checks SHALL be the only verification inside the timed section. The bench SHALL keep a sample of every 100th message plus the first and the last, and after the consume timer stops SHALL parse each sampled document with an XML parser, check its structure as defined for the payload, and compare its text with the document regenerated from the seed for that `seq`. Any failed check SHALL mark the run as failed with the reason.

#### Scenario: Corrupted body detected
- **WHEN** a sampled message's text differs from the expected document
- **THEN** the `RESULT` line is marked failed with the `seq` of that message

#### Scenario: Missing or reordered message detected
- **WHEN** a message is missing or arrives out of order
- **THEN** the run is marked failed with the expected and received `seq`

#### Scenario: Verification outside timing
- **WHEN** a hold run completes successfully
- **THEN** the sample verification runs after `consume-end`, and the reported consume time does not include it

### Requirement: Measurements to compare
For each broker setup the comparison SHALL measure:
- (a) memory of the idle broker after startup;
- (b) memory while holding 100,000 messages of 10 KB (all produced, none consumed);
- (c) produce time and consume time of those 100,000 messages of 10 KB, with async send;
- (d) throughput with 1 KB messages, one producer and one consumer running at the same time, with async send (1,000,000 messages) and with sync send (`jms.alwaysSyncSend=true`, 100,000 messages), reporting produce time, consume time, msgs/s and MB/s;
- (e) memory while holding, produce time and consume time of 10,000 messages of 50 KB, with async send;
- (f) throughput with 3,600 messages of 12 KB, one producer and one consumer running at the same time, with async send and with sync send.

#### Scenario: Complete set per broker
- **WHEN** the comparison finishes for ActiveMQ 5.18.x tuned and for ActiveMQRust with the `amq5` client
- **THEN** the report has values for (a), (b), (c), (d) async, (d) sync, (e), (f) async and (f) sync for both

### Requirement: Medium-size throughput measurement (f)
The comparison SHALL include measurement (f): the `throughput` workload with 3,600 XML documents of 12,288 bytes (12 KB), one producer and one consumer on one queue, run once with async send and once with sync send. It SHALL report produce time, consume time, msgs/s and MB/s with every individual value plus mean, median, min and max and the ActiveMQRust / ActiveMQ ratio, like (d). The 12 KB documents are below the broker compression threshold. (f) is informational: it is not part of the success criteria or the §14.3 verdicts.

#### Scenario: Both send modes
- **WHEN** the comparison runs with the default measurement list
- **THEN** the CSV has rows for `f-async` and `f-sync` with message size 12288 and message count 3600 for every broker setup, and the report has a table for each

### Requirement: Start-up time
For every run the script SHALL measure the broker's start-up time: from just before it starts the broker process to the moment the OpenWire port appears among the machine's listening TCP endpoints, polled with `IPGlobalProperties.GetActiveTcpListeners()` every 5 ms. The CSV SHALL carry the value of every run (`startup_ms`) and the report SHALL give, per broker setup, every value plus mean, median, min and max.

#### Scenario: Start-up measured
- **WHEN** a run starts `mqrust.exe` or ActiveMQ's `java.exe`
- **THEN** the run's `startup_ms` is the time until the OpenWire port is listening, and the report has a start-up time table with one row per broker setup

#### Scenario: Broker that never listens
- **WHEN** the broker process exits or does not listen within 120 s
- **THEN** the run is reported as failed with the reason and the script's exit code is non-zero

### Requirement: Broker compression in the 50 KB scenario
Because 50 KB documents are above ActiveMQRust's default broker compression threshold (32 KB), scenario (e) SHALL run ActiveMQRust twice: with default settings ("broker compression active") and with `compress_threshold_kb = 0` ("broker compression off"). The report and the CSV SHALL label every run with its broker compression state (`active` or `off`; always `off` for ActiveMQ). For ActiveMQRust the label SHALL come from the broker's own compressed-message count, read once through the admin JSON API shortly before the end of the hold window (a broker-wide counter in `/api/overview` when present, otherwise the `compressed` flag of the first page of up to 50 messages of the held queue), and not from the message size or the configuration; `active` means the count is above 0. A run without held messages and without a broker-wide counter SHALL be labelled `unknown`. The like-for-like comparison with ActiveMQ SHALL use the "broker compression off" run, and the "broker compression active" run SHALL be shown separately as the product with default settings. The report SHALL state the achieved compression ratio, measured as ActiveMQRust's message memory in use at the end of the hold window with compression active divided by the same value with compression off (median of the measured runs), and SHALL also give the ratio of `Deflater` level 1 on the sampled documents as a cross-check. If broker compression or the admin console is not available, the "active" run SHALL be skipped and the ratio reported as "not available". The report SHALL state that scenarios (b), (c) and (d) are below the threshold, so broker compression is not active in them.

#### Scenario: Two labelled ActiveMQRust runs
- **WHEN** scenario (e) is run for ActiveMQRust
- **THEN** the report contains one row labelled `broker compression: active` and one labelled `broker compression: off`, and the ActiveMQ row is labelled `off`

#### Scenario: Compression ratio stated
- **WHEN** both ActiveMQRust runs of scenario (e) are complete
- **THEN** the report shows the measured memory ratio and the `Deflater` cross-check ratio next to the scenario (e) results

#### Scenario: Below-threshold scenarios
- **WHEN** scenarios (b), (c) and (d) run against ActiveMQRust with default settings
- **THEN** the ActiveMQRust compressed-message count reported by the admin console stays 0, and the report states that broker compression was not active

#### Scenario: Label from the broker, not from the size
- **WHEN** the "broker compression active" run of scenario (e) holds 50 KB messages but the admin API reports none of them as compressed
- **THEN** the run is labelled `off`, and the compression ratio is reported as "not available"

### Requirement: Memory sampling on Windows
The comparison script SHALL sample the broker process every 250 ms with PowerShell `Get-Process`, recording Working Set (`WorkingSet64`) and Private Bytes (`PrivateMemorySize64`) with timestamps; for ActiveMQ the sampled process SHALL be the `java.exe` running the broker. For every run it SHALL report the peak of each metric over the run and steady values, each the median of the samples in a 5 s window: idle steady from the last 5 s of the 10 s after the broker accepts connections, and hold steady from the last 5 s of the hold window. No garbage collection SHALL be forced.

#### Scenario: Idle memory
- **WHEN** a broker has started and accepted its first TCP connection check
- **THEN** the script waits 10 s and reports the median Working Set and Private Bytes of the last 5 s as idle steady values

#### Scenario: Hold memory
- **WHEN** a hold run is in its hold window
- **THEN** the script reports the median Working Set and Private Bytes of the last 5 s of the window as hold steady values, and the run's peaks

### Requirement: Fair ActiveMQ configuration
The primary ActiveMQ runs SHALL use ActiveMQ 5.18.x and 6.x with `scripts\activemq-bench\activemq-tuned.xml`: `persistent="false"`; a `vmQueueCursor` pending queue policy so no message is spooled to temporary storage; `producerFlowControl="false"` for queues and topics; a broker-wide memory limit of 3 GB (store and temp limits also 3 GB, unused); JMX, advisory support and the scheduler disabled; only an OpenWire transport connector on `127.0.0.1:61616`, with the connector options of the distribution's `activemq.xml`; and a `simpleAuthenticationPlugin` with the benchmark user `admin` / `admin`. The file SHALL be a template for the port: for each run the script SHALL write a copy with the port replaced by the one it was given. The JVM SHALL run with `-Xmx4g`, no `-Xms`, the default garbage collector and the same JDK as the client. The report SHALL include the exact XML and the full JVM command line.

#### Scenario: No spooling and no flow control
- **WHEN** a tuned ActiveMQ run of scenario (b) completes
- **THEN** the temporary storage directory under the run's data directory is empty or absent and the broker log contains no memory-limit or flow-control message

#### Scenario: Invalid tuned run
- **WHEN** a tuned ActiveMQ run shows spooling or a memory-limit message
- **THEN** the run is marked invalid and repeated, at most twice, and if still invalid reported as invalid and excluded from the medians

### Requirement: ActiveMQ default configuration as reference
Each ActiveMQ version SHALL also be run with its own `conf\activemq.xml` and the default JVM memory options of `activemq.bat`, from a copy of the distribution's `conf` directory in which only the listening ports are changed (OpenWire to the script's port, the web console to its admin port, any other transport to the ports that follow the OpenWire port). These results SHALL be reported in a separate table marked as reference only and SHALL NOT be used for the success criteria. Spooling, flow-control blocking or timeouts in these runs SHALL be reported as observed.

#### Scenario: Reference table
- **WHEN** the comparison runs with default options
- **THEN** the report has a table "ActiveMQ default configuration (reference only)" with the same measurements as the tuned runs

### Requirement: ActiveMQRust configuration
ActiveMQRust SHALL run from the release `mqrust.exe` with `--port` and `--admin-port` set to the script's ports and `scripts\activemq-bench\mqrust-bench.toml`, which sets only the benchmark user `admin` / `admin`, the same admin console credentials and `bind = "127.0.0.1"`, and leaves every other key at its default, except the "broker compression off" run of scenario (e), which SHALL use `scripts\activemq-bench\mqrust-bench-nocompress.toml`, identical except for `compress_threshold_kb = 0`. The report SHALL include both files.

#### Scenario: Default settings
- **WHEN** ActiveMQRust is started for scenario (b)
- **THEN** it runs with `mqrust-bench.toml`, whose only settings are the benchmark user, the admin console credentials and the listen address

### Requirement: Run procedure
For each broker setup and each measurement, the script SHALL perform one warm-up run, discarded, followed by 3 measured runs, and SHALL report the median of the measured runs together with every individual value. The broker SHALL be started fresh, with a new data directory, before every run, including the warm-up run, and stopped after it. Inside every run, after the idle memory sample, the bench SHALL exchange 20,000 warm-up messages of the run's size on a separate queue, cycling 1,000 distinct documents, excluded from all timings. Every run SHALL have an overall timeout (default 30 minutes), after which it is reported as timed out. During every run the script SHALL measure the CPU used by processes other than the broker, the bench client and the script itself, as a share of the whole machine; when it is above 20% the run SHALL be marked invalid and repeated, at most twice, and a run still invalid SHALL be reported and excluded from the medians.

#### Scenario: Busy machine during a run
- **WHEN** another process uses 40% of the machine's CPU while a run is in progress
- **THEN** the run is marked `invalid-load` and repeated, and after two more invalid attempts it is listed under invalid runs and excluded from the medians

#### Scenario: Fresh broker per run
- **WHEN** the script performs the 3 measured runs of scenario (b) against ActiveMQRust
- **THEN** it starts and stops `mqrust.exe` 4 times in total (warm-up plus 3), and each run has its own process ID

#### Scenario: Median reported
- **WHEN** the three measured produce times are 9.1 s, 8.7 s and 9.4 s
- **THEN** the report shows 9.1 s as the median and lists all three values

### Requirement: Comparison script
`scripts\compare-activemq.ps1` SHALL run the whole comparison unattended. It SHALL accept the paths of `mqrust.exe`, the ActiveMQ 5.18.x and 6.x installations and the JDK, the OpenWire and admin ports (`-Port`, default 61616, and `-AdminPort`, default 8161, used for every broker), the number of measured runs (default 3), the output directory (`-OutDir`, default `docs\benchmarks`), switches to skip the default-configuration runs or a broker, and a `-Quick` switch for smoke tests (1 measured run, no discarded warm-up run, smaller message counts, shorter idle and hold windows; the report says so).

Before building or starting anything it SHALL run pre-flight checks and stop with a clear message naming the problem if one fails: the paths of `mqrust.exe`, of each ActiveMQ installation and of Java exist; both ports are free; at least 8 GB of physical memory is free; and the machine's CPU load, measured over 5 s, is at most 20%. With `-Force` the memory and CPU checks only print a warning.

For each run it SHALL: check that the ports are free and that at least 8 GB of physical memory is free; start the broker and record its PID; wait until it listens; sample memory; run the bench; collect the `PHASE` and `RESULT` lines; read ActiveMQRust's message memory and compressed-message count from the admin JSON API once, near the end of the hold window; stop the broker; check ActiveMQ's log and temporary storage for spooling; and delete the run's data directory. It SHALL write the CSV and the report to a temporary directory and move them to the output directory only when the whole comparison has finished. It SHALL exit with a non-zero code if a broker fails to start or the bench fails (code 2 when a pre-flight check fails), and with code 0 otherwise, whether or not the success criteria are met.

#### Scenario: Unattended run
- **WHEN** `scripts\compare-activemq.ps1` is run with valid paths
- **THEN** it completes all runs without user input and writes the report and the CSV

#### Scenario: Missing ActiveMQ
- **WHEN** the ActiveMQ 6.x path does not exist
- **THEN** the script stops with an error naming the path before starting any run

#### Scenario: Criteria not met
- **WHEN** all runs complete but a success criterion is not met
- **THEN** the script exits with code 0 and the report shows the criterion as not met

#### Scenario: Busy machine before start
- **WHEN** the script is started without `-Force` while the machine's CPU load is 60%
- **THEN** it stops before building or starting anything, names the measured load and the 20% limit, and exits with a non-zero code

#### Scenario: Interrupted comparison
- **WHEN** the comparison is interrupted before it finishes
- **THEN** the existing report and CSV in the output directory are unchanged, and the broker it had started is stopped

#### Scenario: Smoke test
- **WHEN** the script is run with `-Quick -OutDir <dir>`
- **THEN** it runs every selected measurement once with small message counts, writes the report and the CSV in `<dir>`, and the report states that it is a smoke test and not a valid comparison

### Requirement: Report and CSV
The script SHALL write `activemq-comparison-<yyyy-MM-dd>.md` and `activemq-comparison-<yyyy-MM-dd>.csv` in the output directory (default `docs\benchmarks`). Numbers SHALL use a dot as decimal separator whatever the Windows locale. The CSV SHALL contain one row per run, retries included (warm-up runs flagged), with at least: date, broker, broker version, configuration (`tuned`, `default` or `mqrust-default` / `mqrust-nocompress`), client profile, measurement, message size, message count, send mode, broker compression state, run number, attempt, warm-up flag, valid flag, status and reason, start-up ms, CPU share of other processes, produce ms, consume ms, produce msgs/s, consume msgs/s, produce MB/s, consume MB/s, idle and hold steady Working Set and Private Bytes, peak Working Set and Private Bytes, message memory and compressed-message count read from ActiveMQRust, and the `Deflater` ratio of the sample. The Markdown report SHALL contain: machine details (CPU model, physical and logical cores, RAM, Windows edition, version and build, power plan, JDK version, ActiveMQ versions, ActiveMQRust version); the message format; the client command line; the exact broker configurations and JVM command lines; one table per measurement listing, for each metric and broker setup, every measured value plus mean, median, min and max and the ActiveMQRust / ActiveMQ ratio of the medians (for (d) and (f): produce time, consume time, msgs/s and MB/s); the start-up times; the broker compression labels and the compression ratio; the per-message memory overhead above payload for (b) and (e); the success criteria verdicts; and notes on invalid, failed or timed-out runs.

#### Scenario: Machine details
- **WHEN** the report is written
- **THEN** it lists the CPU model, physical and logical cores, RAM, power plan, Windows edition, version and build, JDK version, both ActiveMQ versions and `ActiveMQRust <version>`

#### Scenario: Every value and the summary statistics
- **WHEN** three measured runs of (d) async give consume rates of 98,000, 101,000 and 99,500 msgs/s
- **THEN** the report lists the three values, their mean 99,500, median 99,500, min 98,000 and max 101,000, and the ratio to ActiveMQ's median

#### Scenario: CSV rows
- **WHEN** the default comparison completes
- **THEN** the CSV has one row for every run performed, each with all listed columns

### Requirement: Success criteria and honest reporting
Using the medians against the tuned ActiveMQ setup of each version, the report SHALL state as met or not met: ActiveMQRust uses less memory than ActiveMQ in (a) and in (b), on both steady Working Set and steady Private Bytes; and ActiveMQRust has shorter produce time and shorter consume time in (c). It SHALL state the same comparisons for (e), for the "broker compression off" run as the like-for-like verdict and for the "broker compression active" run as a separate labelled verdict. It SHALL state the §14.3 targets as met or not met: memory ≤ 1/5 of ActiveMQ, on steady Working Set in (a), (b) and (e); and throughput ≥ ActiveMQ, on msgs/s in (c), (d) async, (d) sync and (e). The report SHALL show the measured values next to each verdict and SHALL state every result as measured, including misses; verdicts SHALL be computed by the script and never edited by hand.

#### Scenario: Criterion met
- **WHEN** ActiveMQRust's steady idle Working Set and Private Bytes are both lower than ActiveMQ 6.x tuned
- **THEN** criterion (a) for 6.x is shown as met with both values

#### Scenario: Target missed
- **WHEN** ActiveMQRust's steady hold Working Set in (b) is more than one fifth of ActiveMQ's
- **THEN** the "memory ≤ 1/5" target for (b) is shown as not met with both values and the ratio, and the per-message overhead above payload is shown beside it

#### Scenario: Default configuration excluded from verdicts
- **WHEN** ActiveMQ's default configuration is slower than its tuned configuration
- **THEN** the verdicts are unchanged, because they use only the tuned runs
