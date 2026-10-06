## Why

ActiveMQRust keeps every message in RAM: a restart, a crash or a Windows update loses every message, persistent ones included, and durable subscriptions (in RAM since 0.4.0) are lost too. That is fine for transient traffic, but it keeps out applications that rely on `DeliveryMode.PERSISTENT` meaning "survives the broker". An optional storage, off by default, lets the same broker serve both: all-in-RAM speed when storage is off, and ActiveMQ-like durability when it is on.

## What Changes

- New `[storage]` section, **off by default**: with it off, nothing changes (no file is written, as today).
- With storage on, **PERSISTENT** messages sent to queues, or published to topics with durable subscriptions, are written to an append-only journal before a synchronous send is answered; NON_PERSISTENT messages are never written.
- Acknowledgements, expirations, DLQ moves, console deletions and purges are written as small removal records; the journal never rewrites a record in place.
- At startup the journal is replayed: persistent messages that were not consumed come back to their queues in their original FIFO order, with their original `JMSMessageID` and headers.
- Group commit: concurrent sends share one `fsync`; sync policy `always` (default), `periodic` or `never`.
- Segmented journal (default 64 MB per segment) with CRC per record; a segment with no live record is deleted, a segment mostly dead is compacted in the background.
- Crash safety: a torn record at the end of the journal is discarded at replay with a warning; a corrupted record elsewhere stops startup with the file and offset, unless the operator chooses to skip it.
- **Durable subscriptions** (in RAM since `add-durable-subscriptions`, 0.4.0) survive restarts when storage is on: their definitions and persistent messages are stored, each message once however many subscriptions reference it.
- Disk limit `max_disk_mb`: when reached, persistent sends are refused like at the memory limit.
- A lock file prevents two brokers from using the same storage folder.
- Admin console: storage size, segments, last fsync latency and replay time on the overview.
- **BREAKING (spec, only with storage on)**: "Delivery mode does not change storage" applies only when storage is off.

## Capabilities

### New Capabilities

- `message-storage`: `[storage]` configuration, journal format and records, write path and sync policies, replay and recovery, segment deletion and compaction, disk limit, folder lock, persistence of durable subscriptions, admin data, platform flush rules.

### Modified Capabilities

- `queue-delivery`: "Delivery mode does not change storage" holds only when storage is off.
- `topic-delivery`: new requirement "Durable subscriptions survive restarts with storage" (depends on `add-durable-subscriptions`).

## Impact

- Code: new `src/storage/` (journal writer thread, record codec, segments, replay, compaction, lock file), `src/broker/` (hooks on store, ack, expiry, DLQ move, purge, durable subscribe/unsubscribe/ack; replay into destinations and durable subscriptions), `src/connection.rs` (synchronous send answered after the durable write), `src/config.rs` (`[storage]`), admin pages and API, README, Java integration tests and the Jakarta TCK run.
- New crate: `crc32fast` (small, pure Rust); no database engine.
- Performance: storage off must keep today's numbers exactly; storage on is measured separately (persistent sync sends are bounded by disk flush latency). Load runs only with the user's approval.
- Planned for 0.6.0; to be re-evaluated at the start of 0.6.0 against what 0.4.0 and 0.5.0 have built.
