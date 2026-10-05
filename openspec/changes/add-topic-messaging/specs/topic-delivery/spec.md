## ADDED Requirements

### Requirement: Non-durable topic subscriptions
Each consumer attached to a topic SHALL have its own subscription with its own list of pending messages. A message published to the topic SHALL be logically copied into the pending list of every subscription attached at the moment the message is stored, while the message itself (body, properties and headers) SHALL be held once in memory and shared by all subscriptions. A subscription SHALL receive only messages published after its `ConsumerInfo` has been processed.

#### Scenario: Three subscribers
- **WHEN** three consumers subscribe to topic `EVENTS` and a producer publishes 100 messages
- **THEN** each consumer receives all 100 messages, in publish order, with equal `JMSMessageID`s

#### Scenario: Shared body
- **WHEN** a 1 MB message is published to a topic with 10 subscribers
- **THEN** the accounted message memory grows by about 1 MB, not 10 MB

#### Scenario: Late subscriber
- **WHEN** a consumer subscribes to a topic after 5 messages have been published
- **THEN** it does not receive those 5 messages, and receives the next one published

### Requirement: Publishing without subscribers
A message published to a topic with no attached subscriptions SHALL be discarded, as JMS semantics require for non-durable topics. A synchronous publish SHALL still receive a `Response`, and a `ProducerAck` SHALL still be sent when the producer's `windowSize` is greater than 0.

#### Scenario: Nobody listening
- **WHEN** a producer publishes a persistent message synchronously to a topic with no consumers
- **THEN** `send()` returns normally and the message is not stored anywhere

### Requirement: Per-subscription FIFO, prefetch and acks
Each subscription SHALL receive its messages in publish order. Prefetch (from `ConsumerInfo.prefetchSize`, Java client default 32767 for topics), prefetch 0 with `MessagePull`, `MessageDispatch` and all ack types (DELIVERED, POISON, STANDARD, REDELIVERED, INDIVIDUAL, UNMATCHED, EXPIRED) SHALL work per subscription exactly as specified for queues by `queue-delivery`, with UNMATCHED behaving as STANDARD. A message that receives a POISON ack SHALL be moved to `ActiveMQ.DLQ` with the `dlqDeliveryFailureCause` property, as for queues. A message is removed from memory when the last subscription holding it no longer references it.

#### Scenario: Prefetch on a topic
- **WHEN** a topic consumer with prefetch 10 acknowledges nothing and 50 messages are published
- **THEN** 10 messages are dispatched to it and 40 wait in its pending list

#### Scenario: Independent acks
- **WHEN** two subscribers receive the same message and only one acknowledges it
- **THEN** the message stays in memory until the second subscriber acknowledges it

#### Scenario: Poison on a topic
- **WHEN** a topic subscriber sends a POISON ack for a message
- **THEN** the message is stored in `ActiveMQ.DLQ` with `dlqDeliveryFailureCause`, and other subscribers are not affected

### Requirement: End of a subscription
When a topic consumer closes or its connection drops, its subscription SHALL be removed together with its pending and inflight messages, which SHALL NOT be redelivered to other subscribers. Their accounted memory SHALL be released when no other subscription references them.

#### Scenario: Subscriber leaves
- **WHEN** a topic subscriber with 20 pending and 5 inflight messages closes
- **THEN** its 25 messages are discarded, the other subscribers keep their own messages, and memory used only by that subscription is released

### Requirement: Slow consumer eviction
The `[broker]` section SHALL accept the key `topic_max_pending_per_consumer`, a non-negative integer, default `10000`, where `0` disables the limit. When storing a message would make a subscription's pending list (messages not yet dispatched) exceed this value, the broker SHALL discard the oldest pending messages of that subscription to make room, and SHALL add each discarded message to the topic's `discarded` counter. This is ActiveMQ's "evict oldest" strategy. Inflight messages SHALL NOT be evicted. Other subscriptions of the same topic SHALL NOT be affected. Evictions SHALL be logged at debug level. A negative or non-integer value SHALL be a configuration validation error that names `broker.topic_max_pending_per_consumer` and exits with code 2.

#### Scenario: Oldest messages evicted
- **WHEN** `topic_max_pending_per_consumer = 100`, a subscriber with prefetch 10 acknowledges nothing, and 200 messages are published
- **THEN** that subscriber holds messages 1–10 inflight and messages 101–200 pending, the topic's `discarded` counter is 90, and a fast subscriber on the same topic receives all 200
- **WHEN** the slow subscriber then acknowledges every message it receives
- **THEN** it receives messages 101–200 in publish order after messages 1–10

#### Scenario: Default limit
- **WHEN** the broker runs with no configuration file
- **THEN** each topic subscription keeps at most 10,000 pending messages

#### Scenario: Invalid value
- **WHEN** the configuration file sets `topic_max_pending_per_consumer = -5`
- **THEN** the broker reports an error naming `broker.topic_max_pending_per_consumer` and exits with code 2

### Requirement: No-local consumers
A topic consumer whose `ConsumerInfo.noLocal` is true SHALL NOT receive messages published by producers of its own connection.

#### Scenario: noLocal subscriber
- **WHEN** a connection creates a topic consumer with `noLocal=true` and a producer on the same topic, and another connection publishes too
- **THEN** the consumer receives only the messages published by the other connection

### Requirement: Temporary topics
A temporary topic SHALL deliver messages with the same rules as a topic. As for every temporary destination: it SHALL be created by `DestinationInfo` ADD from its owning connection; only the owning connection SHALL create consumers on it, others receiving `javax.jms.InvalidDestinationException`; any authenticated connection SHALL be able to publish to it; it SHALL be deleted, with its subscriptions and messages, when the owning connection closes; a synchronous publish to a temporary topic that does not exist SHALL receive `javax.jms.InvalidDestinationException`, and an asynchronous one SHALL be discarded with a debug log entry.

#### Scenario: Reply on a temporary topic
- **WHEN** client A creates a temporary topic and a consumer on it, sends a request with `JMSReplyTo` set to it, and client B publishes a reply there
- **THEN** client A's consumer receives the reply

#### Scenario: Foreign subscriber refused
- **WHEN** client B creates a consumer on a temporary topic owned by client A
- **THEN** client B receives `InvalidDestinationException`

#### Scenario: Owner closes
- **WHEN** the connection that owns a temporary topic closes and client B then publishes to it synchronously
- **THEN** client B receives `InvalidDestinationException`

### Requirement: Durable subscriptions refused
Durable subscriptions are not supported. A `ConsumerInfo` with a subscription name (from `createDurableSubscriber` or `createDurableConsumer`) SHALL receive an `ExceptionResponse` carrying `javax.jms.JMSException` with the message "Durable subscriptions are not supported". A `RemoveSubscriptionInfo` (type 9) with `responseRequired=true` SHALL receive the same kind of `ExceptionResponse`. This is a deliberate difference from ActiveMQ.

#### Scenario: Durable subscriber
- **WHEN** a Java client calls `session.createDurableSubscriber(topic, "sub1")`
- **THEN** the call throws `JMSException` and no subscription is registered

#### Scenario: Unsubscribe
- **WHEN** a Java client calls `session.unsubscribe("sub1")`
- **THEN** the call throws `JMSException`

### Requirement: Topic statistics
For each topic the broker SHALL maintain: consumer count, producer count, total published messages and total discarded messages (evicted by the slow consumer limit). They SHALL be readable as a consistent snapshot. Advisory topics SHALL NOT be included.

#### Scenario: Counters
- **WHEN** 50 messages are published to a topic with one subscriber and no evictions occur
- **THEN** the topic shows 1 consumer, 50 published and 0 discarded
