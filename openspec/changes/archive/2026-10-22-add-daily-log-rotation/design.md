## Context

`src/logging.rs` sets up `tracing_subscriber::fmt` with a local timestamp (`%Y-%m-%d %H:%M:%S`) and either stdout (`init_stdout`, console mode) or a file opened in append mode (`init_file`, service mode, `mqrust.log` next to the executable, fixed in `src/service.rs`). There is no rotation, compression or retention. `[log]` has one key, `level`. `chrono` and `flate2` are already dependencies. The broker runs as a Windows service or in console mode; on macOS only console mode exists.

## Goals / Non-Goals

**Goals:**
- One file per local day, active file with a fixed name, previous days archived with their date and compressed.
- No lost or duplicated lines across a rotation; logging never blocks message dispatch on compression or deletion work.
- Recovery from interruptions (crash during compression, broker stopped across midnight).
- Same behaviour in console and service mode, on Windows and macOS.

**Non-Goals:**
- Size-based rotation or a size cap on a day (decided: rotation by day is enough).
- Changing the level at runtime from the admin console.
- Two broker processes sharing one `log.dir` (each instance needs its own folder; not detected).
- Structured (JSON) logs, syslog or the Windows Event Log.

## Decisions

### D1. A custom rotating writer, not `tracing-appender`
A `RotatingFile` type implements `std::io::Write` and is passed to `tracing_subscriber` through `with_writer` (behind a `Mutex`, as today). On every write it compares the current local date with the date of the open file (one `chrono::Local::now()` per line, already paid for the timestamp) and rotates first when the day changed.
*Alternative*: `tracing-appender`'s daily `RollingFileAppender`. Rejected: it names the active file with the date (no fixed `mqrust.log`), has no compression and no rotation of a stale file at startup.

### D2. Rotation also without traffic: a midnight timer
A background thread sleeps until the next local midnight (recomputed after each wake, so DST changes and clock adjustments are handled) and asks the writer to rotate if the date changed. This way the previous day is archived and compressed even when nothing is logged after midnight.
*Alternative*: rotate only on the first line of the new day. Rejected: at `warn` level a quiet broker could keep yesterday's file active for days.

### D3. Archive date = date of the content
The writer remembers the date of the file it has open. At startup, an existing `mqrust.log` takes the local date of its last modification time; if that date is before today it is rotated before the first line is written, otherwise lines are appended. If the archive name already exists (clock moved back, restarts after a manual copy), a counter is added: `mqrust-YYYY-MM-DD-1.log.gz`, `-2`, …
*Alternative*: date of the rotation moment. Rejected: the archive made at 00:00 of the 6th would be named after the 6th while containing the 5th.

### D4. Rotation steps
Under the writer lock: flush, close, rename `mqrust.log` → `mqrust-YYYY-MM-DD.log`, open a new `mqrust.log`, write the header line. The lock is held only for rename and open (milliseconds); compression happens after the lock is released. If the rename fails (for example the file is opened exclusively by another program on Windows), the writer keeps appending to the old file and retries at the next line after one minute; the failure is reported once on stderr.
*Alternative*: copy-and-truncate. Rejected: lines written during the copy can be lost and it doubles the I/O.

### D5. Compression in one background thread
One compressor thread receives the paths of renamed files through a channel and, for each one, writes `name.log.gz.tmp` with `flate2` (default level), syncs it, renames it to `name.log.gz` and only then deletes `name.log`. At startup the same thread first compresses every `mqrust-YYYY-MM-DD[-N].log` left in the folder and deletes every `*.log.gz.tmp` (an interrupted attempt). With `log.compress = false` this step is skipped and archives stay `.log`.
*Alternative*: compress inline under the lock. Rejected: a large day (debug level) would block every logging thread, and so message handling, for seconds.

### D6. Retention by the date in the name
After each rotation and at startup, the compressor thread deletes archives whose date in the file name is older than `today - retention_days`. Only names matching `mqrust-YYYY-MM-DD.log`, `mqrust-YYYY-MM-DD-N.log` and the same with `.gz` are considered; every other file in the folder is ignored. `retention_days = 0` disables deletion.
*Alternative*: by file modification time. Rejected: copying or touching archives would change what is deleted; the name is the stable fact.

### D7. Header line
Each new active file starts with an `INFO`-formatted line written directly by the writer, independent of `log.level`: `ActiveMQRust <version> log started: pid <pid>, config <path | built-in defaults>`. It is the only line written regardless of level.
*Alternative*: a normal `tracing::info!` at startup. Rejected: absent at `warn`/`error` level and missing from files created by rotation.

### D8. Outputs per mode
Console mode uses two layers, stdout and file, with the same format and level; `log.file = false` keeps stdout only. Service mode always uses the file (stdout does not exist); `log.file` is ignored there. When the configuration cannot be loaded, the service logs the error to `logs\mqrust.log` next to the executable (the default folder) so the reason is always findable.
*Alternative*: file only in service mode. Rejected by product decision: on macOS console mode is the normal way to run.

### D9. Location and keys
`log.dir`: absolute, or relative to the folder of the executable (as `mqrust.toml`); created if missing. `log.retention_days`: integer 0–3650. `log.compress`, `log.file`: booleans. Invalid values are configuration errors naming the key, exit code 2, as for the other sections. `--log-level` (same values as `log.level`) is added to `Overrides`.
*Alternative*: relative to the configuration file. Rejected: the service and the template already resolve paths from the executable folder, and the user decided "next to the executable".

### D10. Write errors never stop the broker
If opening or writing the file fails, the writer reports the error once on stderr (and, in service mode, keeps going without a file), drops the line for the file output and retries opening at the next rotation or after one minute. Console stdout output is unaffected.
*Alternative*: fail the startup when the folder is not writable. Rejected: losing logs is better than losing the broker; `check-config` reports an unwritable folder instead.

## Risks / Trade-offs

- [A whole day at `trace` level can be several GB before compression] → documented; no size limit by decision; compression ratio on log text is typically 10–20×.
- [Rename fails while an editor or antivirus holds `mqrust.log` open on Windows] → retry every minute (D4), stderr warning once; files are opened with share-delete where the platform allows.
- [Clock moved back across midnight] → counter suffix on the archive name (D3), no overwrite.
- [Two instances with the same `log.dir`] → out of scope, documented in the README.
- [Upgrade moves the service log from `mqrust.log` to `logs\mqrust.log`] → the old file is left untouched; README and CHANGELOG mention the move.

## Migration Plan

No data to migrate. After the upgrade the service writes to `logs\mqrust.log`; the old `mqrust.log` next to the executable can be deleted by hand. Rollback: the previous version writes again to `mqrust.log` next to the executable; the `logs` folder is ignored.

## Open Questions

None.
