<!-- ActiveMQRust by Matteo Baccan - SPDX-License-Identifier: MIT -->

# Working on ActiveMQRust

ActiveMQRust is written by a coding agent from specifications. This file is what an agent
should know before touching the repository; `openspec/config.yaml` holds the product context
and the hard constraints, and is the source of truth when the two disagree.

## Hard constraints, in short

Read them in full in `openspec/config.yaml`. The ones that change how you work:

- **Always release**: `cargo build --release` and `cargo test --release`. No debug builds, no
  debug artifacts; acceptance and benchmark runs use the release binary.
- **All documentation in English**, including commit messages and this file.
- **Commits carry only the user's name and never mention AI tools**: no `Co-Authored-By`
  trailer, no session link, no tool name in the message.
- Windows x64 is the deliverable: one dependency-free `mqrust.exe` with a static CRT. macOS
  is built and tested as well, and the code has platform-specific branches.

## The workflow: specifications first

Work is described before it is written, under `openspec/`:

- `openspec/specs/<capability>/spec.md` — what the broker does today, one file per capability,
  `### Requirement:` blocks with `#### Scenario:` blocks in SHALL/WHEN/THEN form.
- `openspec/changes/<change>/` — work that is planned or in progress: `proposal.md` (why and
  what), `design.md` (decisions, each with the alternatives considered), `tasks.md` (the
  checklist you tick as you go) and `specs/<capability>/spec.md`, the delta, headed
  `## ADDED Requirements`, `## MODIFIED Requirements` or `## REMOVED Requirements`.
- `openspec/changes/archive/YYYY-MM-DD-<change>/` — changes that shipped.

Implementing a change means: read all four files, write the code and the tests, tick every
task, and only then archive it — move the directory under `archive/` with today's date and
apply its delta to the capability specs (append what is `ADDED`, replace the requirement of
the same title for `MODIFIED`). The `.claude/skills/openspec-*` skills describe the same
steps; the `openspec` CLI they mention is not always installed, and the files can be edited
by hand.

## Build, test, lint

These are the three commands CI runs, in this order. Run all of them before pushing:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --release
```

The Rust suite is `src/` unit tests plus `tests/`: `broker_semantics`, `connection_semantics`,
`selector_semantics`, `admin_http`, `cli_setup`, `codec_golden`, `compression`, `hot_path`,
`tls_transport`.

The Java side lives in `tests/java-it` and talks to a running broker with the real ActiveMQ
driver, built for two client versions:

```
cd tests/java-it && ./mvnw -q -Pamq5 package -DskipTests      # or -Pamq6
java -jar target/amq5/mqrust-acceptance.jar <mode> --url tcp://127.0.0.1:61616
```

Modes: `accept`, `integration`, `console`, `tls`, `compression`, `compression-golden`,
`expiry-options`, `bench`. The `tls` mode needs a truststore
(`-Djavax.net.ssl.trustStore=... -Djavax.net.ssl.trustStorePassword=...`) and an `ssl://` URL.

Before adding a dependency, and whenever the lockfile moves, run `cargo audit`: the
`guardrails/scan` check on a pull request fails on an unmaintained or vulnerable crate, and
its finding is only readable on the Guardrails dashboard, which an agent cannot open.

## CI and releases

`.github/workflows/build.yml` runs on **main, tags, pull requests and manual dispatch only**:
pushing a branch builds nothing. To compile and test a branch without opening a pull request,
start the workflow by hand on that ref; the release job is tied to tags, so a manual run
publishes nothing.

Releasing is automatic and driven by `Cargo.toml`. Set a non-pre-release version, give
`CHANGELOG.md` a matching `## [x.y.z] - YYYY-MM-DD` section, and once that commit is on main
`release-tag.yml` creates the tag `vx.y.z` and starts `build.yml` on it, which builds the
archives and publishes the GitHub Release with the CHANGELOG section as its body. A version
ending in `-dev` is never tagged.

## Conventions that bite

- **`mqrust.example.toml` must be byte-identical to the `TEMPLATE` constant in
  `src/config.rs`**; two tests check it (`template_matches_the_example_file`,
  `example_file_matches_the_template`). Change both together.
- **The help text is a snapshot** in `tests/data/help-long.txt`. After changing the CLI, run
  `MQRUST_UPDATE_SNAPSHOTS=1 cargo test --release --test cli_setup long_help_snapshot` and
  commit the file.
- **The admin console has no JavaScript.** The Content-Security-Policy has no `script-src`,
  and `style-src 'self'` forbids inline `style` attributes and inline `<style>` elements: every
  rule goes in `src/admin/style.css`, and interaction is built from links, forms, `<details>`
  and CSS `:checked`. SVG presentation attributes are fine.
- **Configuration errors name their key** (`log.retention_days`, `tls.cert`) and exit with
  code 2. `check-config` performs the same checks as startup.
- **Timing-sensitive tests need wide margins.** CI runners stall for hundreds of milliseconds;
  a test that sleeps 40 ms and expects a 150 ms deadline to hold will fail there.
- **`process_memory()` returns `(0, 0)` on Linux**: it is implemented for Windows and macOS
  only, so memory figures and the process-memory chart are empty in a Linux container.
- **Code behind `#[cfg(windows)]` is first compiled by CI.** Cross-checking to
  `x86_64-pc-windows-msvc` fails in a Linux container because `ring` and `mimalloc` need the
  MSVC toolchain. Keep that surface small: put the logic in a cross-platform function with
  tests, and leave a call behind the gate.

## Where things are

| Path | What |
| --- | --- |
| `src/openwire/` | wire format, codec, model, properties, selectors' value types |
| `src/broker/` | destinations, entries, memory accounting, connections registry, compression |
| `src/connection.rs` | one client connection: negotiation, commands, transactions, cleanup |
| `src/server.rs` | listeners (plain and TLS), startup checks (`preflight`), shutdown |
| `src/admin/` | console pages, JSON API, sessions, sorting, XML view, memory history, bans UI |
| `src/config.rs` | the TOML file, validation and the commented template |
| `src/bans.rs`, `src/tls.rs`, `src/logging.rs` | bans and `bans.toml`, TLS material, daily log files |
| `src/service.rs` | the Windows service (`#[cfg(windows)]`) |
| `scripts/`, `tests/java-it/` | ActiveMQ comparison scripts, Java acceptance and integration |
