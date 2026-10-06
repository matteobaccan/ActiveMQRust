## Why

Durable subscriptions are refused ("Durable subscriptions are not supported"), so any application that calls `createDurableSubscriber` or `createDurableConsumer` fails at startup, and 5 of the 7 Jakarta TCK failures come from this. Durability across restarts needs storage (planned for 0.6.0), but the main reason applications use durable subscriptions is to receive the messages published while their subscriber is offline, between reconnections. That works in RAM too, as ActiveMQ does with `persistent="false"`; a broker restart loses them, as it already loses every message.

## What Changes

- Durable subscriptions are supported in RAM: `createDurableSubscriber`, `createDurableConsumer`, `unsubscribe`, identified by client ID and subscription name.
- While no consumer is attached, the subscription keeps the messages published to its topic (persistent and non-persistent), and delivers them in publish order when a consumer reattaches.
- Selector and no-local flag are part of the subscription; reattaching with a different topic, selector or no-local flag replaces it, as in ActiveMQ.
- `unsubscribe` fails while a consumer is attached; a second consumer on an active durable subscription is refused.
- Client IDs become unique per broker: a second connection with a client ID already in use is refused with `InvalidClientIDException`, as in ActiveMQ (needed to make the subscription identity reliable).
- Offline durable subscriptions are not evicted by `topic_max_pending_per_consumer`; they are bounded by the memory limit and by a new optional timeout `broker.durable_offline_timeout_secs` (0 = never, default), equivalent to ActiveMQ's `offlineDurableSubscriberTimeout`.
- Admin console: durable subscriptions on the topic pages and API (client ID, name, selector, pending, active or offline), and a "Delete" action for an offline subscription following the console write rules.
- A restart loses every durable subscription and its messages; the README says so.
- Shared subscriptions (`createSharedConsumer`, `createSharedDurableConsumer`) stay refused, as in ActiveMQ 5.x and 6.x.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `topic-delivery`: "Durable subscriptions refused" is removed; new requirements "Durable subscriptions in memory", "Unique client IDs", "Offline durable subscription limits" and "Shared subscriptions refused"; "End of a subscription" applies to non-durable subscriptions only.

## Impact

- Code: `src/broker/destination.rs` (subscriptions that outlive their consumer, keyed by client ID and name; attach and detach), `src/broker/mod.rs` (registry, client ID uniqueness, offline timeout in housekeeping), `src/connection.rs` (`ConsumerInfo` with `subscriptionName`, `RemoveSubscriptionInfo`, `ConnectionInfo` client ID check), `src/config.rs`, admin pages and API, tests, Jakarta TCK results.
- No new crates.
- Planned for 0.4.0. The `add-message-storage` change (0.6.0) builds on it and adds survival across restarts.
