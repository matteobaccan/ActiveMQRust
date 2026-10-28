# broker-logging Specification

## Purpose
Defines what the broker says while it runs: the lines of a successful start, the events worth an info line and those kept for debug, and the graceful shutdown. Where those lines go, how the files rotate and how the level is set are in `log-files`.
## Requirements
### Requirement: Startup log
At startup the broker SHALL log at info level, one line per item: product and version (`ActiveMQRust <version>`); the configuration source (file path and number of users, or built-in defaults); the OpenWire listen address and maximum protocol version; the admin listen address; and a final ready line.

#### Scenario: Startup lines
- **WHEN** the broker starts successfully
- **THEN** stdout contains, in order, the version line, the configuration line, the OpenWire listener line, the admin listener line and a ready line

#### Scenario: Listen failure
- **WHEN** port 61616 is already in use
- **THEN** the broker logs an error naming the address and exits with a non-zero code

### Requirement: Essential runtime events
At info level the broker SHALL log only:
- connection opened (remote IP, user);
- connection closed (reason);
- failed login, as a warning with remote IP and username, never the password;
- protocol errors, as warnings.

Per-message and per-destination events SHALL be logged at debug level only.

#### Scenario: Failed login is logged without password
- **WHEN** a client fails to authenticate
- **THEN** a warning with the remote IP and username is logged, and the password does not appear in the log

#### Scenario: No per-message logging at info
- **WHEN** the log level is `info` and messages flow through the broker
- **THEN** no log line is written per message

### Requirement: Graceful shutdown
On Ctrl+C or console window close (`CTRL_CLOSE_EVENT`), the broker SHALL stop accepting connections, send `ShutdownInfo` to connected clients, wait at most 5 seconds, log the number of in-memory messages discarded, and exit with code 0.

#### Scenario: Ctrl+C
- **WHEN** the user presses Ctrl+C while clients are connected
- **THEN** clients receive `ShutdownInfo`, the broker logs the discarded message count, and it exits within 5 seconds with code 0
