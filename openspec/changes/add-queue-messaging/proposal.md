## Why

ActiveMQRust aims to be **compatible** with ActiveMQ Classic, **use less RAM** and be **faster** than ActiveMQ, with all message processing done in memory and no storage. After `bootstrap-broker-foundation` a Java client can connect and authenticate, but it cannot send or receive anything. Point-to-point queues are the core of almost every ActiveMQ application, so this change makes the broker carry messages: queues created on first use, strict FIFO delivery, the acknowledgement modes the Java driver uses, message IDs indistinguishable from ActiveMQ's, and memory accounting so a RAM-only broker can protect itself. It is the second milestone of the acceptance program (scenario 1).

## What Changes

- OpenWire codec for messaging: the message commands (types 23–29), `MessageDispatch` (21), `MessageAck` (22), `MessagePull` (20), `ProducerAck` (19), `DestinationInfo` (8), the destination types (100–103) and `MessageId` (110) with its per-version fields; `ConsumerInfo` and `ProducerInfo` become fully handled.
- Destination registry: automatic creation on first use, empty queues kept by default with an optional `auto_delete_empty_after_secs`, temporary destinations owned by their connection, the dead letter queue `ActiveMQ.DLQ`, and rejection of wildcard and composite destinations.
- Queue delivery: arrival sequence numbers and strict FIFO, round-robin across consumers, prefetch window, prefetch 0 with `MessagePull`, `QueueBrowser`, every ack type, reinsertion of unacknowledged messages at their original position, redelivery counter, POISON ack to `ActiveMQ.DLQ`, synchronous and asynchronous sends, `ProducerAck`.
- Message identity: client-generated `MessageId` kept intact, broker sequence ID, ActiveMQ-format IDs for broker-generated objects, duplicate detection per producer.
- Memory management: accounting of message memory, optional `max_memory_mb` limit with `ResourceAllocationException` for synchronous sends and drop-with-warning for asynchronous sends, resume below 90%, no limit on the number of clients.
- Deliberate differences from ActiveMQ, stated in the specs: persistent messages are kept in RAM only; JMS priority, exclusive consumers, consumer priority and message groups do not influence dispatch; POISON messages go to the DLQ whatever their delivery mode; removal of non-temporary destinations through `DestinationInfo` is refused.

## Capabilities

### New Capabilities

- `destination-management`: destination types and names, automatic creation, retention and optional deletion of empty destinations, temporary destinations, `ActiveMQ.DLQ`, rejection of wildcard and composite destinations.
- `queue-delivery`: sending, FIFO ordering, round-robin dispatch, prefetch, pull, browser, acknowledgements, redelivery and DLQ, producer flow control, queue statistics.
- `message-identity`: OpenWire ID structures, preservation of the client `MessageId`, broker sequence IDs, broker-side ID generation, duplicate detection.
- `memory-management`: message memory accounting, `max_memory_mb`, behaviour when the limit is reached and recovered, unlimited clients.

### Modified Capabilities

None.

## Impact

- Code: `src/openwire/` (message, dispatch, ack, pull, producer ack, destination info, destination and message ID marshallers per version), `src/broker/` (`mod.rs` registry and ID generators, `destination.rs`, `subscription.rs`, `memory.rs`), `src/connection.rs` (producer, consumer, send and ack handling), `src/config.rs` (new `[broker]` keys `max_memory_mb` and `auto_delete_empty_after_secs`).
- Tests: broker semantics tests without network (`tests/broker_semantics.rs`), codec round-trip and golden vectors for the new commands, Java integration scenarios.
- No new crates beyond those of the foundation.
- Depends on `bootstrap-broker-foundation`. `add-topic-messaging`, `add-local-transactions`, `add-message-selectors`, `add-message-expiration`, `add-message-compression` and `add-admin-console` build on this change.
