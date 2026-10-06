## 0. Re-evaluation at the start of 0.6.0

- [ ] 0.1 Re-read this change against what 0.4.0 and 0.5.0 built (console deletions, resilience, TLS) and the open questions; update proposal, design and spec with the user before implementing

## 1. Configuration

- [ ] 1.1 `[storage]` keys, validation errors naming the keys, template entries
- [ ] 1.2 Warning when storage is off and `dir` holds a journal

## 2. Journal

- [ ] 2.1 Record codec (magic, length, type, CRC32C, payload) for Add, Remove, Move, TxCommit, DurableSubscribe, DurableUnsubscribe, DurableAck; fuzz the decoder
- [ ] 2.2 Segment files, active segment rollover at `segment_mb`, live-record counts per segment
- [ ] 2.3 Writer thread with group commit; `always`/`periodic`/`never`; `FlushFileBuffers` on Windows, `F_FULLFSYNC` on macOS
- [ ] 2.4 Folder lock file with an exclusive OS lock
- [ ] 2.5 Disk limit with 90 % recovery; OS write errors refuse the send and log an error

## 3. Broker integration

- [ ] 3.1 Synchronous persistent sends answered and dispatched only after the durable write; asynchronous ones written without waiting
- [ ] 3.2 Transactions: one TxCommit group at commit, nothing on rollback
- [ ] 3.3 Removal records for ack, expiry, console delete and purge, poison discard, DLQ move
- [ ] 3.4 Storage off: a single configuration branch on the message path, no other storage code

## 4. Replay and recovery

- [ ] 4.1 Replay before the listeners open: entries, sequences, FIFO order, complete groups only, expired messages dropped, memory-limited start
- [ ] 4.2 Torn-write truncation, corruption with `fail` and `skip`
- [ ] 4.3 Progress log every 5 seconds, replay time in the log and the console

## 5. Compaction

- [ ] 5.1 Delete segments without live records; background compaction below `compact_below_pct`, one segment at a time

## 6. Durable subscriptions

- [ ] 6.1 Record subscribe, replace, unsubscribe, offline timeout and console deletion of durable subscriptions
- [ ] 6.2 Single stored copy of persistent topic messages with the list of durable subscriptions; removal at the last `DurableAck`
- [ ] 6.3 Replay rebuilds offline subscriptions with their persistent messages in publish order

## 7. Admin console

- [ ] 7.1 Storage card on the overview and fields in `/api/overview`

## 8. Tests

- [ ] 8.1 Unit tests: codec, segments, group commit, replay, compaction, durable references
- [ ] 8.2 Crash tests: kill during writes, truncated segment, flipped bytes, restart loops with checks of every answered send
- [ ] 8.3 Java integration tests with storage on (clients 5.19.11 and 6.3.2): restart survival, transactions, durable subscribers
- [ ] 8.4 Jakarta TCK with storage on, including the durable subscription tests
- [ ] 8.5 Ask the user before running benchmarks with storage off and on

## 9. Documentation

- [ ] 9.1 README storage section: configuration, sync policies and their loss windows, disk limit, recovery options, differences from ActiveMQ, results after the approved runs
- [ ] 9.2 CHANGELOG entry
- [ ] 9.3 `cargo fmt` and the full test suite
