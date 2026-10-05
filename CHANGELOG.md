# Changelog

All notable changes to ActiveMQRust are listed here. The section matching a
release tag is used as the body of the GitHub Release; GitHub appends the
list of merged pull requests and the compare link below it.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.3.0] - 2026-10-06

The first release with downloadable binaries: Windows x86_64 and, new, macOS on Apple
Silicon, built and tested by GitHub Actions.

### Added

- **macOS ARM64 (Apple Silicon) build**, `activemq-rust-macos-arm64-<version>.tar.gz`, next
  to the Windows one, `activemq-rust-windows-x86_64-<version>.zip`. Each archive holds the
  broker, LICENSE, README.md and the commented configuration template
  `mqrust.example.toml`. On macOS the admin console shows the resident size and the
  physical footprint of the process; the `service` commands answer that the Windows
  service is available on Windows only (exit code 2).
- **Continuous integration and release pipeline.** Every push and pull request is checked
  on Windows and macOS (formatting, Clippy with warnings as errors, the test suite in
  release mode); a version tag builds the archives and publishes the GitHub Release with a
  downloads grid and this changelog. When the version in `Cargo.toml` changes on `main`,
  the tag is created automatically.
- **Soak scenario** in the Java benchmark client (steady producers with order, loss and
  duplicate checks; full speed with `--rate 0`; client compression option) and
  `scripts/soak-compare.ps1` to run it against ActiveMQRust, ActiveMQ 5.18.7 and 6.3.2.
- **Load-test results in the README**: performance at a glance and every result (steady
  load, full speed at 15, 50 and 300 KB, client compression, ActiveMQ with an 8 GB heap),
  measured with the same Java client for every broker.
- **`scripts\test.cmd`**: formats the sources and runs the tests in release mode.
- **Issue and pull request templates**: requests instead of code contributions.
- **Renovate** keeps the dependencies up to date.

### Changed

- **Broker compression is off by default.** Bodies are stored as the clients send them;
  set `broker.compress_threshold_kb` to compress large bodies in the broker again.
- **Admin console**: the broker addresses are shown under the title, and the theme
  follows the system (the manual light/dark switch is gone).
- **New logo**: "Rust" in oxidised metal with a moving light reflection.
- **Dependencies**: argon2 0.6 (session tokens now come from getrandom), base64 0.23; the
  Java test client's `amq5` profile uses activemq-client 5.19.11 (CVE-2026-33227 and
  CVE-2026-39304).
- **Sources formatted with rustfmt** (`max_width = 120`), with LF line endings.

## [0.2.0] - 2026-10-06

### Added

- **Command line setup**: `init-config`, `set-admin`, `user add | passwd | remove | list`
  and `--password-stdin`, with password and username rules. The configuration file is
  edited keeping comments, key order and line endings, validated and replaced atomically.
  Short help with the getting-started steps, long help with examples and exit codes.
- **Admin console completed**: form login with in-memory sessions, login throttling,
  Origin/Referer checks, an XML view of text bodies, a responsive light/dark stylesheet
  with no JavaScript, a sortable queues table, expiration and compression details in pages
  and in the JSON API.
- **Comparison benchmark** (`scripts/compare-activemq.ps1`): dry run, CPU of broker,
  client and machine and broker memory per phase.
- **Soak comparison** with ActiveMQ 5.18.7 and 6.3.2 published in the README; acceptance
  scenarios also confirmed against the real ActiveMQ brokers.

### Changed

- **Message selectors follow ActiveMQ's selector engine**: Java-typed values and
  conversions, the grammar's lexer rules, all header identifiers, LIKE matchers;
  checked against a conformance table of 568 selectors.
- **Hot path and compression**: zlib level 2, compression golden vectors, FIFO and
  runtime progress during large compressions, benchmarks split by area.
- **Start-up messages** name the configuration source, the console user and the number
  of messaging users, and the commands that replace default credentials.

### Fixed

- **Lost queue messages with concurrent producers**: a message enqueued by another
  connection with a lower sequence number after a consumer's cursor had moved past it was
  never dispatched.

## [0.1.0] - 2026-10-05

The initial broker.

### Added

- **In-memory OpenWire broker** compatible with the ActiveMQ Java client 5.18.x and 6.x
  (protocol versions 9-12): queues, topics, temporary destinations, local transactions,
  selectors, expiration with a sweeper, message compression and ActiveMQ-compatible IDs.
- **Admin console** and JSON API on port 8161.
- **Windows service**: `mqrust.exe service install | uninstall | start | stop | status`.
- **Configuration** in `mqrust.toml`, with errors that name the full key path; socket
  buffers sized for large dispatches and the runtime sized like the JVM
  (`broker.processors`).
- **Tests**: Rust unit and integration tests, golden vectors from the Java OpenWire
  marshaller, Java client acceptance tests and the ActiveMQ comparison scripts.
