## Why

As a Windows service the broker appends every line to a single `mqrust.log` next to the executable, forever: the file grows without limit, old days cannot be removed without stopping the service, and finding one day's events means scanning the whole file. In console mode (the only mode on macOS) the log goes to stdout only and is lost when the window closes. Operators need one file per day, the previous days compressed, old days removed automatically, and the same behaviour in console and service mode.

## What Changes

- Logs go to a folder, `log.dir` (default `logs` next to the executable), into an active file with a fixed name, `mqrust.log`.
- At local midnight the active file is closed, renamed with the date of the day it contains and compressed to `mqrust-YYYY-MM-DD.log.gz`; a new `mqrust.log` is started. At startup, an `mqrust.log` from a previous day is rotated the same way before the first line is written.
- Compression with gzip, in a background thread, written to a temporary file and renamed, the uncompressed file deleted only afterwards; archives left uncompressed by an interruption are compressed at the next startup. `log.compress = false` keeps the archives as `.log`.
- Retention: `log.retention_days` (default 30, `0` = keep everything) removes archives older than that, and only files that follow the archive naming pattern.
- No size limit: a day is never split.
- Every new file starts with a header line: version, process ID and configuration file.
- Console mode writes to stdout **and** to the file (`log.file = true`, can be turned off); the service always writes to the file.
- `log.level` is unchanged; a new `--log-level` option overrides it for one run.
- A logging error (disk full, folder not writable) never stops the broker: one warning on stderr, a new attempt at the next rotation.
- **BREAKING (location)**: the service log moves from `mqrust.log` next to the executable to `logs\mqrust.log`. An existing `mqrust.log` next to the executable is left untouched.

## Capabilities

### New Capabilities

- `log-files`: log folder and file names, daily rotation at midnight and at startup, gzip compression, retention, header line, console and service outputs, `--log-level`, `[log]` keys and their validation, behaviour on write errors.

### Modified Capabilities

None. `log.level` keeps its values and meaning; the `cli-setup` and admin specs do not describe the log file.

## Impact

- Code: `src/logging.rs` (rotating file writer, compressor thread, retention), `src/service.rs` (log path from the configuration, fallback when the configuration cannot be loaded), `src/main.rs` (console mode: stdout plus file, `--log-level`), `src/config.rs` (`[log]` keys `dir`, `file`, `retention_days`, `compress`; `Overrides.log_level`), `mqrust.example.toml` template, README.
- No new crates: `chrono` (local date) and `flate2` (gzip) are already dependencies.
- ActiveMQ comparison: ActiveMQ 5.x/6.x (log4j2) rolls `activemq.log` by size with a fixed number of backups; ActiveMQRust deliberately rolls by day, without a size limit, and keeps archives by age.
