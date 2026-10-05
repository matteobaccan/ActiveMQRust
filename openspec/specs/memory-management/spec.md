# memory-management Specification

## Purpose
Defines how the broker accounts for in-memory message usage against a configurable limit, how it throttles producers when the limit is reached and recovers below 90 percent, while placing no limit on clients.
## Requirements
### Requirement: No limit on clients
The broker SHALL impose no limit on the number of connections, sessions, producers or consumers. The only bound SHALL be the resources of the machine.

#### Scenario: Many consumers and producers
- **WHEN** 1,000 connections each create one session, one producer and one consumer on distinct queues
- **THEN** all of them are accepted and every producer can send a message that its consumer receives

### Requirement: Message memory accounting
The broker SHALL account for the memory used by stored messages: for each message, the size of its body (`content`), plus the size of its properties (`marshalledProperties`), plus a fixed estimated overhead for headers and index entries. A message shared by several holders SHALL be counted once. The accounted total SHALL increase when a message is stored and decrease when it is removed: by ack, POISON move (removed from the source and added to the DLQ), expiry, deletion of a temporary destination, or any other deletion. The current total SHALL be readable at any time.

#### Scenario: Memory follows the queue
- **WHEN** 1,000 messages with a 1 KB body are sent to a queue and then all are consumed and acknowledged
- **THEN** the accounted memory grows by at least 1,000 × 1 KB, and returns to its initial value after the acks

### Requirement: Memory limit configuration
The `[broker]` section SHALL accept the key `max_memory_mb`, a non-negative integer, default `0`, where `0` means no limit. A negative or non-integer value SHALL be a configuration validation error that names `broker.max_memory_mb` and exits with code 2.

#### Scenario: No limit by default
- **WHEN** the broker starts with no configuration file
- **THEN** sends are never rejected for memory reasons

#### Scenario: Invalid value
- **WHEN** the configuration file sets `max_memory_mb = "big"`
- **THEN** the broker reports an error naming `broker.max_memory_mb` and exits with code 2

### Requirement: Behaviour when the limit is reached
When `max_memory_mb` is set and storing a new message would bring the accounted memory above the limit, the broker SHALL enter the limited state. While limited, every new message SHALL be refused: a synchronous send SHALL receive an `ExceptionResponse` carrying `javax.jms.ResourceAllocationException` with a message naming the destination and the limit; an asynchronous send SHALL be discarded and counted. The limit SHALL NOT be applied to messages moved to `ActiveMQ.DLQ` or to messages returning to pending. Consumers SHALL keep receiving and acknowledging messages while the broker is limited. This is a deliberate difference from ActiveMQ, which by default blocks the producer (producer flow control) instead of failing the send.

#### Scenario: Synchronous send rejected
- **WHEN** `max_memory_mb = 1`, 1 MB of messages are stored without consumers, and a producer sends a persistent message
- **THEN** `send()` throws `javax.jms.ResourceAllocationException` and the message is not stored

#### Scenario: Asynchronous send dropped
- **WHEN** the broker is in the limited state and a producer sends a non-persistent message asynchronously
- **THEN** the message is discarded, no error reaches the client, and the dropped-message count increases

#### Scenario: Consumers still served
- **WHEN** the broker is in the limited state and a consumer attaches to a full queue
- **THEN** it receives the stored messages and its acks reduce the accounted memory

### Requirement: Recovery below 90 percent
The broker SHALL leave the limited state only when the accounted memory drops below 90% of `max_memory_mb`. From then on, new messages SHALL be accepted again.

#### Scenario: Hysteresis
- **WHEN** `max_memory_mb = 10`, the limit has been reached, and consumers bring the accounted memory down to 9.5 MB
- **THEN** sends are still refused, and they are accepted again once memory drops below 9 MB

### Requirement: Memory limit logging
The broker SHALL log a warning when it enters the limited state, with the accounted memory and the limit, and a warning when it leaves it, with the number of asynchronous messages discarded while limited. Individual refused or discarded messages SHALL be logged at debug level only.

#### Scenario: Limit reached and recovered
- **WHEN** the limit is reached, 50 asynchronous messages are dropped, and memory then falls below 90% of the limit
- **THEN** the log contains one warning for the limit being reached and one warning for recovery that reports 50 discarded messages

