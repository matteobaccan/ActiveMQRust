## Why

ActiveMQRust aims to be a broker that is **compatible** with ActiveMQ Classic, **uses less RAM** and is **faster** than ActiveMQ. Java applications that use the ActiveMQ driver (`activemq-client`) should switch to it by changing only host and port. Every later feature depends on the same foundation: a single Windows executable, configuration, logging, an OpenWire connection the Java driver accepts, and authentication. This change builds that foundation. It also builds the Java acceptance program, which is the executable definition of "compatible" for all following changes.

## What Changes

- New Rust crate `mqrust` producing one dependency-free Windows x64 executable `mqrust.exe` (static C runtime, embedded assets, Windows version resource `ActiveMQRust <version>`).
- Optional TOML configuration with built-in defaults: OpenWire on `0.0.0.0:61616`, admin on `127.0.0.1:8161`, default credentials `admin`/`admin` with a warning. Command-line overrides and helper commands (`init-config`, `check-config`, `hash-password`, `--version`).
- Essential logging: startup summary, connection events, failed logins, protocol errors; graceful shutdown on Ctrl+C or console close.
- OpenWire transport: TCP listener, framing, `WireFormatInfo` negotiation (versions 9–12, loose encoding, tight encoding and cache disabled), `BrokerInfo`, keep-alive, command/response rules, connection and session lifecycle, advisory consumers accepted (only temporary-destination advisories are published). The broker advertises `ProviderName=ActiveMQRust`.
- Client authentication: username/password from `ConnectionInfo`, checked against plain-text or Argon2 credentials; anonymous access off by default.
- Command-line program that can install, uninstall, start, stop and query itself as a Windows service (`mqrust.exe service install|uninstall|start|stop|status`), logging to `mqrust.log` when it runs as a service.
- Java acceptance program (Maven, with wrapper) that uses the real ActiveMQ driver. All scenarios are written now and validated against a real ActiveMQ. Only the authentication scenario must pass against ActiveMQRust at the end of this change.

## Capabilities

### New Capabilities

- `windows-packaging`: single dependency-free `mqrust.exe`, supported Windows versions, version resource, dependency verification.
- `broker-configuration`: configuration file lookup, built-in defaults, keys, validation, command-line interface.
- `broker-logging`: startup log, runtime events, log level, graceful shutdown.
- `openwire-transport`: framing, wire-format negotiation, protocol versions, broker identity, command/response rules, keep-alive, connection/session lifecycle, advisory topics.
- `client-authentication`: OpenWire login, credential formats, anonymous policy, failed-login handling.
- `windows-service`: CLI foreground mode and Windows service install, uninstall, control and service-mode execution.
- `java-acceptance-suite`: Java program using the ActiveMQ driver, its scenarios, output and reference run against ActiveMQ.

### Modified Capabilities

None.

## Impact

- New Rust project: `Cargo.toml`, `build.rs`, `.cargo/config.toml`, `src/` (main, config, auth, openwire codec, connection handling), `scripts/check-deps.cmd`.
- New Maven project in `tests/java-it/` with Maven Wrapper; needs JDK 17+ on the development machine only.
- Dependencies compiled into the executable: `tokio`, `bytes`, `serde`, `toml`, `clap`, `tracing`, `tracing-subscriber`, `argon2`, `rpassword`, `parking_lot`, `mimalloc`, `windows-service`; build-only: `embed-resource`.
- Later changes depend on this one: `add-queue-messaging`, `add-topic-messaging`, `add-local-transactions`, `add-message-selectors`, `add-message-expiration`, `add-message-compression`, `add-admin-console`, `optimize-broker-performance`.
