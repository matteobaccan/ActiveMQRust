## ADDED Requirements

### Requirement: Log folder and active file
The broker SHALL write its log file into the folder `log.dir`, default `logs`, where a relative path is resolved against the folder of the executable and an absolute path is used as is. The folder SHALL be created if it does not exist. The active file SHALL always be named `mqrust.log`. Each line SHALL keep the current format: local timestamp `YYYY-MM-DD HH:MM:SS`, level, message.

#### Scenario: Default location
- **WHEN** the broker starts with no `[log]` keys and the executable is in `D:\mq`
- **THEN** it writes to `D:\mq\logs\mqrust.log`, creating `D:\mq\logs` if needed

#### Scenario: Absolute folder
- **WHEN** `log.dir = "E:\\logs\\mq"` is configured
- **THEN** the broker writes to `E:\logs\mq\mqrust.log`

### Requirement: Outputs in console and service mode
In console mode the broker SHALL write every log line both to stdout and to the log file when `log.file` is `true` (the default), and to stdout only when `log.file` is `false`. As a Windows service the broker SHALL always write to the log file and SHALL ignore `log.file`. When the service cannot load its configuration, it SHALL write the error to `logs\mqrust.log` next to the executable.

#### Scenario: Console with file
- **WHEN** the broker runs in console mode with default settings and logs a line
- **THEN** the same line appears on stdout and in `mqrust.log`

#### Scenario: Console without file
- **WHEN** the broker runs in console mode with `log.file = false`
- **THEN** lines appear on stdout and no log file or folder is created

#### Scenario: Service with invalid configuration
- **WHEN** the service starts with a configuration that fails validation
- **THEN** the validation error is written to `logs\mqrust.log` next to the executable

### Requirement: Header line
Every new active file, at startup and after each rotation, SHALL start with a header line in the normal line format at `INFO` level containing the broker version, the process ID and the path of the configuration file (or `built-in defaults`). The header SHALL be written whatever `log.level` is. When the broker starts and appends to an existing `mqrust.log` of the same day, the header SHALL be appended too.

#### Scenario: Header at warn level
- **WHEN** the broker starts with `log.level = "warn"`
- **THEN** the first line of the new `mqrust.log` contains the version, the PID and the configuration path

### Requirement: Daily rotation at midnight
When the local date changes, the broker SHALL close `mqrust.log`, rename it `mqrust-YYYY-MM-DD.log` where the date is the day the file contains, and continue in a new `mqrust.log`, even if no line is logged after midnight. No line SHALL be lost or written to both files. A day SHALL never be split by size. If an archive with the same name already exists, the broker SHALL add a counter, `mqrust-YYYY-MM-DD-1.log`, `-2`, and so on, and SHALL NOT overwrite it. If the rename fails, the broker SHALL keep writing to the current file, report the failure once on stderr and retry at least once a minute.

#### Scenario: Midnight with traffic
- **WHEN** the broker logs lines at 23:59:59 on 2026-10-05 and at 00:00:01 on 2026-10-06
- **THEN** the first line is in the archive of 2026-10-05 and the second in the new `mqrust.log`

#### Scenario: Midnight without traffic
- **WHEN** nothing is logged after midnight
- **THEN** within one minute after midnight the previous day's file is archived and a new `mqrust.log` with a header line exists

#### Scenario: Name already taken
- **WHEN** `mqrust-2026-10-05.log.gz` exists and the file of 2026-10-05 is rotated again
- **THEN** the new archive is `mqrust-2026-10-05-1.log.gz` and the existing one is unchanged

### Requirement: Rotation at startup
At startup, if `mqrust.log` exists and the local date of its last modification is before today, the broker SHALL rotate it with that date before writing the first line. If the date is today, the broker SHALL append to it.

#### Scenario: Stopped across midnight
- **WHEN** the broker stopped on 2026-10-05 and starts on 2026-10-07
- **THEN** the old `mqrust.log` becomes the archive of 2026-10-05 and a new `mqrust.log` is started

#### Scenario: Restart on the same day
- **WHEN** the broker is restarted on the day of the last line in `mqrust.log`
- **THEN** new lines are appended to the same file

### Requirement: Compression of archives
When `log.compress` is `true` (the default), each archive SHALL be compressed with gzip to `mqrust-YYYY-MM-DD[-N].log.gz` outside the logging path, so that logging threads never wait for compression. The compressed file SHALL be written as `.gz.tmp`, renamed to `.gz` when complete, and only then SHALL the uncompressed archive be deleted. At startup the broker SHALL delete leftover `.log.gz.tmp` files and compress any uncompressed archive left in the folder. When `log.compress` is `false`, archives SHALL stay as `.log`.

#### Scenario: Archive compressed
- **WHEN** the file of 2026-10-05 is rotated with default settings
- **THEN** `mqrust-2026-10-05.log.gz` appears, decompresses to exactly the lines of that day, and `mqrust-2026-10-05.log` no longer exists

#### Scenario: Interrupted compression
- **WHEN** the broker starts and finds `mqrust-2026-10-05.log` and `mqrust-2026-10-05.log.gz.tmp`
- **THEN** the `.tmp` file is deleted and `mqrust-2026-10-05.log.gz` is produced from the `.log` file

#### Scenario: Compression off
- **WHEN** `log.compress = false` and a day is rotated
- **THEN** the archive stays `mqrust-2026-10-05.log`

### Requirement: Retention
After each rotation and at startup, the broker SHALL delete archives whose date in the file name is earlier than today minus `log.retention_days` days (default `30`). Only files named `mqrust-YYYY-MM-DD.log`, `mqrust-YYYY-MM-DD-N.log` or the same names ending in `.gz` SHALL be considered; every other file in the folder, including `mqrust.log`, SHALL be left untouched. `log.retention_days = 0` SHALL keep every archive.

#### Scenario: Old archives removed
- **WHEN** today is 2026-10-06, `retention_days = 30`, and the folder holds `mqrust-2026-09-05.log.gz`, `mqrust-2026-09-06.log.gz` and `notes.txt`
- **THEN** after startup `mqrust-2026-09-05.log.gz` is deleted, `mqrust-2026-09-06.log.gz` and `notes.txt` are kept

#### Scenario: Keep everything
- **WHEN** `retention_days = 0`
- **THEN** no archive is ever deleted

### Requirement: Log level and command-line override
`log.level` SHALL keep its values `error`, `warn`, `info`, `debug`, `trace` (default `info`) and apply to stdout and file alike. The option `--log-level <level>` SHALL override `log.level` for that run, with the same values; another value SHALL be refused with exit code 2.

#### Scenario: Override
- **WHEN** the broker is started with `--log-level debug` and `log.level = "info"` in the file
- **THEN** debug lines are written

#### Scenario: Invalid override
- **WHEN** the broker is started with `--log-level verbose`
- **THEN** it exits with code 2 and lists the allowed values

### Requirement: Configuration keys
The `[log]` section SHALL accept `level`, `dir` (non-empty string), `file` (boolean), `retention_days` (integer 0–3650) and `compress` (boolean). A value of the wrong type or out of range SHALL be a configuration error that names the key (for example `log.retention_days`) and exits with code 2. `check-config` SHALL report when `log.dir` cannot be created or is not writable. The commented template SHALL list every key with its default.

#### Scenario: Out of range
- **WHEN** `log.retention_days = -1` is configured
- **THEN** startup fails with code 2 and a message naming `log.retention_days`

#### Scenario: Template
- **WHEN** `mqrust.exe init-config` writes the template
- **THEN** the `[log]` section lists `level`, `dir`, `file`, `retention_days` and `compress` commented with their defaults

### Requirement: Logging errors never stop the broker
If the log folder or file cannot be created or written (for example a full disk or a missing permission), the broker SHALL keep running and serving clients, report the problem once on stderr, keep writing to stdout in console mode, and retry the file at the next rotation or at least once a minute.

#### Scenario: Folder not writable
- **WHEN** the broker starts in console mode and `log.dir` points to a read-only folder
- **THEN** the broker accepts connections, logs to stdout, and prints one warning on stderr about the log file

### Requirement: Deliberate difference from ActiveMQ
ActiveMQ (log4j2) rolls `activemq.log` by size and keeps a fixed number of backups. ActiveMQRust SHALL instead rotate by local day, without a size limit, and keep archives by age (`retention_days`). This difference is intentional.

#### Scenario: Large day not split
- **WHEN** a single day produces a log larger than any size threshold
- **THEN** it stays one file, rotated only at midnight
