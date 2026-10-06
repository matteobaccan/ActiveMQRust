# cli-setup Specification

## Purpose
Defines the broker command line for help output, admin and messaging user setup, password rules, unattended configuration, safe config-file edits and clear start-up messages.
## Requirements
### Requirement: Short help with getting started
`mqrust.exe -h` SHALL print, in this order: one line saying what the program is; a "Getting started" section with four numbered steps and their commands; the commands grouped as "Setup" (`init-config`, `set-admin`, `user`, `hash-password`, `check-config`) and "Windows service" (`service`); the options grouped as "Configuration", "Network" and "Performance"; and a last line pointing to `--help` for details. It SHALL fit in 40 lines of 100 columns. The four steps SHALL be:
1. `mqrust.exe init-config` — create `mqrust.toml` next to the executable;
2. `mqrust.exe set-admin` — choose the admin console user and password;
3. `mqrust.exe user add <name>` — create a user for the JMS/OpenWire clients;
4. `mqrust.exe` — start the broker (or `mqrust.exe service install` to run it as a Windows service).

#### Scenario: Short help
- **WHEN** an operator runs `mqrust.exe -h`
- **THEN** the output starts with the product line, shows the four getting-started steps with their commands, and is at most 40 lines long

#### Scenario: Help without arguments to a command group
- **WHEN** an operator runs `mqrust.exe user`
- **THEN** the help of the `user` command is printed and the exit code is 2

### Requirement: Long help
`mqrust.exe --help` SHALL print everything in the short help plus: an explanation of the two kinds of users ("admin console user: `[admin]`, one user, for the web console at http://127.0.0.1:8161" and "messaging users: `[[users]]`, one or more, for JMS/OpenWire clients"), the default `admin`/`admin` for both when there is no configuration file and why it must be changed, where the configuration file is searched (`--config`, then `mqrust.toml` next to the executable, then built-in defaults), at least three complete examples, and the exit codes (0 success, 1 runtime error, 2 configuration or usage error). Every command's own `--help` SHALL include at least one example.

#### Scenario: Two kinds of users explained
- **WHEN** an operator runs `mqrust.exe --help`
- **THEN** the output explains `[admin]` and `[[users]]`, says which program uses each, and shows `set-admin` and `user add` as the way to set them

#### Scenario: Command example
- **WHEN** an operator runs `mqrust.exe set-admin --help`
- **THEN** the output contains at least one example command line

### Requirement: Set the admin console user
`mqrust.exe set-admin [--username <name>] [--password-stdin] [--config <file>]` SHALL set the console administrator. Interactively it SHALL ask for the username (showing the current one as default, `admin` if none), then for the password twice with hidden input. It SHALL write `[admin] username` and `password_hash` (Argon2id) and remove any `password` key in `[admin]`. If the configuration file does not exist, it SHALL create it from the commented template first and say so. It SHALL end by printing the file written, the console URL, and "Restart the broker (or `mqrust.exe service stop` and `service start`) to apply." The password SHALL never be printed, logged, or written in clear.

#### Scenario: First setup
- **WHEN** there is no `mqrust.toml` and the operator runs `mqrust.exe set-admin`, enters `ops` and the same valid password twice
- **THEN** `mqrust.toml` is created from the template, its `[admin]` section has `username = "ops"` and a `password_hash` starting with `$argon2id$`, no `password` key, and the command prints the file path, the console URL and the restart notice

#### Scenario: Passwords do not match
- **WHEN** the two typed passwords differ
- **THEN** the command prints "Passwords do not match", asks again up to three times, then exits with code 2 without changing the file

#### Scenario: Login works after restart
- **WHEN** `set-admin` has set `ops` with a new password and the broker is restarted
- **THEN** the console accepts `ops` with that password and refuses `admin`/`admin`

### Requirement: Manage messaging users
`mqrust.exe user add <name>` SHALL add a `[[users]]` entry with a `password_hash`, refusing a name that already exists; `user passwd <name>` SHALL replace the password of an existing user; `user remove <name>` SHALL remove a user, refusing to remove the last one unless `broker.allow_anonymous = true`; `user list` SHALL print the usernames, one per line, without any secret. `add` and `passwd` SHALL ask for the password twice with hidden input. Each command that changes the file SHALL print the restart notice.

#### Scenario: Add a user
- **WHEN** the operator runs `mqrust.exe user add app1` and types a valid password twice
- **THEN** the file gains a `[[users]]` entry `username = "app1"` with a `password_hash`, and after a restart a JMS client can connect as `app1`

#### Scenario: Duplicate user
- **WHEN** the operator runs `mqrust.exe user add app1` and `app1` already exists
- **THEN** the command exits with code 2, says the user exists and suggests `user passwd app1`, and the file is unchanged

#### Scenario: Last user
- **WHEN** only `app1` exists, anonymous access is off, and the operator runs `mqrust.exe user remove app1`
- **THEN** the command refuses with code 2 and explains that clients could no longer connect

#### Scenario: List
- **WHEN** the operator runs `mqrust.exe user list`
- **THEN** the usernames are printed and no password or hash appears

### Requirement: Password rules
A password set by `set-admin`, `user add` or `user passwd` is insecure when it has fewer than 8 characters, equals the username, or is `admin` or `password`. At the interactive prompt an insecure password SHALL be explained in one line followed by `Use it anyway? [y/N]`: only `y` or `yes` accepts it (then it is repeated as usual), any other answer asks again. An empty password SHALL always be refused. With `--password-stdin` an insecure password SHALL be refused, since nobody can confirm it. Usernames SHALL be 1–64 characters of letters, digits, `.`, `_`, `-` or `@`.

#### Scenario: Weak password
- **WHEN** the operator types `admin` as the new admin password and presses Enter at `Use it anyway? [y/N]`
- **THEN** the command explains why it is insecure and asks again

#### Scenario: Insecure password confirmed
- **WHEN** the operator types `password` as the new admin password and answers `y`
- **THEN** the password is asked a second time and, if it matches, it is used

#### Scenario: Invalid username
- **WHEN** the operator runs `mqrust.exe user add "bad name"`
- **THEN** the command exits with code 2 and lists the allowed characters

### Requirement: Unattended setup
`set-admin`, `user add`, `user passwd` and `hash-password` SHALL accept `--password-stdin`, reading the password from the first line of standard input without asking twice, and applying the same rules. Without `--password-stdin`, when standard input is not a terminal, they SHALL exit with code 2 and suggest `--password-stdin` instead of waiting.

#### Scenario: Scripted admin
- **WHEN** a script runs `echo S3cure-pass| mqrust.exe set-admin --username ops --password-stdin`
- **THEN** the admin is set without any prompt and the exit code is 0

#### Scenario: No terminal
- **WHEN** `mqrust.exe set-admin` is run with standard input redirected and without `--password-stdin`
- **THEN** it exits with code 2 and suggests `--password-stdin`

### Requirement: Safe editing of the configuration file
The setup commands SHALL edit the file chosen by `--config`, or `mqrust.toml` next to the executable. They SHALL keep comments, key order and every key they do not change. They SHALL refuse to edit a file that is not valid TOML or that fails validation for a reason unrelated to the change, naming the error. They SHALL write a temporary file in the same folder and replace the original only after the write succeeds, so an interrupted command never leaves a broken file. The resulting file SHALL pass `mqrust.exe check-config`.

#### Scenario: Comments kept
- **WHEN** `mqrust.toml` contains comments and custom `[broker]` settings and the operator runs `set-admin`
- **THEN** only the `[admin]` keys change; the comments and the `[broker]` settings are byte-for-byte unchanged

#### Scenario: Invalid file
- **WHEN** `mqrust.toml` contains a syntax error and the operator runs `user add app1`
- **THEN** the command exits with code 2, names the line of the error, and the file is unchanged

### Requirement: Clear start-up messages
At start-up the broker SHALL log: the configuration source (file path or "built-in defaults"); `OpenWire listening on <address>`; `admin console on http://<address>` with the admin username; and the number of messaging users. When the built-in default credentials are in use, it SHALL log a warning naming both kinds of users and the commands to fix them: `mqrust.exe set-admin` and `mqrust.exe user add <name>`. When started without a configuration file in an interactive console, it SHALL also print one line suggesting `mqrust.exe init-config`.

#### Scenario: Defaults warning
- **WHEN** the broker starts without a configuration file
- **THEN** the log contains a warning that names `admin`/`admin`, `mqrust.exe set-admin` and `mqrust.exe user add <name>`

#### Scenario: Configured start
- **WHEN** the broker starts with a file that sets the admin `ops` and two users
- **THEN** the log shows the file path, the OpenWire address, the console URL with `ops`, and `2 messaging users`, and no default-credentials warning

