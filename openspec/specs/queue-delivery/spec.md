# queue-delivery Specification

## Purpose
Defines point-to-point queue messaging: send handling, producer flow control, FIFO round-robin dispatch with prefetch, acknowledgements, redelivery, poison messages to the dead letter queue, browsing and queue statistics.
## Requirements
### Requirement: Message send commands
The broker SHALL accept the ActiveMQ message commands `ActiveMQMessage` (23), `ActiveMQBytesMessage` (24), `ActiveMQMapMessage` (25), `ActiveMQObjectMessage` (26), `ActiveMQStreamMessage` (27), `ActiveMQTextMessage` (28) and `ActiveMQBlobMessage` (29), for every supported OpenWire version. It SHALL decode only the headers it needs and keep the message body (`content`) and the properties (`marshalledProperties`) as opaque bytes, delivering them to consumers byte-for-byte unchanged. The message type SHALL be preserved, so a consumer receives the same JMS message class the producer sent.

#### Scenario: All message types round trip
- **WHEN** a Java producer sends a `TextMessage`, `BytesMessage`, `MapMessage`, `ObjectMessage` and `StreamMessage`, each with application properties, to a queue
- **THEN** a Java consumer receives five messages of the same types, with identical bodies and properties

#### Scenario: Cross-version delivery
- **WHEN** a producer using OpenWire version 12 sends a message and a consumer using version 9 receives it
- **THEN** the consumer decodes the message with version 9 encoding, and its body and properties are identical

### Requirement: Delivery mode does not change storage
Every message SHALL be kept in RAM only, whatever its `JMSDeliveryMode`. The `persistent` flag SHALL be preserved and delivered unchanged, but SHALL NOT cause any write to storage. This is a deliberate difference from ActiveMQ, which stores persistent messages on disk: with ActiveMQRust, persistent and non-persistent messages are both lost on restart.

#### Scenario: Persistent message delivered
- **WHEN** a producer sends a message with `DeliveryMode.PERSISTENT`
- **THEN** the consumer receives it with `getJMSDeliveryMode()` equal to `PERSISTENT`, and no file is created by the broker

### Requirement: Synchronous and asynchronous sends
A message with `responseRequired=true` (synchronous send) SHALL receive a `Response` as soon as it has been stored in the destination, or an `ExceptionResponse` if it is rejected. A message with `responseRequired=false` (asynchronous send) SHALL receive no response. The Java driver's defaults (persistent messages synchronous, non-persistent messages asynchronous, `useAsyncSend`, `alwaysSyncSend`) SHALL all work.

#### Scenario: Synchronous send acknowledged
- **WHEN** a producer sends a persistent message with default connection factory settings
- **THEN** the broker replies with a `Response` correlated to the message command, and `send()` returns

#### Scenario: Asynchronous send
- **WHEN** a producer sends a non-persistent message with default settings
- **THEN** the broker stores the message and sends no `Response`

### Requirement: Producer flow control acknowledgements
When a producer registered with `ProducerInfo.windowSize > 0` sends a message asynchronously, the broker SHALL send a `ProducerAck` (type 19) with the producer's `ProducerId` and the message size computed as the ActiveMQ client computes it (`Message.getSize()`), once the message has been processed. This SHALL happen also when the message is discarded (duplicate, memory limit, missing temporary destination), so the client's producer window never stays blocked. No `ProducerAck` SHALL be sent for synchronous sends or for producers with `windowSize` 0.

#### Scenario: Producer window
- **WHEN** a Java client with `producerWindowSize=1024` sends 1000 non-persistent 1 KB messages to a queue with a consumer
- **THEN** the broker sends one `ProducerAck` per message and the producer never blocks indefinitely

#### Scenario: No ProducerAck for synchronous sends
- **WHEN** a producer with `windowSize > 0` sends a message with `responseRequired=true`
- **THEN** the broker replies with `Response` and sends no `ProducerAck`

### Requirement: Producer and consumer registration
The broker SHALL register producers from `ProducerInfo` (type 6) and consumers from `ConsumerInfo` (type 5), using `consumerId`, `destination`, `prefetchSize` and `browser` from `ConsumerInfo`. `RemoveInfo` (type 12) for a producer or consumer, and the close of its session or connection, SHALL unregister it. The number of consumers and producers attached to each destination SHALL be tracked.

#### Scenario: Consumer count
- **WHEN** two consumers are created on queue `Q` and one of them is closed
- **THEN** the broker's statistics for `Q` show one consumer

#### Scenario: Consumer on a destination that receives messages
- **WHEN** a consumer is created on a queue that already holds 3 messages
- **THEN** the 3 messages are dispatched to it without waiting for new messages

### Requirement: Arrival sequence
Each message SHALL receive, when it enters a queue, a sequence number `broker_seq` that is monotonic per queue. The arrival order SHALL be the order in which the broker finishes receiving the message frames for that queue, serialized by the queue's lock. Waiting messages SHALL be kept ordered by `broker_seq`.

#### Scenario: Two producers
- **WHEN** producers P1 and P2 send messages to the same queue and the broker finishes receiving P1's message before P2's
- **THEN** P1's message has a lower `broker_seq` and is dispatched first

### Requirement: FIFO dispatch
Dispatch SHALL always take messages from the head of the queue's pending messages, in `broker_seq` order. A consumer SHALL receive messages in the order they arrived at the broker.

#### Scenario: 10,000 messages in order
- **WHEN** a producer sends 10,000 messages to a queue with a single consumer
- **THEN** the consumer receives all 10,000 messages in send order with equal `JMSMessageID`s

#### Scenario: Backlog delivered in order
- **WHEN** 100 messages are sent to a queue with no consumer and then a consumer is created
- **THEN** the consumer receives the 100 messages in send order

### Requirement: Round-robin across consumers
With several consumers on a queue, the broker SHALL deliver each message to exactly one consumer, choosing the next consumer in round-robin order that has free prefetch space. Delivery order SHALL stay FIFO: each consumer receives an ordered subsequence of the queue. The broker does not guarantee the order in which several consumers process messages in parallel; strict processing order requires a single consumer per queue, as in ActiveMQ.

#### Scenario: Two consumers alternate
- **WHEN** two consumers with free prefetch space are attached to a queue and 10 messages are sent
- **THEN** one consumer receives messages 1, 3, 5, 7, 9 and the other 2, 4, 6, 8, 10, and no message is delivered twice

#### Scenario: Full consumer skipped
- **WHEN** consumer A has a full prefetch window and consumer B has free space
- **THEN** the next message goes to B

### Requirement: Excluded ordering features
JMS priority (`JMSPriority`) SHALL be kept in the message and delivered unchanged but SHALL NOT change the dispatch order. The `exclusive` flag and the consumer priority of `ConsumerInfo`, and message groups (`JMSXGroupID`), SHALL NOT influence dispatch: consumers are always served in round-robin. These are deliberate differences from ActiveMQ, required by strict FIFO delivery.

#### Scenario: Priority does not reorder
- **WHEN** a producer sends a message with priority 1 and then a message with priority 9 to a queue with no consumer, and a consumer is then created
- **THEN** the consumer receives the priority 1 message first, and each message keeps its `JMSPriority`

#### Scenario: Exclusive flag ignored
- **WHEN** two consumers with `consumer.exclusive=true` are attached to the same queue and 4 messages are sent
- **THEN** both consumers receive messages in round-robin

### Requirement: Prefetch window
The prefetch size SHALL come from `ConsumerInfo.prefetchSize` (Java client default 1000 for queues). The broker SHALL send `MessageDispatch` to a consumer only while the number of its inflight messages not covered by a DELIVERED ack is lower than its prefetch size.

#### Scenario: Prefetch limit
- **WHEN** a consumer with prefetch 10 is attached to a queue holding 50 messages and acknowledges nothing
- **THEN** the broker dispatches exactly 10 messages to it and 40 stay pending

#### Scenario: Window refilled after ack
- **WHEN** that consumer acknowledges 5 messages with a STANDARD ack
- **THEN** the broker dispatches 5 more messages to it

### Requirement: Prefetch zero and message pull
A consumer with prefetch 0 SHALL receive messages only in answer to a `MessagePull` (type 20), at most one message per pull. If a message is available, the broker SHALL dispatch it immediately. If none is available: with a timeout greater than 0 the broker SHALL send a `MessageDispatch` with a null message when the timeout expires, unless a message arrives first and is dispatched; with timeout -1 (`receiveNoWait`) it SHALL send the null `MessageDispatch` immediately; with timeout 0 it SHALL wait until a message arrives.

#### Scenario: Pull with a message available
- **WHEN** a prefetch-0 consumer calls `receive(1000)` on a queue holding a message
- **THEN** the broker dispatches exactly that message and no other

#### Scenario: Pull timeout on an empty queue
- **WHEN** a prefetch-0 consumer calls `receive(500)` on an empty queue
- **THEN** after about 500 ms the broker sends a `MessageDispatch` with a null message and `receive` returns null

#### Scenario: Message arrives during a pull
- **WHEN** a prefetch-0 consumer calls `receive(5000)` on an empty queue and a message is sent 1 second later
- **THEN** the consumer receives the message without waiting for the timeout

#### Scenario: Prefetch-0 consumer does not hoard
- **WHEN** a prefetch-0 consumer and a prefetch-1000 consumer share a queue and the prefetch-0 consumer never pulls
- **THEN** all messages go to the prefetch-1000 consumer

### Requirement: Queue browser
For a consumer with `ConsumerInfo.browser=true`, the broker SHALL send a copy of the messages pending in the queue when the browser is created, in FIFO order and without removing them, respecting the browser's prefetch window, followed by a `MessageDispatch` with a null message that signals the end of the browse. Acks from a browser SHALL free its prefetch window but SHALL NOT remove messages. Messages inflight to other consumers SHALL NOT be browsed.

#### Scenario: Browse without consuming
- **WHEN** a client browses a queue holding 5 messages with a `QueueBrowser`
- **THEN** the enumeration returns the 5 messages in FIFO order and ends, and a consumer created afterwards still receives all 5

#### Scenario: Browse an empty queue
- **WHEN** a client browses an empty queue
- **THEN** the broker sends only the null `MessageDispatch`, and the enumeration has no elements

### Requirement: Message dispatch
Each delivery SHALL be a `MessageDispatch` (type 21) carrying the consumer's `ConsumerId`, the destination, the message and its current `redeliveryCounter`, encoded in the consumer connection's OpenWire version. A dispatched message SHALL move from the queue's pending messages to the consumer's inflight messages, which SHALL be kept in dispatch order.

#### Scenario: Redelivery counter dispatched
- **WHEN** a message whose redelivery counter is 2 is dispatched
- **THEN** the `MessageDispatch` carries `redeliveryCounter` 2 and the Java client reports `JMSRedelivered=true`

### Requirement: Acknowledgement types
The broker SHALL process `MessageAck` (type 22) using `consumerId`, `ackType`, `firstMessageId`, `lastMessageId` and `messageCount`, against the consumer's inflight messages in dispatch order. The range of an ack SHALL be from `firstMessageId` (or the oldest inflight message if it is absent) to `lastMessageId`. The types SHALL behave as follows:
- DELIVERED (0): no removal; the messages in the range no longer count against the prefetch window.
- POISON (1): removes the messages in the range from inflight; persistent ones are moved to `ActiveMQ.DLQ`, non-persistent ones are discarded.
- STANDARD (2): removes all inflight messages up to and including `lastMessageId` (cumulative ack).
- REDELIVERED (3): increments the redelivery counter of the messages in the range, which stay inflight.
- INDIVIDUAL (4): removes only the message `lastMessageId`.
- UNMATCHED (5): same as STANDARD.
- EXPIRED (6): removes the messages in the range from inflight and deletes them as expired, never sending them to the DLQ, and increments the queue's `expired` counter.

An ack for an unknown consumer or for messages that are not inflight SHALL be ignored with a debug log entry. Removed messages SHALL release their accounted memory.

#### Scenario: Cumulative ack
- **WHEN** a consumer has messages 1 to 5 inflight and sends a STANDARD ack with `lastMessageId` = message 3
- **THEN** messages 1, 2 and 3 are removed and messages 4 and 5 stay inflight

#### Scenario: CLIENT_ACKNOWLEDGE
- **WHEN** a `CLIENT_ACKNOWLEDGE` consumer receives 3 messages and calls `acknowledge()` on the third
- **THEN** all 3 messages are removed from the broker

#### Scenario: Individual ack
- **WHEN** a consumer with messages 1 to 3 inflight sends an INDIVIDUAL ack for message 2
- **THEN** only message 2 is removed, and messages 1 and 3 stay inflight

#### Scenario: Delivered ack extends the window
- **WHEN** a consumer with prefetch 2 has 2 inflight messages and sends a DELIVERED ack for both
- **THEN** the broker dispatches up to 2 more messages, and the first 2 are still inflight

#### Scenario: Redelivered ack
- **WHEN** a client calls `session.recover()` on a `CLIENT_ACKNOWLEDGE` session with one unacknowledged message
- **THEN** the broker increments that message's redelivery counter, and the redelivered message has `JMSRedelivered=true`

#### Scenario: Expired ack
- **WHEN** a client sends an EXPIRED ack for an inflight message
- **THEN** the message is removed and deleted, the queue's `expired` counter increases by 1, and `ActiveMQ.DLQ` is unchanged

### Requirement: Dispatch resumes after acks
After any ack or other event that frees prefetch space or makes messages pending again, the broker SHALL resume dispatch to the affected consumers without waiting for new messages.

#### Scenario: Slow consumer catches up
- **WHEN** a consumer with prefetch 1 acknowledges each message it receives from a queue holding 100 messages
- **THEN** it receives all 100 messages

### Requirement: Return of unacknowledged messages
When a consumer closes or its connection drops, every message inflight to it and not yet removed by an ack SHALL return to the queue's pending messages at its original position, by `broker_seq`, not at the tail, with its `redeliveryCounter` incremented by 1 if it was delivered to the application. As in ActiveMQ, when the consumer's `RemoveInfo` carries a `lastDeliveredSequenceId` (not `-1`), only the returned messages whose `broker_seq` is at most that value count as delivered; messages that were prefetched but never handed to the application return with their counter unchanged. On a connection drop, or with `lastDeliveredSequenceId = -1`, every returned message counts as delivered. Returned messages SHALL therefore be dispatched before messages that arrived after them. Transaction rollback is covered by the `add-local-transactions` change.

#### Scenario: Consumer killed with messages in flight
- **WHEN** a consumer receives messages 1 to 5 without acknowledging them, its process is killed, and a new consumer attaches
- **THEN** the new consumer receives messages 1 to 5 first, in order, with `JMSRedelivered=true`, followed by messages 6 onwards

#### Scenario: Prefetched but never delivered
- **WHEN** a consumer has messages 1 and 2 inflight, the application received only message 1, and the consumer closes with `lastDeliveredSequenceId` equal to the `broker_seq` of message 1
- **THEN** message 1 returns with `redeliveryCounter` 1 and message 2 returns with `redeliveryCounter` 0

#### Scenario: Original position
- **WHEN** consumer A holds message 3 inflight, consumer B has consumed messages 4 and 5, message 6 is pending, and consumer A closes
- **THEN** message 3 is placed before message 6 and is the next message dispatched

### Requirement: Client-side redelivery policy
The redelivery policy (`maximumRedeliveries`, redelivery delays) SHALL remain client-side, as in ActiveMQ. The broker SHALL only increment `redeliveryCounter` when a message returns to pending or on a REDELIVERED ack, and move messages to `ActiveMQ.DLQ` on a POISON ack.

#### Scenario: Maximum redeliveries exceeded
- **WHEN** a Java consumer with `maximumRedeliveries=2` rolls back or recovers the same message 3 times
- **THEN** the client sends a POISON ack and the message ends up in `ActiveMQ.DLQ`

### Requirement: Poison messages to the dead letter queue
A message that receives a POISON ack SHALL be removed from the consumer's inflight messages. As in ActiveMQ's default dead letter strategy, a **persistent** poison message SHALL be appended to `ActiveMQ.DLQ` with a new `broker_seq` of that queue; it SHALL keep its original `MessageId` and all its headers, SHALL have `originalDestination` set to the destination it came from (unless already set), and SHALL receive the string property `dlqDeliveryFailureCause` describing the cause sent by the client in the ack (or a generic cause if the ack carries none). A **non-persistent** poison message SHALL be discarded, its memory released and the destination's `discarded` counter incremented. Moving a message to the DLQ SHALL NOT be refused by the memory limit.

#### Scenario: Poison message in the DLQ
- **WHEN** a client sends a POISON ack for a persistent message with `JMSMessageID` `ID:h-1-2-1:1:1:1:7`
- **THEN** `ActiveMQ.DLQ` holds a message with the same `JMSMessageID`, body and properties, plus a `dlqDeliveryFailureCause` property, and its `originalDestination` is the queue it came from

#### Scenario: Non-persistent poison message
- **WHEN** a non-persistent message receives a POISON ack
- **THEN** it is discarded, it does not appear in `ActiveMQ.DLQ`, and the source destination's `discarded` counter is incremented

### Requirement: Queue statistics
For each queue the broker SHALL maintain: pending message count, inflight message count, consumer count, producer count, total enqueued, total consumed (removed by ack), total expired and total discarded (non-persistent poison messages and messages dropped by the memory limit). They SHALL be readable as a consistent snapshot without holding the queue lock beyond the copy.

#### Scenario: Counters after a round trip
- **WHEN** 10 messages are sent to a new queue and all are consumed and acknowledged
- **THEN** the queue shows 0 pending, 0 inflight, 10 enqueued and 10 consumed

