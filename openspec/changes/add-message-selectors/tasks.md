## 1. Research against the Java sources

- [x] 1.1 Extract the recognised `JMS*` identifiers and their values from `org.apache.activemq.filter.PropertyExpression` (5.18.x and 6.x) and record them in a code comment table (doc comment of `Header` in `src/selector.rs`)
- [x] 1.2 Confirm the primitive map type codes and value encodings in `MarshallingSupport` (including `BIG_STRING`, `CHAR`, `MAP`, `LIST`)
- [x] 1.3 Confirm ActiveMQ's type rules for incompatible comparisons, ordering comparisons on strings/booleans, and literal formats in `SelectorParser` (answers recorded in the design's resolved open questions)

## 2. Lexer and parser

- [x] 2.1 Implement the lexer in `src/selector.rs`: string literals with `''`, decimal/`L`/hex/octal integers, decimals and exponents, `TRUE`/`FALSE`, case-insensitive keywords, case-sensitive identifiers, operators, token offsets
- [x] 2.2 Implement the parser in `src/selector.rs`: recursive descent with ActiveMQ's precedence (unary `NOT` binds tighter than comparisons), `BETWEEN`, `IN`, `LIKE ... ESCAPE`, `IS [NOT] NULL`, parentheses, compile-time type checks of ActiveMQ
- [x] 2.3 Implement `InvalidSelectorException` errors with token, column and selector; reject `XPATH` / `XQUERY` with `XPath selectors are not supported`
- [x] 2.4 Treat empty and whitespace-only selectors as no selector
- [x] 2.5 Unit tests over the whole grammar, precedence rules and error positions

## 3. Evaluator

- [x] 3.1 Implement the `SVal` value model (Java classes) and ActiveMQ's numeric conversions in `src/selector.rs`
- [x] 3.2 Implement three-valued `AND`/`OR`/`NOT`, comparisons, arithmetic, NULL propagation and runtime errors
- [x] 3.3 Compile `LIKE` into exact/prefix/suffix/contains/match-all/generic matchers and large `IN` lists into a hash set
- [x] 3.4 Unit tests: three-valued truth tables, `LIKE` with `%`, `_` and `ESCAPE`, `BETWEEN` and `IN` with NULL, incompatible types, division by zero, matcher equivalence with the generic matcher

## 4. Message view and lazy properties

- [x] 4.1 Implement the primitive map decoder in `src/openwire/props.rs` with all type codes
- [x] 4.2 Add a once-per-message property cache (`OnceLock` in the shared metadata of `src/broker/entry.rs`) and a `MessageView` that serves `JMS*` headers from decoded fields and properties lazily
- [x] 4.3 Handle undecodable properties as UNKNOWN with a single warning log per message
- [x] 4.4 Unit tests: header-only selectors do not decode properties; properties are decoded once for many consumers; every property type round-trips from golden bytes

## 5. Broker integration

- [x] 5.1 Compile `ConsumerInfo.selector` on consumer creation and reply with `ExceptionResponse` (`javax.jms.InvalidSelectorException`) without registering the consumer on error
- [x] 5.2 Implement the per-consumer cursor and selective queue dispatch with round-robin among matching consumers
- [x] 5.3 Move cursors back when a message is reinserted at its original position (consumer close, connection drop, rollback)
- [x] 5.4 Apply selectors to `QueueBrowser` consumers (matching messages only, FIFO, end-of-browse marker)
- [x] 5.5 Apply selectors at publish time on topics (depends on `add-topic-messaging`)
- [x] 5.6 Expose the selector in the consumer snapshot and show it in the admin queue detail and `/api/queues/{name}` (rendered by `add-admin-console`)
- [x] 5.7 Semantics tests: disjoint selectors each get their full FIFO subsequence; unmatched messages stay without blocking; redelivered message re-examined; matching message behind 100,000 non-matching ones is delivered; topic filtering; filtered browse

## 6. Verification

- [x] 6.1 All selector unit and semantics tests pass (`cargo test`)
- [x] 6.2 Acceptance scenario 2 passes against `mqrust.exe` with the `amq5` and `amq6` profiles
- [x] 6.3 Acceptance scenarios 1 and 3 still pass
- [x] 6.4 Add the Java selector comparison suite in `tests/java-it/` (`selectors` and `selectorParity`) asserting the received messages and order computed with ActiveMQ's selector engine, passing against ActiveMQRust with `amq5` and `amq6`; check a conformance table of ActiveMQ's results in `cargo test`
- [x] 6.5 Earlier Java integration tests (§11 item 4: FIFO 10,000 messages, message types, request/reply, browser, topics, redelivery) still pass
- [ ] 6.6 Add the `criterion` benchmark: dispatch with 10 selective consumers on a queue of 100,000 messages, and record the result (benchmark `selector/dispatch_10_selective_consumers_100k` added; the result is recorded later)
