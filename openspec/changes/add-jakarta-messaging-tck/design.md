## Context

The Jakarta Messaging TCK checks a JMS provider: the client library plus the server. For ActiveMQ the provider is `activemq-client` (6.x implements Jakarta Messaging 3.1) with an OpenWire broker behind it. The TCK needs connection factories and destinations it can look up (JNDI or a porting class), and some tests assume features a JMS provider may legitimately omit or that need a Jakarta EE container.

## Goals / Non-Goals

**Goals:**
- Run the standalone (Java SE) part of the Jakarta Messaging TCK unchanged against ActiveMQRust and ActiveMQ 6.3.2.
- Make the result reproducible from a clean checkout with one command.
- Separate "not applicable by design", "fails on ActiveMQ too" and "ActiveMQRust difference".

**Non-Goals:**
- Certification (the official process requires the full TCK and a Jakarta EE context).
- Container tests (EJB, servlet, JSP, application client) and resource-adapter tests.
- Features excluded by the product: durable and shared subscriptions, XA, persistence, wildcards.

## Decisions

1. **TCK version**: the Jakarta Messaging TCK release for Messaging 3.1 (the API implemented by `activemq-client` 6.3.2), downloaded from the Eclipse/Jakarta download site and verified with its published SHA-256; not stored in the repository.
2. **Provider configuration**: ActiveMQ client's JNDI (`org.apache.activemq.jndi.ActiveMQInitialContextFactory`) with a `jndi.properties` that defines the connection factories and the queue/topic names the TCK expects, pointing at the broker URL given to the runner.
3. **Runner**: `scripts/run-tck.ps1 -Url tcp://host:port [-User] [-Password]` prepares the TCK, runs the Java SE test selection, collects per-test results (passed / failed / excluded / error) into a CSV and a summary.
4. **Exclusions**: one file, one line per test or group with the reason category (durable, shared, xa, persistence, container, wildcard).
5. **Comparison**: the runner is executed against ActiveMQ 6.3.2 first; tests failing there are marked "fails on reference" and are not counted against ActiveMQRust.

## Risks / Trade-offs

- [The TCK harness may need a JavaTest/Ant setup that is heavy on Windows] → Keep everything in the runner; document JDK and tool versions.
- [Some TCK tests depend on timing] → Run each failing test a second time before classifying it.
- [Tests that create and delete destinations administratively] → Use the TCK's porting hooks to create destinations on first use (ActiveMQRust and ActiveMQ create them automatically).
