## 1. Harness

- [ ] 1.1 Identify the Jakarta Messaging TCK release for Messaging 3.1, its download URL and SHA-256; script the download and verification into a cache folder outside the repository
- [ ] 1.2 Provider configuration for `activemq-client` 6.3.2: JNDI properties with the connection factories and destinations the TCK expects, parameterised by broker URL and credentials
- [ ] 1.3 `scripts/run-tck.ps1`: prepare, run the Java SE selection, write per-test results (CSV) and a summary; exit code non-zero on unexpected failures

## 2. Selection

- [ ] 2.1 Exclusion list with a reason category for every excluded test or group (durable, shared, xa, persistence, container, wildcard)
- [ ] 2.2 Re-run failing tests once to separate timing flakiness from real failures

## 3. Comparison and fixes

- [ ] 3.1 Run against ActiveMQ 6.3.2 (tuned and default configuration) and record the reference results
- [ ] 3.2 Run against ActiveMQRust; classify every failure as "fails on reference", "ActiveMQRust difference" or "flaky"
- [ ] 3.3 For each ActiveMQRust difference, add a reproducing test to the project's suites and open a change to fix it, or document it as a known difference

## 4. Reporting

- [ ] 4.1 README section "Jakarta Messaging TCK": totals per broker (run, passed, failed, excluded) and known differences
- [ ] 4.2 Optional CI job (manual dispatch) running the selection against ActiveMQRust on Windows
- [ ] 4.3 `openspec validate add-jakarta-messaging-tck` passes
