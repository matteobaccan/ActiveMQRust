## ADDED Requirements

### Requirement: Selector scope and compilation
The broker SHALL accept a JMS selector in `ConsumerInfo.selector` for queue consumers, topic consumers and `QueueBrowser` consumers (`ConsumerInfo.browser=true`), including consumers on temporary destinations. The selector SHALL be compiled exactly once, when the consumer is created, into an immutable AST shared through `Arc<Selector>`; it SHALL NOT be reparsed per message. An empty selector or a selector made only of whitespace SHALL be treated as no selector. The syntax SHALL be the SQL-92 subset defined by JMS 1.1 / 2.0, with the same semantics as ActiveMQ.

#### Scenario: Selector compiled at consumer creation
- **WHEN** a client creates a consumer with selector `color = 'red'`
- **THEN** the broker compiles it once, answers the `ConsumerInfo` with `Response`, and evaluates the compiled AST for every candidate message

#### Scenario: Whitespace-only selector
- **WHEN** a client creates a consumer with selector `"   "`
- **THEN** the consumer behaves exactly like a consumer without a selector and receives every message

### Requirement: Selector literals
The selector language SHALL support these literals:
- strings in single quotes, where `''` inside the string stands for one quote;
- integers in decimal, with an optional `L` or `l` suffix, in hexadecimal with the `0x` prefix, and in octal with a leading `0`;
- decimals and exponential notation (for example `1.5`, `.5`, `1e3`, `2.5E-2`);
- the boolean literals `TRUE` and `FALSE`, case-insensitive.

#### Scenario: Escaped quote
- **WHEN** a consumer has selector `name = 'O''Brien'` and a message has property `name` equal to `O'Brien`
- **THEN** the message is selected

#### Scenario: Integer formats
- **WHEN** a message has int property `n` equal to 26 and consumers have the selectors `n = 26`, `n = 26L`, `n = 0x1A` and `n = 032`
- **THEN** each of the four consumers selects the message

#### Scenario: Exponential literal
- **WHEN** a message has double property `v` equal to 1500.0 and the selector is `v = 1.5e3`
- **THEN** the message is selected

### Requirement: Identifiers and keywords
Identifiers that name application properties SHALL be case-sensitive. Keywords (`AND`, `OR`, `NOT`, `BETWEEN`, `IN`, `LIKE`, `ESCAPE`, `IS`, `NULL`, `TRUE`, `FALSE`) SHALL be case-insensitive. A property that is not present in the message SHALL evaluate to NULL.

#### Scenario: Case-sensitive property names
- **WHEN** a message has property `Color = 'red'` and a consumer has selector `color = 'red'`
- **THEN** the message is not selected, because `color` is NULL

#### Scenario: Case-insensitive keywords
- **WHEN** a consumer has selector `a = 1 and not b is null` and a message has `a = 1` and `b = 2`
- **THEN** the message is selected

### Requirement: JMS header identifiers
The broker SHALL evaluate these header identifiers from message fields that are already decoded, without decoding `marshalledProperties`:
- `JMSDeliveryMode`: the string `'PERSISTENT'` or `'NON_PERSISTENT'`;
- `JMSPriority`: integer 0–9;
- `JMSMessageID`: the text representation of the `MessageId` (`<connectionId>:<sessionId>:<producerId>:<producerSequenceId>`, as ActiveMQ's `MessageId.toString()`);
- `JMSTimestamp`: long, milliseconds;
- `JMSCorrelationID` and `JMSType`: string, or NULL when absent;
- `JMSXGroupID` and `JMSXGroupSeq`: from the message's `groupID` and `groupSequence` fields;
- `JMSXDeliveryCount`: `redeliveryCounter + 1`.

Any other identifier SHALL be looked up in the application properties. The list of recognised `JMS*` identifiers and their values SHALL match `org.apache.activemq.filter.PropertyExpression` in ActiveMQ 5.18.x / 6.x.

#### Scenario: Delivery mode
- **WHEN** a consumer has selector `JMSDeliveryMode = 'PERSISTENT'` and two messages are sent, one persistent and one non-persistent
- **THEN** only the persistent message is selected

#### Scenario: Correlation ID absent
- **WHEN** a message has no correlation ID and the selector is `JMSCorrelationID IS NULL`
- **THEN** the message is selected

#### Scenario: Delivery count after redelivery
- **WHEN** a message with `redeliveryCounter = 1` is evaluated against `JMSXDeliveryCount = 2`
- **THEN** the message is selected

#### Scenario: Header-only selector does not decode properties
- **WHEN** a consumer has selector `JMSPriority > 3` and a matching message arrives
- **THEN** the message is selected and its `marshalledProperties` are not decoded

### Requirement: Logical operators and three-valued logic
The selector language SHALL support `AND`, `OR` and `NOT` with three-valued logic (TRUE, FALSE, UNKNOWN) as defined by the JMS specification:
- `AND`: FALSE if either operand is FALSE; TRUE if both are TRUE; otherwise UNKNOWN;
- `OR`: TRUE if either operand is TRUE; FALSE if both are FALSE; otherwise UNKNOWN;
- `NOT`: TRUE becomes FALSE, FALSE becomes TRUE, UNKNOWN stays UNKNOWN.

A message SHALL be selected only if the whole selector evaluates to TRUE; FALSE and UNKNOWN SHALL both mean "not selected".

#### Scenario: NOT of UNKNOWN
- **WHEN** a message has no property `size` and the selector is `NOT (size > 2)`
- **THEN** the message is not selected

#### Scenario: OR with UNKNOWN
- **WHEN** a message has `color = 'red'` and no property `size`, and the selector is `color = 'red' OR size > 2`
- **THEN** the message is selected

#### Scenario: AND with UNKNOWN
- **WHEN** a message has `color = 'red'` and no property `size`, and the selector is `color = 'red' AND size > 2`
- **THEN** the message is not selected

### Requirement: Comparison and arithmetic operators
The selector language SHALL support the comparison operators `=`, `<>`, `<`, `<=`, `>`, `>=` and the arithmetic operators `+`, `-`, `*`, `/` and unary `-`, with numeric promotion integer → long → double (byte, short and int values count as integer; float counts as double). Strings and booleans SHALL be comparable only with `=` and `<>`. Parentheses SHALL be supported, and operator precedence SHALL follow the JMS specification: unary `+`/`-`, then `*` `/`, then `+` `-`, then comparison operators and `BETWEEN`/`IN`/`LIKE`/`IS NULL`, then `NOT`, then `AND`, then `OR`.

#### Scenario: Numeric promotion
- **WHEN** a message has int property `a = 3` and double property `b = 0.5`, and the selector is `a * b = 1.5`
- **THEN** the message is selected

#### Scenario: Precedence
- **WHEN** a message has `a = 1`, `b = 0`, `c = 0` and the selector is `a = 1 OR b = 1 AND c = 1`
- **THEN** the message is selected, because `AND` binds tighter than `OR`

#### Scenario: Ordering comparison on a string literal
- **WHEN** a client creates a consumer with selector `name > 'abc'`
- **THEN** the broker rejects it with `javax.jms.InvalidSelectorException`

### Requirement: BETWEEN, IN, LIKE and IS NULL
The selector language SHALL support:
- `x [NOT] BETWEEN a AND b`, equivalent to `a <= x AND x <= b` (negated with `NOT`);
- `x [NOT] IN ('a', 'b', ...)` on string values; a NULL `x` gives UNKNOWN;
- `x [NOT] LIKE 'pattern' [ESCAPE 'c']`, where `%` matches any sequence of characters, `_` matches exactly one character, and the escape character makes the next `%` or `_` literal; a NULL `x` gives UNKNOWN;
- `x IS [NOT] NULL`, which is always TRUE or FALSE, never UNKNOWN.

#### Scenario: LIKE with wildcards
- **WHEN** the selector is `JMSCorrelationID LIKE 'ORD-A-%'` and messages have correlation IDs `ORD-A-100`, `ORD-B-200`, `ORD-A-300`
- **THEN** only `ORD-A-100` and `ORD-A-300` are selected

#### Scenario: LIKE with underscore and ESCAPE
- **WHEN** the selector is `code LIKE 'A\_%' ESCAPE '\'` and messages have `code` values `A_1` and `AB1`
- **THEN** only `A_1` is selected

#### Scenario: IN with NULL
- **WHEN** a message has no property `region` and the selector is `region NOT IN ('EU', 'US')`
- **THEN** the message is not selected

#### Scenario: BETWEEN with NULL
- **WHEN** a message has no property `size` and the selector is `size BETWEEN 1 AND 10`
- **THEN** the message is not selected

#### Scenario: IN on correlation ID
- **WHEN** the selector is `JMSCorrelationID IN ('ORD-A','ORD-C')` and messages with `ORD-A`, `ORD-B` and `ORD-C` arrive
- **THEN** only the `ORD-A` and `ORD-C` messages are selected

### Requirement: Type rules
Evaluation SHALL follow ActiveMQ's type rules (`org.apache.activemq.filter.ComparisonExpression`, verified in the 5.18 sources):
- a comparison between non-null values of incompatible types, for example a string with a number, SHALL be FALSE (not UNKNOWN), so `x = 5` does not select a message whose `x` is the string `'5'`, while `NOT (x = 5)` does;
- `x = y` with `x` NULL SHALL be UNKNOWN; with `x` non-null and `y` NULL it SHALL be FALSE; ordering comparisons (`<`, `<=`, `>`, `>=`) with a NULL operand SHALL be UNKNOWN;
- `LIKE` on a non-string value SHALL be FALSE (TRUE for `NOT LIKE`); `IN` on a non-string value SHALL be UNKNOWN;
- arithmetic with a NULL operand SHALL give NULL; `+` with a string left operand SHALL concatenate;
- a property whose value is a byte array, map or list SHALL be a non-null value that compares unequal to everything.

#### Scenario: String compared with number
- **WHEN** a message has string property `qty = '5'` and the selector is `qty = 5`
- **THEN** the message is not selected

#### Scenario: Negated type mismatch
- **WHEN** a message has string property `qty = '5'` and the selector is `NOT (qty = 5)`
- **THEN** the message is selected, as in ActiveMQ

#### Scenario: Arithmetic with NULL
- **WHEN** a message has no property `a` and the selector is `a + 1 > 0`
- **THEN** the message is not selected

### Requirement: Selective dispatch on queues without head-of-line blocking
On a queue, each consumer with a selector SHALL receive the first message in `pending`, in `broker_seq` order, that matches its selector. A message that matches no current consumer SHALL stay in the queue and SHALL NOT block the delivery of the following messages. Each consumer SHALL receive its matching messages as an ordered FIFO subsequence of the queue. Among several consumers that accept the same message, the round-robin rule of queue dispatch SHALL apply: the message goes to the first consumer, in round-robin order, that has free prefetch and a matching selector.

#### Scenario: Disjoint selectors on the same queue
- **WHEN** a queue receives messages alternating `type = 'A'` and `type = 'B'`, and two consumers have selectors `type = 'A'` and `type = 'B'`
- **THEN** each consumer receives all and only its own messages, in send order

#### Scenario: Unmatched messages do not block
- **WHEN** a queue holds `ORD-B-1` at its head followed by `ORD-A-1`, and the only consumer has selector `JMSCorrelationID = 'ORD-A'`
- **THEN** the consumer receives `ORD-A-1` and `ORD-B-1` stays in the queue

#### Scenario: Unmatched messages are kept for later consumers
- **WHEN** a selective consumer has taken all its messages and is closed, and a consumer without a selector is then created on the same queue
- **THEN** the new consumer receives exactly the messages that were never selected, in FIFO order

#### Scenario: Round-robin among matching consumers
- **WHEN** two consumers have the same selector and free prefetch, and four matching messages arrive
- **THEN** the messages alternate between the two consumers, and each consumer receives its messages in FIFO order

### Requirement: Per-consumer cursor
Each queue consumer with a selector SHALL keep the last `broker_seq` it examined, so that it does not rescan the head of the queue at every dispatch. New messages arrive at the tail and SHALL be examined only once per consumer. When a message returns to `pending` at its original position (consumer close, connection drop or transaction rollback), every cursor beyond that position SHALL be moved back so that the message is examined again. The amortised cost SHALL be O(1) per message and consumer.

#### Scenario: Redelivered message is re-examined
- **WHEN** a consumer with selector `type = 'A'` has already examined messages 1–100, and an unacknowledged `type = 'A'` message with `broker_seq` 10 returns to `pending` after another consumer closes
- **THEN** the selective consumer receives the message with `broker_seq` 10 before any later matching message

#### Scenario: No rescan of examined messages
- **WHEN** a selective consumer has examined 10,000 non-matching messages and a new matching message arrives
- **THEN** the dispatch examines only the new message for that consumer, not the 10,000 earlier ones

### Requirement: No page limit
The broker SHALL NOT apply a page limit to selective dispatch: a selective consumer SHALL receive a matching message regardless of how many non-matching messages precede it in the queue. This is a deliberate difference from ActiveMQ, whose `maxPageSize` (default 200) can stall selective consumers on long queues.

#### Scenario: Matching message behind a long backlog
- **WHEN** a queue holds 100,000 messages with `type = 'B'` followed by one message with `type = 'A'`, and a consumer with selector `type = 'A'` is created
- **THEN** the consumer receives the `type = 'A'` message

### Requirement: Selectors on topics
On a topic, the selector SHALL be applied at publish time: a published message SHALL enter only the pending lists of the subscribers whose selector accepts it (subscribers without a selector receive every message). A message accepted by no subscriber SHALL be discarded as a message published with no consumers.

#### Scenario: Topic subscribers with selectors
- **WHEN** three subscribers on a topic have selectors `level = 'ERROR'`, `level = 'WARN'` and none, and messages with `level` `ERROR`, `WARN` and `INFO` are published
- **THEN** the first subscriber receives only `ERROR`, the second only `WARN`, and the third all three, each in publish order

#### Scenario: Non-matching message not queued
- **WHEN** a message is published that no subscriber's selector accepts
- **THEN** it enters no subscriber's pending list and uses no memory after publishing

### Requirement: QueueBrowser with a selector
A `QueueBrowser` created with a selector SHALL receive copies of only the matching messages in `pending`, in FIFO order, without removing them, followed by the end-of-browse `MessageDispatch` with a null message.

#### Scenario: Filtered browse
- **WHEN** a queue holds `ORD-A-1`, `ORD-B-1`, `ORD-A-2` and a browser is created with selector `JMSCorrelationID LIKE 'ORD-A%'`
- **THEN** the browser enumerates `ORD-A-1` then `ORD-A-2`, and the queue still holds all three messages

### Requirement: Lazy property decoding
The broker SHALL decode `marshalledProperties` (OpenWire primitive map) only the first time a selector needs an application property of that message, and SHALL cache the result in the message (`OnceLock<Arc<PropertyMap>>`), so each message is decoded at most once whatever the number of consumers. Consumers without a selector and selectors that use only JMS header identifiers SHALL never trigger the decoding. The primitive map format SHALL be: the number of entries, then for each entry the key (string) and a typed value with the type codes NULL=0, BOOLEAN=1, BYTE=2, CHAR=3, SHORT=4, INTEGER=5, LONG=6, DOUBLE=7, FLOAT=8, STRING=9, BYTE_ARRAY=10, MAP=11, LIST=12, BIG_STRING=13, as defined in ActiveMQ's `MarshallingSupport`. Properties are never compressed, so selector evaluation SHALL never decompress anything.

#### Scenario: Decoded once for many consumers
- **WHEN** five consumers with property selectors examine the same queue message
- **THEN** the message's properties are decoded exactly once

#### Scenario: All property types
- **WHEN** a Java client sends a message with boolean, byte, short, int, long, float, double and string properties, and a consumer has a selector that tests each of them
- **THEN** every property is decoded with its correct type and value and the message is selected

#### Scenario: Compressed body
- **WHEN** a message with a compressed body (`compressed=true`) and property `k = 1` is evaluated against `k = 1`
- **THEN** the message is selected without its body being decompressed

### Requirement: Compiled LIKE and IN
At compile time, every `LIKE` pattern SHALL be turned into a dedicated matcher: a prefix, suffix or contains test when the pattern allows it, otherwise a generic matcher. An `IN` list with many elements SHALL be evaluated through a hash set rather than a linear scan. These optimisations SHALL NOT change results.

#### Scenario: Prefix pattern
- **WHEN** the selector `name LIKE 'abc%'` is compiled
- **THEN** it is evaluated with a prefix test, and gives the same results as a generic matcher for any input

#### Scenario: Large IN list
- **WHEN** a selector has an `IN` list with 1,000 strings
- **THEN** membership is tested through a hash set and gives the same results as a linear comparison

### Requirement: Invalid selector errors
A selector with a syntax error SHALL be rejected with an `ExceptionResponse` carrying `javax.jms.InvalidSelectorException`, whose message gives the offending token, its position and the selector, for example `Unexpected token 'AN' at column 14 in selector: color = 'red' AN size > 2`. The Java client SHALL therefore raise the exception from `createConsumer()`, and no consumer SHALL be registered.

#### Scenario: Syntax error
- **WHEN** a Java client calls `createConsumer(queue, "JMSCorrelationID = = 'X'")`
- **THEN** the call throws `InvalidSelectorException` and the broker has no new consumer on the queue

#### Scenario: Error message with position
- **WHEN** a client creates a consumer with selector `color = 'red' AN size > 2`
- **THEN** the `ExceptionResponse` message names the token `AN`, its column and the full selector

### Requirement: XPath and XQuery selectors rejected
A selector that starts with `XPATH '...'` or `XQUERY '...'` (keywords case-insensitive) SHALL be rejected with `javax.jms.InvalidSelectorException("XPath selectors are not supported")`. This is a deliberate difference from ActiveMQ, which supports XPath selectors on XML message bodies.

#### Scenario: XPath selector
- **WHEN** a client creates a consumer with selector `XPATH '//order[@id=1]'`
- **THEN** the client receives `InvalidSelectorException` with the message `XPath selectors are not supported`

### Requirement: Runtime evaluation errors
An error during evaluation, such as division by zero, SHALL make the selector UNKNOWN for that message and that consumer only. The broker SHALL NOT send an error to the client and SHALL NOT remove the message; it stays available to other consumers. A message whose `marshalledProperties` cannot be decoded SHALL be treated the same way, and the broker SHALL log the decoding failure once per message at warning level.

#### Scenario: Division by zero
- **WHEN** a message has `a = 1` and `b = 0` and the selector is `a / b > 0`
- **THEN** the message is not selected, the consumer stays open, and no error is sent to the client

### Requirement: Selector visible in the admin
The consumer snapshot used by the admin console SHALL include the consumer's selector text, and the queue detail page and the `/api/queues/{name}` JSON SHALL show the selector of each consumer. Messages that no consumer selects SHALL remain visible in the queue contents.

#### Scenario: Selector in the queue detail
- **WHEN** a consumer with selector `JMSCorrelationID = 'ORD-A'` is attached to a queue and the admin requests `/api/queues/{name}`
- **THEN** the consumer entry contains the selector `JMSCorrelationID = 'ORD-A'`

### Requirement: Selector acceptance gate
Scenario 2 of the Java acceptance program SHALL pass against `mqrust.exe` with both the `amq5` and `amq6` profiles, and scenarios 1 and 3 SHALL keep passing. The Java integration tests SHALL run the same set of selectors and messages against a real ActiveMQ and against ActiveMQRust, and both SHALL produce the same received messages in the same order.

#### Scenario: Acceptance scenario 2
- **WHEN** the acceptance program runs scenario 2 against `mqrust.exe`
- **THEN** it prints `PASS`

#### Scenario: Same results as ActiveMQ
- **WHEN** the selector comparison suite runs against real ActiveMQ and against ActiveMQRust
- **THEN** for every selector both brokers deliver the same messages in the same order
