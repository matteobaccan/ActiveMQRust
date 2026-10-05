## ADDED Requirements

### Requirement: Optional configuration file
The broker SHALL look for its TOML configuration in this order: the path given with `--config <file>`, then `mqrust.toml` next to the executable. If neither is found, it SHALL start with the built-in defaults. If `--config` names a file that does not exist, the broker SHALL exit with an error.

#### Scenario: No configuration file
- **WHEN** the broker starts with no `--config` and no `mqrust.toml` next to the executable
- **THEN** it starts with the built-in defaults and logs that no configuration file is in use

#### Scenario: File next to the executable
- **WHEN** `mqrust.toml` exists next to the executable and no `--config` is given
- **THEN** the broker loads that file

#### Scenario: Missing explicit file
- **WHEN** the broker is started with `--config C:\missing.toml` and the file does not exist
- **THEN** the broker prints an error naming the file and exits with code 2

### Requirement: Built-in defaults
Without a configuration file, the broker SHALL use these defaults: OpenWire on `0.0.0.0:61616`; admin on `127.0.0.1:8161`; one OpenWire user `admin`/`admin`; admin user `admin`/`admin`; `broker.name` `ActiveMQRust`; log level `info`. Any key missing from a configuration file SHALL take its default value.

#### Scenario: Default port
- **WHEN** the broker starts without a configuration file
- **THEN** it listens for OpenWire on port 61616 on all interfaces

#### Scenario: Partial file
- **WHEN** the configuration file contains only a `[[users]]` section
- **THEN** all other settings take their default values

#### Scenario: Default credentials warning
- **WHEN** the broker runs with the default `admin`/`admin` credentials
- **THEN** it logs a warning at every start asking to configure `[[users]]` and `[admin]`

### Requirement: Configuration keys
The configuration file SHALL support at least: `[broker]` `name`, `bind`, `port`, `max_frame_size_mb` (default 100) and `allow_anonymous` (default false); `[admin]` `bind`, `port`, `username`, and `password` or `password_hash`; `[log]` `level`; `[[users]]` entries with `username`, and `password` or `password_hash`. Later changes add their own keys. Unknown keys SHALL cause a validation error that names the key.

#### Scenario: Custom bind and port
- **WHEN** the file sets `[broker] bind = "127.0.0.1"` and `port = 61617`
- **THEN** the broker listens for OpenWire only on `127.0.0.1:61617`

#### Scenario: Unknown key
- **WHEN** the file contains the unknown key `prot = 1` in `[broker]`
- **THEN** the broker reports an error naming `broker.prot` and exits with code 2

### Requirement: Configuration validation
At startup, and with `check-config`, the broker SHALL validate the configuration: ports in range, parseable IP addresses, at least one user unless `allow_anonymous` is true, an admin password, no duplicate usernames, and exactly one of `password` or `password_hash` per credential. A validation error SHALL print the offending field and exit with code 2.

#### Scenario: Duplicate users
- **WHEN** two `[[users]]` entries have the same `username`
- **THEN** the broker reports the duplicate username and exits with code 2

#### Scenario: Both password forms
- **WHEN** a user entry has both `password` and `password_hash`
- **THEN** the broker reports that entry and exits with code 2

### Requirement: Command-line interface
`mqrust.exe` SHALL support:
- no arguments: start the broker;
- `--config <file>`;
- `--bind <ip>` and `--port <n>` for OpenWire;
- `--admin-bind <ip>` and `--admin-port <n>`;
- `hash-password`: read a password with hidden input and print an Argon2id hash;
- `check-config [--config <file>]`: validate and exit with code 0 or 2;
- `init-config`: write a commented `mqrust.toml` next to the executable, never overwriting an existing file;
- `--version`: print `ActiveMQRust <version>`.

Precedence SHALL be command line, then file, then defaults.

#### Scenario: Port override
- **WHEN** the file sets `port = 61617` and the broker is started with `--port 61620`
- **THEN** the broker listens for OpenWire on port 61620

#### Scenario: Version
- **WHEN** `mqrust.exe --version` is run
- **THEN** it prints `ActiveMQRust <crate version>` and exits with code 0

#### Scenario: init-config does not overwrite
- **WHEN** `mqrust.exe init-config` is run and `mqrust.toml` already exists
- **THEN** the existing file is left unchanged and the command reports that it exists

#### Scenario: hash-password
- **WHEN** `mqrust.exe hash-password` is run and a password is typed
- **THEN** the input is not echoed, and the printed `$argon2id$` hash is accepted as `password_hash` for that password
