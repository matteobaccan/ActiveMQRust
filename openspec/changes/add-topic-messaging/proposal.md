## Why

ActiveMQRust aims to be **compatible** with ActiveMQ Classic, **use less RAM** and be **faster** than ActiveMQ, processing messages in RAM with no storage. Besides queues, ActiveMQ applications commonly use non-durable topics for event and notification fan-out, cache invalidation and status updates, and temporary topics for request/reply. These are exactly the transient, high-rate workloads an in-memory broker is built for. `add-queue-messaging` provides the destination registry, codec, acks and memory accounting; this change adds publish/subscribe delivery on top of them, with a bounded per-subscriber backlog so a slow subscriber cannot exhaust memory.

## What Changes

- Non-durable topic subscriptions: each consumer has its own list of pending messages; a published message is logically copied to every attached subscription while its body is shared in memory.
- Messages published to a topic with no subscribers are discarded, following JMS semantics.
- FIFO order, prefetch (client default 32767 for topics), pull and every ack type work per subscription as they do for queues; POISON messages go to `ActiveMQ.DLQ`.
- Slow consumer protection: a subscription whose pending list exceeds `topic_max_pending_per_consumer` (new `[broker]` key, default 10000) loses its oldest messages, which are counted ("evict oldest", as in ActiveMQ).
- Temporary topics: delivery for the request/reply and private-notification patterns, with the ownership rules of temporary destinations.
- `noLocal` consumers do not receive messages published by their own connection.
- Durable subscriptions are refused explicitly (`ConsumerInfo` with a subscription name, `RemoveSubscriptionInfo`).
- Topic statistics: consumers, producers, published messages and discarded messages.

## Capabilities

### New Capabilities

- `topic-delivery`: non-durable topic and temporary topic publish/subscribe, per-subscription FIFO, prefetch and acks, slow consumer eviction with `topic_max_pending_per_consumer`, `noLocal`, refusal of durable subscriptions, topic statistics.

### Modified Capabilities

None.

## Impact

- Code: `src/broker/destination.rs` (topic type and fan-out), `src/broker/subscription.rs` (per-subscription pending list and eviction), `src/connection.rs` (topic consumers, `RemoveSubscriptionInfo`), `src/config.rs` (`topic_max_pending_per_consumer`), codec support for `RemoveSubscriptionInfo` (type 9) decoding.
- Tests: broker semantics tests for fan-out and eviction; Java integration test "topic with 3 subscribers" and temporary topic request/reply.
- No new crates.
- Depends on `add-queue-messaging` (destination registry, temporary destination ownership, codec, ack handling, memory accounting). `add-message-selectors` and `add-message-expiration` extend topic delivery afterwards.
