@echo off
REM ActiveMQRust by Matteo Baccan
REM SPDX-License-Identifier: MIT
REM
REM Builds the acceptance program with the Maven Wrapper and runs it.
REM Usage: run-acceptance.cmd [amq5|amq6] [--url tcp://127.0.0.1:61616] [--user admin] [--password admin] [--only 1|2|3]

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
java -jar "target\%PROFILE%\mqrust-acceptance.jar" accept %ARGS%
exit /b %ERRORLEVEL%
