@echo off
REM ActiveMQRust by Matteo Baccan
REM SPDX-License-Identifier: MIT
REM
REM Builds the program with the Maven Wrapper and runs a benchmark scenario.
REM Usage: run-bench.cmd [amq5|amq6] --scenario hold|throughput|scale|latency [--messages n] [--size bytes]
REM        [--send async|sync] [--producers n] [--consumers n] [--queues n] [--rate n] [--warmup n]
REM        [--hold-seconds n] [--timeout-seconds n]
REM throughput: producers and consumers share one queue (default 1 / 1).
REM scale: producer i sends to queue i mod Q, consumer j reads queue j mod Q (default 10 / 10 / 10).

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
