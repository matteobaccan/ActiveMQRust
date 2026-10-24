## ADDED Requirements

### Requirement: Java acceptance program
The repository SHALL contain, in `tests/java-it/`, a Maven project (Java 17 target, built through the included Maven Wrapper `mvnw.cmd`) that produces an executable fat jar `mqrust-acceptance.jar`. The program SHALL use the original ActiveMQ driver `org.apache.activemq:activemq-client`, selected by Maven profile: `amq5` (5.18.x, `javax.jms`) or `amq6` (6.x, `jakarta.jms`).

#### Scenario: Build without installed Maven
- **WHEN** `tests\java-it\run-acceptance.cmd` is run on a machine with only a JDK 17+
- **THEN** the jar is built through the Maven Wrapper and executed

### Requirement: Command line and output
The program SHALL accept `--url`, `--user` and `--password` (plus an optional `--only <scenario>`). It SHALL print one `PASS` or `FAIL <reason>` line per scenario, and exit with code 0 if every scenario passes and 1 otherwise. Every scenario SHALL use queue names with a random suffix, so repeated runs do not interfere. Every `receive` SHALL use a timeout (5 s for expected messages, 1 s for expected absence), so a missing message produces FAIL instead of a hang.

#### Scenario: Failing scenario
- **WHEN** one scenario fails
- **THEN** its line reads `FAIL` with the reason, the other scenarios still run, and the exit code is 1

### Requirement: Scenario 1, queue round trip in FIFO order
The program SHALL connect, start the connection, open an `AUTO_ACKNOWLEDGE` session, and send 10 `TextMessage`s `msg-1` … `msg-10`, with an int property `seq` from 1 to 10, to a new queue `TEST.FIFO.<random>`. It records each `JMSMessageID` and reads the messages back with a consumer. It SHALL check:
- exactly 10 messages are received, and a further `receive(1000)` returns null;
- the order is the send order;
- each `JMSMessageID` equals the producer-side ID and matches `ID:<host>-<port>-<timestamp>-<n>:<n>:<n>:<n>:<n>`;
- `JMSRedelivered` is false for all of them.

#### Scenario: Round trip passes
- **WHEN** scenario 1 runs against a compliant broker
- **THEN** it prints `PASS`

### Requirement: Scenario 2, correlation ID selector
The program SHALL send 12 messages to a new queue `TEST.CORR.<random>`, cycling correlation IDs `ORD-A`, `ORD-B`, `ORD-C` (4 each), with text `<correlationId>-<n>`. It SHALL check that:
- a consumer with selector `JMSCorrelationID IN ('ORD-A','ORD-C')` receives exactly the 8 `ORD-A`/`ORD-C` messages, in send order, and then `null`, even though `ORD-B` messages remain;
- after closing it, a consumer without a selector receives exactly `ORD-B-1` … `ORD-B-4`, in order;
- on another new queue holding `ORD-A-100`, `ORD-B-200`, `ORD-A-300`, a consumer with `JMSCorrelationID LIKE 'ORD-A-%'` receives only `ORD-A-100` then `ORD-A-300`;
- `createConsumer(queue, "JMSCorrelationID = = 'X'")` throws `InvalidSelectorException`.

#### Scenario: Selector scenario passes
- **WHEN** scenario 2 runs against a compliant broker
- **THEN** it prints `PASS`

### Requirement: Scenario 3, authentication
The program SHALL check that a connection with a wrong password fails with `JMSSecurityException` on `createConnection()` or `start()`, and that a connection with the given credentials succeeds.

#### Scenario: Wrong password rejected
- **WHEN** scenario 3 runs against a compliant broker
- **THEN** the wrong-password connection fails with `JMSSecurityException`, the correct one succeeds, and the program prints `PASS`

### Requirement: Reference run against ActiveMQ
The same jar SHALL pass every scenario against a real ActiveMQ 5.18.x / 6.x configured with the same user. The README SHALL document how to start a local ActiveMQ for this reference run.

#### Scenario: Reference broker
- **WHEN** the program runs against a real ActiveMQ with matching credentials
- **THEN** every scenario prints `PASS`

### Requirement: Milestones per change
The program SHALL be the acceptance gate of each change: scenario 3 for `bootstrap-broker-foundation`; scenario 1 for `add-queue-messaging`; scenario 2 for `add-message-selectors`. Each later change SHALL keep all earlier scenarios passing.

#### Scenario: Foundation gate
- **WHEN** `bootstrap-broker-foundation` is complete
- **THEN** scenario 3 prints `PASS` against `mqrust.exe` started with no arguments, using `admin`/`admin`
