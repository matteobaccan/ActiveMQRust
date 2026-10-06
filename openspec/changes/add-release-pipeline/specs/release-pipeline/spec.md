## ADDED Requirements

### Requirement: Checks on every change
Every push and pull request SHALL run, on Windows x86_64 and macOS ARM64, the release-mode test suite and `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` on one runner. A failing check SHALL fail the workflow.

#### Scenario: Unformatted code
- **WHEN** a pull request contains Rust code that `cargo fmt` would change
- **THEN** the workflow fails at the formatting step

### Requirement: Release binaries for Windows and macOS
On a version tag `vX.Y.Z` the pipeline SHALL build `activemq-rust-windows-x86_64-X.Y.Z.zip` (with `mqrust.exe`) and `activemq-rust-macos-arm64-X.Y.Z.tar.gz` (with `mqrust`), each also containing LICENSE, README.md and `mqrust.example.toml`, and SHALL fail if the tag does not match the version in `Cargo.toml`.

#### Scenario: Tag and version mismatch
- **WHEN** the tag `v0.3.1` is built while `Cargo.toml` says 0.3.0
- **THEN** packaging fails with an error naming both versions

### Requirement: Automatic release
When the version in `Cargo.toml` changes on `main` and `CHANGELOG.md` has a section for it, the pipeline SHALL create the tag `vX.Y.Z` and start the release build; after every build and test job succeeds it SHALL publish a GitHub Release whose body is a downloads grid followed by that CHANGELOG section.

#### Scenario: Version bump merged
- **WHEN** a commit on `main` sets `version = "0.3.0"` and CHANGELOG.md has `## [0.3.0]`
- **THEN** the tag `v0.3.0` is created and the release with both archives is published once all jobs pass

### Requirement: Cross-platform broker
The broker SHALL build and pass its tests on macOS. Windows-only parts (the Windows service, the version resource, Win32 memory counters) SHALL be compiled only on Windows. On macOS the admin console SHALL report process memory from `proc_pid_rusage`, and `mqrust service …` SHALL print that the Windows service is available on Windows only and exit with code 2.

#### Scenario: Service command on macOS
- **WHEN** `./mqrust service install` is run on macOS
- **THEN** it prints "the Windows service is available on Windows only" and exits with code 2
