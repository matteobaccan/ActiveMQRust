## 1. Help

- [x] 1.1 Rewrite the clap definitions: short help with product line, "Getting started" in four steps, commands grouped as Setup and Windows service, options grouped as Configuration, Network and Performance, pointer to `--help`
- [x] 1.2 Long help: the two kinds of users, default credentials, configuration search order, three examples, exit codes; an example in every command's help
- [x] 1.3 `user` and `service` without a subcommand print their help and exit with code 2
- [x] 1.4 Test that `-h` is at most 40 lines and contains the four steps; snapshot of `--help`

## 2. Setup module

- [x] 2.1 Add `toml_edit`; create `src/setup.rs` with load/validate/atomic-write of the configuration file (create from template when missing, refuse invalid files naming the error)
- [x] 2.2 Password rules and username rules in one function; hidden double prompt with three attempts; `--password-stdin`; fail fast when stdin is not a terminal
- [x] 2.3 Unit tests: comments and other keys preserved, `password` removed when `password_hash` is set, invalid file untouched, interrupted write leaves the original, rules accept and refuse the documented cases

## 3. Commands

- [x] 3.1 `set-admin` with `--username`, `--password-stdin`, `--config`; final message with file path, console URL and restart notice
- [x] 3.2 `user add`, `user passwd`, `user remove` (last-user protection), `user list` (no secrets)
- [x] 3.3 `hash-password --password-stdin`
- [x] 3.4 Integration tests running the executable: first setup from no file, scripted setup, duplicate user, last user, weak password, mismatched passwords; the result passes `check-config`

## 4. Start-up messages

- [x] 4.1 Log configuration source, OpenWire address, console URL with admin username, number of messaging users
- [x] 4.2 Default-credentials warning naming `set-admin` and `user add <name>`; `init-config` hint on interactive consoles only
- [x] 4.3 Test: log contents with defaults and with a configured file

## 5. Documentation and verification

- [x] 5.1 Rewrite the README "Getting started" around `init-config`, `set-admin`, `user add` and start
- [x] 5.2 End-to-end check: set admin and a user with the new commands, restart, log in to the console and connect a Java client
- [x] 5.3 `openspec validate improve-cli-setup` passes
