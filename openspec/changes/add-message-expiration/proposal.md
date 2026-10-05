## Why

ActiveMQRust keeps every message in RAM with no storage. A message whose time-to-live has passed but stays in a queue wastes memory, and memory use is one of the project's main goals (**less RAM** than ActiveMQ). Java applications set `MessageProducer.setTimeToLive()` routinely for transient data such as telemetry, status updates and request/reply, and expect the broker to honour `JMSExpiration` exactly as ActiveMQ does (**compatibility**). This change makes expiration work at every point where a message could be delivered and removes expired messages actively, at low and predictable cost (**faster**: the sweeper must not affect dispatch latency). By explicit product decision, an expired message is **deleted**, never moved to a DLQ, including messages already in `ActiveMQ.DLQ`; this deliberately differs from ActiveMQ.

## What Changes

- The broker keeps the client's `expiration` field intact (`0` = never expires) unless a broker-side option changes it; the consumer sees the effective value as `JMSExpiration`.
- Expiry index per queue and per topic subscription pending list (`BTreeSet<(expiration, broker_seq)>`) holding only messages with an expiration.
- A single periodic sweeper task (`check_interval_ms`, default 1000 ms) that deletes expired messages, at most 10,000 per round and destination, and does nothing when no destination has expiring messages.
- Expiration checks at dispatch, `MessagePull`, `QueueBrowser`, reinsertion into `pending` (rollback, consumer close, connection drop), and `EXPIRED` acks for messages that expired in `inflight`. Messages already delivered are never taken away from the consumer.
- **Expired messages are permanently deleted**: no DLQ, no option, on every destination including `ActiveMQ.DLQ`. **Deliberate difference from ActiveMQ**, which by default moves expired persistent messages to `ActiveMQ.DLQ`.
- New `[expiry]` configuration keys equivalent to ActiveMQ's `TimeStampingBrokerPlugin`: `check_interval_ms` (1000), `use_broker_clock` (false), `ttl_ceiling_ms` (0 = none), `default_ttl_ms` (0 = none).
- Per-destination `expired` counter, expiration data in the admin (queue detail, message detail, expired marking in contents) and a rate-limited info log summary.

## Capabilities

### New Capabilities

- `message-expiration`: expiration source and broker-side options, expiry index and sweeper, delivery-time checks, deletion semantics without DLQ, `[expiry]` configuration keys, statistics, admin data and logging.

### Modified Capabilities

None.

## Impact

- Code: `src/broker/destination.rs` (expiry index, delivery-time checks, deletion, `EXPIRED` ack, topic subscription pending lists), `src/broker/mod.rs` (sweeper task, set of destinations with expiring messages, counters, options on arrival), `src/broker/entry.rs` (accounted memory released when the last copy of a message is dropped), `src/connection.rs` (expiry on arrival and at commit), `src/config.rs` (`[expiry]` section), admin snapshot data.
- No new crates: the sweeper uses `tokio` timers already in the project.
- Depends on `add-queue-messaging` (queues, dispatch, acks, browser, redelivery, DLQ on POISON ack). The topic part depends on `add-topic-messaging`; the rollback check depends on `add-local-transactions`; the admin display is rendered by `add-admin-console`.
- The Java acceptance scenarios 1, 2 and 3 keep passing.
