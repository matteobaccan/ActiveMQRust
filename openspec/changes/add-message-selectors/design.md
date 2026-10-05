## Context

After `add-queue-messaging`, ActiveMQRust delivers queue messages in strict FIFO order, with prefetch, acks, browsers and redelivery at the original position. `ConsumerInfo.selector` is still ignored. Many Java applications rely on JMS selectors, most often on `JMSCorrelationID` in request/reply and on application properties to split a queue or filter topic events. The project goals are compatibility with ActiveMQ Classic, lower RAM and higher speed; the hot path never decodes bodies or properties (§14.1 of the design spec). ActiveMQ's own selector implementation lives in `org.apache.activemq.filter` and `org.apache.activemq.selector.SelectorParser`; its queue cursors page at most `maxPageSize` messages (default 200), which is a known cause of stalled selective consumers. Acceptance scenario 2 of the Java program defines the minimum observable behaviour.

## Goals / Non-Goals

**Goals:**
- Accept every selector that ActiveMQ accepts in SQL-92 syntax, reject the same invalid ones, and select the same messages.
- Keep FIFO: each consumer receives an ordered subsequence; non-matching messages never block others.
- Zero cost for consumers without a selector; header-only selectors never decode properties.
- Scale to long queues with many selective consumers without a page limit.

**Non-Goals:**
- XPath/XQuery selectors (rejected).
- Selector-based routing beyond consumer filtering (composite or virtual destinations).
- Indexing messages by property values across consumers.
- Durable subscriptions (excluded from the product).

## Decisions

### D1. Hand-written lexer and recursive-descent parser
`src/selector/lexer.rs` produces tokens with their character offsets; `parser.rs` builds an AST with one function per precedence level; `eval.rs` evaluates it.
- *Alternatives:* a parser generator (`pest`, `lalrpop`) adds a build dependency and makes ActiveMQ-style error messages harder to control; porting ActiveMQ's JavaCC grammar literally brings Java idioms and is not smaller. A hand-written parser for this small grammar is about the same size and gives precise positions.

### D2. Compile once, share an immutable AST
The selector is compiled when `ConsumerInfo` arrives; the consumer holds `Arc<Selector>`. Constant folding and type checks of literals happen at compile time (for example a string literal used with `<` is rejected, as ActiveMQ's `ComparisonExpression.checkLessThanOperand` does).
- *Alternatives:* interpreting the source string per message (wasteful); compiling to closures (`Box<dyn Fn>`), which is slightly faster but harder to test and debug. An enum AST with a tight `match` evaluator is fast enough and can be revisited in `optimize-broker-performance`.

### D3. Three-valued evaluation with a `Value` enum
Evaluation returns `Value` = `Null | Bool | Long | Double | String | Unsupported` (byte arrays, maps, lists). Integer-family values are widened to `i64`, `float` to `f64`; mixed arithmetic promotes to `f64`. Boolean results use `Option<bool>` where `None` is UNKNOWN. Division by zero and any other runtime error yield `Null`/UNKNOWN, never a panic.
- *Alternatives:* separate integer types per width (more code, no observable difference in selector results); `f64` for everything (loses precision on large longs such as `JMSTimestamp`).

### D4. Lazy, cached property decoding
`StoredMessage` gets `properties: OnceLock<Arc<PropertyMap>>`. The evaluator asks a `MessageView` for an identifier: `JMS*` names are served from decoded header fields; anything else triggers `get_or_init` on the property map. The decoder for the OpenWire primitive map lives in `src/openwire/message_body.rs` and is shared with the admin console.
- *Alternatives:* decoding properties on arrival (cost paid by every message even without selectors, against §14.1); decoding per evaluation without caching (repeated work with several selective consumers).

### D5. Per-consumer cursor over the `pending` BTreeMap
Each selective queue consumer stores `cursor: broker_seq`, the last sequence it examined. Dispatch for that consumer uses `pending.range((Excluded(cursor), Unbounded))` and stops at the first match or at the end, advancing the cursor over every examined message. When a message is reinserted at `broker_seq = s` (rule 4 of queue FIFO), every cursor `>= s` is set to `s - 1`. Consumers without a selector keep taking from the head and need no cursor. A matched message that cannot be delivered because prefetch is full is not skipped: the cursor stops before it.
- *Alternatives:* ActiveMQ-style paging with `maxPageSize` (simple but stalls selective consumers when the first page holds no match); rescanning from the head at every dispatch (O(n) per message, quadratic on long queues); per-selector secondary indexes (complex, memory-hungry, only helps equality selectors).

### D6. Dispatch loop with selectors
The queue dispatch loop iterates consumers in round-robin order; for each consumer with free prefetch it finds the next candidate (head for plain consumers, cursor scan for selective ones) and dispatches it. A message is offered to consumers in round-robin order and goes to the first one with free prefetch and a matching selector. Messages that no consumer selects stay in `pending`.
- *Alternatives:* iterating messages and searching a consumer for each (re-evaluates every unmatched message on every pass, the head-of-line pattern this change avoids).

### D7. Topics filter at publish time
On publish, each subscription's selector is evaluated once and the `Arc<StoredMessage>` is pushed only into matching pending lists. A message matching nobody is dropped immediately.
- *Alternatives:* filtering at dispatch time from per-subscriber lists (stores messages that will never be delivered, wasting RAM, against the goal).

### D8. LIKE and IN compilation
`LIKE` patterns are classified at compile time: exact, prefix (`abc%`), suffix (`%abc`), contains (`%abc%`), or generic. The generic matcher is a small backtracking matcher over chars with `%`, `_` and the escape character. `IN` lists above 8 elements are stored in a `HashSet<Arc<str>>`; smaller lists use a linear scan, which is faster for few elements.
- *Alternatives:* translating `LIKE` to the `regex` crate (adds a dependency and binary size for no gain on this simple pattern language).

### D9. Error messages
Lexer and parser errors carry the token text and its zero-based character offset and are returned as `InvalidSelectorException` with the message format `Unexpected token '<tok>' at column <n> in selector: <selector>`. `XPATH` / `XQUERY` as the first token is detected before parsing and rejected with `XPath selectors are not supported`.
- *Alternatives:* copying ActiveMQ's JavaCC `ParseException` text verbatim (tied to generated-parser internals; the acceptance test only requires the exception class).

## Risks / Trade-offs

- [Subtle differences between our evaluator and ActiveMQ's] → A Java comparison suite runs the same selectors and messages against real ActiveMQ and ActiveMQRust and compares the received messages and order; every divergence becomes a unit test.
- [Selectors that never match make messages pile up] → Messages stay visible in the admin with each consumer's selector shown; the memory limit (`max_memory_mb`) and expiration (`add-message-expiration`) bound the growth.
- [Cursor bookkeeping bugs on redelivery cause skipped messages] → Semantics tests for reinsertion before, at and after every cursor position, with several selective consumers.
- [Long scans for a consumer whose selector rarely matches] → The cursor makes each message examined once per consumer; a benchmark with 10 selective consumers on 100,000 messages checks the cost.
- [Malformed `marshalledProperties` from a buggy client] → Decoding errors make the selector UNKNOWN for that message, logged once, never a crash.

## Migration Plan

No data migration: the broker keeps no state across restarts. Applications that already use selectors with ActiveMQ work unchanged once this change is deployed. Rollback means deploying the previous `mqrust.exe`, where selectors are ignored, or pointing clients back to ActiveMQ.

## Open Questions

- Verify the exact list of `JMS*` identifiers and their values (including `JMSXGroupSeq` default, `JMSXDeliveryCount`, `JMSDeliveryMode` strings, and whether `JMSRedelivered`, `JMSDestination`, `JMSReplyTo`, `JMSExpiration`, `JMSXUserID`, `JMSXProducerTXID` are recognised) against `org.apache.activemq.filter.PropertyExpression` in 5.18.x / 6.x.
- Verify the primitive map type codes and the encoding of `BIG_STRING`, `CHAR`, `MAP` and `LIST` values against `org.apache.activemq.util.MarshallingSupport`.
- Verify how ActiveMQ evaluates `=` between incompatible types (UNKNOWN versus FALSE) and ordering comparisons with string or boolean property values at runtime, and align with it; the design spec states UNKNOWN.
- Verify octal and hexadecimal literal handling and `L` suffix limits in ActiveMQ's `SelectorParser` grammar.
- Verify how ActiveMQ treats `CHAR` properties in comparisons (as a string of one character or as unsupported).
