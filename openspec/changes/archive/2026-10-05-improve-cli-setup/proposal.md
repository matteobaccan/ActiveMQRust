## Why

Setting up the admin user today needs knowledge that the help does not give: run `init-config`, run `hash-password`, copy the hash, open `mqrust.toml`, find the `[admin]` section, replace `password` with `password_hash`, restart. The difference between the console administrator (`[admin]`) and the users of the messaging clients (`[[users]]`, both `admin` by default) is not explained anywhere in the command line, and `mqrust.exe -h` lists commands without saying in which order to use them. A first-time operator ends up running the broker with the default `admin`/`admin` credentials.

## What Changes

- `mqrust.exe -h` gives a short, grouped help with a "Getting started" section in 4 steps; `mqrust.exe --help` gives the long help with examples, where the configuration is searched, the two kinds of users, and the exit codes.
- New `set-admin` command: asks for the console username and the password (hidden, typed twice), checks the password rules, and writes `[admin] username` and `password_hash` into the configuration file, creating it from the commented template if it does not exist. Comments and other keys in the file are preserved.
- New `user` commands for the messaging users: `user add`, `user passwd`, `user remove`, `user list`, with the same rules and file handling.
- `--password-stdin` on `set-admin`, `user add`, `user passwd` and `hash-password` for unattended setup.
- Clearer start-up: the broker logs where its configuration comes from, the OpenWire and console addresses, and, when default credentials are in use, a warning that names the exact command to fix it.
- Every command that changes the file tells the operator to restart the broker or the Windows service to apply it.

## Capabilities

### New Capabilities

- `cli-setup`: command-line help, admin and user management commands, password rules, safe editing of the configuration file, start-up guidance.

### Modified Capabilities

None. The configuration format is unchanged; the new commands only write keys that already exist.

## Impact

- `src/main.rs` (help texts, new subcommands), new `src/setup.rs` (prompting, password rules, file editing), `src/server.rs` (start-up messages).
- New dependency `toml_edit`, to change the file while keeping comments and order (used only by the setup commands, not by the broker at run time).
- README: "Getting started" rewritten around the new commands.
- No change to the broker's behaviour, protocol or memory.
