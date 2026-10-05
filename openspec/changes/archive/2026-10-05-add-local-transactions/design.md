## Context

`add-queue-messaging` provides synchronous and asynchronous sends, per-consumer inflight messages ordered by dispatch, every ack type, reinsertion at the original position with `redeliveryCounter + 1`, and memory accounting. Transacted sessions are common in ActiveMQ applications, especially with Spring JMS, so local transactions are needed for compatibility. The broker has no storage: a transaction provides atomic visibility of sends and acks, and redelivery after rollback, for the lifetime of the broker process. XA is out of scope. The reference behaviour is ActiveMQ's `TransactionBroker`, `LocalTransaction` and, on the client side, `TransactionContext` and `ActiveMQMessageConsumer` (commit, rollback, `ackLater`).

## Goals / Non-Goals

**Goals:**
- Transacted producers and consumers of the Java driver work unchanged, including Spring's transacted listener containers.
- Sends become visible atomically at commit, in send order; acks take effect at commit.
- Rollback and connection loss never lose a consumed message and never deliver it twice.

**Non-Goals:**
- XA transactions and two-phase commit (refused explicitly).
- Durability of committed messages across restarts.
- Isolation guarantees across destinations beyond "nothing is visible before commit".

## Decisions

### D1. Transaction table per connection
Each connection owns a map `LocalTransactionId → Transaction`, where a transaction holds the buffered messages (`Vec<(Destination, Arc<StoredMessage>)>` in send order) and the recorded acks (`Vec<(ConsumerId, MessageAck)>`). Closing the connection rolls back every entry. No global lock is needed because a local transaction is only ever touched by its own connection's reader task.
- *Alternatives:* a broker-wide transaction registry (needed only for XA and recovery, adds a shared lock); per-session tables (the OpenWire `LocalTransactionId` is scoped by connection, not by session).

### D2. Messages enter destinations at commit
Buffered messages are not inserted into destinations until commit, so they get their `broker_seq` at commit time and appear after messages that other producers committed or sent earlier. Commit applies the buffer in send order, taking each destination lock in turn, then applies the recorded acks, then replies.
- *Alternatives:* insert at send time with an invisible flag (keeps the send-time position, but every dispatch would have to skip invisible entries, and the design spec requires `broker_seq` to be assigned at commit); taking all destination locks at once for cross-destination atomicity (deadlock risk and lock ordering complexity for a guarantee JMS does not require).

### D3. Memory accounted at send time, never refused at commit
A buffered message is accounted as soon as it arrives, and the memory limit is checked at that moment. Commit never fails for memory reasons, because failing a commit after the client believes its sends succeeded would be harder to handle than failing the send.
- *Alternatives:* check the limit at commit (a large transaction could fail at the last step); exclude buffers from accounting (unbounded hidden memory).

### D4. Rollback keeps consumed messages with their open consumer
The design spec says consumed messages return to pending at their original position on rollback. The ActiveMQ Java client, however, redelivers rolled-back messages itself: it sends a REDELIVERED ack (incrementing the counter on the broker) and puts the messages back at the head of its own local queue for the same consumer, then sends the ROLLBACK. If the broker also returned the messages to pending, they would be dispatched again and delivered twice. The broker therefore discards the recorded acks and leaves the messages inflight to the consumer; the observable result is the one the design spec requires (redelivered first, counter + 1). If the consumer has closed, or the connection drops, the messages go back to pending at their original position with `redeliveryCounter + 1`, exactly as in the design spec. When the client exceeds `maximumRedeliveries`, it sends a POISON ack and the message goes to `ActiveMQ.DLQ`.
- *Alternatives:* return every rolled-back message to pending (duplicates with the Java client); remove the messages from the consumer and rely on the client to re-ack (inconsistent inflight state).

### D5. Which acks are deferred
Removal acks (STANDARD, INDIVIDUAL, UNMATCHED, POISON) carrying an open transaction ID are recorded and applied at commit. DELIVERED acks only move the prefetch window and take effect immediately, as do REDELIVERED and EXPIRED. Recorded acks also free the prefetch window immediately, otherwise a transacted consumer with more messages than its prefetch would stall before it can commit.
- *Alternatives:* defer every ack carrying a transaction ID (transacted consumers stall at the prefetch limit); apply removal acks immediately and undo them on rollback (the message would have to be resurrected after its memory was released).

### D6. XA refused per command
Any command carrying an `XATransactionId` and the operations PREPARE, COMMIT_TWO_PHASE and RECOVER get `ExceptionResponse(javax.jms.JMSException: XA transactions not supported)`; the connection stays open so the application can report the error.
- *Alternatives:* close the connection (harsher than needed); accept XA as local one-phase transactions (silent loss of two-phase semantics).

## Risks / Trade-offs

- [The rollback behaviour depends on the Java client's local redelivery] → Verified against the client sources and by the integration test with commit and rollback on both driver versions; messages are returned to pending whenever their consumer is gone, so no message is ever lost.
- [Cross-destination commit is not atomic for an observer racing the commit] → JMS only requires that nothing is visible before commit; documented here, and the commit reply is sent only after every message is in place.
- [Large uncommitted transactions hold memory] → Buffers are accounted and limited by `max_memory_mb`; the connection's drop rolls them back.
- [Messages reserved by a transaction after their consumer closes are invisible until the transaction ends] → The Java client defers consumer close until the transaction completes; connection loss always ends the transaction.

## Migration Plan

No data migration and no new configuration keys. Deploy the new executable; rollback is the previous executable, against which transacted sessions fail at the first `TransactionInfo`.

## Open Questions

To verify in the Java sources (5.18.x and 6.x):
- `ActiveMQMessageConsumer.rollback()`: the exact sequence of REDELIVERED / POISON acks and local redelivery, including `nonBlockingRedelivery` and redelivery delays, to confirm decision D4.
- Which ack types the client sends inside a transaction (`ackLater` with DELIVERED, STANDARD in `beforeEnd`) and whether DELIVERED acks carry the transaction ID.
- Whether the client sends END or FORGET for local transactions in any version, and what ActiveMQ replies.
- The per-version fields of `TransactionInfo`, `LocalTransactionId` and `XATransactionId`, and the exact message text ActiveMQ uses for an unknown transaction.
- Whether ActiveMQ assigns the queue position of a transacted message at send time or at commit time in its in-memory cursor (this change follows the design spec: commit time).
