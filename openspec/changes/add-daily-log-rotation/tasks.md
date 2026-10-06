## 1. Configuration

- [ ] 1.1 Add `dir`, `file`, `retention_days`, `compress` to `LogSection` with defaults `logs`, `true`, `30`, `true`; validate types and range 0–3650, errors naming the key, exit code 2
- [ ] 1.2 Resolve `log.dir` against the executable folder when relative; expose the resolved path in `Config`
- [ ] 1.3 Add `--log-level` to the CLI and `Overrides.log_level`, refusing unknown values with code 2
- [ ] 1.4 `check-config`: report a `log.dir` that cannot be created or written
- [ ] 1.5 Update the `[log]` section of the template (`mqrust.example.toml` and the built-in `init-config` text) and its tests

## 2. Rotating writer

- [ ] 2.1 `RotatingFile` in `src/logging.rs`: open/append `mqrust.log`, remember the content date, rotate when the local date changes on write
- [ ] 2.2 Startup check: rotate an `mqrust.log` whose modification date is before today, using that date
- [ ] 2.3 Archive naming with the `-N` counter when the name is taken; never overwrite
- [ ] 2.4 Header line (version, PID, config path or `built-in defaults`) on every new or reopened file, independent of the level
- [ ] 2.5 Midnight timer thread that triggers rotation without traffic (recompute the wait after each wake)
- [ ] 2.6 Failure handling: rename or open failures keep the current file or drop file output, one stderr warning, retry at least once a minute
- [ ] 2.7 Make the clock injectable so tests can cross midnight without waiting

## 3. Compression and retention

- [ ] 3.1 Compressor thread fed by a channel: `.gz.tmp` → sync → rename `.gz` → delete `.log`
- [ ] 3.2 Startup recovery: delete `*.log.gz.tmp`, compress leftover uncompressed archives
- [ ] 3.3 `log.compress = false` keeps `.log` archives
- [ ] 3.4 Retention by the date in the file name, only matching names, `0` = keep all; run at startup and after each rotation

## 4. Wiring

- [ ] 4.1 Console mode: stdout layer plus file layer (unless `log.file = false`), same format and level
- [ ] 4.2 Service mode: file in the configured folder; on configuration errors, default `logs\mqrust.log` next to the executable
- [ ] 4.3 Remove the fixed `mqrust.log` path from `src/service.rs`

## 5. Tests

- [ ] 5.1 Unit tests with a fake clock: rotation at midnight with and without traffic, no lost or duplicated line, startup rotation, same-day append, counter suffix
- [ ] 5.2 Unit tests: compression output equals the original lines, interrupted `.tmp` recovery, compress off
- [ ] 5.3 Unit tests: retention keeps/deletes by date, ignores foreign files and `mqrust.log`, `0` keeps all
- [ ] 5.4 Config tests for every new key, range errors and `--log-level`
- [ ] 5.5 CLI test: console mode writes both stdout and `logs\mqrust.log`; `log.file = false` creates no folder; read-only folder keeps the broker running

## 6. Documentation

- [ ] 6.1 README: logging section (folder, names, rotation, compression, retention, `--log-level`, one folder per instance, difference from ActiveMQ) and test results after the run
- [ ] 6.2 CHANGELOG: entry under Unreleased, including the move of the service log to `logs\mqrust.log`
- [ ] 6.3 `cargo fmt` and the full test suite before committing
