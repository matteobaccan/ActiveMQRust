## 1. Broker

- [ ] 1.1 Durable subscription type on topics: key `(clientId, subscriptionName)`, definition (selector, no-local), active flag, offline since
- [ ] 1.2 Detach on consumer close or connection drop: inflight back to pending in order, redelivered; keep receiving publications
- [ ] 1.3 Reattach, replace on changed definition, refuse a second active consumer
- [ ] 1.4 Unsubscribe: offline removal with memory release, in-use and unknown errors
- [ ] 1.5 No eviction for durable subscriptions; `durable_offline_timeout_secs` in housekeeping
- [ ] 1.6 Client ID uniqueness on `ConnectionInfo`, released on close

## 2. Protocol

- [ ] 2.1 `ConsumerInfo` with `subscriptionName`: create or attach; refuse without client ID; refuse shared subscriptions
- [ ] 2.2 `RemoveSubscriptionInfo` handling with ActiveMQ exception types and texts

## 3. Configuration and console

- [ ] 3.1 `broker.durable_offline_timeout_secs` with validation and template entry
- [ ] 3.2 Topic pages and `/api/topics`: durable subscriptions list
- [ ] 3.3 "Delete subscription" for offline subscriptions (confirmation, `POST`, origin check, read-only, log)

## 4. Tests

- [ ] 4.1 Rust tests for every scenario of the spec
- [ ] 4.2 Java integration tests with clients 5.19.11 and 6.3.2: offline accumulation, redelivery, selector change, unsubscribe, duplicate client ID
- [ ] 4.3 Jakarta TCK: re-run the durable subscription tests and update the README results

## 5. Documentation

- [ ] 5.1 README: durable subscriptions in RAM, lost on restart, offline timeout, unique client IDs, shared subscriptions refused
- [ ] 5.2 CHANGELOG entry under Unreleased
- [ ] 5.3 `cargo fmt` and the test suite
