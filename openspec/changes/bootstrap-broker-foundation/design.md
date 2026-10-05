## Context

ActiveMQRust replaces ActiveMQ Classic for Java applications using `activemq-client` 5.18.x / 6.x over OpenWire (`tcp://`). The project goals are compatibility, lower RAM and higher speed than ActiveMQ. Only a Windows x64 build matters, and the deliverable is a single executable with no runtime dependencies. OpenWire has no formal specification: the reference is the Java source (`org.apache.activemq.openwire.v1` … `v12` marshallers and `OpenWireFormat`). This change is the base that all other changes build on.

## Goals / Non-Goals

**Goals:**
- A `mqrust.exe` that starts with no arguments and no files and accepts an authenticated OpenWire connection from the Java driver on port 61616.
- A codec and connection layer designed so that messaging, selectors and the other features plug in without rework.
- An acceptance program that turns "compatible with ActiveMQ" into a repeatable PASS/FAIL check.

**Non-Goals:**
- Sending or receiving messages (`add-queue-messaging`), topics, transactions, selectors, expiration, compression, admin console, performance tuning: each has its own change.
- TLS (`ssl://`), Linux/macOS builds.
- Tight encoding and marshalling cache.

## Decisions

### D1. Loose encoding only, negotiated by the broker
The broker's `WireFormatInfo` advertises `TightEncodingEnabled=false` and `CacheEnabled=false`. The client ANDs both flags with its own, so the session uses loose encoding with no cache. This is fully within the protocol.
- *Alternatives:* tight + loose with cache (about 3× the codec code and more compatibility risk, for a few bytes saved per command); generating the codec from Java definitions (`openwire-generator`, which adds Java/Groovy tooling and hard-to-maintain generated code).
- The codec sits behind a `WireCodec` trait, so tight encoding can be added later if benchmarks justify it.

### D2. Version range 9–12 with per-field version guards
The broker advertises version 12; the effective version is `min(client, 12)`. Clients below 9 are refused. Fields that differ between versions are handled with `if version >= N` guards, derived from the Java marshallers.

### D3. Tokio, one reader task and one writer task per connection
The reader decodes frames and handles commands; the writer drains an mpsc channel into the socket and batches writes. No lock is ever held during I/O.
- *Alternatives:* an actor per destination (more channels and latency, harder to debug); a thread per connection (memory cost contradicts the RAM goal).

### D4. Identity
Product name `ActiveMQRust`, version from `CARGO_PKG_VERSION`. The name appears in `WireFormatInfo` (`ProviderName`, `ProviderVersion`, `PlatformDetails`), the default `BrokerInfo.brokerName`, the log, `--version` and the Windows version resource. The executable file is `mqrust.exe`.

### D5. Configuration: optional TOML, layered
Precedence is command line > file > built-in defaults. The file is looked up at `--config`, then next to the executable. Without a file, the default credentials `admin`/`admin` apply (the same defaults as ActiveMQ) and a warning is logged on every start.

### D6. Authentication
Passwords are either plain text (compared in constant time, with a warning) or Argon2id hashes produced by `mqrust.exe hash-password`. A failed login gets `ExceptionResponse(java.lang.SecurityException)` and the connection is closed; the Java client raises `JMSSecurityException`.

### D7. Dependency-free executable
`.cargo/config.toml` sets `+crt-static` for `x86_64-pc-windows-msvc`. Assets are embedded with `include_str!`, and no TLS means no OpenSSL. `scripts/check-deps.cmd` runs `dumpbin /dependents` against an allow-list of system DLLs. A version resource is embedded through `build.rs` with `embed-resource`.

### D9. Windows service through the `windows-service` crate
`mqrust.exe service install|uninstall|start|stop|status` talks to the Service Control Manager (advapi32, a system DLL, so no new runtime dependency). The registered command is `<abs exe> service run [--config <abs path>]`; in service mode the log goes to `mqrust.log` next to the executable because there is no console. Stop and Shutdown map to the same graceful shutdown as Ctrl+C.
- *Alternatives:* external wrappers such as NSSM or `sc.exe create` (an extra tool to install and document); logging to the Windows Event Log (needs a registered message source and is harder to read than a text file).

### D8. Acceptance program first
`tests/java-it/` is a Maven project (Java 17 target, `maven-shade-plugin` fat jar, Maven Wrapper because Maven is not installed). Profiles `amq5` (5.18.x, `javax.jms`) and `amq6` (6.x, `jakarta.jms`) select the driver. All scenarios are written in this change and validated against a real ActiveMQ, so later changes have a fixed target.

## Risks / Trade-offs

- [Undocumented field differences between OpenWire versions] → Derive them from the Java marshallers; check against golden byte vectors captured from a real client and against both driver versions.
- [Java client closes idle connections after 30 s] → The broker sends `KeepAliveInfo` after `MaxInactivityDuration / 2` without writes; there is a test with more than 60 s of idle time.
- [Default `admin`/`admin` credentials on `0.0.0.0`] → A warning on every start, the README advises setting passwords, and the admin console binds to localhost only.
- [Hidden client behaviour, e.g. advisory consumers] → Accept advisory consumers with a `Response`; publish only the temporary-destination advisories (`DestinationInfo` on `ActiveMQ.Advisory.TempQueue`/`TempTopic`) that the driver relies on to track temporary destinations; any divergence found becomes a test.

## Migration Plan

Not applicable: this is a new product. Deployment means copying `mqrust.exe`. Rollback means pointing clients back to ActiveMQ.

## Open Questions

- The exact per-version field list of `WireFormatInfo`, `ConnectionInfo`, `SessionInfo`, `BrokerInfo`, `RemoveInfo` and `ExceptionResponse` must be extracted from the 5.18 / 6.x sources at implementation time.
