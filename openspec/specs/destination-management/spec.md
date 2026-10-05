# destination-management Specification

## Purpose
Defines how the broker creates, retains, removes and restricts queue and topic destinations, including temporary destinations and the dead letter queue.
## Requirements
### Requirement: Destination types
The broker SHALL support four destination types, encoded with the ActiveMQ OpenWire destination types: queue (`queue://`, type 100), topic (`topic://`, type 101), temporary queue (`temp-queue://`, type 102) and temporary topic (`temp-topic://`, type 103). A destination SHALL be identified by its type and its physical name. Names SHALL be case-sensitive, and a queue and a topic with the same name SHALL be different destinations. Message delivery on topics and temporary topics is specified by the `add-topic-messaging` change.

#### Scenario: Queue and topic with the same name
- **WHEN** a client creates a consumer on queue `ORDERS` and another client sends to topic `ORDERS`
- **THEN** the registry holds two distinct destinations, and the message sent to the topic never reaches the queue consumer

#### Scenario: Case-sensitive names
- **WHEN** a message is sent to queue `Orders` and a consumer is created on queue `ORDERS`
- **THEN** the consumer does not receive the message, and both queues exist in the registry

#### Scenario: Destination types decoded
- **WHEN** a client sends a command containing a destination of type 102 with physical name `ID:host-1-2-1:1:1`
- **THEN** the broker decodes it as a temporary queue with that name, and re-encodes it with type 102 and the same name

### Requirement: Automatic destination creation
A non-temporary destination SHALL be created automatically, with no prior configuration, on the first `ProducerInfo` that names it, the first `ConsumerInfo` on it, or the first message sent to it, whichever comes first. Creation SHALL be logged at debug level only.

#### Scenario: Created by a consumer
- **WHEN** a client creates a consumer on queue `TEST.NEW` that does not exist
- **THEN** the broker creates the queue, replies with `Response`, and the queue exists in the registry

#### Scenario: Created by a producer
- **WHEN** a client sends `ProducerInfo` with destination queue `TEST.PROD`
- **THEN** the queue `TEST.PROD` exists in the registry before any message is sent

#### Scenario: Created by a message
- **WHEN** an anonymous producer (no destination in `ProducerInfo`) sends a message to queue `TEST.MSG` that does not exist
- **THEN** the broker creates the queue and the message is stored in it

### Requirement: Retention of empty destinations
A queue with no messages, no consumers and no producers SHALL stay in the registry, as in ActiveMQ by default, and SHALL remain visible to the admin console, unless automatic deletion is enabled.

#### Scenario: Empty queue kept
- **WHEN** a queue has been emptied, its consumers and producers have closed, and `auto_delete_empty_after_secs` is not set
- **THEN** the queue is still in the registry 10 minutes later

### Requirement: Optional deletion of empty destinations
The `[broker]` section SHALL accept the key `auto_delete_empty_after_secs`, a non-negative integer, default `0`, where `0` disables automatic deletion. When it is greater than 0, a non-temporary destination that has had no pending messages, no inflight messages, no consumers and no producers continuously for at least that number of seconds SHALL be removed from the registry. The check SHALL run at least once per second (every `min(expiry.check_interval_ms, 1000)` milliseconds). The dead letter queue `ActiveMQ.DLQ` and temporary destinations SHALL never be deleted automatically. A destination that is used again after removal SHALL be created again by the automatic creation rule. A negative or non-integer value SHALL be a configuration validation error that names `broker.auto_delete_empty_after_secs` and exits with code 2.

#### Scenario: Idle empty queue removed
- **WHEN** `auto_delete_empty_after_secs = 5`, a queue is emptied and its last consumer and producer close
- **THEN** the queue is no longer in the registry within 6 seconds

#### Scenario: Activity resets the idle time
- **WHEN** `auto_delete_empty_after_secs = 5` and a consumer attaches to an empty queue 3 seconds after it became idle
- **THEN** the queue is not removed while the consumer is attached, and the 5-second idle time starts again when it detaches

#### Scenario: Queue with messages kept
- **WHEN** `auto_delete_empty_after_secs = 5` and a queue holds one pending message with no consumers or producers
- **THEN** the queue is not removed

#### Scenario: DLQ never auto-deleted
- **WHEN** `auto_delete_empty_after_secs = 5` and `ActiveMQ.DLQ` has been empty and unused for 10 seconds
- **THEN** `ActiveMQ.DLQ` is still in the registry

#### Scenario: Invalid value
- **WHEN** the configuration file sets `auto_delete_empty_after_secs = -1`
- **THEN** the broker reports an error naming `broker.auto_delete_empty_after_secs` and exits with code 2

### Requirement: Temporary destination creation and removal
The broker SHALL handle `DestinationInfo` (type 8). An ADD operation for a temporary queue or temporary topic SHALL create it and record the connection that sent the command as its owner. A REMOVE operation for a temporary destination SHALL delete it together with its messages. A REMOVE of a temporary destination that still has consumers SHALL fail with an `ExceptionResponse` carrying `javax.jms.JMSException` with a message stating that the destination still has an active subscription. An ADD for a non-temporary destination SHALL create it as automatic creation would. A REMOVE for a non-temporary destination (for example from `ActiveMQConnection.destroyDestination()`) SHALL behave as in ActiveMQ: if the destination has no active consumers it SHALL be deleted together with its messages, releasing their memory; if it still has active consumers the REMOVE SHALL fail with an `ExceptionResponse` carrying `javax.jms.JMSException` stating that the destination still has an active subscription. A later use of the same name SHALL create the destination again, empty.

#### Scenario: Temporary queue created
- **WHEN** a Java client calls `session.createTemporaryQueue()`
- **THEN** the broker creates the temporary queue owned by that connection and replies with `Response`

#### Scenario: Temporary queue deleted by the client
- **WHEN** the owning client calls `TemporaryQueue.delete()` after closing its consumers on it
- **THEN** the broker removes the temporary queue and all its messages, and the memory they used is released

#### Scenario: Removal with an active consumer
- **WHEN** a `DestinationInfo` REMOVE arrives for a temporary queue that has an active consumer
- **THEN** the broker replies with an `ExceptionResponse` carrying `javax.jms.JMSException` and the temporary queue is kept

#### Scenario: Removal of a normal queue
- **WHEN** a client calls `destroyDestination()` for queue `ORDERS`, which holds 5 messages and has no consumers
- **THEN** the broker replies with `Response`, the queue and its 5 messages are deleted, and the accounted message memory decreases accordingly

#### Scenario: Removal of a normal queue with consumers
- **WHEN** a `DestinationInfo` REMOVE with `responseRequired=true` arrives for queue `ORDERS` while it has an active consumer
- **THEN** the broker replies with an `ExceptionResponse` carrying `javax.jms.JMSException`, and the queue and its messages are kept

### Requirement: Temporary destination ownership
Only the owning connection SHALL be able to create consumers on a temporary destination. A `ConsumerInfo` from another connection, or for a temporary destination that does not exist, SHALL receive an `ExceptionResponse` carrying `javax.jms.InvalidDestinationException`. Any authenticated connection SHALL be able to send messages to an existing temporary destination, which is what the request/reply pattern with `JMSReplyTo` needs.

#### Scenario: Request/reply
- **WHEN** client A creates a temporary queue, sends a request with `JMSReplyTo` set to it, and client B sends a reply to that `JMSReplyTo`
- **THEN** client A's consumer on the temporary queue receives the reply

#### Scenario: Consumer from another connection
- **WHEN** client B creates a consumer on a temporary queue owned by client A
- **THEN** client B receives `InvalidDestinationException` and no consumer is registered

### Requirement: Temporary destination lifetime
When the owning connection closes, by request or by network failure, the broker SHALL delete all its temporary destinations together with their messages and release their accounted memory. A message sent to a temporary destination that does not exist (never created, or already deleted) SHALL NOT create it: a synchronous send SHALL receive an `ExceptionResponse` carrying `javax.jms.InvalidDestinationException`, and an asynchronous send SHALL be discarded with a debug log entry.

#### Scenario: Owner disconnects
- **WHEN** the connection that owns a temporary queue holding 5 messages drops
- **THEN** the temporary queue and its 5 messages are removed, and the accounted message memory decreases accordingly

#### Scenario: Send to a deleted temporary queue
- **WHEN** a client sends a message synchronously to a temporary queue whose owner has disconnected
- **THEN** the client receives `InvalidDestinationException` and no destination is created

### Requirement: Dead letter queue
The broker SHALL use a queue named `ActiveMQ.DLQ` as the shared dead letter queue for messages that receive a POISON ack. It SHALL be created automatically the first time it is needed, or earlier if a client uses it. It SHALL behave as an ordinary queue: clients can consume and browse it, and its messages follow the same FIFO, ack and memory rules as any other queue.

#### Scenario: DLQ created on demand
- **WHEN** the first POISON ack arrives and `ActiveMQ.DLQ` does not exist
- **THEN** the broker creates `ActiveMQ.DLQ` and stores the poisoned message in it

#### Scenario: DLQ consumable
- **WHEN** a client creates a consumer on queue `ActiveMQ.DLQ` that holds 2 messages
- **THEN** the consumer receives the 2 messages in the order they entered the DLQ

### Requirement: Wildcard and composite destinations rejected
Wildcard destinations (a physical name containing `*` or `>`) and composite destinations (a physical name containing `,`) SHALL be rejected with an `ExceptionResponse` carrying `javax.jms.InvalidDestinationException` when used in `ConsumerInfo`, `ProducerInfo`, `DestinationInfo` or a message send with `responseRequired=true`. An asynchronous message sent to such a destination SHALL be discarded with a warning log entry. Consumers on `ActiveMQ.Advisory.*` topics, including the composite advisory consumer created by the Java driver, SHALL remain accepted as specified by `openwire-transport`.

#### Scenario: Wildcard consumer
- **WHEN** a client creates a consumer on queue `ORDERS.>`
- **THEN** the client receives `InvalidDestinationException` and no destination is created

#### Scenario: Composite producer
- **WHEN** a client creates a producer on queue `A,B`
- **THEN** the client receives `InvalidDestinationException` and neither `A` nor `B` is created

#### Scenario: Advisory composite still accepted
- **WHEN** a Java client with `watchTopicAdvisories=true` connects
- **THEN** its consumer on `ActiveMQ.Advisory.TempQueue,ActiveMQ.Advisory.TempTopic` is accepted with `Response`

