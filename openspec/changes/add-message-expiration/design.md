## Context

ActiveMQRust stores messages only in RAM. After `add-queue-messaging`, a queue holds `pending: BTreeMap<broker_seq, Arc<StoredMessage>>`, a per-consumer `inflight` map, acks of every type and redelivery at the original position; POISON acks move messages to `ActiveMQ.DLQ`. The `expiration` field is decoded from every message but not yet acted upon. Java clients compute `expiration = timestamp + ttl` on their own clock; the ActiveMQ client also discards expired messages it receives and sends an `EXPIRED` ack. ActiveMQ checks expiration lazily during dispatch and with a periodic `expireMessagesPeriod` scan, and by default moves expired persistent messages to `ActiveMQ.DLQ`. The product owner decided that ActiveMQRust deletes expired messages, never moving them to a DLQ, including messages already in `ActiveMQ.DLQ`.

## Goals / Non-Goals

**Goals:**
- An expired message is never delivered, wherever it sits (pending, pull, browser, reinsertion).
- Expired messages leave RAM promptly even with no consumers, and memory accounting drops at once.
- Messages without expiration cost nothing extra; the sweeper never stalls dispatch.
- Broker-side TTL controls equivalent to ActiveMQ's `TimeStampingBrokerPlugin`.

**Non-Goals:**
- Moving expired messages to a DLQ or any per-destination expiry policy (explicitly rejected).
- Revoking messages already in a consumer's `inflight`.
- Advisory messages for expired messages (the broker publishes no advisories).
- Admin actions on expired messages (the admin is read-only).

## Decisions

### D1. Delete on expiry, no DLQ, no option
Expired messages are dropped and counted. The DLQ is not special: messages moved there by a POISON ack keep their `expiration` and expire like any other.
- *Alternatives:* ActiveMQ's default (expired persistent messages to `ActiveMQ.DLQ`, DLQ copies with expiration cleared) keeps dead data in RAM, which is the opposite of what an in-memory broker needs; a configurable policy adds code and configuration for a behaviour the product owner rejected.

### D2. Ordered expiry index per pending list
Each queue and each topic subscription pending list keeps `BTreeSet<(expiration, broker_seq)>` only for messages with `expiration > 0`. The earliest expiry is the first element; removing a message from `pending` by `broker_seq` is O(log n).
- *Alternatives:* scanning `pending` periodically (O(n) per round, as ActiveMQ's `expireMessagesPeriod`); a binary heap (no O(log n) removal of arbitrary entries when messages are dispatched); a timer wheel (efficient but more code, and the BTreeSet already gives ordering and removal).

### D3. One sweeper task for the whole broker
A single `tokio` task wakes every `min(check_interval_ms, 1000)` ms (the same task checks idle destinations once per second) and sweeps every `check_interval_ms`. The broker keeps a set of destinations that may hold messages with an expiration: a destination enters the set when a message with `expiration > 0` is stored in it, and leaves it when a sweep finds no message with an expiration left in it (pending, subscription pending lists, inflight or reserved by a transaction), checked under the destination lock so that a concurrent store re-registers it. The task visits only the destinations in the set. For each, it locks the destination, deletes up to 10,000 expired entries, unlocks, and continues. When the set is empty a round locks nothing. Missed ticks are skipped rather than bursting.
- *Alternatives:* a timer per destination or per message (many tasks and wake-ups, more RAM); lazy expiry only at dispatch (expired messages stay in RAM forever on queues without consumers).

### D4. Delivery-time checks in a single helper
`is_expired(msg, now)` is called by queue dispatch, topic dispatch, pull handling, browser enumeration and reinsertion; `now` is read once per dispatch batch to avoid repeated clock calls. An expired candidate is deleted on the spot (same path as the sweeper) and the loop continues with the next message. In-flight messages are never touched; the `EXPIRED` ack is handled like an individual ack followed by deletion.
- *Alternatives:* relying only on the sweeper (an expired message can be delivered between two rounds); revoking in-flight messages (the client already owns them, and the ActiveMQ client discards them itself).

### D5. Broker-side options applied once on arrival
On receipt of a producer message, in this order: (1) `use_broker_clock` rebases `timestamp` and `expiration` when `expiration > 0`; (2) `default_ttl_ms` gives an expiration to messages with `expiration = 0`, measured from the timestamp, or from the broker arrival time when the timestamp is 0 or the broker clock is in use; (3) `ttl_ceiling_ms` caps `expiration - base`. The resulting header values are written into the stored message, so consumers see them. Messages moved to the DLQ by the broker are not re-stamped.
- *Alternatives:* computing an internal broker-only deadline while delivering the client's original `JMSExpiration` (consumers and broker would disagree about expiry); copying ActiveMQ's plugin literally (it always rebases on the broker clock and also has `futureOnly`, more options than needed).

### D6. Counting and logging
Each path first takes the message out of the structure that holds it (`pending` and the expiry index, a subscription's pending list, `inflight` for an `EXPIRED` ack, a reinsertion batch, a browser snapshot, or the arrival and commit checks before the message is stored), because the structures differ. Every path then hands the message to one function, `Dest::expire`, which increments the destination's `expired` counter and its "expired this minute" counter (an atomic, read without the destination lock), logs the message at debug level, and drops it; dropping the last copy releases the accounted memory. A once-per-minute pass reads and resets the minute counters of all destinations and logs the summary line for those with a non-zero value, so the summary covers every kind of expiry, not only the sweeper.
- *Alternatives:* per-message info logs (floods the console under load).

## Risks / Trade-offs

- [Sweeper lock time hurts dispatch latency] → Bounded batches of 10,000 per destination and round; a benchmark with 1,000,000 random-TTL messages checks p99 dispatch latency.
- [Client and broker clocks disagree, so messages expire too early or too late] → `use_broker_clock` rebases expiration on the broker clock; tests simulate a client 1 hour ahead and behind.
- [Deleting from the DLQ surprises users who expect ActiveMQ behaviour] → The difference is documented in the spec and README; messages without TTL in the DLQ are never deleted.
- [Expired message delivered between check and send] → The check runs under the destination lock just before a message moves to `inflight`; anything after that is handled by the client's `EXPIRED` ack.
- [Many messages expiring at once keep memory high for several rounds] → Delivery-time checks also delete them, and the next rounds continue; `check_interval_ms` can be lowered.

## Migration Plan

No data migration: the broker keeps no state across restarts. Applications using time-to-live keep working without changes. Operators who relied on ActiveMQ moving expired messages to the DLQ must accept that ActiveMQRust deletes them. Rollback means deploying the previous `mqrust.exe` or pointing clients back to ActiveMQ.

## Open Questions

- Verify in the ActiveMQ client sources (`ActiveMQMessageConsumer`) when the client sends the `EXPIRED` ack (ack type 6) and whether it carries a single message or a range, for 5.18.x and 6.x.
- Verify that ActiveMQ's `SharedDeadLetterStrategy` clears the expiration of messages moved to `ActiveMQ.DLQ` (its `expiration` attribute), to document the difference precisely.
- Verify `TimeStampingBrokerPlugin` behaviour for messages with timestamp 0 (`disableMessageTimestamp`) and align the `default_ttl_ms` base with it. *Resolved:* the plugin only rebases messages with a timestamp, so with `use_broker_clock` a message with `timestamp = 0` and an expiration is left unchanged; `default_ttl_ms` measures from the arrival time when the timestamp is 0 or the broker clock is in use, and in the latter case also sets `timestamp` to the arrival time.
- Transacted sends: the options are applied on arrival; decide whether a transacted message that expires before COMMIT is counted as expired at commit time. *Resolved:* yes, it is deleted and counted as expired at commit instead of being queued.
