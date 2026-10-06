## REMOVED Requirements

### Requirement: Durable subscriptions refused
**Reason**: Durable subscriptions are now supported in memory.
**Migration**: None for clients: `createDurableSubscriber`, `createDurableConsumer` and `unsubscribe` now succeed. Subscriptions and their messages are lost on broker restart.

## MODIFIED Requirements

### Requirement: End of a subscription
When a consumer of a non-durable topic subscription closes or its connection drops, its subscription SHALL be removed together with its pending and inflight messages, which SHALL NOT be redelivered to other subscribers. Their accounted memory SHALL be released when no other subscription references them. Durable subscriptions SHALL instead be detached as defined in "Durable subscriptions in memory".

#### Scenario: Subscriber leaves
- **WHEN** a non-durable topic subscriber with 20 pending and 5 inflight messages closes
- **THEN** its 25 messages are discarded, the other subscribers keep their own messages, and memory used only by that subscription is released

## ADDED Requirements

### Requirement: Durable subscriptions in memory
A `ConsumerInfo` on a topic with a subscription name SHALL create, or reattach to, the durable subscription identified by the connection's client ID and the subscription name, defined by its topic, selector and no-local flag. When its consumer closes or its connection drops, the subscription SHALL be kept: its inflight messages SHALL return to its pending list in their original order and be redelivered with `JMSRedelivered=true`, and messages published to the topic SHALL keep being added, persistent and non-persistent alike. A consumer reattaching with the same client ID and name SHALL receive the pending messages in publish order, then new ones. Reattaching with a different topic, selector or no-local flag while the subscription is offline SHALL replace it and drop its messages. A second consumer on an active durable subscription SHALL be refused with `javax.jms.JMSException` "Durable consumer is in use for client: <id> and subscriptionName: <name>". A durable `ConsumerInfo` on a connection without a client ID SHALL be refused with `javax.jms.InvalidClientIDException`. `session.unsubscribe(name)` SHALL remove an offline subscription and its messages, SHALL fail with `JMSException` while a consumer is attached, and SHALL fail with `InvalidDestinationException` for an unknown name. Durable subscriptions SHALL be kept in RAM only and SHALL be lost when the broker restarts.

#### Scenario: Messages kept while offline
- **WHEN** client `app1` creates durable subscriber `sub1` on topic `EVENTS`, closes it, 20 messages are published, and `app1` creates `sub1` again
- **THEN** it receives the 20 messages in publish order

#### Scenario: Unacknowledged messages redelivered
- **WHEN** `sub1` received 5 messages without acknowledging them and its connection drops
- **THEN** after reattaching it receives those 5 messages first, with `JMSRedelivered=true`

#### Scenario: Changed selector
- **WHEN** offline subscription `sub1` with selector `type = 'a'` holds 10 messages and is reattached with selector `type = 'b'`
- **THEN** the 10 messages are dropped and only new messages matching `type = 'b'` are delivered

#### Scenario: In use
- **WHEN** `sub1` has an active consumer and another session of the same client creates `sub1` again
- **THEN** the second call fails with `JMSException` naming the client and the subscription

#### Scenario: Unsubscribe
- **WHEN** `sub1` is offline with 20 pending messages and the client calls `session.unsubscribe("sub1")`
- **THEN** the subscription and its messages are removed and their memory is released

#### Scenario: Restart
- **WHEN** the broker is restarted while `sub1` is offline with pending messages
- **THEN** the subscription and its messages no longer exist

### Requirement: Unique client IDs
A `ConnectionInfo` whose client ID is already used by another open connection SHALL be refused with `javax.jms.InvalidClientIDException` with the message "Broker: <broker name> - Client: <client ID> already connected from <address>". The client ID SHALL become available again when the connection that uses it closes.

#### Scenario: Duplicate client ID
- **WHEN** a connection with client ID `app1` is open and a second connection calls `setClientID("app1")` and starts
- **THEN** the second connection fails with `InvalidClientIDException` and the first is unaffected

#### Scenario: Reuse after close
- **WHEN** the first connection with client ID `app1` closes
- **THEN** a new connection with client ID `app1` succeeds

### Requirement: Offline durable subscription limits
`topic_max_pending_per_consumer` SHALL NOT evict messages of durable subscriptions; durable subscriptions SHALL be bounded by the broker memory limit. The `[broker]` section SHALL accept `durable_offline_timeout_secs`, a non-negative integer, default `0` (never), and the broker SHALL remove a durable subscription that has been offline for longer than that, with its messages, logging it at info level. An invalid value SHALL be a configuration error naming `broker.durable_offline_timeout_secs` with exit code 2.

#### Scenario: Not evicted
- **WHEN** `topic_max_pending_per_consumer = 100` and 500 messages are published while `sub1` is offline
- **THEN** `sub1` delivers all 500 when it reattaches

#### Scenario: Offline timeout
- **WHEN** `durable_offline_timeout_secs = 3600` and `sub1` has been offline for 61 minutes
- **THEN** `sub1` no longer exists and the log names it

### Requirement: Durable subscriptions in the admin console
The topic pages and `/api/topics` SHALL list each topic's durable subscriptions with client ID, subscription name, selector, no-local flag, pending and inflight messages, state (active or offline) and the time it went offline. An offline durable subscription SHALL be deletable from the console with the confirmation, `POST`, same-origin and `admin.read_only` rules of the other console write operations, logged at info level as `admin <user> from <ip> deleted durable subscription <client ID>:<name> on <topic>`.

#### Scenario: Listed
- **WHEN** `app1:sub1` is offline with 20 pending messages
- **THEN** the topic page lists it as offline with 20 pending and the time it went offline

#### Scenario: Active not deletable
- **WHEN** `app1:sub1` has an active consumer
- **THEN** the console offers no delete action for it, and the delete `POST` answers that the subscription is in use

### Requirement: Shared subscriptions refused
A `ConsumerInfo` for a shared subscription (`createSharedConsumer`, `createSharedDurableConsumer`) SHALL be refused with `javax.jms.JMSException`, as ActiveMQ 5.x and 6.x do.

#### Scenario: Shared durable consumer
- **WHEN** a Jakarta client calls `session.createSharedDurableConsumer(topic, "s1")`
- **THEN** the call throws `JMSException` and no subscription is registered
