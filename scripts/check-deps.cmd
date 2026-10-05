@echo off
REM ActiveMQRust by Matteo Baccan
REM SPDX-License-Identifier: MIT
REM
REM Checks that an executable imports only Windows system DLLs (no Visual C++ runtime).
REM Usage: check-deps.cmd [path\to\mqrust.exe]

setlocal
set "EXE=%~1"
if "%EXE%"=="" set "EXE=%~dp0..\target\release\mqrust.exe"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0check-deps.ps1" -Exe "%EXE%"
exit /b %ERRORLEVEL%
