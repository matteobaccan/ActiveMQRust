## Why

ActiveMQRust's compatibility is checked today by its own Java tests (acceptance, integration, compression, console) and by 568 selectors compared with ActiveMQ's selector engine. These tests were written together with the broker, so they share its assumptions. The Jakarta Messaging TCK (Technology Compatibility Kit) is the independent, standard suite that JMS providers use to prove conformance: running it with the ActiveMQ Java client against ActiveMQRust, and against Apache ActiveMQ 6 as a reference, shows how much of the JMS behaviour seen by applications is covered and where the two brokers differ. Target: version 0.4.0.

## What Changes

- A reproducible way to run the Jakarta Messaging TCK (the version matching `activemq-client` 6.3.2, Jakarta Messaging 3.1) with the ActiveMQ client against any OpenWire broker URL, on Windows, without modifying the TCK sources.
- A documented exclusion list: tests for features ActiveMQRust does not provide by design (durable subscriptions, shared subscriptions, XA, persistence across restarts, wildcards, and anything requiring a Jakarta EE container), each with the reason.
- A comparison run: the same selection against ActiveMQ 6.3.2 (tuned in-RAM configuration and default configuration) and ActiveMQRust, so that failures caused by the client or the TCK setup are separated from broker differences.
- Every TCK failure on ActiveMQRust that passes on ActiveMQ becomes a bug report with a reproducing test in the project's own suites, and is fixed or documented as a known difference.
- README: a "Jakarta Messaging TCK" section with the totals per broker (run, passed, failed, excluded) and the list of known differences.

## Capabilities

### New Capabilities

- `jms-conformance`: running the Jakarta Messaging TCK against ActiveMQRust and ActiveMQ, the exclusion list, the comparison and the reporting of results.

### Modified Capabilities

None.

## Impact

- New `tests/tck/` folder: scripts to download and verify the TCK bundle, the JNDI/provider configuration for the ActiveMQ client, the exclusion list and a runner (`scripts/run-tck.ps1`).
- No change to the broker for the harness itself; broker fixes found by the TCK go through their own changes.
- CI: optional job (manual dispatch) that runs the TCK selection against ActiveMQRust on Windows.
