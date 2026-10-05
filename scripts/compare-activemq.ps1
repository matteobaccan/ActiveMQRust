# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Compares ActiveMQRust with Apache ActiveMQ 5.18.x and 6.x on this machine, unattended.
# The same Java client (tests/java-it, bench mode) runs against every broker. Memory is sampled
# from the broker process every 250 ms. Results go to docs\benchmarks\activemq-comparison-<date>.md/.csv.
#
# Example:
#   pwsh scripts\compare-activemq.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2

param(
    [string]$Mqrust = (Join-Path $PSScriptRoot '..\target\release\mqrust.exe'),
    [string]$ActiveMQ5,
    [string]$ActiveMQ6,
    [string]$Java = 'java',
    [int]$Runs = 3,
    [switch]$SkipDefault,
    [switch]$SkipActiveMQ5,
    [switch]$SkipActiveMQ6,
    [string[]]$Measurements = @('a', 'bc', 'd-async', 'd-sync', 'e'),
    [int]$TimeoutMinutes = 30,
    [int]$WarmupMessages = 20000,
    [int]$HoldSeconds = 10
)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
$benchDir = Join-Path $PSScriptRoot 'activemq-bench'
$jarDir = Join-Path $root 'tests\java-it\target'
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("mqrust-compare-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$date = Get-Date -Format 'yyyy-MM-dd'
$outDir = Join-Path $root 'docs\benchmarks'
New-Item -ItemType Directory -Force $outDir | Out-Null
$csvPath = Join-Path $outDir "activemq-comparison-$date.csv"
$mdPath = Join-Path $outDir "activemq-comparison-$date.md"

function Log([string]$m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m) }

function Test-PortFree([int]$port) {
    $l = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue
    return -not $l
}

function Wait-Port([int]$port, [int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $c = New-Object System.Net.Sockets.TcpClient
            $c.Connect('127.0.0.1', $port)
            $c.Close()
            return $true
        } catch { Start-Sleep -Milliseconds 200 }
    }
    return $false
}

function Free-MemoryGB {
    $os = Get-CimInstance Win32_OperatingSystem
    return [math]::Round($os.FreePhysicalMemory / 1MB, 1)
}

function Get-JavaVersion {
    $v = & $Java -version 2>&1 | Select-Object -First 1
    return "$v"
}

function Get-AmqVersion([string]$amqHome) {
    $jar = Get-ChildItem (Join-Path $amqHome 'activemq-all-*.jar') -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($jar -and $jar.Name -match 'activemq-all-(.+)\.jar') { return $Matches[1] }
    return (Split-Path $amqHome -Leaf)
}

# -- build the client jars -------------------------------------------------------
foreach ($p in @('amq5', 'amq6')) {
    $jar = Join-Path $jarDir "$p\mqrust-acceptance.jar"
    if (-not (Test-Path $jar)) {
        Log "building client jar ($p)"
        Push-Location (Join-Path $root 'tests\java-it')
        & .\mvnw.cmd -q -P $p package -DskipTests
        if ($LASTEXITCODE -ne 0) { throw "client build failed ($p)" }
        Pop-Location
    }
}
if (-not (Test-Path $Mqrust)) { throw "mqrust.exe not found: $Mqrust (run scripts\build.cmd)" }
$mqrustVersion = (& $Mqrust --version).Trim()

# -- broker setups -------------------------------------------------------------------
$setups = @()
foreach ($pair in @(@{ Profile = 'amq5'; Home = $ActiveMQ5; Skip = $SkipActiveMQ5 }, @{ Profile = 'amq6'; Home = $ActiveMQ6; Skip = $SkipActiveMQ6 })) {
    if ($pair.Skip -or -not $pair.Home) { continue }
    $ver = Get-AmqVersion $pair.Home
    $setups += [pscustomobject]@{ Name = "activemq-$($pair.Profile)-tuned"; Broker = 'ActiveMQ'; Version = $ver; Config = 'tuned'; Profile = $pair.Profile; Home = $pair.Home; Reference = $false }
    if (-not $SkipDefault) {
        $setups += [pscustomobject]@{ Name = "activemq-$($pair.Profile)-default"; Broker = 'ActiveMQ'; Version = $ver; Config = 'default'; Profile = $pair.Profile; Home = $pair.Home; Reference = $true }
    }
    $setups += [pscustomobject]@{ Name = "mqrust-$($pair.Profile)"; Broker = 'ActiveMQRust'; Version = $mqrustVersion; Config = 'mqrust-default'; Profile = $pair.Profile; Home = $null; Reference = $false }
    $setups += [pscustomobject]@{ Name = "mqrust-$($pair.Profile)-nocompress"; Broker = 'ActiveMQRust'; Version = $mqrustVersion; Config = 'mqrust-nocompress'; Profile = $pair.Profile; Home = $null; Reference = $false }
}
if ($setups.Count -eq 0) { throw 'nothing to compare: pass -ActiveMQ5 and/or -ActiveMQ6' }

$measureDefs = @{
    'a'       = @{ Scenario = $null; Messages = 0; Size = 0; Send = '-' }
    'bc'      = @{ Scenario = 'hold'; Messages = 100000; Size = 10240; Send = 'async' }
    'd-async' = @{ Scenario = 'throughput'; Messages = 1000000; Size = 1024; Send = 'async' }
    'd-sync'  = @{ Scenario = 'throughput'; Messages = 100000; Size = 1024; Send = 'sync' }
    'e'       = @{ Scenario = 'hold'; Messages = 10000; Size = 51200; Send = 'async' }
}

function Start-Broker($setup, [string]$dataDir) {
    New-Item -ItemType Directory -Force $dataDir | Out-Null
    $log = Join-Path $dataDir 'broker.log'
    if ($setup.Broker -eq 'ActiveMQRust') {
        $cfg = if ($setup.Config -eq 'mqrust-nocompress') { 'mqrust-bench-nocompress.toml' } else { 'mqrust-bench.toml' }
        return Start-Process -FilePath $Mqrust -ArgumentList @('--config', (Join-Path $benchDir $cfg)) -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError "$log.err"
    }
    $amqHome = $setup.Home
    $jvmArgs = @()
    if ($setup.Config -eq 'tuned') { $jvmArgs += '-Xmx4g' } else { $jvmArgs += @('-Xms64M', '-Xmx1G') }
    $jvmArgs += @(
        "-Djava.util.logging.config.file=logging.properties",
        "-Djava.security.auth.login.config=$amqHome\conf\login.config",
        "-Dactivemq.home=$amqHome", "-Dactivemq.base=$amqHome", "-Dactivemq.conf=$amqHome\conf",
        "-Dactivemq.data=$dataDir", "-Djava.io.tmpdir=$dataDir\tmp",
        '-jar', "$amqHome\bin\activemq.jar", 'start')
    if ($setup.Config -eq 'tuned') { $jvmArgs += "xbean:file:" + (Join-Path $benchDir 'activemq-tuned.xml').Replace([char]92, [char]47) }
    New-Item -ItemType Directory -Force "$dataDir\tmp" | Out-Null
    return Start-Process -FilePath $Java -ArgumentList $jvmArgs -WorkingDirectory $amqHome -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError "$log.err"
}

function Stop-Broker($proc) {
    if ($proc -and -not $proc.HasExited) {
        try { Stop-Process -Id $proc.Id -Force } catch {}
        $proc.WaitForExit(15000) | Out-Null
    }
    Start-Sleep -Seconds 2
}

function Median([double[]]$v) {
    if (-not $v -or $v.Count -eq 0) { return $null }
    $s = $v | Sort-Object
    $n = $s.Count
    if ($n % 2 -eq 1) { return $s[[int](($n - 1) / 2)] }
    return ($s[$n / 2 - 1] + $s[$n / 2]) / 2
}

function Sample($proc, $samples, [bool]$api) {
    try {
        $p = Get-Process -Id $proc.Id -ErrorAction Stop
        $mm = $null
        if ($api) {
            try {
                $cred = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('admin:admin'))
                $r = Invoke-RestMethod -Uri 'http://127.0.0.1:8161/api/overview' -Headers @{ Authorization = "Basic $cred" } -TimeoutSec 2
                $mm = [double]$r.messageMemory
            } catch {}
        }
        $samples.Add([pscustomobject]@{ T = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); WS = [double]$p.WorkingSet64; PB = [double]$p.PrivateMemorySize64; MM = $mm }) | Out-Null
    } catch {}
}

function Window($samples, [long]$from, [long]$to, [string]$field) {
    $v = @($samples | Where-Object { $_.T -ge $from -and $_.T -le $to -and $null -ne $_.$field } | ForEach-Object { $_.$field })
    return Median $v
}

$rows = New-Object System.Collections.ArrayList

function Run-One($setup, [string]$m, [int]$run, [bool]$warm) {
    $def = $measureDefs[$m]
    if (-not (Test-PortFree 61616)) { throw 'port 61616 is busy' }
    $free = Free-MemoryGB
    if ($free -lt 8) { Log "warning: only $free GB of free physical memory (8 GB recommended)" }
    $dataDir = Join-Path $work ("{0}-{1}-{2}" -f $setup.Name, $m, $run)
    $proc = Start-Broker $setup $dataDir
    $samples = New-Object System.Collections.ArrayList
    $api = $setup.Broker -eq 'ActiveMQRust'
    $valid = $true
    $reason = ''
    $phase = @{}
    $result = @{}
    try {
        if (-not (Wait-Port 61616 90)) { throw 'broker did not accept connections within 90 s' }
        $t0 = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
        $end = (Get-Date).AddSeconds(10)
        while ((Get-Date) -lt $end) { Sample $proc $samples $api; Start-Sleep -Milliseconds 250 }
        $idleWS = Window $samples ($t0 + 5000) ($t0 + 10000) 'WS'
        $idlePB = Window $samples ($t0 + 5000) ($t0 + 10000) 'PB'
        if ($def.Scenario) {
            $jar = Join-Path $jarDir "$($setup.Profile)\mqrust-acceptance.jar"
            $out = Join-Path $dataDir 'bench.out'
            $bargs = @('-Xmx3g', '-jar', $jar, 'bench', '--url', 'tcp://127.0.0.1:61616', '--user', 'admin', '--password', 'admin',
                '--scenario', $def.Scenario, '--messages', $def.Messages, '--size', $def.Size, '--send', $def.Send,
                '--warmup', $WarmupMessages, '--hold-seconds', $HoldSeconds, '--timeout-seconds', ($TimeoutMinutes * 60))
            $bench = Start-Process -FilePath $Java -ArgumentList $bargs -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError "$out.err"
            $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
            while (-not $bench.HasExited) {
                Sample $proc $samples $api
                if ((Get-Date) -gt $deadline) { Stop-Process -Id $bench.Id -Force; $valid = $false; $reason = 'timeout'; break }
                Start-Sleep -Milliseconds 250
            }
            foreach ($line in Get-Content $out) {
                if ($line -match '^PHASE (\S+) (\d+)') { $phase[$Matches[1]] = [long]$Matches[2] }
                if ($line -match '^RESULT ') {
                    foreach ($kv in ($line.Substring(7) -split ' ')) { if ($kv -match '^([^=]+)=(.*)$') { $result[$Matches[1]] = $Matches[2] } }
                }
            }
            if ($result['status'] -ne 'ok') { $valid = $false; if (-not $reason) { $reason = $result['reason'] } }
        }
        $holdWS = $null; $holdPB = $null; $holdMM = $null
        if ($phase.ContainsKey('hold-end')) {
            $he = $phase['hold-end']
            $holdWS = Window $samples ($he - 5000) $he 'WS'
            $holdPB = Window $samples ($he - 5000) $he 'PB'
            $holdMM = Window $samples ($he - 5000) $he 'MM'
        }
        $peakWS = ($samples | Measure-Object -Property WS -Maximum).Maximum
        $peakPB = ($samples | Measure-Object -Property PB -Maximum).Maximum
    } catch {
        $valid = $false
        $reason = "$_"
    } finally {
        Stop-Broker $proc
    }
    $comp = if ($setup.Broker -eq 'ActiveMQ') { 'off' } elseif ($setup.Config -eq 'mqrust-nocompress' -or $def.Size -le 32768) { 'off' } else { 'active' }
    $row = [pscustomobject]@{
        date = $date; broker = $setup.Broker; broker_version = $setup.Version; configuration = $setup.Config; client_profile = $setup.Profile
        measurement = $m; message_size = $def.Size; message_count = $def.Messages; send_mode = $def.Send; broker_compression = $comp
        run = $run; warmup = $warm; valid = $valid; reason = $reason
        produce_ms = $result['produce_ms']; consume_ms = $(if ($result['consume_ms']) { $result['consume_ms'] } else { $result['elapsed_ms'] })
        produce_msgs_s = $result['produce_msgs_s']; consume_msgs_s = $(if ($result['consume_msgs_s']) { $result['consume_msgs_s'] } else { $result['msgs_s'] })
        produce_mb_s = $result['produce_mb_s']; consume_mb_s = $(if ($result['consume_mb_s']) { $result['consume_mb_s'] } else { $result['mb_s'] })
        idle_ws = $idleWS; idle_pb = $idlePB; hold_ws = $holdWS; hold_pb = $holdPB; peak_ws = $peakWS; peak_pb = $peakPB; hold_message_memory = $holdMM
    }
    $rows.Add($row) | Out-Null
    $row | Export-Csv -Path $csvPath -NoTypeInformation -Append
    Log ("{0} {1} run {2}{3}: valid={4} produce_ms={5} consume_ms={6} idleWS={7:N0} holdWS={8:N0} {9}" -f $setup.Name, $m, $run, $(if ($warm) { ' (warm-up)' } else { '' }), $valid, $row.produce_ms, $row.consume_ms, $idleWS, $holdWS, $reason)
}

if (Test-Path $csvPath) { Remove-Item $csvPath }
Log "work directory: $work"
foreach ($setup in $setups) {
    foreach ($m in $Measurements) {
        if ($setup.Config -eq 'mqrust-nocompress' -and $m -ne 'e') { continue }
        Run-One $setup $m 0 $true
        for ($r = 1; $r -le $Runs; $r++) { Run-One $setup $m $r $false }
    }
}

# -- report ----------------------------------------------------------------------------
function Med($setupName, $m, $field) {
    $v = @($rows | Where-Object { -not $_.warmup -and $_.valid -and $_.measurement -eq $m -and (Get-SetupName $_) -eq $setupName -and $_.$field } | ForEach-Object { [double]$_.$field })
    return Median $v
}
function Get-SetupName($row) {
    if ($row.broker -eq 'ActiveMQ') { return "activemq-$($row.client_profile)-$($row.configuration)" }
    if ($row.configuration -eq 'mqrust-nocompress') { return "mqrust-$($row.client_profile)-nocompress" }
    return "mqrust-$($row.client_profile)"
}
function Fmt($v, [string]$unit) {
    if ($null -eq $v) { return 'n/a' }
    if ($unit -eq 'MB') { return ('{0:N1} MB' -f ($v / 1MB)) }
    if ($unit -eq 'ms') { return ('{0:N0} ms' -f $v) }
    return ('{0:N1}' -f $v)
}
function Verdict($rust, $amq) {
    if ($null -eq $rust -or $null -eq $amq) { return 'n/a' }
    if ($rust -lt $amq) { return 'met' } else { return 'not met' }
}

$cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
$ram = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
$osv = (Get-CimInstance Win32_OperatingSystem).Caption + ' ' + (Get-CimInstance Win32_OperatingSystem).Version
$md = New-Object System.Text.StringBuilder
[void]$md.AppendLine("# ActiveMQRust vs ActiveMQ comparison ($date)")
[void]$md.AppendLine()
[void]$md.AppendLine("- Machine: $cpu, $ram GB RAM, $osv")
[void]$md.AppendLine("- JDK: $(Get-JavaVersion)")
[void]$md.AppendLine("- ActiveMQRust: $mqrustVersion (release build)")
[void]$md.AppendLine("- Runs: 1 warm-up (discarded) + $Runs measured; values are medians. Warm-up inside each run: $WarmupMessages messages.")
[void]$md.AppendLine('- Messages: XML TextMessage with 20 random fields plus a base64 buffer, NON_PERSISTENT, AUTO_ACKNOWLEDGE, prefetch 1000.')
[void]$md.AppendLine('- 1 KB and 10 KB messages are below the 32 KB broker compression threshold, so ActiveMQRust compression is not active in (b), (c) and (d).')
[void]$md.AppendLine()
foreach ($p in @('amq5', 'amq6')) {
    $amq = "activemq-$p-tuned"; $rust = "mqrust-$p"; $rustNc = "mqrust-$p-nocompress"
    if (-not ($rows | Where-Object { (Get-SetupName $_) -eq $amq })) { continue }
    $ver = ($rows | Where-Object { (Get-SetupName $_) -eq $amq } | Select-Object -First 1).broker_version
    [void]$md.AppendLine("## ActiveMQ $ver (tuned, in RAM) vs ActiveMQRust - client $p")
    [void]$md.AppendLine()
    [void]$md.AppendLine('| Measurement | ActiveMQ | ActiveMQRust | Verdict (ActiveMQRust better) |')
    [void]$md.AppendLine('|---|---|---|---|')
    $lines = @(
        @('(a) idle Working Set', 'a', 'idle_ws', 'MB'), @('(a) idle Private Bytes', 'a', 'idle_pb', 'MB'),
        @('(b) 100k x 10 KB held: Working Set', 'bc', 'hold_ws', 'MB'), @('(b) 100k x 10 KB held: Private Bytes', 'bc', 'hold_pb', 'MB'),
        @('(c) produce time 100k x 10 KB', 'bc', 'produce_ms', 'ms'), @('(c) consume time 100k x 10 KB', 'bc', 'consume_ms', 'ms'),
        @('(d) 1 KB async: elapsed', 'd-async', 'consume_ms', 'ms'), @('(d) 1 KB sync: elapsed', 'd-sync', 'consume_ms', 'ms'),
        @('(e) 10k x 50 KB held: Working Set (compression off)', 'e', 'hold_ws', 'MB'), @('(e) produce time 10k x 50 KB (compression off)', 'e', 'produce_ms', 'ms'),
        @('(e) consume time 10k x 50 KB (compression off)', 'e', 'consume_ms', 'ms')
    )
    foreach ($l in $lines) {
        $r = if ($l[1] -eq 'e') { $rustNc } else { $rust }
        $a = Med $amq $l[1] $l[2]; $b = Med $r $l[1] $l[2]
        [void]$md.AppendLine("| $($l[0]) | $(Fmt $a $l[3]) | $(Fmt $b $l[3]) | $(Verdict $b $a) |")
    }
    $ewsOn = Med $rust 'e' 'hold_ws'; $ewsA = Med $amq 'e' 'hold_ws'
    $mmOn = Med $rust 'e' 'hold_message_memory'; $mmOff = Med $rustNc 'e' 'hold_message_memory'
    [void]$md.AppendLine("| (e) 10k x 50 KB held: Working Set (ActiveMQRust defaults, compression active) | $(Fmt $ewsA 'MB') | $(Fmt $ewsOn 'MB') | $(Verdict $ewsOn $ewsA) |")
    $ratio = if ($mmOn -and $mmOff) { '{0:P1}' -f ($mmOn / $mmOff) } else { 'not available' }
    [void]$md.AppendLine()
    [void]$md.AppendLine("Broker compression ratio in (e) (message memory with compression / without): $ratio")
    $idleA = Med $amq 'a' 'idle_ws'; $idleR = Med $rust 'a' 'idle_ws'
    $t1 = if ($idleA -and $idleR) { if ($idleR -le $idleA / 5) { 'met' } else { 'not met' } } else { 'n/a' }
    $t2 = if ($idleR) { if ($idleR -lt 20MB) { 'met' } else { 'not met' } } else { 'n/a' }
    [void]$md.AppendLine()
    [void]$md.AppendLine("- Target memory <= 1/5 of ActiveMQ at idle (Working Set): **$t1**")
    [void]$md.AppendLine("- Target idle Working Set < 20 MB: **$t2**")
    [void]$md.AppendLine()
}
if (-not $SkipDefault) {
    [void]$md.AppendLine('## Reference only: ActiveMQ with its default configuration')
    [void]$md.AppendLine()
    [void]$md.AppendLine('| Setup | Measurement | Produce | Consume | Idle WS | Hold WS | Valid runs |')
    [void]$md.AppendLine('|---|---|---|---|---|---|---|')
    foreach ($s in ($setups | Where-Object { $_.Reference })) {
        foreach ($m in $Measurements) {
            $valid = @($rows | Where-Object { (Get-SetupName $_) -eq $s.Name -and $_.measurement -eq $m -and -not $_.warmup -and $_.valid }).Count
            [void]$md.AppendLine("| $($s.Name) | $m | $(Fmt (Med $s.Name $m 'produce_ms') 'ms') | $(Fmt (Med $s.Name $m 'consume_ms') 'ms') | $(Fmt (Med $s.Name $m 'idle_ws') 'MB') | $(Fmt (Med $s.Name $m 'hold_ws') 'MB') | $valid/$Runs |")
        }
    }
    [void]$md.AppendLine()
}
[void]$md.AppendLine('## Configuration used')
[void]$md.AppendLine()
[void]$md.AppendLine('ActiveMQ tuned (`scripts/activemq-bench/activemq-tuned.xml`, JVM `-Xmx4g`):')
[void]$md.AppendLine('```xml')
[void]$md.AppendLine((Get-Content (Join-Path $benchDir 'activemq-tuned.xml') -Raw))
[void]$md.AppendLine('```')
[void]$md.AppendLine('ActiveMQRust (`scripts/activemq-bench/mqrust-bench.toml`):')
[void]$md.AppendLine('```toml')
[void]$md.AppendLine((Get-Content (Join-Path $benchDir 'mqrust-bench.toml') -Raw))
[void]$md.AppendLine('```')
[void]$md.AppendLine('Every individual run is in the CSV file next to this report.')
Set-Content -Path $mdPath -Value $md.ToString() -Encoding UTF8
Log "report: $mdPath"
Log "csv:    $csvPath"
