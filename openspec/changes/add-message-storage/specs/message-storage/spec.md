## ADDED Requirements

### Requirement: Storage configuration
The `[storage]` section SHALL accept `enabled` (boolean, default `false`), `dir` (default `data`, relative to the folder of the executable or absolute), `sync` (`"always"` default, `"periodic"` or `"never"`), `sync_interval_ms` (1–60000, default 1000), `segment_mb` (8–1024, default 64), `compact_below_pct` (10–90, default 50), `max_disk_mb` (0 = no limit, default 0) and `on_corruption` (`"fail"` default or `"skip"`). Invalid values SHALL be configuration errors naming the key, exit code 2. With `enabled = false` the broker SHALL NOT create, read or change any storage file; if `dir` contains a journal it SHALL log a warning that the stored messages are ignored.

#### Scenario: Off by default
- **WHEN** the broker runs without a `[storage]` section and receives persistent messages
- **THEN** no file is created under `data`, and behaviour and performance are those of the version without storage

#### Scenario: Invalid policy
- **WHEN** `storage.sync = "sometimes"`
- **THEN** startup fails with exit code 2 naming `storage.sync`

### Requirement: What is stored
With storage on, every PERSISTENT message stored in a queue SHALL be written to the journal with the bytes received from the producer, its destination and its broker sequence. NON_PERSISTENT messages SHALL never be written. Messages sent in a local transaction SHALL be written as one atomic group when the transaction commits, and nothing SHALL be written for a rolled-back transaction. A removal SHALL be recorded when a stored message is acknowledged (including a committed transacted ack), expires, is deleted or purged from the admin console, is discarded as poison, or is moved to `ActiveMQ.DLQ` (recorded as one move).

#### Scenario: Non-persistent not stored
- **WHEN** storage is on and 1000 non-persistent messages are sent and the broker is restarted before they are consumed
- **THEN** none of them is delivered after the restart and the journal did not grow by their size

#### Scenario: Rolled-back transaction
- **WHEN** a transacted session sends 5 persistent messages and rolls back, and the broker is restarted
- **THEN** none of the 5 messages exists after the restart

### Requirement: Durability of synchronous sends
With storage on, a synchronous send of a persistent message SHALL be answered only after its record is written and, with `sync = "always"`, flushed to the device; the message SHALL become available to consumers only after that point. A transaction commit containing persistent messages SHALL be answered only after its group is durable under the same rule. With `sync = "periodic"` the answer SHALL come after the write, and the journal SHALL be flushed at least every `sync_interval_ms`; with `"never"` flushing SHALL be left to the operating system. Flushing SHALL use `FlushFileBuffers` on Windows and `F_FULLFSYNC` on macOS. Concurrent sends SHALL share flushes (group commit). Asynchronous persistent sends SHALL be written without being answered, as today.

#### Scenario: Power loss after the answer
- **WHEN** storage is on with `sync = "always"`, a synchronous persistent send is answered, and the broker process is killed immediately afterwards
- **THEN** the message is delivered after the restart

#### Scenario: Group commit
- **WHEN** 50 producers send persistent messages synchronously at the same time
- **THEN** the number of flushes is lower than the number of messages, and every send is answered only after the flush that covers it

### Requirement: Replay at startup
With storage on, before opening any listener the broker SHALL replay the journal: messages added and not removed SHALL return to their destinations with their original `JMSMessageID`, headers, properties and body, in their original FIFO order, and new messages SHALL be ordered after them. A transaction group SHALL be applied only if it is complete. Messages already expired SHALL be dropped and their removal recorded. Replayed messages SHALL be delivered with `JMSRedelivered=false` unless they are redelivered after the restart. Replayed messages SHALL count in the memory limit; when they exceed it, the broker SHALL start in the memory-limited state. Replay progress SHALL be logged at least every 5 seconds, and the replay time SHALL be logged and shown in the admin console.

#### Scenario: FIFO after restart
- **WHEN** 10 persistent messages M1…M10 are in a queue, M1…M3 are consumed and acknowledged, and the broker is restarted
- **THEN** a consumer receives M4…M10 in that order, followed by any message sent after the restart

#### Scenario: Unacknowledged message
- **WHEN** a consumer received M4 without acknowledging it and the broker is restarted
- **THEN** M4 is delivered again after the restart

### Requirement: Torn writes and corruption
At replay, a record cut off at the end of the last segment, or failing its CRC as the last record of the last segment, SHALL be discarded with a warning and the segment truncated at the last valid record. A record failing its checks anywhere else SHALL stop startup with exit code 1 and a message naming the segment file and offset when `on_corruption = "fail"`; with `on_corruption = "skip"` the broker SHALL skip to the next valid record, log every skipped range with its size, and start.

#### Scenario: Kill during a write
- **WHEN** the broker is killed while appending a record
- **THEN** the next start discards the incomplete record with a warning and every message whose send was answered is present

#### Scenario: Flipped byte
- **WHEN** a byte in the middle of an older segment is changed and `on_corruption = "fail"`
- **THEN** startup fails with exit code 1 naming the segment and the offset

### Requirement: Segments, deletion and compaction
The journal SHALL be split into segment files of about `segment_mb` each. A segment other than the active one SHALL be deleted when it holds no live record. A segment whose live records are fewer than `compact_below_pct` percent of its records SHALL be compacted in the background, one segment at a time: its live records SHALL be appended again with the same content and sequence, flushed, and the old segment deleted. Compaction SHALL NOT change the order or the content of any message.

#### Scenario: Drained queue frees disk
- **WHEN** 1 million persistent messages are sent and all are consumed
- **THEN** within one minute the journal holds only the active segment

#### Scenario: One old message
- **WHEN** one persistent message stays unconsumed while 10 GB of later messages are sent and consumed
- **THEN** the journal does not keep 10 GB of segments, because compaction moves the old message forward

### Requirement: Disk limit and folder lock
With `max_disk_mb` greater than 0, when the journal reaches that size the broker SHALL refuse new persistent sends with `javax.jms.ResourceAllocationException` and SHALL accept them again when the journal is below 90 % of the limit; non-persistent messages SHALL be unaffected. A write error from the operating system SHALL refuse the send that caused it and be logged as an error, without stopping the broker. At startup the broker SHALL take an exclusive lock on a lock file in `storage.dir`; if another process holds it, startup SHALL fail with exit code 2 naming the folder.

#### Scenario: Disk limit
- **WHEN** `max_disk_mb = 100` and 100 MB of persistent messages are pending
- **THEN** the next synchronous persistent send fails with `ResourceAllocationException`, and a non-persistent send succeeds

#### Scenario: Two brokers, one folder
- **WHEN** a second broker starts with the same `storage.dir` while the first runs
- **THEN** the second exits with code 2 naming the folder

### Requirement: Durable subscriptions with storage
With storage on, creating, replacing and unsubscribing a durable subscription SHALL be recorded with its client ID, name, topic, selector and no-local flag, and a persistent message added to one or more durable subscriptions SHALL be stored once with the list of subscriptions that hold it, and removed when the last of them acknowledges it. At replay every stored durable subscription SHALL be rebuilt offline with its persistent messages in publish order; its non-persistent messages SHALL be lost. Removal of a subscription by unsubscribe, by the offline timeout or from the admin console SHALL be recorded. Non-durable subscriptions SHALL write nothing.

#### Scenario: Offline accumulation across a restart
- **WHEN** a durable subscriber `sub1` closes, 20 persistent messages are published, the broker is restarted, and `sub1` reattaches
- **THEN** it receives the 20 messages in publish order

#### Scenario: Unsubscribe recorded
- **WHEN** `sub1` is unsubscribed and the broker is restarted
- **THEN** `sub1` does not exist after the restart

#### Scenario: Stored once
- **WHEN** a 1 MB persistent message is published to a topic with 10 durable subscriptions
- **THEN** the journal grows by about 1 MB, not 10 MB

### Requirement: Storage data in the admin console
With storage on, the overview page and `/api/overview` SHALL show the storage folder, the journal size and its limit, the number of segments, the sync policy, the duration of the last flush and the replay time at startup.

#### Scenario: Storage card
- **WHEN** storage is on and holds 3 segments of 64 MB
- **THEN** the overview shows about 192 MB of journal, 3 segments and the sync policy

### Requirement: Storage off keeps today's performance
With storage off, the broker SHALL NOT execute any storage code on the message path beyond one branch on the configuration, and the benchmark results SHALL stay within the normal run-to-run variation of the version without storage. Results with storage on SHALL be published separately in the README, after the user approves the load runs.

#### Scenario: Benchmarks with storage off
- **WHEN** the throughput benchmarks are run, with the user's approval, with storage off before and after the change
- **THEN** the results stay within the normal run-to-run variation

### Requirement: Deliberate differences from ActiveMQ
These differences SHALL be intentional: storage is off by default; the journal format is not KahaDB; durable subscriptions survive restarts only with storage on; `JMSRedelivered` is not persisted across restarts (as ActiveMQ's default `persistJMSRedelivered=false`); every live message is also held in RAM, so backlogs cannot exceed the memory limit.

#### Scenario: Documented differences
- **WHEN** an operator reads the README storage section
- **THEN** it lists these differences and how to enable storage
