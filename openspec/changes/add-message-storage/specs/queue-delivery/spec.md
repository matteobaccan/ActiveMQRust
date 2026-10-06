## MODIFIED Requirements

### Requirement: Delivery mode does not change storage
When `storage.enabled` is `false` (the default), every message SHALL be kept in RAM only, whatever its `JMSDeliveryMode`. The `persistent` flag SHALL be preserved and delivered unchanged, but SHALL NOT cause any write to storage. This is a deliberate difference from ActiveMQ, which stores persistent messages on disk: with storage off, persistent and non-persistent messages are both lost on restart. When `storage.enabled` is `true`, persistent messages SHALL be stored as defined by the `message-storage` capability, and non-persistent messages SHALL still be kept in RAM only.

#### Scenario: Persistent message delivered
- **WHEN** storage is off and a producer sends a message with `DeliveryMode.PERSISTENT`
- **THEN** the consumer receives it with `getJMSDeliveryMode()` equal to `PERSISTENT`, and no file is created by the broker

#### Scenario: Persistent message with storage on
- **WHEN** storage is on and a producer sends a persistent message synchronously, and the broker is then restarted
- **THEN** a consumer receives the message after the restart, with `getJMSDeliveryMode()` equal to `PERSISTENT`
