@echo off
REM ActiveMQRust by Matteo Baccan
REM SPDX-License-Identifier: MIT
REM
REM Builds the program with the Maven Wrapper and runs a benchmark scenario.
REM Usage: run-bench.cmd [amq5|amq6] --scenario hold|throughput|latency [--messages n] [--size bytes]
REM        [--send async|sync] [--producers n] [--rate n] [--warmup n] [--hold-seconds n]

setlocal
set "PROFILE=amq5"
if /i "%~1"=="amq5" (set "PROFILE=amq5" & shift)
if /i "%~1"=="amq6" (set "PROFILE=amq6" & shift)
cd /d "%~dp0"
call mvnw.cmd -q -P %PROFILE% package -DskipTests || exit /b 1
set ARGS=
:collect
if "%~1"=="" goto run
set ARGS=%ARGS% %1
shift
goto collect
:run
java -Xmx3g -jar "target\%PROFILE%\mqrust-acceptance.jar" bench %ARGS%
exit /b %ERRORLEVEL%
