## Context

A topic consumer gets its own subscription (pending list, inflight, prefetch, acks); the subscription is removed when its consumer closes or its connection drops ("End of a subscription"). A `ConsumerInfo` with `subscriptionName` and any `RemoveSubscriptionInfo` are answered with "Durable subscriptions are not supported". Client IDs are stored per connection but not checked for uniqueness. The slow-consumer limit `topic_max_pending_per_consumer` evicts the oldest pending messages of a subscription. The broker has a global memory limit with a limited state.

## Goals / Non-Goals

**Goals:**
- ActiveMQ-compatible durable subscriptions while the broker runs, for clients 5.19.x and 6.x and for the Jakarta TCK.
- No cost for non-durable topics.

**Non-Goals:**
- Survival across restarts (the `add-message-storage` change, 0.6.0).
- Shared subscriptions (JMS 2 `createShared…Consumer`), refused as in ActiveMQ.
- Virtual topics and composite durable destinations.

## Decisions

### D1. A subscription that outlives its consumer
A durable subscription is a topic subscription with a key `(clientId, subscriptionName)` and a flag "active". When its consumer closes or its connection drops, the subscription is detached instead of removed: inflight messages go back to its pending list in order, marked redelivered, and new publications keep being added. Reattaching a consumer with the same key resumes delivery from the oldest pending message. A per-topic map from the key to the subscription gives O(1) lookup.
*Alternative*: a hidden queue per durable subscription. Rejected: it would copy each published message once per subscription instead of sharing it, and change topic statistics and advisory behaviour.

### D2. Identity and definition
The subscription is created by the first `ConsumerInfo` with that key; its definition is the topic, the selector and the no-local flag. A `ConsumerInfo` with the same key but a different definition, while the subscription is offline, replaces it (old messages dropped), as ActiveMQ does. While it is active, a second consumer with the same key is refused with `javax.jms.JMSException` "Durable consumer is in use for client: <id> and subscriptionName: <name>". A `ConsumerInfo` with `subscriptionName` but no client ID on the connection is refused with `javax.jms.InvalidClientIDException`, as the JMS 1.1 API requires.
*Alternative*: keep the old subscription and refuse the new definition. Rejected: ActiveMQ replaces it, and applications rely on that to change selectors.

### D3. Unique client IDs
A `ConnectionInfo` whose client ID is already used by another open connection is refused with `javax.jms.InvalidClientIDException` "Broker: <name> - Client: <id> already connected from <address>", as ActiveMQ does. The ID is released when its connection closes. Generated client IDs of the Java client are unique by construction, so only explicit `setClientID` calls can collide.
*Alternative*: allow duplicates. Rejected: two connections could attach to the same durable subscription concurrently.

### D4. Unsubscribe
`RemoveSubscriptionInfo` with an offline subscription removes it and its pending messages (memory released when no other subscription references them). With an active subscription it fails with `JMSException` "Durable consumer is in use". An unknown subscription fails with `InvalidDestinationException`, as in ActiveMQ.

### D5. Limits for offline subscriptions
An offline subscription is not evicted by `topic_max_pending_per_consumer`: evicting would break the promise of the subscription; ActiveMQ also applies its pending-message limit to non-durable subscribers only. Offline subscriptions count in the memory limit like every message. `broker.durable_offline_timeout_secs` (0 = never, default) removes a subscription that has been offline longer than that, checked by housekeeping, logged at info level.
*Alternative*: apply the eviction limit to durable subscriptions too. Rejected: silent loss of messages the application explicitly asked to keep.

### D6. Admin console
The topic detail and `/api/topics` list durable subscriptions with client ID, name, selector, no-local, pending, inflight, active or offline, and offline since. An offline subscription can be deleted from the console ("Delete subscription"), with the confirmation, `POST`, same-origin and `admin.read_only` rules of the other console write operations, and an info log line.

### D7. Restart
Durable subscriptions and their messages live in RAM: a broker restart loses them, and clients re-create them on their next `createDurableSubscriber`. The README states it next to the existing "RAM only" note.

## Risks / Trade-offs

- [An abandoned durable subscription keeps growing] → memory limit, optional offline timeout, offline subscriptions visible and deletable in the console.
- [Applications expect durability across restarts] → documented; real durability comes with storage in 0.6.0.
- [Unique client IDs may break a setup that reused IDs] → that setup is already broken on ActiveMQ; the error message names the other connection.
