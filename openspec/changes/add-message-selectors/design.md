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
The whole engine lives in one module, `src/selector.rs`: a lexer that produces tokens with their character offsets (token rules of ActiveMQ's `SelectorParser.jj`), a parser that builds an AST with one function per precedence level of the same grammar (so `NOT` is a unary operator that binds tighter than comparisons), the compile-time checks of ActiveMQ (`checkLessThanOperand`, `checkEqualOperand`, `asBooleanExpression`) and the evaluator. Header identifiers are resolved to a `Header` enum at compile time, so evaluation never matches identifier strings.
- *Alternatives:* a parser generator (`pest`, `lalrpop`) adds a build dependency and makes ActiveMQ-style error messages harder to control; porting ActiveMQ's JavaCC grammar literally brings Java idioms and is not smaller. A hand-written parser for this small grammar is about the same size and gives precise positions.

### D2. Compile once, share an immutable AST
The selector is compiled when `ConsumerInfo` arrives; the consumer holds `Arc<Selector>`. Type checks of literals and operands happen at compile time (there is no constant folding: ActiveMQ treats `-1` as a negation, not a literal, and its checks depend on that) (for example a string literal used with `<` is rejected, as ActiveMQ's `ComparisonExpression.checkLessThanOperand` does).
- *Alternatives:* interpreting the source string per message (wasteful); compiling to closures (`Box<dyn Fn>`), which is slightly faster but harder to test and debug. An enum AST with a tight `match` evaluator is fast enough and can be revisited in `optimize-broker-performance`.

### D3. Three-valued evaluation with Java-typed values
Evaluation returns `Result<SVal, EvalError>`. `SVal` keeps the Java class ActiveMQ works with: `Null | Bool | Byte | Short | Int | Long | BigInt | Float | Double | Char | Str | Opaque` (`BigInt` is a decimal literal beyond the `long` range, `Opaque` a byte array, map or list property). `SVal::Null` is UNKNOWN for boolean results. The class matters because ActiveMQ's `ComparisonExpression.compare` converts only some pairs (a narrower number on the left is widened to the right operand's class, `Short` vs `Byte` is FALSE, `Double` vs `Float` raises), and `ArithmeticExpression` computes `int`, `long` or `double` by operand classes, with `/` and `%` always in `double` (so division by zero gives Infinity or NaN). `EvalError` stands for the cases where ActiveMQ raises an exception (for example a number plus a boolean) and for undecodable properties: it aborts the whole evaluation and the message is not selected for that consumer, never a panic.
- *Alternatives:* collapsing integers to `i64` and floats to `f64` (simpler, but gives different results from ActiveMQ for `300 = shortProp`, `intProp = floatProp`, `1 / 2` and overflow); treating exceptions as UNKNOWN (wrong under `NOT`, which would select the message).

### D4. Lazy, cached property decoding
The metadata shared by every copy of a stored message (`Meta` in `src/broker/entry.rs`) holds `props: OnceLock<Props>`, where `Props` is absent, a decoded `Arc<PrimitiveMap>` or undecodable. The evaluator asks a `MessageView` (implemented by `Entry`) for a header (`Header` enum, served from decoded message fields) or a property, which triggers `get_or_init`; an undecodable map is logged once and makes property lookups return `EvalError`. The decoder for the OpenWire primitive map lives in `src/openwire/props.rs` and is shared with the admin console.
- *Alternatives:* decoding properties on arrival (cost paid by every message even without selectors, against §14.1); decoding per evaluation without caching (repeated work with several selective consumers).

### D5. Per-consumer cursor over the `pending` BTreeMap
Each selective queue consumer stores `cursor: broker_seq`, the last sequence it examined. Dispatch for that consumer uses `pending.range((Excluded(cursor), Unbounded))` and stops at the first match or at the end, advancing the cursor over every examined message. When a message is reinserted at `broker_seq = s` (rule 4 of queue FIFO), every cursor `>= s` is set to `s - 1`. Consumers without a selector keep taking from the head and need no cursor. A matched message that cannot be delivered because prefetch is full is not skipped: the cursor stops before it.
- *Alternatives:* ActiveMQ-style paging with `maxPageSize` (simple but stalls selective consumers when the first page holds no match); rescanning from the head at every dispatch (O(n) per message, quadratic on long queues); per-selector secondary indexes (complex, memory-hungry, only helps equality selectors).

### D6. Dispatch loop with selectors
The queue dispatch loop iterates consumers in round-robin order; for each consumer with free prefetch it finds the next candidate (head for plain consumers, cursor scan for selective ones) and dispatches it. A message is offered to consumers in round-robin order and goes to the first one with free prefetch and a matching selector. Messages that no consumer selects stay in `pending`.
- *Alternatives:* iterating messages and searching a consumer for each (re-evaluates every unmatched message on every pass, the head-of-line pattern this change avoids).

### D7. Topics filter at publish time
On publish, each subscription's selector is evaluated once and the shared `Entry` (an `Arc<Message>` plus its shared metadata) is pushed only into matching pending lists. A message matching nobody is dropped immediately.
- *Alternatives:* filtering at dispatch time from per-subscriber lists (stores messages that will never be delivered, wasting RAM, against the goal).

### D8. LIKE and IN compilation
`LIKE` patterns are compiled to a sequence of literal characters and wildcards (escape rules of `LikeExpression`: the escape character makes a following `%`, `_` or itself literal, and is literal otherwise), then classified: exact, prefix (`abc%`), suffix (`%abc`), contains (`%abc%`), match-all (`%`), or generic. The generic matcher is a small backtracking matcher over chars. A unit test checks the compiled matchers and the generic matcher against a recursive reference matcher on 20,000 random patterns. `IN` lists above 8 elements are stored in a `HashSet<String>`; smaller lists use a linear scan, which is faster for few elements.
- *Alternatives:* translating `LIKE` to the `regex` crate (adds a dependency and binary size for no gain on this simple pattern language).

### D9. Error messages
Lexer and parser errors carry the token text and its zero-based character offset and are returned as `InvalidSelectorException` with the message format `Unexpected token '<tok>' at column <n> in selector: <selector>` (compile-time type errors give their reason instead of the token). `XPATH` / `XQUERY` anywhere in the selector is rejected with exactly `XPath selectors are not supported` (`SelectorError::exception_message`).
- *Alternatives:* copying ActiveMQ's JavaCC `ParseException` text verbatim (tied to generated-parser internals; the acceptance test only requires the exception class).

### D10. Conformance with ActiveMQ's engine
ActiveMQ's selector classes are in `activemq-client`, so they can be run in-process without a broker. A table of more than 500 selectors (`tests/data/selector_conformance.tsv`) records the result of ActiveMQ's engine (TRUE, FALSE, NULL, parse error or exception; identical for 5.18.7 and 6.3.2) on one message with every property type and header; `tests/selector_semantics.rs` evaluates the same selectors on the same message through `Entry` and must match every line. The Java integration scenarios `selectors` and `selectorParity` assert the expected messages computed the same way.
- *Alternatives:* comparing only against a running ActiveMQ broker (slower, needs an installation, and cannot see UNKNOWN versus FALSE).

## Risks / Trade-offs

- [Subtle differences between our evaluator and ActiveMQ's] → The conformance table of D10 and the Java selector scenarios pin ActiveMQ's results; every divergence found becomes a table line.
- [Selectors that never match make messages pile up] → Messages stay visible in the admin with each consumer's selector shown; the memory limit (`max_memory_mb`) and expiration (`add-message-expiration`) bound the growth.
- [Cursor bookkeeping bugs on redelivery cause skipped messages] → Semantics tests for reinsertion before, at and after every cursor position, with several selective consumers.
- [Long scans for a consumer whose selector rarely matches] → The cursor makes each message examined once per consumer; a benchmark with 10 selective consumers on 100,000 messages checks the cost.
- [Malformed `marshalledProperties` from a buggy client] → Decoding errors make the selector UNKNOWN for that message, logged once, never a crash.
- [Known remaining differences] → ActiveMQ's function-call extension (`REGEX(...)`, `INLIST(...)`) and XQuery are rejected; decimal literals beyond 128 bits are rejected (ActiveMQ accepts any length); the text of an XA `JMSXProducerTXID` approximates `XATransactionId.toString()`.

## Migration Plan

No data migration: the broker keeps no state across restarts. Applications that already use selectors with ActiveMQ work unchanged once this change is deployed. Rollback means deploying the previous `mqrust.exe`, where selectors are ignored, or pointing clients back to ActiveMQ.

## Open Questions

All resolved by reading and running ActiveMQ's own classes (activemq-client 5.18.7 and 6.3.2 behave identically):

- *JMS identifiers* (resolved): `PropertyExpression` recognises 19 names: `JMSDestination`, `JMSReplyTo`, `JMSType`, `JMSDeliveryMode`, `JMSPriority`, `JMSMessageID`, `JMSTimestamp`, `JMSCorrelationID`, `JMSExpiration`, `JMSRedelivered`, `JMSXDeliveryCount`, `JMSXGroupID`, `JMSXUserID`, `JMSXGroupSeq`, `JMSXProducerTXID`, `JMSActiveMQBrokerInTime`, `JMSActiveMQBrokerOutTime`, `JMSActiveMQBrokerPath`, `JMSXGroupFirstForConsumer`. Destinations and transaction ids are strings (`queue://Q`, `TX:<conn>:<n>`), `JMSXGroupSeq` defaults to 0, `JMSXDeliveryCount` is `redeliveryCounter + 1`, `JMSXUserID` falls back to the property of the same name, and the broker path is the string `null` when absent. The table with types is the doc comment of `Header` in `src/selector.rs`.
- *Primitive map encoding* (resolved): type codes and encodings match `MarshallingSupport`; golden bytes for every type are tested in `src/openwire/props.rs`.
- *Incompatible types and runtime ordering* (resolved): `=` between incompatible non-null values is FALSE (not UNKNOWN); `ComparisonExpression.compare` uses `compareTo` for values of the same class, so two string or two boolean property values can be ordered at run time, while string/boolean literals are rejected at compile time; numeric conversions depend on the left operand's class (see D3).
- *Literals* (resolved): decimal literals take an optional `L`; hexadecimal (`0x`) and octal (leading `0`) take no suffix and must fit in a `long`; a decimal literal is an `int` when it fits, else a `long`, else a `BigDecimal`; floating literals are `double` and take no `f`/`d` suffix; identifiers are ASCII only; `/* */` comments are skipped and `--` is not a comment.
- *CHAR properties* (resolved): they are `java.lang.Character`, comparable only with other chars: `c = 'x'` and `c LIKE 'x'` are FALSE, `c IN ('x')` is UNKNOWN, and `'a' + c` concatenates the character.
