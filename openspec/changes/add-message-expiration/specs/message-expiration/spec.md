## ADDED Requirements

### Requirement: Expiration source
The broker SHALL take a message's expiration from the `expiration` field set by the producing client (`timestamp + timeToLive`, computed on the client clock by `MessageProducer.setTimeToLive()` or `send(..., timeToLive)`). An `expiration` of 0 SHALL mean the message never expires. The broker SHALL keep `expiration` unchanged unless one of the `[expiry]` options changes it, and the consumer SHALL see the effective value as `JMSExpiration`. A message is expired when `expiration > 0` and `expiration <= now`, where `now` is the broker's wall clock in milliseconds. A message that is already expired when it would enter a destination, after the `[expiry]` options have been applied, SHALL NOT be queued: it SHALL be deleted as expired, and a synchronous send SHALL still receive a normal `Response`. The same applies to a transacted message that has expired when its transaction commits: it is deleted and counted as expired instead of being queued. A message sent to a temporary destination that does not exist SHALL be handled by the temporary destination rules of `destination-management` (refused when synchronous, discarded when asynchronous) whether or not it has expired, and SHALL NOT be counted as expired by any destination.

#### Scenario: Already expired on arrival
- **WHEN** a producer sends a message whose `expiration` is already in the past on the broker clock
- **THEN** the producer's send succeeds, the message is never delivered, and the destination's `expired` counter is incremented

#### Scenario: Expired before commit
- **WHEN** a transacted producer sends a message with a 100 ms TTL and commits 200 ms later
- **THEN** the commit succeeds, the message is never delivered, and the destination's `expired` counter is incremented

#### Scenario: Expired message to a missing temporary queue
- **WHEN** a producer sends an already expired message synchronously to a temporary queue that has been deleted
- **THEN** the send fails with `javax.jms.InvalidDestinationException` and no destination's `expired` counter changes

#### Scenario: Expiration preserved
- **WHEN** a producer sends a message with `setTimeToLive(60000)` and default `[expiry]` settings, and a consumer receives it
- **THEN** the consumer's `JMSExpiration` equals the value computed by the producer

#### Scenario: No expiration
- **WHEN** a message is sent with time-to-live 0
- **THEN** its `expiration` is 0 and the broker never expires it

### Requirement: Expiry configuration keys
The configuration SHALL support an `[expiry]` section with these keys and defaults:
- `check_interval_ms` (default 1000): how often the sweeper runs;
- `use_broker_clock` (default false): recompute expiration on the broker clock;
- `ttl_ceiling_ms` (default 0 = no ceiling): maximum time-to-live;
- `default_ttl_ms` (default 0 = none): time-to-live applied to messages without an expiration.

All values SHALL be non-negative integers, and `check_interval_ms` SHALL be at least 1. An invalid value SHALL be reported with the field name and exit code 2, like every other configuration error. Without a configuration file, or with the section missing, the defaults SHALL apply.

#### Scenario: Defaults
- **WHEN** the broker starts without an `[expiry]` section
- **THEN** the sweeper runs every 1000 ms, the broker clock is not used, and no TTL ceiling or default TTL is applied

#### Scenario: Invalid interval
- **WHEN** the configuration sets `[expiry] check_interval_ms = 0`
- **THEN** the broker reports `expiry.check_interval_ms` and exits with code 2

### Requirement: Broker clock option
When `use_broker_clock = true`, for every message arriving from a producer with `expiration > 0`, the broker SHALL set `expiration = now_broker + (expiration - timestamp)` and `timestamp = now_broker`, preserving the time-to-live and correcting the skew between the client and broker clocks. This option is equivalent to ActiveMQ's `TimeStampingBrokerPlugin`. A message with `timestamp = 0` (the producer disabled timestamps) and `expiration > 0` SHALL be left unchanged, because its time-to-live cannot be derived; `TimeStampingBrokerPlugin` likewise only rebases messages that carry a timestamp.

#### Scenario: Client clock one hour ahead
- **WHEN** `use_broker_clock = true` and a client whose clock is one hour ahead sends a message with a 10 s TTL
- **THEN** the message is delivered normally, and it expires about 10 s after arrival on the broker clock, not one hour later

#### Scenario: Client clock one hour behind
- **WHEN** `use_broker_clock = true` and a client whose clock is one hour behind sends a message with a 10 s TTL
- **THEN** the message is not treated as expired on arrival, and it expires about 10 s after arrival on the broker clock

#### Scenario: Message without timestamp
- **WHEN** `use_broker_clock = true` and a message arrives with `timestamp = 0` and an expiration
- **THEN** its `timestamp` and `expiration` are not changed

#### Scenario: Option disabled
- **WHEN** `use_broker_clock = false` and a client whose clock is one hour behind sends a message with a 10 s TTL
- **THEN** the broker treats the message as expired on arrival, using the client-computed `expiration`

### Requirement: Default TTL option
When `default_ttl_ms > 0`, every message arriving from a producer with `expiration = 0` SHALL get `expiration = base + default_ttl_ms`, where `base` is the message `timestamp`, or the broker arrival time when the timestamp is 0 or `use_broker_clock` is true (in which case `timestamp` is also set to the broker arrival time).

#### Scenario: Default TTL applied
- **WHEN** `default_ttl_ms = 500` and a message without expiration is sent to a queue with no consumers
- **THEN** the message gets an expiration 500 ms after its timestamp and is deleted after it expires

#### Scenario: Explicit TTL kept
- **WHEN** `default_ttl_ms = 500` and a message is sent with a 60 s TTL
- **THEN** its expiration is unchanged

### Requirement: TTL ceiling option
When `ttl_ceiling_ms > 0`, for every message arriving from a producer whose expiration is greater than 0 after the broker clock and default TTL options have been applied, the broker SHALL cap the expiration to `base + ttl_ceiling_ms`, where `base` is the message `timestamp` (or the broker arrival time when the timestamp is 0). The ceiling SHALL apply to messages without an expiration only through `default_ttl_ms`; when `default_ttl_ms` is 0, a message with `expiration = 0` SHALL stay without expiration.

#### Scenario: Long TTL capped
- **WHEN** `ttl_ceiling_ms = 1000` and a message is sent with a 60 s TTL
- **THEN** the consumer sees `JMSExpiration` equal to `JMSTimestamp + 1000`, and the message is deleted if not consumed within about 1 s

#### Scenario: Default TTL capped
- **WHEN** `ttl_ceiling_ms = 1000` and `default_ttl_ms = 5000`, and a message without expiration is sent
- **THEN** its expiration is `timestamp + 1000`

#### Scenario: No expiration without default
- **WHEN** `ttl_ceiling_ms = 1000`, `default_ttl_ms = 0`, and a message without expiration is sent
- **THEN** the message keeps `expiration = 0` and never expires

### Requirement: Modified expiration visible to consumers
When an `[expiry]` option changes `expiration` (or `timestamp`), the new values SHALL be the ones stored in the message and delivered to consumers as `JMSExpiration` (and `JMSTimestamp`). The options SHALL be applied once, when the message arrives from a producer; a message moved to `ActiveMQ.DLQ` by a POISON ack SHALL keep its stored expiration unchanged.

#### Scenario: Consumer sees the capped expiration
- **WHEN** `ttl_ceiling_ms = 2000` and a message with a 60 s TTL is consumed immediately
- **THEN** the consumer's `JMSExpiration` is `JMSTimestamp + 2000`

### Requirement: Expiry index
Each queue and each topic subscription pending list SHALL keep an `expiry_index: BTreeSet<(expiration, broker_seq)>` containing only the pending messages with `expiration > 0`. Messages without expiration SHALL add no entry and no extra cost. Every removal from `pending` (dispatch, deletion, reinsertion) SHALL keep the index consistent, so removing an expired message from the middle of `pending` costs O(log n).

#### Scenario: Index holds only expiring messages
- **WHEN** a queue receives 10 messages without expiration and 5 with a TTL
- **THEN** its expiry index holds exactly 5 entries

#### Scenario: Index consistent after dispatch
- **WHEN** a message with a TTL is dispatched to a consumer
- **THEN** its entry is removed from the expiry index of the queue

### Requirement: Active removal by the sweeper
The broker SHALL run a single periodic sweeper task every `check_interval_ms`. On each round it SHALL visit only the destinations (queues and topic subscription pending lists) whose expiry index is not empty, take from the head of the index every message with `expiration <= now` and delete it from `pending`. To avoid holding a destination's lock for long, the sweeper SHALL process at most 10,000 messages per round and per destination, then release the lock and move to the next destination; the remaining expired messages SHALL be handled in the next round. When no destination has messages with an expiration, the round SHALL do no work.

#### Scenario: Expired message removed without consumers
- **WHEN** a message with a 100 ms TTL is sent to a queue with no consumers
- **THEN** after 100 ms plus `check_interval_ms` it is no longer in the queue and the accounted memory has gone down by its size

#### Scenario: Bounded work per round
- **WHEN** 25,000 messages in one queue expire at the same time
- **THEN** the sweeper deletes at most 10,000 of them per round, releasing the queue lock between rounds, and all 25,000 are deleted within three rounds

#### Scenario: Slow topic subscriber
- **WHEN** a slow topic subscriber's pending list holds a message whose TTL has passed
- **THEN** the sweeper removes it from that list without affecting other subscribers' copies that were already delivered

### Requirement: Expiration checks at delivery time
Besides the sweeper, the broker SHALL check expiration at every point where a message is about to be delivered, so an expired message is never delivered even between two sweeper rounds:
- dispatch to a consumer (queues and topics): an expired message SHALL NOT be delivered; it SHALL be treated as expired and dispatch SHALL move to the next message;
- `MessagePull` (prefetch 0): as for dispatch; if every message in the queue has expired and the pull has a timeout, the normal pull timeout SHALL apply;
- `QueueBrowser`: expired messages SHALL be skipped and treated as expired;
- reinsertion into `pending` after transaction rollback, consumer close or connection drop: an expired message SHALL NOT go back into the queue and SHALL be treated as expired.

#### Scenario: Alternating expired and valid messages
- **WHEN** a queue holds alternating expired and valid messages and a consumer is attached before the sweeper runs
- **THEN** the consumer receives only the valid messages, in FIFO order

#### Scenario: Pull with everything expired
- **WHEN** a consumer with prefetch 0 calls `receive(2000)` on a queue whose messages have all expired
- **THEN** no message is delivered, the expired messages are deleted, and `receive` returns null after the timeout

#### Scenario: Browser skips expired messages
- **WHEN** a `QueueBrowser` enumerates a queue holding an expired message that the sweeper has not yet removed
- **THEN** the expired message is not returned and is deleted

#### Scenario: Expiry during rollback
- **WHEN** a transacted consumer receives a message with a short TTL, the TTL passes, and the session rolls back
- **THEN** the message is not redelivered and is deleted

#### Scenario: Expiry before consumer close
- **WHEN** a consumer closes with an unacknowledged message whose TTL has passed
- **THEN** the message does not return to `pending` and is deleted

### Requirement: In-flight messages and EXPIRED acks
A message already delivered and held in a consumer's `inflight` SHALL NOT be taken away from the consumer when it expires. When the client receives an expired message, discards it and sends a `MessageAck` with ack type `EXPIRED` (6), the broker SHALL remove the message from `inflight`, delete it as expired, free the prefetch slot and resume dispatch.

#### Scenario: EXPIRED ack
- **WHEN** a client sends an `EXPIRED` ack for an in-flight message
- **THEN** the message leaves `inflight`, is deleted, the `expired` counter is incremented, and the consumer receives the next message

#### Scenario: In-flight message not revoked
- **WHEN** a message in a consumer's `inflight` passes its expiration and the sweeper runs
- **THEN** the message stays in `inflight` until the client acknowledges it

### Requirement: FIFO preserved under expiration
Removing an expired message SHALL NOT change the relative order of the other messages: the remaining messages SHALL be delivered in `broker_seq` order.

#### Scenario: Order after removal
- **WHEN** messages 1 to 5 are queued and message 3 expires and is removed
- **THEN** a consumer receives messages 1, 2, 4 and 5 in that order

### Requirement: Expired messages are deleted, never moved to a DLQ
An expired message SHALL be permanently deleted: it SHALL NOT be moved to `ActiveMQ.DLQ` or any other destination, and there SHALL be no option to change this. Deletion SHALL remove the message from `pending`, from the expiry index and, for an `EXPIRED` ack, from `inflight`, and SHALL free its accounted memory immediately. The rule SHALL apply to persistent and non-persistent messages and to every destination, including `ActiveMQ.DLQ`: a message that reached the DLQ through a POISON ack keeps its own `expiration` and, if it has one, SHALL be deleted on expiry like any other message. This is a deliberate difference from ActiveMQ, which by default moves expired persistent messages to `ActiveMQ.DLQ` and does not expire messages held in the DLQ.

#### Scenario: Persistent and non-persistent expired messages
- **WHEN** a persistent and a non-persistent message with a 100 ms TTL expire in a queue
- **THEN** both are deleted and `ActiveMQ.DLQ` stays empty

#### Scenario: Message in the DLQ expires
- **WHEN** a message with a 2 s TTL is moved to `ActiveMQ.DLQ` by a POISON ack and its TTL passes
- **THEN** it is deleted from `ActiveMQ.DLQ` and the DLQ's `expired` counter is incremented

#### Scenario: Memory released
- **WHEN** an expired message is deleted
- **THEN** the broker's accounted message memory decreases by the size of that message immediately

### Requirement: Expired counter
Each destination SHALL have an `expired` counter, incremented once for every message deleted on expiry (by the sweeper, by a delivery-time check, by a reinsertion check or by an `EXPIRED` ack). The counter SHALL be shown in the admin queues table.

#### Scenario: Counter value
- **WHEN** 3 messages expire in a queue, one removed by the sweeper, one skipped at dispatch and one acknowledged with `EXPIRED`
- **THEN** the queue's `expired` counter is 3

### Requirement: Expiration data in the admin
The admin console SHALL show:
- in the queue detail, the number of messages with an expiration and the next expiration time;
- in the message detail, `Expiration` as a readable local date and time, plus the remaining time or "expired";
- in the queue contents, expired messages that the sweeper has not removed yet, marked as "expired".

The same data SHALL be available in the JSON API.

#### Scenario: Queue detail
- **WHEN** a queue holds 2 messages with expirations at T1 < T2 and 3 without, and `/api/queues/{name}` is requested
- **THEN** the response reports 2 messages with an expiration and next expiration T1

#### Scenario: Expired marking
- **WHEN** the queue contents are requested while an expired message has not yet been removed by the sweeper
- **THEN** that message is listed and marked "expired"

### Requirement: Expiration logging
The broker SHALL log each expired message at debug level. At info level it SHALL write at most one line per minute per destination with a summary, for example `queue X: 1,234 messages expired in the last minute`, and nothing when no message expired in that minute.

#### Scenario: Rate-limited summary
- **WHEN** 1,234 messages expire in queue `X` within one minute at log level `info`
- **THEN** a single summary line for queue `X` is logged for that minute, and no per-message lines

### Requirement: Sweeper does not affect dispatch latency
The sweeper SHALL hold each destination lock only for bounded work (at most 10,000 deletions per round) so that it does not measurably increase the p99 dispatch latency.

#### Scenario: Benchmark with random TTLs
- **WHEN** a benchmark enqueues 1,000,000 messages with random TTLs while consumers receive messages
- **THEN** the p99 dispatch latency with the sweeper running is not measurably higher than without expiring messages
