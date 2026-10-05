## 1. Configuration

- [x] 1.1 Add the `[expiry]` section to the configuration model with defaults `check_interval_ms = 1000`, `use_broker_clock = false`, `ttl_ceiling_ms = 0`, `default_ttl_ms = 0`
- [ ] 1.2 Validate the section (non-negative values, `check_interval_ms >= 1`, field-named errors, exit code 2) and add it to `init-config` and `mqrust.example.toml`
- [x] 1.3 Unit tests for defaults, partial section and invalid values

## 2. Expiration on arrival

- [x] 2.1 Apply `use_broker_clock`, `default_ttl_ms` and `ttl_ceiling_ms` in that order to producer messages, writing the new `timestamp`/`expiration` into the stored message
- [x] 2.2 Delete messages already expired on arrival, still answering `Response` to synchronous sends
- [x] 2.3 Unit tests: client clock 1 hour ahead and behind, default TTL, ceiling on explicit and default TTL, no change with default settings

## 3. Expiry index and deletion

- [x] 3.1 Add `expiry_index: BTreeSet<(expiration, broker_seq)>` to queues and topic subscription pending lists, kept consistent on enqueue, dispatch, reinsertion and deletion
- [x] 3.2 Implement the single deletion path: remove from `pending`/index/`inflight`, release accounted memory, increment `expired` and the per-minute counter
- [x] 3.3 Track the set of destinations with a non-empty expiry index
- [x] 3.4 Ensure deletion never moves a message to `ActiveMQ.DLQ` and applies to `ActiveMQ.DLQ` itself

## 4. Sweeper

- [x] 4.1 Implement the periodic sweeper task with `check_interval_ms`, visiting only destinations with expiring messages
- [x] 4.2 Limit work to 10,000 deletions per round and destination, releasing the lock between destinations
- [x] 4.3 Semantics tests: 100 ms TTL with no consumers removed after 100 ms + interval with memory decrease; 25,000 simultaneous expiries handled in bounded rounds; idle when nothing expires

## 5. Delivery-time checks and acks

- [x] 5.1 Check expiration in queue and topic dispatch, skipping and deleting expired candidates
- [x] 5.2 Check expiration in `MessagePull` handling, keeping the normal pull timeout
- [x] 5.3 Skip and delete expired messages in `QueueBrowser` enumeration
- [x] 5.4 Check expiration on reinsertion after consumer close, connection drop and transaction rollback (rollback with `add-local-transactions`)
- [x] 5.5 Handle `EXPIRED` ack (type 6): remove from `inflight`, delete, free prefetch, resume dispatch
- [x] 5.6 Semantics tests: alternating expired/valid messages delivered in FIFO order; expiry during rollback not redelivered; `EXPIRED` ack; in-flight message not revoked; expired message in a slow topic subscriber's list removed

## 6. Statistics, admin data and logging

- [x] 6.1 Expose the `expired` counter, the number of messages with an expiration and the next expiration in destination snapshots
- [x] 6.2 Provide the expired marking for queue contents and the readable expiration with remaining time or "expired" for message detail (rendered by `add-admin-console`, also in the JSON API)
- [x] 6.3 Log each expired message at debug level and at most one info summary per minute per destination

## 7. Verification

- [x] 7.1 All unit and semantics tests pass (`cargo test`), including persistent and non-persistent expiry with `ActiveMQ.DLQ` empty, and a POISON-acked message with TTL deleted from the DLQ
- [x] 7.2 Java integration tests (§11 item 7): `setTimeToLive()` with expiry before consumption, `JMSExpiration` checked on the consumer side, `ttl_ceiling_ms`, `default_ttl_ms` and `use_broker_clock`, with the `amq5` and `amq6` profiles
- [x] 7.3 Acceptance scenarios 1, 2 and 3 still pass against `mqrust.exe`
- [x] 7.4 Earlier Java integration tests still pass
- [ ] 7.5 Benchmark: 1,000,000 messages with random TTLs; record that the sweeper does not measurably increase p99 dispatch latency
