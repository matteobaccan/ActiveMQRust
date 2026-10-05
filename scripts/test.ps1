# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Formats the sources (rustfmt.toml: max_width 120), then runs the Rust tests in release
# mode, as the CI does. Works with PowerShell 7 on Windows and macOS. No debug build is made.

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
cargo fmt
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --release
exit $LASTEXITCODE
