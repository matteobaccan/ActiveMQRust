## Context

Every message is held in RAM: `Entry` (shared `Message`, `MemTicket`, cached properties) in per-destination maps keyed by broker sequence, with FIFO order, inflight tracking per consumer, transactions applied at commit (`deliver_checked(…, check_memory = false)`), expiry index, DLQ moves on POISON acks, and (from 0.4.0) console deletion and purge. The `persistent` flag is carried but ignored. Durable subscriptions exist in RAM (`add-durable-subscriptions`, 0.4.0), keyed by client ID and subscription name, and are lost on restart. A synchronous send is answered as soon as the message is stored in the destination. The product goals are fewer resources and higher speed than ActiveMQ, Windows first and macOS; a single executable with no external services.

## Goals / Non-Goals

**Goals:**
- With storage off: zero change in behaviour, files and performance.
- With storage on: a persistent message whose synchronous send was answered survives a broker crash or power loss (with `sync = "always"`), and comes back in FIFO order with the same ID and headers.
- Durable subscriptions and their persistent messages survive restarts when storage is on.
- Bounded startup time and disk usage; clear recovery rules after a crash.

**Non-Goals:**
- Messages larger in total than RAM (paging bodies out of memory): every live message stays in RAM, the journal is for durability only. Covered by the separate change `add-message-paging`.
- Storing non-persistent messages.
- Replication, high availability, shared storage between brokers, KahaDB compatibility.
- Persisting redelivery counters (`JMSRedelivered` after a restart), as ActiveMQ's default `persistJMSRedelivered=false`.
- Persisting XA transactions (XA stays refused).

## Decisions

### D1. Own append-only segmented journal
Records are appended to segment files `journal-<16-digit sequence>.log` in `storage.dir`. Each record: magic, length, type, CRC32C of the payload, payload. Types: `Add` (destination, broker sequence, the message as marshalled OpenWire bytes, exactly as received), `Remove` (destination, broker sequence; one or many), `Move` (DLQ: remove from one destination, add to another with the new sequence and the DLQ properties), `TxCommit` (a group of `Add`/`Remove` applied atomically: either all records of the group are replayed or none), `DurableSubscribe`/`DurableUnsubscribe`, `DurableAck` (subscription, sequence). The message bytes are those the broker received, so a replayed message is byte-identical to the original.
*Alternative 1*: RocksDB. Rejected: a C++ dependency that grows the executable and the build, with an LSM design tuned for random keys rather than FIFO queues.
*Alternative 2*: SQLite. Rejected: one row insert and one delete per message is far slower than appending for a FIFO workload.
*Alternative 3*: sled. Rejected: no longer actively maintained.
*Alternative 4*: KahaDB format. Rejected: complex, Java-specific, and compatibility is not a goal.

### D2. One writer thread and group commit
Connection tasks send records to a single journal writer through a channel. The writer takes everything queued, writes it in one call, flushes according to the policy, then completes every waiting send. Under load many sends share one flush; with one producer the latency is one flush. Policies:
- `always` (default): flush before answering a synchronous persistent send.
- `periodic`: answer after the write, flush every `sync_interval_ms` (default 1000); a crash can lose up to that interval.
- `never`: leave flushing to the OS; for tests and throwaway data.
Flush is `FlushFileBuffers` on Windows and `fcntl(F_FULLFSYNC)` on macOS (plain `fsync` on macOS does not flush the disk cache).
*Alternative*: write from each connection task under a lock. Rejected: no batching, and a slow disk blocks the async runtime threads.

### D3. When a send is answered
A synchronous persistent send to a queue is answered after its `Add` record is durable under the policy, and only then is the message made available to consumers, so a consumer never receives a message that a crash could make disappear. An asynchronous persistent send is not answered (as today) and is written without waiting. Messages sent in a local transaction are written as one `TxCommit` group when the transaction commits, and the commit is answered after it is durable; a rollback writes nothing.
*Alternative*: make the message available before the write completes. Rejected: a consumer could process a message that does not exist after a crash; ActiveMQ also stores before dispatching for persistent messages.

### D4. Removals
A removal record is written when a stored message leaves for good: acknowledged (standard or individual ack, also when a transacted ack commits), expired, deleted or purged from the console, discarded as poison from a non-DLQ destination, or moved to the DLQ (`Move`). Removal records are written asynchronously and do not delay the ack; after a crash before a removal was flushed, the message is delivered again, which is at-least-once and matches ActiveMQ.

### D5. Replay at startup
Before the listeners open, segments are read in order: `Add` creates the entry, `Remove` drops it, `TxCommit` applies its group only if the whole group is present and valid, durable records rebuild subscriptions. Messages expired at replay time are dropped and a removal is recorded. Destinations are created as needed; broker sequences are restored, so FIFO order is the original one and new messages get higher sequences. Replay progress is logged every 5 seconds and the total time is shown in the console. Replayed messages count in the memory limit; if they exceed it, the broker starts in the limited state rather than refusing to start.
*Alternative*: a separate index file to avoid full replay. Rejected for the first version: compaction (D7) keeps the journal close to the live data, so replay reads little more than what must be loaded into RAM anyway.

### D6. Corruption and torn writes
A record whose length runs past the end of the last segment, or whose CRC fails as the last record of the last segment, is a torn write: it is truncated with a warning (its send was never answered under `always`). A bad record anywhere else stops startup with exit code 1, naming the segment and offset. `storage.on_corruption = "skip"` instead skips to the next valid record (searching for the magic and a valid CRC), logs every skipped range, and starts.
*Alternative*: always skip. Rejected: silently losing acknowledged data must be an explicit choice.

### D7. Segment deletion and compaction
Each segment keeps a count of live `Add` records (and live durable references). A segment that is not the active one and has no live record is deleted. A background task compacts the oldest segment whose live ratio is below `compact_below_pct` (default 50) and older than one minute: its live records are appended again to the active segment (same sequences, same bytes), flushed, and the old segment is deleted. Compaction runs at low priority and at most one segment at a time.
*Alternative*: never compact. Rejected: one old unconsumed message would keep every segment after it alive forever.

### D8. Persistence of durable subscriptions
The in-RAM durable subscriptions of 0.4.0 keep their behaviour; storage adds records. Creating, replacing or unsubscribing writes `DurableSubscribe`/`DurableUnsubscribe` with client ID, name, topic, selector and no-local flag. A persistent message published to a topic that has at least one durable subscription is written once (`Add` on the topic) with the list of durable subscriptions it was added to; each subscription's ack writes `DurableAck`, and the record dies when the last reference is gone. Non-durable subscriptions write nothing. At replay, subscriptions are rebuilt offline with their persistent messages in publish order; non-persistent messages of durable subscriptions are lost on restart. The offline timeout of 0.4.0 also removes the stored subscription.
*Alternative*: one copy of the message per durable subscription. Rejected: a topic with 10 durable subscriptions would write 10 times the data.

### D9. Disk limit and folder lock
`max_disk_mb` (0 = no limit) caps the journal size: when reached, persistent sends are refused with `javax.jms.ResourceAllocationException` as at the memory limit, and accepted again below 90 %. At startup the broker creates `storage.dir/lock` with an exclusive OS lock; if another process holds it, startup fails with exit code 2 naming the folder.

### D10. Configuration
`[storage]`: `enabled` (false), `dir` ("data", relative to the executable folder), `sync` ("always"), `sync_interval_ms` (1000), `segment_mb` (64, 8–1024), `compact_below_pct` (50, 10–90), `max_disk_mb` (0), `on_corruption` ("fail"). All validated with errors naming the key, exit code 2, and listed in the template. Turning storage off with a non-empty `dir` logs a warning that the stored messages are ignored, and deletes nothing.

## Risks / Trade-offs

- [Persistent synchronous sends become as slow as a disk flush] → group commit amortises it under load; `periodic` trades a small loss window for speed; measured and published only after the user approves a load run.
- [Replay time grows with the backlog] → compaction keeps the journal close to the live data; progress is logged; the console shows the replay time.
- [Messages larger than RAM are still not possible] → non-goal for this version; the memory limit applies as today.
- [Disk full] → `max_disk_mb` refuses new persistent sends before the disk fills; a write error from the OS refuses the send and logs an error without stopping the broker.
- [Bugs in a new storage engine] → fuzzed record decoder, crash-injection tests (kill during writes, truncated files, flipped bytes), the TCK and the Java suites run with storage on.

## Migration Plan

Off by default: upgrading changes nothing. Enabling: set `[storage] enabled = true`; existing in-RAM messages at that moment are not stored (they are lost at the restart that enables storage). Disabling: set `enabled = false`; the journal is kept on disk and ignored. Rollback to a version without storage: the folder is ignored.

## Open Questions

- Paging message bodies out of RAM (backlogs larger than memory) is the separate change `add-message-paging`, planned for the last release of the roadmap.
- To be re-evaluated at the start of 0.6.0, together with the whole change.
