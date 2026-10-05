@echo off
REM ActiveMQRust by Matteo Baccan
REM SPDX-License-Identifier: MIT
REM
REM Builds the Windows release of mqrust.exe, runs the Rust tests in release mode and
REM checks that the executable imports only Windows system DLLs. No debug build is made.

setlocal
cd /d "%~dp0.."
cargo build --release || exit /b 1
cargo test --release || exit /b 1
call "%~dp0check-deps.cmd" target\release\mqrust.exe || exit /b 1
echo.
echo Release build ready: %CD%\target\release\mqrust.exe
