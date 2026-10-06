## Why

Releases were built by hand on one Windows machine and only for Windows. Users also need macOS binaries, and every change should be built and tested automatically on both systems before it is merged or released, as in the maintainer's other Rust projects.

## What Changes

- GitHub Actions `build.yml`: on every push and pull request, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and the release-mode test suite on Windows x86_64 and macOS ARM64; on version tags (and manual runs) release binaries for both systems; on tags, a GitHub Release with a downloads grid and the CHANGELOG section of the version.
- `release-tag.yml`: when the version in `Cargo.toml` changes on `main` and `CHANGELOG.md` has its section, tag `vX.Y.Z` and start the release build.
- The broker builds and runs on macOS: the Windows service, the Windows version resource and the Win32 process-memory API are Windows-only; on macOS the admin console reads process memory with `proc_pid_rusage`; the `service` command explains that it is Windows-only.
- `CHANGELOG.md`, `scripts/test.cmd` / `test.ps1` (format, then test), README badge and "Download" section, `rustfmt.toml`, `.gitattributes`.

## Capabilities

### New Capabilities

- `release-pipeline`: CI checks, release binaries for Windows and macOS, automatic tagging and publishing, cross-platform build.

### Modified Capabilities

None.

## Impact

- `.github/workflows/`, `.github/scripts/downloads_table.py`, `CHANGELOG.md`, `scripts/test.*`, `Cargo.toml` (target-specific dependencies), `src/main.rs`, `src/lib.rs`, `src/admin/mod.rs`, `src/server.rs`, README.
