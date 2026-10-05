## 1. Configuration and codec

- [x] 1.1 Add `topic_max_pending_per_consumer` (default 10000, 0 = no limit) to the configuration model with validation naming the key and exit code 2
- [ ] 1.2 Decode `RemoveSubscriptionInfo` (type 9) and the `ConsumerInfo` fields `subscriptionName` and `noLocal` for every supported version, with round-trip tests

## 2. Topic subscriptions

- [x] 2.1 Implement the topic destination with its subscription list and per-topic publish sequence
- [x] 2.2 Implement subscription creation from `ConsumerInfo` (start at the next published message) and removal on `RemoveInfo`, session close and connection drop, discarding its pending and inflight messages
- [x] 2.3 Implement publish fan-out with a shared `Arc<StoredMessage>`, discard with no subscribers, `Response` and `ProducerAck` as for queues
- [x] 2.4 Reuse the queue dispatch path per subscription: prefetch window, `MessagePull`, `MessageDispatch`
- [x] 2.5 Apply every ack type per subscription, including POISON to `ActiveMQ.DLQ` and EXPIRED deletion
- [ ] 2.6 Implement evict-oldest on pending lists with the `discarded` counter and debug logging
- [x] 2.7 Implement `noLocal` filtering by the producer's connection
- [x] 2.8 Refuse durable subscriptions (`ConsumerInfo` with `subscriptionName`, `RemoveSubscriptionInfo`) with `javax.jms.JMSException`
- [x] 2.9 Serve temporary topics with the ownership, deletion and missing-destination rules of temporary destinations
- [x] 2.10 Maintain topic statistics (consumers, producers, published, discarded), excluding advisory topics

## 3. Broker semantics tests (no network)

- [ ] 3.1 Fan-out to N subscribers in publish order; late subscriber gets only new messages; no subscribers discards
- [ ] 3.2 Shared memory accounting and release when the last subscription acks or ends
- [ ] 3.3 Eviction with a slow subscriber while a fast one receives everything; limit disabled with 0
- [ ] 3.4 `noLocal`, durable refusal, temporary topic ownership and deletion

## 4. Verification

- [x] 4.1 Java integration test: topic with 3 subscribers each receiving every message in order
- [x] 4.2 Java integration test: request/reply over a `TemporaryTopic`
- [x] 4.3 Java acceptance scenarios 1 and 3 (and scenario 2 if `add-message-selectors` is already applied) still pass with both profiles (`amq5`, `amq6`), and the `add-queue-messaging` integration tests still pass
