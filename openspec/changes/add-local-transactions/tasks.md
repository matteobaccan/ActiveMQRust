## 1. Codec

- [x] 1.1 Extract from the Java marshallers (v9–v12) the fields of `TransactionInfo` (7), `LocalTransactionId` (111) and `XATransactionId` (112)
- [ ] 1.2 Implement decoding and encoding of the three types, and the `transactionId` field of messages and `MessageAck`, with round-trip tests and golden vectors from a transacted Java session

## 2. Transaction model

- [x] 2.1 Implement the per-connection transaction table with BEGIN, END and FORGET, and the "has not been started" error for unknown transactions
- [ ] 2.2 Route transacted sends to the transaction buffer, with destination auto-creation, memory accounting, the memory limit at send time and `Response` for synchronous sends
- [ ] 2.3 Record STANDARD, INDIVIDUAL, UNMATCHED and POISON acks with a transaction ID, free the prefetch window immediately, and keep DELIVERED, REDELIVERED and EXPIRED immediate
- [x] 2.4 Keep messages with recorded acks reserved when their consumer closes before the transaction ends

## 3. Commit and rollback

- [x] 3.1 Implement COMMIT_ONE_PHASE: buffered messages into destinations in send order with `broker_seq` and duplicate detection, then recorded acks, then the reply
- [x] 3.2 Implement ROLLBACK: discard buffered messages and recorded acks; keep consumed messages inflight to their open consumer; return messages of closed consumers to pending at the original position with `redeliveryCounter + 1`
- [x] 3.3 Roll back every open transaction when its connection closes or drops
- [ ] 3.4 Refuse `XATransactionId`, PREPARE, COMMIT_TWO_PHASE and RECOVER with `javax.jms.JMSException: XA transactions not supported`

## 4. Broker semantics tests (no network)

- [ ] 4.1 Invisibility before commit, commit order relative to non-transacted sends, commit across two destinations
- [ ] 4.2 Deferred acks, window freed before commit, consumed counter after commit
- [ ] 4.3 Rollback of sends (memory released) and of receives (no duplicates, counter + 1), rollback then consumer close
- [ ] 4.4 Connection drop mid-transaction, XA refusal, unknown transaction errors

## 5. Verification

- [x] 5.1 Java integration test: transacted session with commit and rollback, on both profiles (`amq5`, `amq6`)
- [x] 5.2 Java integration test: transacted consumer with `maximumRedeliveries` exceeded ends in `ActiveMQ.DLQ`
- [x] 5.3 Java acceptance scenarios 1 and 3 (and scenario 2 if `add-message-selectors` is already applied) still pass, and the `add-queue-messaging` integration tests still pass
