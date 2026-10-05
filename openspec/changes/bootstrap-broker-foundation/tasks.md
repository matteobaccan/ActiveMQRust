## 1. Java acceptance program (written first)

- [x] 1.1 Create `tests/java-it/` Maven project with Maven Wrapper, Java 17 target, shade plugin and profiles `amq5` (activemq-client 5.18.x) and `amq6` (6.x)
- [x] 1.2 Implement argument parsing (`--url`, `--user`, `--password`, `--only`), PASS/FAIL reporting and exit code
- [x] 1.3 Implement scenario 1 (queue round trip, FIFO, message IDs)
- [x] 1.4 Implement scenario 2 (correlation ID selectors, LIKE, invalid selector)
- [x] 1.5 Implement scenario 3 (authentication)
- [x] 1.6 Add `run-acceptance.cmd` and document in the README how to start a local ActiveMQ
- [ ] 1.7 Run all scenarios against a real ActiveMQ with both profiles and confirm PASS

## 2. Project skeleton and packaging

- [x] 2.1 Create the `mqrust` crate, the release profile (LTO fat, codegen-units 1, panic abort) and the `mimalloc` global allocator
- [x] 2.2 Add `.cargo/config.toml` with `+crt-static` for `x86_64-pc-windows-msvc`
- [x] 2.3 Add `build.rs` with the Windows version resource (`ActiveMQRust`, crate version)
- [x] 2.4 Add `scripts/check-deps.cmd` with the system DLL allow-list and verify the release build passes it
- [x] 2.5 Add `scripts/build.cmd` that builds only the Windows release (`cargo build --release`) and runs tests with `cargo test --release`; no debug builds are produced or used anywhere in the project

## 3. Configuration and CLI

- [x] 3.1 Define the configuration model with defaults and TOML loading (lookup order, unknown-key errors)
- [x] 3.2 Implement validation with field-specific errors and exit code 2
- [x] 3.3 Implement the CLI (`--config`, `--bind`, `--port`, `--admin-bind`, `--admin-port`, `--version`) with precedence over the file
- [x] 3.4 Implement `check-config`, `init-config` (no overwrite) and `hash-password` (hidden input, Argon2id)
- [x] 3.5 Unit tests for defaults, partial files, precedence and every validation error

## 4. Logging and lifecycle

- [x] 4.1 Set up `tracing` with the configured level, local timestamps and the startup lines
- [x] 4.2 Implement graceful shutdown on Ctrl+C and `CTRL_CLOSE_EVENT` (stop accepting, send `ShutdownInfo`, 5 s limit, discarded-count log)

## 5. OpenWire codec

- [x] 5.1 Extract the per-version field lists (v9–v12) of the commands used by this change from the Java marshallers
- [x] 5.2 Implement framing (length prefix, max frame size) and the loose-encoding primitives
- [x] 5.3 Implement `WireFormatInfo` with its properties map, `BrokerInfo`, `ConnectionInfo`, `SessionInfo`, `ConsumerInfo`/`ProducerInfo` (decode only), `RemoveInfo`, `ShutdownInfo`, `KeepAliveInfo`, `Response`, `ExceptionResponse`, `ConnectionError` and the ID types
- [x] 5.4 Capture golden byte vectors from a real Java client and add round-trip tests

## 6. Connection handling

- [x] 6.1 Implement the TCP listener and per-connection reader/writer tasks with the outbound channel
- [x] 6.2 Implement negotiation (version range, flags, provider identity) and `BrokerInfo`
- [x] 6.3 Implement command/response correlation and the unsupported-command reply
- [x] 6.4 Implement keep-alive writes and the inactivity timeout
- [x] 6.5 Implement the connection/session lifecycle and resource release on close or drop
- [x] 6.6 Accept advisory consumers; publish only the temporary-destination advisories

## 7. Authentication

- [x] 7.1 Implement the credential store (plain text with constant-time compare, Argon2id) and default credentials
- [x] 7.2 Authenticate `ConnectionInfo`, reject with `SecurityException`, apply the anonymous policy, log failures without passwords

## 8. Windows service

- [x] 8.1 Add the `service` subcommands (`install`, `uninstall`, `start`, `stop`, `status`, `run`) with `--name` and `--config`, using the `windows-service` crate
- [x] 8.2 Implement service-mode execution: SCM dispatcher, running/stopped status, Stop/Shutdown mapped to graceful shutdown, log to `mqrust.log` next to the executable
- [x] 8.3 Report clear errors for missing administrator rights, existing or missing services, and `service run` outside the SCM

## 9. Verification

- [x] 9.1 Acceptance scenario 3 passes against `mqrust.exe` started with no arguments (both profiles)
- [x] 9.2 The idle connection test (over 60 s) passes
- [ ] 9.3 The clean-machine test in Windows Sandbox passes (executable alone, scenario 3)
- [ ] 9.4 Install, start, stop, uninstall the service on the development machine and check `mqrust.log`
