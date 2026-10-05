## Context

`bootstrap-broker-foundation` delivers the executable, the configuration, the OpenWire connection with negotiation and authentication, and the Java acceptance program. Nothing can be sent or received yet. This change adds the broker core for point-to-point messaging. The project goals are compatibility with ActiveMQ Classic for `activemq-client` 5.18.x / 6.x, lower RAM and higher speed than ActiveMQ, with everything held in memory and no storage. The hard constraints that shape this change are strict FIFO delivery (R9) and message IDs identical to ActiveMQ's (R10). The reference for wire behaviour is the Java source: the `org.apache.activemq.openwire.vN` marshallers, `ActiveMQMessageConsumer`, `ActiveMQMessageProducer`, and the broker classes `Queue`, `PrefetchSubscription` and `QueueBrowserSubscription`.

## Goals / Non-Goals

**Goals:**
- Java scenario 1 (10 messages, FIFO, equal `JMSMessageID`) passes against `mqrust.exe`.
- A data model for queues and subscriptions that later changes (topics, transactions, selectors, expiration, compression, admin) extend without rework.
- Correct behaviour for every ack type and for the driver's default options (async sends for non-persistent messages, optimized acks, prefetch, `QueueBrowser`, prefetch 0).
- Memory accounting that makes a RAM-only broker safe to run without a configured limit and controllable with one.

**Non-Goals:**
- Topic delivery and temporary topic delivery (`add-topic-messaging`). Until that change, the destination registry knows topic types but consumers and producers on non-advisory topics are not served.
- Local transactions (`add-local-transactions`), selectors (`add-message-selectors`), expiry sweeper and TTL options (`add-message-expiration`), compression (`add-message-compression`), admin pages (`add-admin-console`), write batching and vectored writes (`optimize-broker-performance`).
- Durable subscriptions, XA, wildcard and composite destinations, exclusive consumers, message groups, priorities.

## Decisions

### D1. Queue data structure: `BTreeMap` keyed by arrival sequence
Each queue holds `pending: BTreeMap<broker_seq, Arc<StoredMessage>>`. Key order is FIFO order; reinserting a message at its original position, and later removing an expired message from the middle, cost O(log n).
- *Alternatives:* a `VecDeque` (O(1) at the ends but O(n) for reinsertion at the original position and for removal from the middle); a linked list with an index (more code and allocations for no gain at the expected sizes).

### D2. Per-consumer inflight ordered by dispatch, not by arrival
Each subscription keeps `inflight: BTreeMap<dispatch_seq, InflightEntry>`, where `dispatch_seq` is a per-consumer counter and the entry stores the `Arc<StoredMessage>`, its `broker_seq`, and a `delivered_acked` flag. The design spec describes inflight as a map keyed by `seq`; dispatch order is used because the client's cumulative acks (STANDARD, and ranges of DELIVERED / REDELIVERED / POISON) are expressed in the order the client received the messages, and that order can differ from `broker_seq` once a returned message is dispatched after later ones. A small `MessageId → dispatch_seq` lookup resolves ack bounds.
- *Alternatives:* key by `broker_seq` (wrong cumulative ack range after a reinsertion); a plain `VecDeque` (O(n) for INDIVIDUAL acks in the middle).

### D3. One mutex per destination, dispatch under the lock, I/O outside
The registry is a partitioned concurrent map of `Arc<Destination>`. Each queue has its own `parking_lot::Mutex`. Enqueue, dispatch decisions and ack processing run under that lock and are O(1) or O(log n); they produce `MessageDispatch` values that are pushed to the consumer connection's outbound channel. No socket write ever happens under the lock.
- *Alternatives:* one actor task per destination (no locks, but an extra channel hop and more latency per message); a global lock (simple but serializes unrelated queues).

### D4. Round-robin with a rotating cursor
The queue keeps its consumers in a vector and a cursor. For each pending message at the head, the broker tries consumers starting from the cursor, picks the first one with free window, and moves the cursor past it. Prefetch-0 consumers have a window only while a `MessagePull` is outstanding. This matches ActiveMQ's per-message rotation of non-exclusive consumers.
- *Alternatives:* fill one consumer's prefetch before the next (simpler, but starves other consumers when prefetch is large); least-loaded selection (non-deterministic order, harder to test).

### D5. Prefetch window counts inflight minus DELIVERED-acked
The design spec states `inflight.len() < prefetchSize`. The window is refined to exclude messages covered by a DELIVERED ack, because the Java client sends DELIVERED acks exactly to extend the window (for example in transacted sessions and with `optimizeAcknowledge`). Without this, those consumers would stall.
- *Alternatives:* strict `inflight.len()` (stalls transacted consumers whose acks only arrive at commit).

### D6. Messages keep opaque body and properties
The decoder reads only the headers the broker needs (destination, `MessageId`, `producerId`, `transactionId`, expiration, `persistent`, `compressed`, `redeliveryCounter`, `replyTo`) and keeps `content` and `marshalledProperties` as `bytes::Bytes` slices of the read buffer. The message is re-encoded per consumer version from the decoded headers plus the opaque bytes. Adding the `dlqDeliveryFailureCause` property is the only case where the broker rewrites `marshalledProperties`; it does so by appending one entry to the primitive map and fixing the entry count, never by decoding the whole map into Java types.
- *Alternatives:* keep the original frame bytes and forward them (impossible across different OpenWire versions and when the broker changes fields such as `brokerSequenceId` and `redeliveryCounter`); fully decode messages (CPU and memory cost against the goals).

### D7. `ProducerAck` size uses the client's formula
The Java client increases its producer window by `Message.getSize()` and decreases it by the size in each `ProducerAck`. The broker therefore computes the same value: ActiveMQ's minimum message size (1024) plus the lengths of `marshalledProperties` and `content`, measured on the message as received. A `ProducerAck` is sent also for discarded messages, otherwise the client window would leak and the producer would block forever.
- *Alternatives:* send the accounted memory size (different value, the client window drifts); omit `ProducerAck` for discarded messages (producer eventually blocks).

### D8. Memory accounting with hysteresis
`memory.rs` holds an atomic counter of accounted bytes and a `limited` flag. A message is accounted once when its `Arc<StoredMessage>` is created and released when the last holder drops it, via the `Drop` of a small guard owned by `StoredMessage`. The limited state is entered when a new message would exceed `max_memory_mb` and left below 90%. The per-message overhead is a constant (initially 256 bytes, to be measured against real structure sizes during implementation).
- *Alternatives:* compute usage on demand by walking queues (expensive, inconsistent under load); block producers like ActiveMQ's flow control (needs per-producer pausing and risks deadlocks with consumers on the same connection; the design spec chose rejection).

### D9. Duplicate detection per destination and producer
Each queue keeps, per `ProducerId`, a bounded window of the last 1024 `producerSequenceId` values (a bitmap anchored at the highest sequence seen). The design spec says "already in the queue"; the audit window is used instead, like ActiveMQ's `producerAudit`, because it also catches a resend of a message that has already been consumed, and it costs O(1) per message.
- *Alternatives:* look up the `MessageId` among pending messages (misses consumed messages, needs an extra index); a single window per producer across all destinations (equally correct, because a producer has one sequence space, but it needs its own shared lock on the send path; per-destination windows stay under the queue lock that is already held).

### D10. Temporary destination ownership by connection
A temporary destination records the owning `ConnectionId` at `DestinationInfo` ADD. The connection keeps the list of temporary destinations it owns and deletes them on close. Ownership is checked on `ConsumerInfo`. Temporary destinations are never auto-created by sends, consumers or producers, as in ActiveMQ.
- *Alternatives:* derive the owner from the physical name (works for names generated by the Java driver, but not for other clients).

### D11. Unsupported destination operations
`DestinationInfo` REMOVE on a normal queue is refused, because the first version has no write operations on non-temporary destinations (admin purging and deletion are also excluded). ADD on a normal destination is accepted as an explicit auto-creation.
- *Alternatives:* implement removal as ActiveMQ does (destroys messages from a client call, against the read-only stance of the first version); ignore the command silently (hides the failure from the application).

## Risks / Trade-offs

- [Per-message round-robin lowers batching compared with filling one consumer at a time] → Dispatch decisions are cheap and the writer task batches frames per connection; benchmarks in `optimize-broker-performance` can revisit the policy.
- [`ProducerAck` size mismatch with the client formula would slowly block producers] → Unit test the size formula against values computed by the Java client in the integration suite; send acks even for discarded messages.
- [A consumer that never acks holds up to its prefetch in memory] → Accounted memory and the optional limit cover it; the admin console will show inflight counts per consumer.
- [Hidden client behaviour around acks (optimized acks, DUPS_OK batching, browser acks)] → Integration tests for each acknowledge mode and for `QueueBrowser`; ignore acks that do not match inflight messages instead of failing the connection.
- [Rejecting sends instead of blocking producers changes behaviour for applications that rely on flow control] → Documented as a deliberate difference; the limit is off by default.
- [Rewriting `marshalledProperties` for DLQ messages could corrupt the map] → Golden test: the Java client reads `dlqDeliveryFailureCause` and every original property from a DLQ message.

## Migration Plan

No data migration: the broker holds no state across restarts. Deployment replaces `mqrust.exe` with the new build. Configuration files from the foundation remain valid; `max_memory_mb` and `auto_delete_empty_after_secs` are optional. Rollback is the previous executable.

## Open Questions

To verify in the Java sources (5.18.x and 6.x) before or during implementation:
- The exact per-version field list of `ActiveMQMessage` and subclasses, `MessageDispatch`, `MessageAck` (including `poisonCause`), `MessagePull` (including `correlationId` and `messageId` in recent versions), `ProducerAck`, `DestinationInfo`, `ConsumerInfo` and `ProducerInfo` in `org.apache.activemq.openwire.v6` … `v12`.
- `MessageId` marshalling and `toString()` with `textView` (v10+), and how `textView` is produced by the 5.18 / 6.x clients.
- `MessagePull` timeout semantics (`0`, `-1`, positive) in `ActiveMQMessageConsumer` and `PrefetchSubscription.pullMessage`.
- Whether `QueueBrowser` consumers send acks and with which type, and whether ActiveMQ includes messages that arrive after the browser is created; this change browses a snapshot taken at creation.
- The text of `dlqDeliveryFailureCause` set by ActiveMQ (`RegionBroker.sendToDeadLetterQueue`), and whether ActiveMQ also sets `originalDestination` on DLQ messages (if it does, it should be added).
- Whether `RemoveInfo.lastDeliveredSequenceId` should limit which returned messages get `redeliveryCounter + 1` on consumer close, as ActiveMQ does for messages prefetched but never delivered to the application.
- Whether ActiveMQ sets `brokerInTime` / `brokerOutTime` on messages by default, and if so whether to replicate it.
- `Message.getSize()` and `DEFAULT_MINIMUM_MESSAGE_SIZE` in 5.18 / 6.x, and whether the client computes the size before or after client-side compression.
