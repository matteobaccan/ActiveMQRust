## ADDED Requirements

### Requirement: Local transaction lifecycle
The broker SHALL handle `TransactionInfo` (type 7) carrying a `LocalTransactionId` (type 111, fields `connectionId` and `value`). A transaction SHALL belong to the connection that began it. The operations SHALL behave as follows:
- BEGIN: registers the transaction as open;
- COMMIT_ONE_PHASE: commits it (see the commit requirement) and closes it;
- ROLLBACK: rolls it back (see the rollback requirement) and closes it;
- END and FORGET: acknowledged with `Response`, with no other effect.

A COMMIT_ONE_PHASE or ROLLBACK for a transaction that has not been begun on that connection SHALL receive an `ExceptionResponse` carrying `javax.jms.JMSException` with the message "Transaction '<id>' has not been started.". Every `TransactionInfo` with `responseRequired=true` SHALL receive a `Response` or `ExceptionResponse` after the operation has been fully applied.

#### Scenario: Transacted session works
- **WHEN** a Java client creates a session with `createSession(true, Session.SESSION_TRANSACTED)`, sends a message and calls `commit()`
- **THEN** every `TransactionInfo` receives a `Response` and `commit()` returns normally

#### Scenario: Commit of an unknown transaction
- **WHEN** a client sends COMMIT_ONE_PHASE for a `LocalTransactionId` that was never begun
- **THEN** the broker replies with an `ExceptionResponse` carrying `javax.jms.JMSException`

### Requirement: Buffered transacted sends
A message whose `transactionId` refers to an open local transaction SHALL be stored in that transaction's buffer and SHALL NOT be visible to any consumer, browser or statistic of pending messages until the transaction commits. The destination SHALL still be created at send time by the automatic creation rule. A synchronous transacted send SHALL receive a `Response` once the message is buffered. Buffered messages SHALL be included in the accounted message memory, and the memory limit SHALL apply at send time exactly as for non-transacted sends (`javax.jms.ResourceAllocationException` for synchronous sends, discard with warning for asynchronous ones). A message referring to a transaction that is not open SHALL receive an `ExceptionResponse` carrying `javax.jms.JMSException` if sent synchronously, and SHALL be discarded with a warning log entry otherwise.

#### Scenario: Not visible before commit
- **WHEN** a transacted producer sends 3 messages to queue `Q` without committing, and a consumer is attached to `Q`
- **THEN** the consumer receives nothing, and `Q` shows 0 pending messages

#### Scenario: Buffered memory accounted
- **WHEN** a transacted producer sends 100 messages of 1 KB without committing
- **THEN** the accounted message memory includes those 100 messages

### Requirement: Commit
On COMMIT_ONE_PHASE the broker SHALL, in this order: move the buffered messages into their destinations in send order, each getting its `broker_seq` (and, for topics, being delivered to the subscriptions attached at commit time) when it enters; then apply the recorded acks; then reply. Duplicate detection SHALL be applied when the messages enter their destinations. Commit SHALL NOT be refused by the memory limit, because the messages are already accounted. A buffered message for a temporary destination deleted before commit SHALL be discarded with a debug log entry.

#### Scenario: Commit order
- **WHEN** a transacted producer sends `t-1`, `t-2`, `t-3` to queue `Q`, a non-transacted producer sends `n-1` to `Q`, and then the transaction commits
- **THEN** a consumer on `Q` receives `n-1`, `t-1`, `t-2`, `t-3` in that order

#### Scenario: Commit across destinations
- **WHEN** one transaction sends a message to queue `A` and one to queue `B` and commits
- **THEN** both messages become available, and neither is available before the commit reply

### Requirement: Deferred transacted acks
An ack of type STANDARD, INDIVIDUAL, UNMATCHED or POISON whose `transactionId` refers to an open transaction SHALL be recorded in the transaction and applied only at commit. Until then, the acked messages SHALL stay inflight to the consumer and SHALL NOT be dispatched to any other consumer, but SHALL no longer count against the consumer's prefetch window. Acks of type DELIVERED, REDELIVERED and EXPIRED SHALL take effect immediately, whether or not they carry a transaction ID. If the consumer closes while the transaction holds acks for its messages, those messages SHALL remain reserved by the transaction until it commits (they are removed) or rolls back (they return to pending as on rollback).

#### Scenario: Ack applied at commit
- **WHEN** a transacted consumer receives a message, the client acks it in the transaction, and the transaction has not yet committed
- **THEN** the message is still held by the broker; after `commit()` it is removed and the queue's consumed counter increases by 1

#### Scenario: Window freed before commit
- **WHEN** a transacted consumer with prefetch 10 receives and acks 10 messages in an open transaction, with more messages pending
- **THEN** the broker dispatches further messages to it before the commit

### Requirement: Rollback
On ROLLBACK the broker SHALL discard the transaction's buffered messages and release their accounted memory, and SHALL discard the recorded acks, so the consumed messages are not removed. Those consumed messages SHALL be redelivered before messages that arrived after them, with `redeliveryCounter` incremented by 1. While the consumer that received them is still open, the messages SHALL stay inflight to it, because the ActiveMQ Java client redelivers rolled-back messages to the same consumer itself after sending a REDELIVERED ack; the broker SHALL NOT also return them to pending, which would deliver them twice. Messages whose consumer has closed SHALL return to pending at their original position, by `broker_seq`, with `redeliveryCounter + 1`.

#### Scenario: Rollback of sends
- **WHEN** a transacted producer sends 5 messages and calls `rollback()`
- **THEN** no consumer ever receives those messages and the accounted memory returns to its previous value

#### Scenario: Rollback of receives
- **WHEN** a transacted consumer receives messages 1 and 2 from queue `Q` holding messages 1 to 4, and calls `rollback()`
- **THEN** its next receives return messages 1 and 2 with `JMSRedelivered=true`, followed by messages 3 and 4, and each message is delivered to the application once per attempt

#### Scenario: Rollback then close
- **WHEN** a transacted consumer receives message 1, rolls back, and is closed before receiving it again, and another consumer attaches
- **THEN** the other consumer receives message 1 first, with `JMSRedelivered=true`

### Requirement: Rollback on connection loss
When a connection closes or drops, every transaction it still has open SHALL be rolled back: buffered messages are discarded, recorded acks are discarded, and the consumed messages return to pending at their original position with `redeliveryCounter + 1`.

#### Scenario: Client crashes mid-transaction
- **WHEN** a transacted session sends 2 messages and consumes 3, and the client process is killed before commit
- **THEN** the 2 sent messages are never delivered, and the 3 consumed messages are delivered again first to the next consumer, with `JMSRedelivered=true`

### Requirement: XA transactions refused
XA transactions are not supported. A `TransactionInfo` or a message or ack carrying an `XATransactionId` (type 112), and the operations PREPARE, COMMIT_TWO_PHASE and RECOVER, SHALL receive an `ExceptionResponse` carrying `javax.jms.JMSException` with the message "XA transactions not supported" when a response is required; without `responseRequired`, they SHALL be discarded with a warning log entry. This is a deliberate difference from ActiveMQ.

#### Scenario: XA session
- **WHEN** a Java client uses an `ActiveMQXAConnectionFactory` session and starts an XA transaction
- **THEN** the operation fails with `JMSException` "XA transactions not supported" and the connection stays open
