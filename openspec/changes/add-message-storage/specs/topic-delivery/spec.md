## ADDED Requirements

### Requirement: Durable subscriptions survive restarts with storage
When `storage.enabled` is `true`, durable subscriptions (defined in "Durable subscriptions in memory") and their persistent messages SHALL survive a broker restart, as defined by the `message-storage` capability; their non-persistent messages SHALL still be lost on restart. When `storage.enabled` is `false`, durable subscriptions SHALL stay in RAM only.

#### Scenario: Restart with storage on
- **WHEN** storage is on, durable subscription `app1:sub1` is offline with 20 persistent pending messages, and the broker is restarted
- **THEN** `app1` reattaching to `sub1` receives the 20 messages in publish order

#### Scenario: Restart with storage off
- **WHEN** storage is off and the broker is restarted while `app1:sub1` is offline
- **THEN** the subscription no longer exists
