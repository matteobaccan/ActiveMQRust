## Context

The CLI is built with `clap` derive in `src/main.rs`. Configuration is read with `toml` + `serde` in `src/config.rs`; `hash-password` reads a hidden password with `rpassword` and prints an Argon2id hash from `src/auth.rs`; `init-config` writes the commented template. Nothing in the CLI edits an existing file.

## Goals / Non-Goals

**Goals:**
- A first-time operator can secure the broker by following `mqrust.exe -h`, without editing TOML by hand.
- Clear distinction between the console administrator and the messaging users.
- Scriptable setup for automated installs.

**Non-Goals:**
- Changing the configuration format, adding roles or per-destination permissions.
- Applying changes to a running broker without restart (no hot reload).
- Managing the Windows service account or firewall.

## Decisions

1. **`toml_edit` for file changes.** It keeps comments, whitespace and key order, which `toml` + `serde` round-trips lose. The edited document is validated by parsing it with the existing `config::from_toml` before writing. *Alternative:* text substitution with regexes — rejected as fragile.

2. **Atomic replace.** Write `mqrust.toml.tmp` in the same folder, flush, then `std::fs::rename` over the original (`MoveFileExW` with replace on Windows). Same folder keeps the rename atomic on one volume.

3. **Short and long help with clap.** `about`/`long_about`, `after_help` for the getting-started block and `after_long_help` for the explanations, examples and exit codes; `next_help_heading` to group options; `help_template` to put commands in "Setup" and "Windows service" groups. `arg_required_else_help` on `user` and `service`.

4. **Password input.** `rpassword` for hidden input (already a dependency), asked twice, three attempts. `std::io::IsTerminal` detects redirected stdin to fail fast instead of blocking. `--password-stdin` reads one line and trims the line ending only.

5. **Password rules in one function** in `src/setup.rs`, shared by all commands; hashing reuses `auth::hash_password`.

6. **Start-up messages** come from the validated `Config` (`source`, addresses, admin username, users count, `default_credentials`) in `server::run`; the interactive hint uses `IsTerminal` on stdout and is skipped when running as a service.

## Risks / Trade-offs

- [The operator edits the file while the broker runs and expects it to apply] → Every command prints the restart notice, with the service commands.
- [Removing the last user locks clients out] → Refused unless anonymous access is on.
- [`toml_edit` adds compile time and size] → Small; used only by setup commands. Size measured in the tasks.
- [Password piped through `echo` can end up in shell history] → The help recommends reading from a file or a secret store (`Get-Content secret.txt | mqrust.exe ... --password-stdin`).
