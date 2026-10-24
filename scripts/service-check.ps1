# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Installs the broker as a Windows service, starts it, checks that it listens and writes its
# log, then stops it and removes it. This is the one part of the broker no Rust test reaches:
# the Service Control Manager has to be there. Run it from an elevated console on a
# development machine, or let the Windows job of the CI run it after the test suite.
#
#   pwsh scripts\service-check.ps1 -Exe target\release\mqrust.exe
#
# It installs under a name of its own, so it never touches an ActiveMQRust service already
# installed on the machine, and it removes what it installed even when a check fails.
#
# Variable names in PowerShell ignore case, so the locals below are never a different case of
# a parameter: $brokerExe is the copy, $Exe stays the executable that was passed in.

param(
    [string]$Exe = 'target\release\mqrust.exe',
    [string]$Work = 'target\service-check',
    [string]$Name = 'ActiveMQRustCheck',
    [int]$Port = 61626,
    [int]$AdminPort = 8126,
    [int]$TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

if (-not (Test-Path $Exe)) { Write-Error "not found: $Exe (build it with cargo build --release)"; exit 2 }
$source = (Resolve-Path $Exe).Path

$step = 0
function Step([string]$what) {
    $script:step++
    Write-Host "[$script:step] $what"
}

function Get-ServiceState {
    ((& $brokerExe service status --name $Name 2>&1) | Out-String).Trim()
}

function Wait-For([scriptblock]$condition, [string]$what) {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (& $condition) { return }
        Start-Sleep -Milliseconds 250
    }
    throw "timed out after $TimeoutSeconds s waiting for $what"
}

function Test-Listening([int]$p) {
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $client.Connect('127.0.0.1', $p)
        return $true
    } catch {
        return $false
    } finally {
        $client.Dispose()
    }
}

# A copy of its own, so the service writes its log beside that copy and neither the build
# tree nor an installed broker is touched.
if (Test-Path $Work) { Remove-Item $Work -Recurse -Force }
New-Item -ItemType Directory -Path $Work | Out-Null
$workDir = (Resolve-Path $Work).Path
$brokerExe = Join-Path $workDir 'mqrust.exe'
Copy-Item $source $brokerExe
$configFile = Join-Path $workDir 'mqrust.toml'
$logFile = Join-Path $workDir 'logs\mqrust.log'

# The console needs a credential and this check has nobody to log in: a password that lives
# for the few seconds of the run, written down only as the Argon2id hash the broker computes
# for it, which is what an operator is told to put in the file anyway.
$secret = [guid]::NewGuid().ToString('N')
$hash = (($secret | & $brokerExe hash-password --password-stdin) | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $hash.StartsWith('$argon2id$')) {
    throw "hash-password returned no hash: $hash"
}
# .Replace, not -replace: an Argon2id hash is full of dollar signs and a regex replacement
# would read them as group references.
(@"
# Written by scripts\service-check.ps1 and thrown away with the service at the end.
[broker]
port = $Port
allow_anonymous = true

[admin]
port = $AdminPort
username = "admin"
password_hash = "ARGON2ID_HASH"
"@).Replace('ARGON2ID_HASH', $hash) | Set-Content -Path $configFile -Encoding utf8

try {
    Step 'the configuration is valid'
    & $brokerExe --config $configFile check-config
    if ($LASTEXITCODE -ne 0) { throw "check-config exited $LASTEXITCODE" }

    Step 'the service is not installed yet'
    $state = Get-ServiceState
    if ($state -ne 'not installed') { throw "expected 'not installed', got '$state'" }

    Step 'install'
    & $brokerExe --config $configFile service install --name $Name
    if ($LASTEXITCODE -ne 0) { throw "service install exited $LASTEXITCODE" }

    Step 'it is installed, stopped, and set to start automatically'
    $state = Get-ServiceState
    if ($state -ne 'stopped') { throw "expected 'stopped', got '$state'" }
    $qc = (& sc.exe qc $Name | Out-String)
    # The numeric code, not the label beside it: the label is translated on a localized Windows.
    if ($qc -notmatch 'START_TYPE\s*:\s*2') { throw "the service does not start automatically:`n$qc" }

    Step 'start'
    & $brokerExe service start --name $Name
    if ($LASTEXITCODE -ne 0) { throw "service start exited $LASTEXITCODE" }
    Wait-For { (Get-ServiceState) -eq 'running' } 'the service to report running'

    Step "it accepts a connection on the OpenWire port ($Port)"
    Wait-For { Test-Listening $Port } "the OpenWire port $Port to accept a connection"

    Step "and on the admin port ($AdminPort)"
    Wait-For { Test-Listening $AdminPort } "the admin port $AdminPort to accept a connection"

    Step 'it writes its log beside the executable'
    Wait-For { (Test-Path $logFile) -and ((Get-Content $logFile -Raw) -match 'ready') } 'the log to record a start'
    $text = Get-Content $logFile -Raw
    Get-Content $logFile | ForEach-Object { Write-Host "      $_" }
    foreach ($expected in @([regex]::Escape($configFile), "OpenWire listening on 0\.0\.0\.0:$Port")) {
        if ($text -notmatch $expected) { throw "the log does not contain /$expected/" }
    }

    Step 'stop'
    & $brokerExe service stop --name $Name
    if ($LASTEXITCODE -ne 0) { throw "service stop exited $LASTEXITCODE" }
    Wait-For { (Get-ServiceState) -eq 'stopped' } 'the service to report stopped'
    Wait-For { -not (Test-Listening $Port) } "the OpenWire port $Port to be released"

    Step 'uninstall'
    & $brokerExe service uninstall --name $Name
    if ($LASTEXITCODE -ne 0) { throw "service uninstall exited $LASTEXITCODE" }
    $state = Get-ServiceState
    if ($state -ne 'not installed') { throw "expected 'not installed', got '$state'" }

    Write-Host 'OK: installed, started, served, logged, stopped and removed'
    $code = 0
} catch {
    Write-Host "FAIL: $_"
    if (Test-Path $logFile) {
        Write-Host '--- mqrust.log'
        Get-Content $logFile | ForEach-Object { Write-Host "      $_" }
    }
    $code = 1
} finally {
    # Never leave a service behind, whatever happened above.
    if ((Get-ServiceState) -ne 'not installed') {
        & $brokerExe service uninstall --name $Name 2>&1 | Out-Null
    }
}
exit $code
