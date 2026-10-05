## Context

`add-queue-messaging` provides the destination registry (including the topic and temporary topic types and temporary destination ownership), the message codec, `MessageDispatch`, every ack type, the DLQ and memory accounting. Topics are not served yet. ActiveMQ applications use non-durable topics for fan-out of events, notifications and cache invalidation, which are core use cases of an in-RAM broker. The goals remain compatibility with ActiveMQ Classic for `activemq-client` 5.18.x / 6.x, lower RAM and higher speed. Durable subscriptions are excluded from the first version. The reference implementation is ActiveMQ's `Topic`, `TopicSubscription` and `OldestMessageEvictionStrategy`.

## Goals / Non-Goals

**Goals:**
- Fan-out to any number of subscribers with one in-memory copy of each message.
- Per-subscription FIFO, prefetch and acks with the same code paths as queues.
- Bounded memory per slow subscriber, configurable with `topic_max_pending_per_consumer`.
- Temporary topics for request/reply and private notifications.

**Non-Goals:**
- Durable subscriptions (refused explicitly), virtual topics, wildcard and composite topics.
- Selectors at publish time (`add-message-selectors`) and expiry in subscription lists (`add-message-expiration`); the data structures leave room for both.
- Publishing advisory messages.

## Decisions

### D1. One shared message, one pending list per subscription
A published message becomes one `Arc<StoredMessage>`; each subscription's pending list holds a clone of the `Arc` keyed by the message's broker-wide `broker_seq`, the same monotonic sequence used for queues. Since `broker_seq` grows in publish order, it gives every subscription the topic's publish order without a separate per-topic counter. Memory is accounted once for the `StoredMessage` and released when the last `Arc` is dropped.
- *Alternatives:* a single topic log with a cursor per subscription (less memory per entry, but eviction for one slow subscriber and independent acks become complex, and the log cannot be trimmed while any cursor lags); a deep copy per subscriber (multiplies memory by the number of subscribers, against the RAM goal).

### D2. Fan-out under the topic lock, dispatch through the queue code path
Publishing takes the topic's mutex, appends the message to every matching subscription and runs the same dispatch routine used by queue subscriptions (prefetch window, pull, `MessageDispatch` to the outbound channel). There is no round-robin: every subscription is its own single consumer.
- *Alternatives:* a separate mutex per subscription (less contention with many subscribers, but publish order across subscriptions would need extra coordination; can be revisited in `optimize-broker-performance` if benchmarks show contention); an actor per subscription (extra hops and latency).

### D3. Evict oldest pending, never inflight
When a subscription's pending list would exceed `topic_max_pending_per_consumer`, the oldest pending entries are popped and counted in the topic's `discarded` counter. Inflight messages are untouched because the client already holds them and will ack them. `0` disables the limit, consistent with the other `0 = disabled` keys.
- *Alternatives:* block the publisher (one slow subscriber would stall all of them and the publisher); drop the newest message (the subscriber keeps stale data, which is worse for status updates and cache invalidation); disconnect the slow consumer (ActiveMQ's `abortSlowConsumerStrategy`, too disruptive as a default).

### D4. Subscription end discards its messages
Non-durable subscriptions have no one to hand messages to when they end, so closing a topic consumer drops its pending and inflight entries. This matches ActiveMQ.
- *Alternatives:* none compatible with non-durable semantics.

### D5. `noLocal` by connection
A `noLocal` subscription skips messages whose `ProducerId` belongs to the same connection, checked at publish time by comparing the producer's connection ID. The design spec does not mention `noLocal`; it is included because it is part of the JMS API and ignoring it would silently deliver unwanted messages.
- *Alternatives:* ignore `noLocal` (silent behaviour change for applications that use it); reject it (breaks applications that set it).

### D6. Explicit refusal of durable subscriptions
A `ConsumerInfo` with a `subscriptionName` and a `RemoveSubscriptionInfo` receive `ExceptionResponse(javax.jms.JMSException)`. Failing loudly is better than creating a non-durable subscription that silently loses messages while the client is offline.
- *Alternatives:* downgrade to a non-durable subscription (silent data loss, misleading); in-memory durable subscriptions (future work, §12 of the design spec).

## Risks / Trade-offs

- [One mutex per topic serializes publishers on a hot topic with many subscribers] → Fan-out is a few pointer pushes per subscriber; benchmark in `optimize-broker-performance` before splitting the lock.
- [Eviction silently loses messages for slow subscribers] → Counted in `discarded`, visible in the admin console, logged at debug level; the limit is configurable and can be disabled.
- [A memory-heavy topic with many slow subscribers: up to `topic_max_pending_per_consumer` × subscribers references] → Messages are shared, so memory grows with distinct messages, not references; the global `max_memory_mb` still applies to publishing.
- [Default topic prefetch 32767 lets a stalled client hold many messages inflight] → Same as ActiveMQ; inflight is accounted and visible in the admin console.

## Migration Plan

No data migration. Deploy the new executable; existing configuration files stay valid because `topic_max_pending_per_consumer` is optional. Rollback is the previous executable, which does not serve topics.

## Open Questions

To verify in the Java sources (5.18.x and 6.x):
- How ActiveMQ's `TopicSubscription` handles acks, in particular whether the client sends DELIVERED or STANDARD acks for topic consumers with `optimizeAcknowledge` and the default prefetch, and whether UNMATCHED acks appear without selectors.
- Whether ActiveMQ's `noLocal` check uses the producer's connection ID or the message's `producerId` connection part, and how it behaves for messages sent through another connection of the same client.
- Whether ActiveMQ sends non-persistent POISON topic messages to the DLQ. *Resolved:* its default dead letter strategy (`processNonPersistent=false`) discards them; this broker does the same, as for queues: persistent POISON messages go to `ActiveMQ.DLQ`, non-persistent ones are discarded and counted in the topic's `discarded` counter.
- The `ConsumerInfo` field carrying the durable subscription name in each OpenWire version (`subscriptionName`) and the fields of `RemoveSubscriptionInfo`.
- Whether the topic `discarded` counter shown in the admin should also count messages published with no subscribers. *Resolved:* it does not; it counts evictions and non-persistent POISON messages.
