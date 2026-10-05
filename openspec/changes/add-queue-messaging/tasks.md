## 1. OpenWire codec for messaging

- [ ] 1.1 Extract from the Java marshallers (v6–v12) the per-version field lists of the message commands 23–29, `MessageDispatch`, `MessageAck`, `MessagePull`, `ProducerAck`, `DestinationInfo`, `ConsumerInfo`, `ProducerInfo`, the destination types 100–103 and `MessageId`
- [ ] 1.2 Implement the destination types 100–103 with physical names, and `MessageId` (110) with `textView` from version 10
- [ ] 1.3 Implement decode and encode of the message commands 23–29 with headers decoded and `content` / `marshalledProperties` kept as opaque `Bytes`
- [ ] 1.4 Implement `MessageDispatch` (encode, including null message), `MessageAck`, `MessagePull`, `ProducerAck` and `DestinationInfo`, and complete `ConsumerInfo` / `ProducerInfo` decoding
- [ ] 1.5 Capture golden vectors from a real Java client for every new command and add round-trip tests for each supported version

## 2. Message identity

- [ ] 2.1 Implement the ID text representations (`ConnectionId` … `MessageId`) matching ActiveMQ's `toString()`
- [ ] 2.2 Implement the broker-wide monotonic `brokerSequenceId` counter
- [ ] 2.3 Implement the `IdGenerator`-equivalent generator for broker-created IDs and use it for `BrokerId`
- [ ] 2.4 Implement the per-destination, per-producer duplicate window (last 1024 `producerSequenceId`) and its release on connection close
- [ ] 2.5 Unit tests: text forms, cross-version `MessageId` encoding, ID uniqueness, duplicate discard with `Response`

## 3. Memory management

- [ ] 3.1 Add `max_memory_mb` (default 0) and `auto_delete_empty_after_secs` (default 0) to the configuration model, with validation errors naming the key and exit code 2
- [ ] 3.2 Implement `StoredMessage` with accounted size (body + properties + fixed overhead) and release on last drop
- [ ] 3.3 Implement the limited state (enter above the limit, leave below 90%), the dropped-message counter and the warning log lines
- [ ] 3.4 Unit tests: accounting up and down, hysteresis, DLQ moves and returns not refused, no limit by default

## 4. Destination registry

- [ ] 4.1 Implement the partitioned registry keyed by destination type and name, with automatic creation from `ProducerInfo`, `ConsumerInfo` and message sends
- [ ] 4.2 Reject wildcard (`*`, `>`) and composite (`,`) destinations with `InvalidDestinationException`, keeping the advisory consumer exemption
- [ ] 4.3 Implement `DestinationInfo` ADD/REMOVE: temporary destinations with owner, removal refused with active consumers, removal of non-temporary destinations refused
- [ ] 4.4 Implement temporary destination ownership checks on `ConsumerInfo`, deletion on owner close, and send to a missing temporary destination (sync error, async discard)
- [ ] 4.5 Implement the idle-destination check for `auto_delete_empty_after_secs` (at least once per second)
- [ ] 4.6 Create `ActiveMQ.DLQ` on demand as an ordinary queue

## 5. Queue delivery

- [ ] 5.1 Implement producer and consumer registration and removal, with per-destination counts
- [ ] 5.2 Implement enqueue with `broker_seq`, `pending` as `BTreeMap`, and synchronous `Response` / `ExceptionResponse`
- [ ] 5.3 Implement `ProducerAck` for asynchronous sends of producers with `windowSize > 0`, using the client size formula, also for discarded messages
- [ ] 5.4 Implement the subscription with inflight ordered by dispatch sequence and the prefetch window excluding DELIVERED-acked messages
- [ ] 5.5 Implement FIFO dispatch from the head with round-robin across consumers, ignoring priority, exclusive flag, consumer priority and message groups
- [ ] 5.6 Implement prefetch 0 with `MessagePull` (timeouts `> 0`, `-1`, `0`) and the null `MessageDispatch`
- [ ] 5.7 Implement `QueueBrowser`: snapshot in FIFO order within the prefetch window, null end marker, acks never remove
- [ ] 5.8 Implement every ack type (DELIVERED, POISON, STANDARD, REDELIVERED, INDIVIDUAL, UNMATCHED, EXPIRED) and resume dispatch after acks
- [ ] 5.9 Implement return of unacknowledged messages at the original position with `redeliveryCounter + 1` on consumer close and connection drop
- [ ] 5.10 Implement the POISON move to `ActiveMQ.DLQ` with `dlqDeliveryFailureCause`, original `MessageId` kept, for every delivery mode
- [ ] 5.11 Maintain queue statistics (pending, inflight, consumers, producers, enqueued, consumed, expired) with snapshot reads

## 6. Broker semantics tests (no network)

- [ ] 6.1 FIFO with 1 and N consumers, round-robin distribution, priority not reordering
- [ ] 6.2 Prefetch limit and refill, DELIVERED window extension, prefetch 0 and pull timeouts, browser
- [ ] 6.3 Every ack type, including cumulative ranges after a reinsertion
- [ ] 6.4 Redelivery at the original position on consumer close and connection drop; DLQ on POISON
- [ ] 6.5 Temporary destinations, auto-delete of empty destinations, wildcard and composite rejection
- [ ] 6.6 Duplicates and the memory limit for synchronous and asynchronous sends

## 7. Verification

- [ ] 7.1 Java acceptance scenario 1 passes against `mqrust.exe` with both profiles (`amq5`, `amq6`)
- [ ] 7.2 Java acceptance scenario 3 still passes
- [ ] 7.3 Java integration tests pass: 10,000 messages in order with equal `JMSMessageID`, all 5 message types with properties, request/reply with `TemporaryQueue`, `CLIENT_ACKNOWLEDGE` with recover, `QueueBrowser`, consumer killed with messages in flight redelivered first with `JMSRedelivered=true`
- [ ] 7.4 The idle connection test (over 60 s) still passes
