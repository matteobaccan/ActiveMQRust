## Why

ActiveMQRust aims to be **compatible** with ActiveMQ Classic, **use less RAM** and be **faster** than ActiveMQ for in-RAM message processing without storage. JMS selectors (`messageSelector`) are widely used by existing Java applications: consumers that share one queue but each take a subset of it (for example by `JMSCorrelationID` in request/reply), and topic subscribers that only want some events. Without selectors these applications cannot switch from ActiveMQ by changing only host and port. Selectors also expose a known ActiveMQ weakness: with `maxPageSize`, selective consumers on long queues stall. Implementing them with a per-consumer cursor and no page limit makes ActiveMQRust both compatible and faster in exactly this case, and acceptance scenario 2 becomes the gate of this change.

## What Changes

- New selector engine (`src/selector/`): SQL-92 JMS selector lexer, recursive-descent parser and three-valued evaluator, with ActiveMQ semantics. A selector is compiled once per consumer into an immutable shared AST.
- Full grammar: string, integer (decimal, `L` suffix, hexadecimal `0x`, octal), decimal and exponential literals, `TRUE`/`FALSE`; `AND`/`OR`/`NOT`; comparison and arithmetic operators with numeric promotion; `[NOT] BETWEEN`, `[NOT] IN`, `[NOT] LIKE ... [ESCAPE]`, `IS [NOT] NULL`; parentheses and JMS precedence.
- JMS header identifiers (`JMSDeliveryMode`, `JMSPriority`, `JMSMessageID`, `JMSTimestamp`, `JMSCorrelationID`, `JMSType`, `JMSXGroupID`, `JMSXGroupSeq`, `JMSXDeliveryCount`) and application properties decoded lazily from `marshalledProperties` (OpenWire primitive map), cached once per message.
- Selective dispatch on queues: each consumer gets the first FIFO message that matches its selector; non-matching messages stay in the queue and never block the following ones. A per-consumer cursor gives O(1) amortised cost per message and consumer. **No page limit** (deliberate difference from ActiveMQ's `maxPageSize`).
- Topics: the selector is applied at publish time, so a message enters only the pending lists of matching subscribers.
- `QueueBrowser` with a selector returns only matching messages, in FIFO order.
- Errors: syntax errors and XPath/XQuery selectors are rejected with `javax.jms.InvalidSelectorException` (with position and reason); runtime evaluation errors make the message UNKNOWN for that consumer.
- Optimisations: `LIKE` compiled into a dedicated matcher, `IN` with many elements uses a hash set, header-only selectors never decode properties.
- Each consumer's selector is exposed in the consumer snapshot and shown in the admin queue detail.

## Capabilities

### New Capabilities

- `message-selectors`: selector compilation and grammar, identifier values, three-valued logic and type rules, selective dispatch on queues and topics, browser filtering, lazy property decoding, performance rules, errors, admin visibility.

### Modified Capabilities

None.

## Impact

- New code: `src/selector/` (`mod.rs`, `lexer.rs`, `parser.rs`, `eval.rs`); primitive-map property decoder in `src/openwire/message_body.rs` (shared with the admin); selector handling in `src/broker/subscription.rs` and `src/broker/destination.rs`; `ConsumerInfo.selector` handling in `src/connection.rs`.
- No new crates: the hash set and the cached property map use the Rust standard library (`HashSet`, `OnceLock`).
- Depends on `add-queue-messaging` (queues, dispatch, browser, redelivery at the original position). The topic part depends on `add-topic-messaging`. The admin display of selectors is rendered by `add-admin-console`.
- Acceptance gate: scenario 2 of the Java acceptance program (`java-acceptance-suite`) passes against `mqrust.exe`, while scenarios 1 and 3 keep passing.
