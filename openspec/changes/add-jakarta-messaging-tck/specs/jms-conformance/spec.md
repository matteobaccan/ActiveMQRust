## ADDED Requirements

### Requirement: Reproducible TCK run
The project SHALL provide `scripts/run-tck.ps1 -Url <openwire-url> [-User <u>] [-Password <p>]`, which downloads and verifies the Jakarta Messaging TCK for Messaging 3.1 (by SHA-256) into a cache folder outside the repository, configures it for `activemq-client` 6.3.2 through JNDI, runs the Java SE test selection against the given broker, and writes one CSV row per test (name, outcome passed / failed / excluded / error, duration, message) plus a summary with the totals. The TCK sources SHALL NOT be modified.

#### Scenario: Run against ActiveMQRust
- **WHEN** `pwsh scripts\run-tck.ps1 -Url tcp://127.0.0.1:61616` runs against a started ActiveMQRust
- **THEN** it prints the totals of run, passed, failed and excluded tests and writes the CSV, without any manual step

#### Scenario: Integrity of the TCK bundle
- **WHEN** the downloaded TCK bundle does not match the published SHA-256
- **THEN** the runner stops with an error and runs nothing

### Requirement: Documented exclusions
Every excluded test or test group SHALL be listed in one exclusion file with a reason category: `durable` (durable subscriptions), `shared` (shared subscriptions), `xa` (distributed transactions), `persistence` (delivery across a broker restart), `container` (needs a Jakarta EE container), `wildcard` (wildcard or composite destinations). No test SHALL be excluded for any other reason without a linked issue.

#### Scenario: Excluded test reported
- **WHEN** the TCK selection contains a durable-subscription test
- **THEN** the CSV lists it as `excluded` with the category `durable`

### Requirement: Comparison with ActiveMQ
The same selection SHALL be run against ActiveMQ 6.3.2 as a reference. A test that fails on ActiveMQ 6.3.2 too SHALL be classified "fails on reference" and not counted against ActiveMQRust; a test that passes on ActiveMQ and fails on ActiveMQRust SHALL be classified "ActiveMQRust difference"; a failing test SHALL be run a second time and classified "flaky" if the outcomes differ.

#### Scenario: Difference found
- **WHEN** a test passes on ActiveMQ 6.3.2 and fails on ActiveMQRust twice
- **THEN** it is reported as an ActiveMQRust difference with its failure message, and a reproducing test is added to the project's suites

### Requirement: Published conformance results
The README SHALL contain a "Jakarta Messaging TCK" section with, for ActiveMQRust and ActiveMQ 6.3.2, the TCK version, the date, the totals (run, passed, failed, excluded) and the list of known differences with their status.

#### Scenario: Results in the README
- **WHEN** a TCK run completes for both brokers
- **THEN** the README shows both totals side by side and every ActiveMQRust difference
