## Why

ActiveMQRust aims to be **compatible** with ActiveMQ Classic, **use less RAM** and be **faster** than ActiveMQ, processing messages in RAM with no storage. Transacted JMS sessions are widely used, in particular by Spring JMS (`DefaultMessageListenerContainer` with `sessionTransacted=true`, `JmsTemplate` in transacted mode). An application that opens a transacted session against a broker without transaction support fails immediately, so local transactions are needed for compatibility. Without storage, a transaction gives atomic visibility and redelivery within the broker's lifetime, not durability; this is the expected trade-off of an in-memory broker.

## What Changes

- `TransactionInfo` handling for local transactions (`LocalTransactionId`, type 111): BEGIN, COMMIT_ONE_PHASE, ROLLBACK, END and FORGET; transactions belong to their connection.
- Transacted sends are buffered and become visible only at commit, entering their destinations in send order and getting their `broker_seq` then.
- Transacted acks are recorded and applied only at commit.
- Rollback discards buffered sends and redelivers consumed messages ahead of later ones with `redeliveryCounter + 1`.
- Open transactions are rolled back when their connection drops.
- XA is refused: `XATransactionId` (type 112) and the two-phase operations get `ExceptionResponse` with `javax.jms.JMSException: XA transactions not supported`.

## Capabilities

### New Capabilities

- `local-transactions`: local transaction lifecycle, buffered sends, deferred acks, commit, rollback and redelivery, rollback on connection drop, refusal of XA.

### Modified Capabilities

None.

## Impact

- Code: `src/broker/transaction.rs` (transaction table per connection, send buffer, recorded acks), `src/connection.rs` (`TransactionInfo`, routing of transacted sends and acks), `src/broker/destination.rs` (commit entry points for messages and acks), codec for `TransactionInfo` (type 7), `LocalTransactionId` (111) and decoding of `XATransactionId` (112).
- Tests: broker semantics tests for commit and rollback; Java integration test "transacted session with commit and rollback".
- No new crates.
- Depends on `add-queue-messaging` (send path, acks, reinsertion at the original position, memory accounting). If `add-topic-messaging` is applied, transacted publishes to topics follow the same rules.
