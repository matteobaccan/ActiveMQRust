## ADDED Requirements

### Requirement: Command-line program
`mqrust.exe` SHALL be a command-line program: started from a console with no subcommand it runs the broker in the foreground, logging to stdout, until Ctrl+C or console close. Every other mode SHALL be selected by a subcommand.

#### Scenario: Foreground run
- **WHEN** a user runs `mqrust.exe` in a console
- **THEN** the broker runs in the foreground and logs to stdout

### Requirement: Install as a Windows service
`mqrust.exe service install` SHALL register the broker as a Windows service with:
- service name `ActiveMQRust` (override with `--name <name>`);
- display name `ActiveMQRust <name suffix if overridden>` and description "ActiveMQRust OpenWire message broker";
- start type automatic;
- account LocalSystem.

The registered command line SHALL be the absolute path of the current `mqrust.exe` followed by `service run`, plus `--config <absolute path>` when `--config` is given to `install`. A configuration file is otherwise looked up next to the executable as usual. The command SHALL print the outcome and exit with code 0 on success.

#### Scenario: Install
- **WHEN** an administrator runs `mqrust.exe service install`
- **THEN** a service named `ActiveMQRust` with automatic start appears in the Windows Services console, and the command exits with code 0

#### Scenario: Install with configuration
- **WHEN** an administrator runs `mqrust.exe service install --config C:\mq\mqrust.toml`
- **THEN** the registered command line contains `service run --config C:\mq\mqrust.toml`

#### Scenario: Already installed
- **WHEN** `mqrust.exe service install` runs and a service with the same name exists
- **THEN** the command reports that the service already exists, changes nothing and exits with a non-zero code

#### Scenario: Not an administrator
- **WHEN** a user without administrator rights runs `mqrust.exe service install`
- **THEN** the command reports that administrator rights are required and exits with a non-zero code

### Requirement: Uninstall the Windows service
`mqrust.exe service uninstall [--name <name>]` SHALL stop the service if it is running (waiting at most 30 seconds), then delete it. It SHALL report and exit with a non-zero code if the service does not exist or administrator rights are missing.

#### Scenario: Uninstall a running service
- **WHEN** an administrator runs `mqrust.exe service uninstall` while the service is running
- **THEN** the service is stopped, then removed from the Services console, and the command exits with code 0

#### Scenario: Service not installed
- **WHEN** `mqrust.exe service uninstall` runs and no such service exists
- **THEN** the command reports that the service is not installed and exits with a non-zero code

### Requirement: Service control commands
`mqrust.exe service start`, `service stop` and `service status` (each with optional `--name`) SHALL start the service, stop it, and print its state (`running`, `stopped`, `start pending`, `stop pending`, `not installed`).

#### Scenario: Status
- **WHEN** the service is installed and running and `mqrust.exe service status` is run
- **THEN** it prints `running`

### Requirement: Running under the Service Control Manager
`mqrust.exe service run` SHALL be the entry point used by the Service Control Manager. It SHALL report `running` once the OpenWire listener is bound and SHALL handle Stop and Shutdown requests with the same graceful shutdown as Ctrl+C (send `ShutdownInfo`, wait at most 5 seconds). Because a service has no console, it SHALL write the log to `mqrust.log` next to the executable (appending, same format and level as the console log). If startup fails, for example because the configuration is invalid or the port is in use, the service SHALL log the error and report a stopped state with a non-zero exit code.

#### Scenario: Service start and stop
- **WHEN** the installed service is started, then stopped from the Services console
- **THEN** `mqrust.log` contains the startup lines and a shutdown line, and clients received `ShutdownInfo` before the process ended

#### Scenario: Service startup failure
- **WHEN** the service starts with an invalid configuration file
- **THEN** `mqrust.log` contains the configuration error and the service ends in the stopped state

#### Scenario: Run outside the service manager
- **WHEN** a user runs `mqrust.exe service run` from a console
- **THEN** the command reports that it must be started by the Service Control Manager and exits with a non-zero code
