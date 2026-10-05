@echo off
REM ActiveMQRust by Matteo Baccan
REM SPDX-License-Identifier: MIT
REM
REM Formats the sources (rustfmt.toml: max_width 120), then runs the Rust tests in release
REM mode, as the CI does. No debug build is made.

setlocal
cd /d "%~dp0.."
cargo fmt || exit /b 1
cargo test --release || exit /b 1
