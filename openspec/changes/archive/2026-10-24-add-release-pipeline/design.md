## Context

The maintainer's fasttail project already has a proven GitHub Actions setup (test matrix, release builds, tag automation, downloads grid); this change copies it and adapts it to ActiveMQRust.

## Goals / Non-Goals

**Goals:** automatic checks on Windows and macOS; reproducible release archives; one manual step to release (merge the version bump).
**Non-Goals:** Linux or Intel macOS binaries, installers, code signing and notarisation.

## Decisions

1. Same workflow structure, action versions and naming as fasttail; `cargo fmt --check` runs on the Windows runner (no Linux job).
2. Tests run in release mode (fat LTO), built first with `--no-run`, with a 10-minute timeout for the run.
3. Windows archive: `mqrust.exe`, LICENSE, README.md, `mqrust.example.toml`, plus a separate symbols zip with the PDB when present; macOS archive: `mqrust` and the same files.
4. macOS: `proc_pid_rusage` (`ri_resident_size` as Working Set, `ri_phys_footprint` as Private Bytes); SO_REUSEADDR on Unix so a restarted broker can bind while old connections are in TIME_WAIT; the `service` command exits with code 2 and a clear message.
5. `clippy::large_enum_variant` on the dispatch queue item is allowed on purpose: boxing would add an allocation per dispatched message.

## Risks / Trade-offs

- [macOS is checked only in CI] → The first CI run confirms linking and tests; failures are fixed before the release is published (the release job needs every build and test job).
- [Unsigned macOS binary] → README explains removing the quarantine attribute.
