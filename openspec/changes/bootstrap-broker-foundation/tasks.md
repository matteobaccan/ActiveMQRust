## 1. Java acceptance program (written first)

- [ ] 1.1 Create `tests/java-it/` Maven project with Maven Wrapper, Java 17 target, shade plugin and profiles `amq5` (activemq-client 5.18.x) and `amq6` (6.x)
- [ ] 1.2 Implement argument parsing (`--url`, `--user`, `--password`, `--only`), PASS/FAIL reporting and exit code
- [ ] 1.3 Implement scenario 1 (queue round trip, FIFO, message IDs)
- [ ] 1.4 Implement scenario 2 (correlation ID selectors, LIKE, invalid selector)
- [ ] 1.5 Implement scenario 3 (authentication)
- [ ] 1.6 Add `run-acceptance.cmd` and document in the README how to start a local ActiveMQ
- [ ] 1.7 Run all scenarios against a real ActiveMQ with both profiles and confirm PASS

## 2. Project skeleton and packaging

- [ ] 2.1 Create the `mqrust` crate, the release profile (LTO fat, codegen-units 1, panic abort) and the `mimalloc` global allocator
- [ ] 2.2 Add `.cargo/config.toml` with `+crt-static` for `x86_64-pc-windows-msvc`
- [ ] 2.3 Add `build.rs` with the Windows version resource (`ActiveMQRust`, crate version)
- [ ] 2.4 Add `scripts/check-deps.cmd` with the system DLL allow-list and verify the release build passes it
- [ ] 2.5 Add `scripts/build.cmd` that builds only the Windows release (`cargo build --release`) and runs tests with `cargo test --release`; no debug builds are produced or used anywhere in the project

## 3. Configuration and CLI

- [ ] 3.1 Define the configuration model with defaults and TOML loading (lookup order, unknown-key errors)
- [ ] 3.2 Implement validation with field-specific errors and exit code 2
- [ ] 3.3 Implement the CLI (`--config`, `--bind`, `--port`, `--admin-bind`, `--admin-port`, `--version`) with precedence over the file
- [ ] 3.4 Implement `check-config`, `init-config` (no overwrite) and `hash-password` (hidden input, Argon2id)
- [ ] 3.5 Unit tests for defaults, partial files, precedence and every validation error

## 4. Logging and lifecycle

- [ ] 4.1 Set up `tracing` with the configured level, local timestamps and the startup lines
- [ ] 4.2 Implement graceful shutdown on Ctrl+C and `CTRL_CLOSE_EVENT` (stop accepting, send `ShutdownInfo`, 5 s limit, discarded-count log)

## 5. OpenWire codec

- [ ] 5.1 Extract the per-version field lists (v6–v12) of the commands used by this change from the Java marshallers
- [ ] 5.2 Implement framing (length prefix, max frame size) and the loose-encoding primitives
- [ ] 5.3 Implement `WireFormatInfo` with its properties map, `BrokerInfo`, `ConnectionInfo`, `SessionInfo`, `ConsumerInfo`/`ProducerInfo` (decode only), `RemoveInfo`, `ShutdownInfo`, `KeepAliveInfo`, `Response`, `ExceptionResponse`, `ConnectionError` and the ID types
- [ ] 5.4 Capture golden byte vectors from a real Java client and add round-trip tests

## 6. Connection handling

- [ ] 6.1 Implement the TCP listener and per-connection reader/writer tasks with the outbound channel
- [ ] 6.2 Implement negotiation (version range, flags, provider identity) and `BrokerInfo`
- [ ] 6.3 Implement command/response correlation and the unsupported-command reply
- [ ] 6.4 Implement keep-alive writes and the inactivity timeout
- [ ] 6.5 Implement the connection/session lifecycle and resource release on close or drop
- [ ] 6.6 Accept advisory consumers silently

## 7. Authentication

- [ ] 7.1 Implement the credential store (plain text with constant-time compare, Argon2id) and default credentials
- [ ] 7.2 Authenticate `ConnectionInfo`, reject with `SecurityException`, apply the anonymous policy, log failures without passwords

## 8. Verification

- [ ] 8.1 Acceptance scenario 3 passes against `mqrust.exe` started with no arguments (both profiles)
- [ ] 8.2 The idle connection test (over 60 s) passes
- [ ] 8.3 The clean-machine test in Windows Sandbox passes (executable alone, scenario 3)
