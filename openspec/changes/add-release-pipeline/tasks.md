## 1. Pipeline

- [x] 1.1 `build.yml` with the test matrix (Windows x86_64, macOS ARM64), formatting and clippy checks, release builds and the GitHub Release job
- [x] 1.2 `release-tag.yml` and `.github/scripts/downloads_table.py`
- [x] 1.3 `CHANGELOG.md` with sections for 0.1.0, 0.2.0 and 0.3.0

## 2. macOS

- [x] 2.1 Windows-only dependencies, service and version resource behind `cfg(windows)`
- [x] 2.2 Process memory on macOS with `proc_pid_rusage`; SO_REUSEADDR on Unix
- [x] 2.3 Clippy clean for `aarch64-apple-darwin` (type-checked from Windows)
- [ ] 2.4 First CI run green on macOS (link, tests, memory figures non-zero)

## 3. Tooling and docs

- [x] 3.1 `rustfmt.toml`, `.gitattributes`, `scripts/test.cmd` and `scripts/test.ps1`
- [x] 3.2 README badge, "Download" section and test script in "Build"
- [ ] 3.3 First release `v0.3.0` published with both archives
