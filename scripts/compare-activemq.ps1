# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Compares ActiveMQRust with Apache ActiveMQ 5.18.x and 6.x on this machine, unattended.
# The same Java client (tests/java-it, bench mode) runs against every broker. Memory is sampled
# from the broker process every 250 ms. Results are written to a temporary directory and moved to
# <OutDir>\activemq-comparison-<date>.md/.csv only when the whole comparison has finished.
#
# Examples:
#   pwsh scripts\compare-activemq.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2
#   pwsh scripts\compare-activemq.ps1 -ActiveMQ6 C:\tools\apache-activemq-6.3.2 -SkipActiveMQ5 -Quick -OutDir $env:TEMP\bench
#   pwsh scripts\compare-activemq.ps1 -ActiveMQ5 ... -ActiveMQ6 ... -DryRun     (checks and plan only, starts nothing)
#
# Exit code: 0 when every run completed (whether or not the criteria are met), 1 when a broker
# failed to start or a bench run failed, 2 when the pre-flight checks failed.

param(
    [string]$Mqrust = (Join-Path $PSScriptRoot '..\target\release\mqrust.exe'),
    [string]$ActiveMQ5,
    [string]$ActiveMQ6,
    [string]$Java = 'java',
    [int]$Port = 61616,
    [int]$AdminPort = 8161,
    [int]$Runs = 3,
    [switch]$SkipDefault,
    [switch]$SkipActiveMQ5,
    [switch]$SkipActiveMQ6,
    [string[]]$Measurements = @('a', 'bc', 'd-async', 'd-sync', 'e', 'f-async', 'f-sync'),
    [int]$TimeoutMinutes = 30,
    [int]$WarmupMessages = 20000,
    [int]$HoldSeconds = 10,
    [int]$IdleSeconds = 10,
    [int]$MaxRetries = 2,
    [double]$MinFreeMemoryGB = 8,
    [double]$MaxCpuPercent = 20,
    [string]$OutDir = (Join-Path $PSScriptRoot '..\docs\benchmarks'),
    [switch]$Quick,
    [switch]$Force,
    [switch]$KeepData,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
# Numbers in the CSV and in the report always use a dot as decimal separator.
[System.Threading.Thread]::CurrentThread.CurrentCulture = [cultureinfo]::InvariantCulture

# "-Measurements a,e" arrives as one string when the script is run with pwsh -File.
$Measurements = @($Measurements | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$benchDir = Join-Path $PSScriptRoot 'activemq-bench'
$jarDir = Join-Path $root 'tests\java-it\target'
$date = Get-Date -Format 'yyyy-MM-dd'
$started = Get-Date

if ($Quick) {
    # Smoke test: one measured run, no discarded warm-up run, small message counts.
    $Runs = 1
    $WarmupMessages = [math]::Min($WarmupMessages, 2000)
    $HoldSeconds = [math]::Min($HoldSeconds, 6)
    $IdleSeconds = [math]::Min($IdleSeconds, 6)
}

$measureDefs = [ordered]@{
    'a'       = @{ Title = '(a) idle broker'; Scenario = $null; Messages = 0; Size = 0; Send = '-' }
    'bc'      = @{ Title = '(b)/(c) 10 KB messages held'; Scenario = 'hold'; Messages = 100000; Size = 10240; Send = 'async' }
    'd-async' = @{ Title = '(d) 1 KB throughput, async send'; Scenario = 'throughput'; Messages = 1000000; Size = 1024; Send = 'async' }
    'd-sync'  = @{ Title = '(d) 1 KB throughput, sync send'; Scenario = 'throughput'; Messages = 100000; Size = 1024; Send = 'sync' }
    'e'       = @{ Title = '(e) 50 KB messages held'; Scenario = 'hold'; Messages = 10000; Size = 51200; Send = 'async' }
    'f-async' = @{ Title = '(f) 12 KB throughput, async send'; Scenario = 'throughput'; Messages = 3600; Size = 12288; Send = 'async' }
    'f-sync'  = @{ Title = '(f) 12 KB throughput, sync send'; Scenario = 'throughput'; Messages = 3600; Size = 12288; Send = 'sync' }
}
if ($Quick) {
    $measureDefs['bc'].Messages = 5000
    $measureDefs['d-async'].Messages = 50000
    $measureDefs['d-sync'].Messages = 5000
    $measureDefs['e'].Messages = 1000
}

function Log([string]$m) { Write-Host ("[{0}] {1}" -f (Get-Date -Format 'HH:mm:ss'), $m) }
function Fail-Preflight([string]$m) { Write-Host "pre-flight check failed: $m" -ForegroundColor Red; exit 2 }

# -- measurements of the machine ----------------------------------------------------------
Add-Type -Namespace MqBench -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll")]
public static extern bool GetSystemTimes(out long idle, out long kernel, out long user);
'@

function Get-CpuTimes {
    $idle = 0L; $kernel = 0L; $user = 0L
    [void][MqBench.Native]::GetSystemTimes([ref]$idle, [ref]$kernel, [ref]$user)
    # Kernel time includes idle time; all values are summed over the logical processors.
    return [pscustomobject]@{ Busy = ($kernel - $idle + $user); Total = ($kernel + $user) }
}

function Get-ProcessCpuTicks($procs) {
    $t = 0L
    foreach ($p in $procs) {
        if (-not $p) { continue }
        try { $p.Refresh(); $t += $p.TotalProcessorTime.Ticks } catch {}
    }
    return $t
}

function Measure-MachineCpu([int]$seconds) {
    $a = Get-CpuTimes
    Start-Sleep -Seconds $seconds
    $b = Get-CpuTimes
    if ($b.Total -le $a.Total) { return 0 }
    return 100.0 * ($b.Busy - $a.Busy) / ($b.Total - $a.Total)
}

function Free-MemoryGB {
    return [math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB, 1)
}

function Get-Listeners {
    return [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()
}

function Test-PortFree([int]$p) {
    return -not (Get-Listeners | Where-Object { $_.Port -eq $p })
}

function Wait-PortFree([int]$p, [int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-PortFree $p) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return (Test-PortFree $p)
}

function Quote-Arg([string]$a) {
    if ($a -match '[\s"]') { return '"' + $a.Replace('"', '\"') + '"' }
    return $a
}

function Get-AmqVersion([string]$amqHome) {
    $jar = Get-ChildItem (Join-Path $amqHome 'activemq-all-*.jar') -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($jar -and $jar.Name -match 'activemq-all-(.+)\.jar') { return $Matches[1] }
    return (Split-Path $amqHome -Leaf)
}

# JVM memory options of activemq.bat (ACTIVEMQ_OPTS), used for the default-configuration runs.
function Get-AmqDefaultMemoryOpts([string]$amqHome) {
    $bat = Join-Path $amqHome 'bin\activemq.bat'
    $line = Get-Content $bat -ErrorAction SilentlyContinue | Where-Object { $_ -match 'set ACTIVEMQ_OPTS=' } | Select-Object -First 1
    $opts = @()
    if ($line) { $opts = @([regex]::Matches($line, '-Xm[sx]\S+') | ForEach-Object { $_.Value }) }
    if ($opts.Count -eq 0) { $opts = @('-Xms1G', '-Xmx1G') }
    return $opts
}

function Get-MachineDetails {
    $cpu = @(Get-CimInstance Win32_Processor)
    $os = Get-CimInstance Win32_OperatingSystem
    $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
    $plan = (& powercfg /getactivescheme 2>$null | Out-String).Trim()
    if ($plan -match '\(([^)]+)\)') { $plan = $Matches[1] }
    $jdk = try { (& $Java -version 2>&1 | ForEach-Object { "$_" }) -join '; ' } catch { 'not found' }
    return [ordered]@{
        'CPU'             = ($cpu[0].Name).Trim()
        'Physical cores'  = ($cpu | Measure-Object -Property NumberOfCores -Sum).Sum
        'Logical cores'   = ($cpu | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum
        'RAM'             = ('{0:N1} GB' -f ((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB))
        'Power plan'      = $plan
        'Windows edition' = $os.Caption
        'Windows version' = $(if ($cv.DisplayVersion) { "$($cv.DisplayVersion) ($($os.Version))" } else { $os.Version })
        'Windows build'   = $(if ($null -ne $cv.UBR) { "$($os.BuildNumber).$($cv.UBR)" } else { $os.BuildNumber })
        'JDK'             = $jdk
    }
}

# -- pre-flight: everything is validated before any build or run ---------------------------
$problems = @()
$warnings = @()
if (-not (Test-Path $Mqrust -PathType Leaf)) { $problems += "mqrust.exe not found: $Mqrust (run scripts\build.cmd)" }
$javaCmd = Get-Command $Java -ErrorAction SilentlyContinue
if (-not $javaCmd) { $problems += "Java not found: $Java" } else { $Java = $javaCmd.Source }
$amqHomes = @()
foreach ($pair in @(@{ Profile = 'amq5'; Home = $ActiveMQ5; Skip = $SkipActiveMQ5; Name = '-ActiveMQ5' },
                    @{ Profile = 'amq6'; Home = $ActiveMQ6; Skip = $SkipActiveMQ6; Name = '-ActiveMQ6' })) {
    if ($pair.Skip) { continue }
    if (-not $pair.Home) { $problems += "$($pair.Name) is required (or pass -Skip$($pair.Name.TrimStart('-')))"; continue }
    if (-not (Test-Path (Join-Path $pair.Home 'bin\activemq.jar'))) {
        $problems += "ActiveMQ installation not found: $($pair.Home) (bin\activemq.jar is missing)"; continue
    }
    $amqHomes += [pscustomobject]@{ Profile = $pair.Profile; Home = (Resolve-Path $pair.Home).Path }
}
if ($amqHomes.Count -eq 0 -and $problems.Count -eq 0) { $problems += 'nothing to compare: pass -ActiveMQ5 and/or -ActiveMQ6' }
foreach ($m in $Measurements) { if (-not $measureDefs.Contains($m)) { $problems += "unknown measurement: $m" } }
foreach ($p in @($Port, $AdminPort)) { if (-not (Test-PortFree $p)) { $problems += "port $p is already in use" } }
if ($problems.Count -gt 0 -and -not $DryRun) { Fail-Preflight ($problems -join '; ') }

$free = Free-MemoryGB
if ($free -lt $MinFreeMemoryGB) { $warnings += "only $free GB of physical memory is free ($MinFreeMemoryGB GB required)" }
Log 'measuring machine CPU load (5 s)'
$load = Measure-MachineCpu 5
if ($load -gt $MaxCpuPercent) { $warnings += ('machine CPU load is {0:N0}% (at most {1}% allowed)' -f $load, $MaxCpuPercent) }
if ($warnings.Count -gt 0 -and -not $DryRun) {
    if (-not $Force) { Fail-Preflight (($warnings -join '; ') + '. Close other programs, or pass -Force to continue anyway.') }
    foreach ($w in $warnings) { Log "warning: $w (continuing because of -Force)" }
}
$machine = Get-MachineDetails
$preflight = ('free memory {0} GB, machine CPU load {1:N1}%' -f $free, $load)
$mqrustVersion = if (Test-Path $Mqrust -PathType Leaf) { (& $Mqrust --version | Out-String).Trim() } else { 'not found' }

# -- broker setups --------------------------------------------------------------------------
$setups = @()
foreach ($a in $amqHomes) {
    $ver = Get-AmqVersion $a.Home
    $setups += [pscustomobject]@{ Name = "activemq-$($a.Profile)-tuned"; Broker = 'ActiveMQ'; Version = $ver; Config = 'tuned'; Profile = $a.Profile; Home = $a.Home; Reference = $false }
    if (-not $SkipDefault) {
        $setups += [pscustomobject]@{ Name = "activemq-$($a.Profile)-default"; Broker = 'ActiveMQ'; Version = $ver; Config = 'default'; Profile = $a.Profile; Home = $a.Home; Reference = $true }
    }
    $setups += [pscustomobject]@{ Name = "mqrust-$($a.Profile)"; Broker = 'ActiveMQRust'; Version = $mqrustVersion; Config = 'mqrust-default'; Profile = $a.Profile; Home = $null; Reference = $false }
    $setups += [pscustomobject]@{ Name = "mqrust-$($a.Profile)-nocompress"; Broker = 'ActiveMQRust'; Version = $mqrustVersion; Config = 'mqrust-nocompress'; Profile = $a.Profile; Home = $null; Reference = $false }
}

# -DryRun: report the checks and the plan, start nothing.
if ($DryRun) {
    Write-Host ''
    Write-Host 'Pre-flight checks:'
    foreach ($x in $problems) { Write-Host "  FAIL  $x" -ForegroundColor Red }
    foreach ($x in $warnings) { Write-Host "  $(if ($Force) { 'WARN' } else { 'FAIL' })  $x" -ForegroundColor Yellow }
    Write-Host "  ok    paths: mqrust.exe $(if (Test-Path $Mqrust -PathType Leaf) { 'found' } else { 'MISSING' }), Java $(if ($javaCmd) { $Java } else { 'MISSING' })"
    Write-Host "  info  $preflight; ports $Port / $AdminPort $(if ((Test-PortFree $Port) -and (Test-PortFree $AdminPort)) { 'free' } else { 'IN USE' })"
    Write-Host ''
    Write-Host "Plan$(if ($Quick) { ' (-Quick smoke test)' }):"
    Write-Host "  ActiveMQRust: $mqrustVersion ($Mqrust)"
    foreach ($a in $amqHomes) {
        $jar = Join-Path $jarDir "$($a.Profile)\mqrust-acceptance.jar"
        Write-Host "  ActiveMQ $(Get-AmqVersion $a.Home) ($($a.Home)); client jar $($a.Profile): $(if (Test-Path $jar) { 'built' } else { 'will be built' })"
    }
    $total = 0
    foreach ($setup in $setups) {
        $ms = @($Measurements | Where-Object { $setup.Config -ne 'mqrust-nocompress' -or $_ -eq 'e' })
        $n = $ms.Count * ($Runs + $(if ($Quick) { 0 } else { 1 }))
        $total += $n
        Write-Host ("  {0,-30} measurements {1}; {2} run(s)" -f $setup.Name, ($ms -join ', '), $n)
    }
    foreach ($m in $Measurements) {
        $d = $measureDefs[$m]
        if (-not $d) { continue }
        Write-Host ("  {0,-8} {1}: {2} message(s) of {3} bytes, send {4}" -f $m, $d.Title, $d.Messages, $d.Size, $d.Send)
    }
    Write-Host "  runs: $total in total ($Runs measured per setup and measurement$(if (-not $Quick) { ' + 1 discarded warm-up' }), up to $MaxRetries retries each); warm-up messages $WarmupMessages, idle $IdleSeconds s, hold $HoldSeconds s, run timeout $TimeoutMinutes min"
    Write-Host "  ports: OpenWire $Port, admin $AdminPort"
    Write-Host "  output: $(Join-Path $OutDir "activemq-comparison-$date.md") and .csv (written at the end only)"
    Write-Host ''
    if ($problems.Count -gt 0 -or ($warnings.Count -gt 0 -and -not $Force)) { Write-Host 'dry run: the pre-flight checks would stop the comparison' -ForegroundColor Red; exit 2 }
    Write-Host 'dry run: pre-flight ok, nothing was started'
    exit 0
}
Log "pre-flight ok: $preflight"

# -- build the client jars ------------------------------------------------------------------
foreach ($a in $amqHomes) {
    $jar = Join-Path $jarDir "$($a.Profile)\mqrust-acceptance.jar"
    if (-not (Test-Path $jar)) {
        Log "building client jar ($($a.Profile))"
        & (Join-Path $root 'tests\java-it\mvnw.cmd') -q -f (Join-Path $root 'tests\java-it\pom.xml') -P $a.Profile package -DskipTests
        if ($LASTEXITCODE -ne 0) { Write-Host "client build failed ($($a.Profile))" -ForegroundColor Red; exit 1 }
    }
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("mqrust-compare-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$csvTmp = Join-Path $work "activemq-comparison-$date.csv"
$mdTmp = Join-Path $work "activemq-comparison-$date.md"

# -- broker setups --------------------------------------------------------------------------
$setups = @()
foreach ($a in $amqHomes) {
    $ver = Get-AmqVersion $a.Home
    $setups += [pscustomobject]@{ Name = "activemq-$($a.Profile)-tuned"; Broker = 'ActiveMQ'; Version = $ver; Config = 'tuned'; Profile = $a.Profile; Home = $a.Home; Reference = $false }
    if (-not $SkipDefault) {
        $setups += [pscustomobject]@{ Name = "activemq-$($a.Profile)-default"; Broker = 'ActiveMQ'; Version = $ver; Config = 'default'; Profile = $a.Profile; Home = $a.Home; Reference = $true }
    }
    $setups += [pscustomobject]@{ Name = "mqrust-$($a.Profile)"; Broker = 'ActiveMQRust'; Version = $mqrustVersion; Config = 'mqrust-default'; Profile = $a.Profile; Home = $null; Reference = $false }
    $setups += [pscustomobject]@{ Name = "mqrust-$($a.Profile)-nocompress"; Broker = 'ActiveMQRust'; Version = $mqrustVersion; Config = 'mqrust-nocompress'; Profile = $a.Profile; Home = $null; Reference = $false }
}

# Writes the tuned XML for this run with the OpenWire port substituted.
function New-TunedConfig([string]$dataDir) {
    $xml = (Get-Content (Join-Path $benchDir 'activemq-tuned.xml') -Raw).Replace('127.0.0.1:61616', "127.0.0.1:$Port")
    $path = Join-Path $dataDir 'activemq-tuned.xml'
    Set-Content -Path $path -Value $xml -Encoding UTF8
    return $path
}

# Copies the distribution's conf directory for this run; only the listening ports are changed.
function New-DefaultConf([string]$amqHome, [string]$dataDir) {
    $conf = Join-Path $dataDir 'conf'
    Copy-Item -Recurse (Join-Path $amqHome 'conf') $conf
    $ports = @{ '61616' = $Port; '5672' = $Port + 1; '61613' = $Port + 2; '1883' = $Port + 3; '61614' = $Port + 4 }
    $xmlPath = Join-Path $conf 'activemq.xml'
    $xml = Get-Content $xmlPath -Raw
    foreach ($k in $ports.Keys) { $xml = $xml.Replace("0.0.0.0:$k", "127.0.0.1:$($ports[$k])") }
    Set-Content -Path $xmlPath -Value $xml -Encoding UTF8
    foreach ($f in @('jetty.xml', 'jetty-spring.properties')) {
        $p = Join-Path $conf $f
        if (Test-Path $p) {
            $t = (Get-Content $p -Raw).Replace('name="port" value="8161"', "name=`"port`" value=`"$AdminPort`"").Replace('jetty.http.port=8161', "jetty.http.port=$AdminPort")
            Set-Content -Path $p -Value $t -Encoding UTF8
        }
    }
    return $conf
}

function Get-BrokerCommand($setup, [string]$dataDir) {
    if ($setup.Broker -eq 'ActiveMQRust') {
        $cfg = if ($setup.Config -eq 'mqrust-nocompress') { 'mqrust-bench-nocompress.toml' } else { 'mqrust-bench.toml' }
        return @{ File = $Mqrust; Args = @('--config', (Join-Path $benchDir $cfg), '--port', "$Port", '--admin-port', "$AdminPort"); Dir = $dataDir }
    }
    $amqHome = $setup.Home
    New-Item -ItemType Directory -Force (Join-Path $dataDir 'tmp') | Out-Null
    if ($setup.Config -eq 'tuned') {
        $conf = Join-Path $amqHome 'conf'
        $mem = @('-Xmx4g')
        $target = @('xbean:file:' + (New-TunedConfig $dataDir).Replace('\', '/'))
    } else {
        $conf = New-DefaultConf $amqHome $dataDir
        $mem = Get-AmqDefaultMemoryOpts $amqHome
        $target = @()
    }
    $jvm = $mem + @(
        '-Djava.util.logging.config.file=logging.properties',
        "-Djava.security.auth.login.config=$conf\login.config",
        "-Dactivemq.home=$amqHome", "-Dactivemq.base=$amqHome", "-Dactivemq.conf=$conf",
        "-Dactivemq.data=$dataDir", "-Djava.io.tmpdir=$dataDir\tmp",
        '-jar', "$amqHome\bin\activemq.jar", 'start') + $target
    return @{ File = $Java; Args = $jvm; Dir = $amqHome }
}

$script:broker = $null
$script:bench = $null

function Stop-Proc($proc) {
    if ($proc -and -not $proc.HasExited) {
        try { Stop-Process -Id $proc.Id -Force } catch {}
        try { $proc.WaitForExit(15000) | Out-Null } catch {}
    }
}

function Median([double[]]$v) {
    if (-not $v -or $v.Count -eq 0) { return $null }
    $s = @($v | Sort-Object)
    $n = $s.Count
    if ($n % 2 -eq 1) { return $s[[int](($n - 1) / 2)] }
    return ($s[$n / 2 - 1] + $s[$n / 2]) / 2
}

# One sample: broker memory and CPU time, client CPU time and Working Set (when the client runs),
# and the machine's CPU counters. CPU times are in 100 ns ticks.
function Sample($proc, $samples) {
    try {
        $proc.Refresh()
        $m = Get-CpuTimes
        $c = $null; $cws = $null
        if ($script:bench) {
            try { $script:bench.Refresh(); $c = $script:bench.TotalProcessorTime.Ticks; if (-not $script:bench.HasExited) { $cws = [double]$script:bench.WorkingSet64 } } catch {}
        }
        $samples.Add([pscustomobject]@{
                T = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); WS = [double]$proc.WorkingSet64; PB = [double]$proc.PrivateMemorySize64
                BCpu = $proc.TotalProcessorTime.Ticks; CCpu = $c; CWS = $cws; MBusy = $m.Busy; MTotal = $m.Total
            }) | Out-Null
    } catch {}
}

$logical = [Environment]::ProcessorCount
$phaseNames = @('startup', 'idle', 'warmup', 'produce', 'hold', 'consume', 'throughput')
$phaseMetrics = [ordered]@{
    'broker_cpu_core_pct' = @('Broker CPU, % of one core', 'x'); 'broker_cpu_machine_pct' = @('Broker CPU, % of the machine', 'x')
    'broker_cpu_ms_per_1k_msgs' = @('Broker CPU ms per 1,000 messages', 'x'); 'broker_cpu_ms_per_mb' = @('Broker CPU ms per MB', 'x')
    'client_cpu_core_pct' = @('Client CPU, % of one core', 'x'); 'client_cpu_machine_pct' = @('Client CPU, % of the machine', 'x')
    'client_peak_ws' = @('Client peak Working Set', 'MB')
    'machine_cpu_pct' = @('Machine CPU, %', 'x'); 'other_cpu_pct' = @('Other processes CPU, % of the machine', 'x')
    'ws_avg' = @('Broker Working Set, average', 'MB'); 'ws_peak' = @('Broker Working Set, peak', 'MB')
    'pb_avg' = @('Broker Private Bytes, average', 'MB'); 'pb_peak' = @('Broker Private Bytes, peak', 'MB')
    'broker_cpu_ms' = @('Broker CPU time', 'ms'); 'client_cpu_ms' = @('Client CPU time', 'ms'); 'ms' = @('Phase duration', 'ms')
}

# Resource usage of one phase [from, to] (ms since the epoch). CPU deltas use the last sample at or
# before the start and the first sample at or after the end (250 ms resolution); memory averages and
# peaks use the samples inside the phase. $a can be given explicitly (start-up phase).
function Get-PhaseUsage($samples, [long]$from, [long]$to, [long]$messages, [int]$size, $a = $null) {
    $r = [ordered]@{}
    foreach ($k in $phaseMetrics.Keys) { $r[$k] = $null }
    if (-not $a) { $a = @($samples | Where-Object { $_.T -le $from }) | Select-Object -Last 1 }
    if (-not $a) { $a = $samples | Select-Object -First 1 }
    $b = @($samples | Where-Object { $_.T -ge $to }) | Select-Object -First 1
    if (-not $b) { $b = $samples | Select-Object -Last 1 }
    if (-not $a -or -not $b -or $b.T -le $a.T) { return $r }
    $dt = [double]($b.T - $a.T)
    $r.ms = $to - $from
    $bcpu = ($b.BCpu - $a.BCpu) / 10000.0
    $r.broker_cpu_ms = [math]::Round($bcpu, 1)
    $r.broker_cpu_core_pct = [math]::Round(100.0 * $bcpu / $dt, 2)
    $r.broker_cpu_machine_pct = [math]::Round(100.0 * $bcpu / $dt / $logical, 2)
    $cmach = 0.0
    if ($null -ne $b.CCpu) {
        $ccpu = ($b.CCpu - $(if ($null -ne $a.CCpu) { $a.CCpu } else { 0 })) / 10000.0
        $r.client_cpu_ms = [math]::Round($ccpu, 1)
        $r.client_cpu_core_pct = [math]::Round(100.0 * $ccpu / $dt, 2)
        $cmach = 100.0 * $ccpu / $dt / $logical
        $r.client_cpu_machine_pct = [math]::Round($cmach, 2)
    }
    if ($b.MTotal -gt $a.MTotal) {
        $mach = 100.0 * ($b.MBusy - $a.MBusy) / ($b.MTotal - $a.MTotal)
        $r.machine_cpu_pct = [math]::Round($mach, 2)
        $r.other_cpu_pct = [math]::Round([math]::Max(0.0, $mach - 100.0 * $bcpu / $dt / $logical - $cmach), 2)
    }
    $in = @($samples | Where-Object { $_.T -ge $from -and $_.T -le $to })
    if ($in.Count -eq 0) { $in = @($b) }
    $r.ws_avg = ($in | Measure-Object -Property WS -Average).Average
    $r.ws_peak = ($in | Measure-Object -Property WS -Maximum).Maximum
    $r.pb_avg = ($in | Measure-Object -Property PB -Average).Average
    $r.pb_peak = ($in | Measure-Object -Property PB -Maximum).Maximum
    $cws = @($in | Where-Object { $null -ne $_.CWS } | ForEach-Object { $_.CWS })
    if ($cws.Count -gt 0) { $r.client_peak_ws = ($cws | Measure-Object -Maximum).Maximum }
    if ($messages -gt 0) {
        $r.broker_cpu_ms_per_1k_msgs = [math]::Round($bcpu * 1000.0 / $messages, 3)
        if ($size -gt 0) { $r.broker_cpu_ms_per_mb = [math]::Round($bcpu / ($messages * [double]$size / 1MB), 3) }
    }
    return $r
}

function Window($samples, [long]$from, [long]$to, [string]$field) {
    $v = @($samples | Where-Object { $_.T -ge $from -and $_.T -le $to } | ForEach-Object { $_.$field })
    return Median $v
}

$apiAuth = @{ Authorization = 'Basic ' + [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes('admin:admin')) }
function Api([string]$path) {
    return Invoke-RestMethod -Uri "http://127.0.0.1:$AdminPort$path" -Headers $apiAuth -TimeoutSec 5
}

# Reads ActiveMQRust's message memory and its count of compressed messages, once.
# The count comes from a broker-wide counter when /api/overview has one, otherwise from the first
# page (up to 50 messages) of the queue held by the bench.
function Read-Compression {
    $r = [ordered]@{ MessageMemory = $null; Compressed = $null; Checked = $null }
    try {
        $o = Api '/api/overview'
        $r.MessageMemory = [double]$o.messageMemory
        foreach ($k in @('compressedMessages', 'messagesCompressed', 'compressed')) {
            if ($o.PSObject.Properties.Name -contains $k) { $r.Compressed = [long]$o.$k; $r.Checked = -1; return $r }
        }
        # Invoke-RestMethod returns a JSON array as one object: enumerate it explicitly.
        $all = Api '/api/queues'
        $q = @($all) | ForEach-Object { $_ } | Where-Object { $_.name -like 'BENCH.HOLD*' -and $_.pending -gt 0 } | Select-Object -First 1
        if ($q) {
            $page = Api ("/api/queues/{0}/messages?limit=50" -f [uri]::EscapeDataString($q.name))
            $msgs = @($page.messages)
            $r.Checked = $msgs.Count
            $r.Compressed = @($msgs | Where-Object { $_.compressed }).Count
        }
    } catch {}
    return $r
}

# Spooling, memory-limit or flow-control evidence in an ActiveMQ run. The start-up lines about the
# store and temporary store limits, and the PListStore start-up line, are not evidence.
$spoolPattern = '(?i)usage manager|memory limit reached|producers will be throttled|producer-flow-control|is full|percentUsage=1\d\d%'
function Test-AmqSpooling([string]$dataDir) {
    $hits = @()
    $log = Join-Path $dataDir 'activemq.log'
    if (Test-Path $log) { $hits += @(Select-String -Path $log -Pattern $spoolPattern | ForEach-Object { $_.Line.Trim() }) }
    $tmpFiles = @(Get-ChildItem -Recurse -File -Path $dataDir -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\tmp_storage\\' -and $_.Length -gt 0 })
    if ($tmpFiles.Count -gt 0) { $hits += "temporary storage used: $($tmpFiles.Count) file(s) under tmp_storage" }
    return $hits
}

$rows = New-Object System.Collections.ArrayList
$benchCommandLine = $null
$script:anyFailure = $false

function Run-One($setup, [string]$m, [int]$run, [bool]$warm, [int]$attempt) {
    $def = $measureDefs[$m]
    $dataDir = Join-Path $work ("{0}-{1}-{2}-{3}" -f $setup.Name, $m, $run, $attempt)
    New-Item -ItemType Directory -Force $dataDir | Out-Null
    $status = 'ok'; $reason = ''
    $phase = @{}; $result = @{}
    $samples = New-Object System.Collections.ArrayList
    $idleWS = $null; $idlePB = $null; $holdWS = $null; $holdPB = $null; $peakWS = $null; $peakPB = $null
    $startupMs = $null; $otherCpu = $null; $comp = $null; $spool = @()
    $usage = @{}; $t0 = $null; $startSnap = $null
    try {
        if (-not (Wait-PortFree $Port 30)) { throw "port $Port is still in use" }
        if (-not (Wait-PortFree $AdminPort 30)) { throw "port $AdminPort is still in use" }
        $free = Free-MemoryGB
        if ($free -lt $MinFreeMemoryGB) {
            if (-not $Force) { throw "only $free GB of physical memory is free ($MinFreeMemoryGB GB required)" }
            Log "warning: only $free GB of physical memory is free"
        }
        $cmd = Get-BrokerCommand $setup $dataDir
        $log = Join-Path $dataDir 'broker.out'
        $cpu0 = Get-CpuTimes
        $self = [System.Diagnostics.Process]::GetCurrentProcess()
        $selfCpu0 = Get-ProcessCpuTicks @($self)
        $m0 = Get-CpuTimes
        $startSnap = [pscustomobject]@{ T = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); WS = 0.0; PB = 0.0; BCpu = 0L; CCpu = $null; CWS = $null; MBusy = $m0.Busy; MTotal = $m0.Total }
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $script:broker = Start-Process -FilePath $cmd.File -ArgumentList ($cmd.Args | ForEach-Object { Quote-Arg $_ }) -WorkingDirectory $cmd.Dir -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError "$log.err"
        while ($true) {
            if (Get-Listeners | Where-Object { $_.Port -eq $Port }) { break }
            if ($script:broker.HasExited) { throw "broker exited during start-up (exit code $($script:broker.ExitCode))" }
            if ($sw.Elapsed.TotalSeconds -gt 120) { throw 'broker did not listen within 120 s' }
            Start-Sleep -Milliseconds 5
        }
        $startupMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 0)
        Sample $script:broker $samples
        $t0 = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
        $end = (Get-Date).AddSeconds($IdleSeconds)
        while ((Get-Date) -lt $end) { Sample $script:broker $samples; Start-Sleep -Milliseconds 250 }
        $idleWS = Window $samples ($t0 + ($IdleSeconds - 5) * 1000) ($t0 + $IdleSeconds * 1000) 'WS'
        $idlePB = Window $samples ($t0 + ($IdleSeconds - 5) * 1000) ($t0 + $IdleSeconds * 1000) 'PB'
        if ($def.Scenario) {
            $jar = Join-Path $jarDir "$($setup.Profile)\mqrust-acceptance.jar"
            $out = Join-Path $dataDir 'bench.out'
            $bargs = @('-Xmx3g', '-jar', $jar, 'bench', '--url', "tcp://127.0.0.1:$Port", '--user', 'admin', '--password', 'admin',
                '--scenario', $def.Scenario, '--messages', $def.Messages, '--size', $def.Size, '--send', $def.Send,
                '--warmup', $WarmupMessages, '--hold-seconds', $HoldSeconds, '--timeout-seconds', ($TimeoutMinutes * 60))
            if (-not $script:benchCommandLine) {
                $script:benchCommandLine = 'java ' + (($bargs | ForEach-Object { "$_" }) -join ' ').Replace($jar, "tests\java-it\target\<profile>\mqrust-acceptance.jar")
            }
            $script:bench = Start-Process -FilePath $Java -ArgumentList ($bargs | ForEach-Object { Quote-Arg "$_" }) -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError "$out.err"
            $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
            $apiAt = $null
            $wantApi = $setup.Broker -eq 'ActiveMQRust' -and $def.Scenario -eq 'hold'
            while (-not $script:bench.HasExited) {
                Sample $script:broker $samples
                if ($wantApi -and -not $comp) {
                    if (-not $apiAt -and (Test-Path $out)) {
                        $pe = Select-String -Path $out -Pattern '^PHASE produce-end (\d+)' | Select-Object -First 1
                        if ($pe) { $apiAt = [long]$pe.Matches[0].Groups[1].Value + $HoldSeconds * 1000 - 750 }
                    }
                    if ($apiAt -and [DateTimeOffset]::Now.ToUnixTimeMilliseconds() -ge $apiAt) { $comp = Read-Compression }
                }
                if ((Get-Date) -gt $deadline) { Stop-Proc $script:bench; $status = 'timeout'; $reason = "run timed out after $TimeoutMinutes min"; break }
                Start-Sleep -Milliseconds 250
            }
            Sample $script:broker $samples
            foreach ($line in Get-Content $out) {
                if ($line -match '^PHASE (\S+) (\d+)') { $phase[$Matches[1]] = [long]$Matches[2] }
                if ($line -match '^RESULT ') {
                    foreach ($kv in ($line.Substring(7) -split ' +')) { if ($kv -match '^([^=]+)=(.*)$') { $result[$Matches[1]] = $Matches[2] } }
                }
            }
            if ($status -eq 'ok' -and $result['status'] -ne 'ok') {
                $status = 'failed'
                $reason = if ($result['reason']) { $result['reason'] } else { 'no RESULT line (see bench.out.err)' }
            }
            if ($setup.Broker -eq 'ActiveMQRust' -and -not $comp) { $comp = Read-Compression }
        }
        if ($phase.ContainsKey('hold-end')) {
            $he = $phase['hold-end']
            $holdWS = Window $samples ($he - 5000) $he 'WS'
            $holdPB = Window $samples ($he - 5000) $he 'PB'
        }
        # Resource usage per phase.
        $n = [long]$def.Messages; $sz = [int]$def.Size
        if ($samples.Count -gt 0) {
            $usage['startup'] = Get-PhaseUsage $samples $startSnap.T $samples[0].T 0 0 $startSnap
            $usage['idle'] = Get-PhaseUsage $samples $t0 ($t0 + $IdleSeconds * 1000) 0 0
        }
        if ($phase.ContainsKey('warmup-start') -and $phase.ContainsKey('warmup-end')) {
            $usage['warmup'] = Get-PhaseUsage $samples $phase['warmup-start'] $phase['warmup-end'] $WarmupMessages $sz
        }
        if ($def.Scenario -eq 'hold') {
            if ($phase.ContainsKey('produce-end')) { $usage['produce'] = Get-PhaseUsage $samples $phase['produce-start'] $phase['produce-end'] $n $sz }
            if ($phase.ContainsKey('hold-end')) { $usage['hold'] = Get-PhaseUsage $samples $phase['produce-end'] $phase['hold-end'] 0 0 }
            if ($phase.ContainsKey('consume-end')) { $usage['consume'] = Get-PhaseUsage $samples $phase['consume-start'] $phase['consume-end'] $n $sz }
        } elseif ($def.Scenario -and $phase.ContainsKey('consume-end')) {
            $usage['throughput'] = Get-PhaseUsage $samples $phase['produce-start'] $phase['consume-end'] $n $sz
        }
        $peakWS = ($samples | Measure-Object -Property WS -Maximum).Maximum
        $peakPB = ($samples | Measure-Object -Property PB -Maximum).Maximum
        $cpu1 = Get-CpuTimes
        $ours = (Get-ProcessCpuTicks @($script:broker, $script:bench, $self)) - $selfCpu0
        if ($cpu1.Total -gt $cpu0.Total) { $otherCpu = [math]::Max(0.0, 100.0 * (($cpu1.Busy - $cpu0.Busy) - $ours) / ($cpu1.Total - $cpu0.Total)) }
    } catch {
        $status = 'failed'
        $reason = "$_"
    } finally {
        Stop-Proc $script:bench
        Stop-Proc $script:broker
        $script:bench = $null
    }
    if ($setup.Broker -eq 'ActiveMQ') { $spool = @(Test-AmqSpooling $dataDir) }
    if ($status -eq 'ok' -and $null -ne $otherCpu -and $otherCpu -gt $MaxCpuPercent) {
        $status = 'invalid-load'; $reason = ('other processes used {0:N0}% CPU' -f $otherCpu)
    }
    if ($status -eq 'ok' -and $spool.Count -gt 0) {
        $status = 'invalid-spooling'; $reason = ($spool | Select-Object -First 2) -join ' / '
    }
    if ($status -in @('failed', 'timeout')) { $script:anyFailure = $true }
    $compressionLabel = 'off'
    if ($setup.Broker -eq 'ActiveMQRust') {
        if ($comp -and $null -ne $comp.Compressed) { $compressionLabel = $(if ($comp.Compressed -gt 0) { 'active' } else { 'off' }) }
        else { $compressionLabel = 'unknown' }
    }
    $h = [ordered]@{
        date = $date; broker = $setup.Broker; broker_version = $setup.Version; configuration = $setup.Config; client_profile = $setup.Profile
        measurement = $m; message_size = $def.Size; message_count = $def.Messages; send_mode = $def.Send; broker_compression = $compressionLabel
        run = $run; attempt = $attempt; warmup = $warm; valid = ($status -eq 'ok'); status = $status; reason = $reason
        startup_ms = $startupMs; other_cpu_pct = $(if ($null -ne $otherCpu) { [math]::Round($otherCpu, 1) } else { $null })
        produce_ms = $result['produce_ms']; consume_ms = $result['consume_ms']
        produce_msgs_s = $result['produce_msgs_s']; consume_msgs_s = $result['consume_msgs_s']
        produce_mb_s = $result['produce_mb_s']; consume_mb_s = $result['consume_mb_s']
        idle_ws = $idleWS; idle_pb = $idlePB; hold_ws = $holdWS; hold_pb = $holdPB; peak_ws = $peakWS; peak_pb = $peakPB
        hold_message_memory = $(if ($comp) { $comp.MessageMemory } else { $null })
        compressed_messages = $(if ($comp) { $comp.Compressed } else { $null })
        compressed_checked = $(if ($comp) { $comp.Checked } else { $null })
        deflate_ratio = $result['deflate_ratio']; samples_verified = $result['samples']
        spooling = ($spool -join ' / ')
    }
    $h['memory_per_held_msg'] = $(if ($null -ne $holdWS -and $null -ne $idleWS -and $def.Scenario -eq 'hold') { [math]::Round(($holdWS - $idleWS) / $def.Messages, 1) } else { $null })
    foreach ($ph in $phaseNames) {
        foreach ($k in $phaseMetrics.Keys) { $h["${ph}_$k"] = $(if ($usage[$ph]) { $usage[$ph][$k] } else { $null }) }
    }
    $row = [pscustomobject]$h
    $rows.Add($row) | Out-Null
    $row | Export-Csv -Path $csvTmp -NoTypeInformation -Append -Encoding UTF8
    Log ("{0} {1} run {2}{3}{4}: {5} startup={6}ms produce_ms={7} consume_ms={8} idleWS={9:N1}MB holdWS={10:N1}MB compression={11} otherCPU={12}% {13}" -f `
            $setup.Name, $m, $run, $(if ($warm) { ' (warm-up)' } else { '' }), $(if ($attempt -gt 0) { " retry $attempt" } else { '' }),
            $status, $startupMs, $row.produce_ms, $row.consume_ms, ($idleWS / 1MB), ($holdWS / 1MB), $compressionLabel, $row.other_cpu_pct, $reason)
    if (-not $KeepData) { Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue }
    return $row
}

function Run-WithRetry($setup, [string]$m, [int]$run, [bool]$warm) {
    for ($attempt = 0; ; $attempt++) {
        $row = Run-One $setup $m $run $warm $attempt
        $retryable = $row.status -eq 'invalid-load' -or ($row.status -eq 'invalid-spooling' -and $setup.Config -eq 'tuned')
        if (-not $retryable -or $attempt -ge $MaxRetries) { return }
        Log "run marked invalid ($($row.status)); repeating it"
    }
}

# -- run everything ---------------------------------------------------------------------------
Log "work directory: $work"
try {
    foreach ($setup in $setups) {
        foreach ($m in $Measurements) {
            if ($setup.Config -eq 'mqrust-nocompress' -and $m -ne 'e') { continue }
            if (-not $Quick) { Run-WithRetry $setup $m 0 $true }
            for ($r = 1; $r -le $Runs; $r++) { Run-WithRetry $setup $m $r $false }
        }
    }
} finally {
    Stop-Proc $script:bench
    Stop-Proc $script:broker
}

# -- report -------------------------------------------------------------------------------------
$inv = [cultureinfo]::InvariantCulture
function Get-SetupName($row) {
    if ($row.broker -eq 'ActiveMQ') { return "activemq-$($row.client_profile)-$($row.configuration)" }
    if ($row.configuration -eq 'mqrust-nocompress') { return "mqrust-$($row.client_profile)-nocompress" }
    return "mqrust-$($row.client_profile)"
}
function Values($setupName, $m, $field) {
    return @($rows | Where-Object { -not $_.warmup -and $_.valid -and $_.measurement -eq $m -and (Get-SetupName $_) -eq $setupName -and $null -ne $_.$field -and "$($_.$field)" -ne '' } | ForEach-Object { [double]$_.$field })
}
function Med($setupName, $m, $field) { return Median (Values $setupName $m $field) }
function Fmt($v, [string]$unit) {
    if ($null -eq $v) { return 'n/a' }
    switch ($unit) {
        'MB' { return ([double]$v / 1MB).ToString('N1', $inv) + ' MB' }
        'ms' { return ([double]$v).ToString('N0', $inv) + ' ms' }
        'n' { return ([double]$v).ToString('N0', $inv) }
        default { return ([double]$v).ToString('N2', $inv) }
    }
}
function Ratio($rust, $amq) {
    if ($null -eq $rust -or $null -eq $amq -or $amq -eq 0) { return 'n/a' }
    return ($rust / $amq).ToString('N2', $inv)
}
function StatsCells($v, [string]$unit) {
    if (-not $v -or $v.Count -eq 0) { return 'n/a | n/a | n/a | n/a | n/a' }
    $all = ($v | ForEach-Object { Fmt $_ $unit }) -join ', '
    $mean = ($v | Measure-Object -Average).Average
    return "$all | $(Fmt $mean $unit) | $(Fmt (Median $v) $unit) | $(Fmt ($v | Measure-Object -Minimum).Minimum $unit) | $(Fmt ($v | Measure-Object -Maximum).Maximum $unit)"
}
function Compact($v, [string]$unit) {
    if (-not $v -or $v.Count -eq 0) { return 'n/a' }
    $mean = ($v | Measure-Object -Average).Average
    return "$(Fmt (Median $v) $unit) ($(Fmt $mean $unit); $(Fmt ($v | Measure-Object -Minimum).Minimum $unit) - $(Fmt ($v | Measure-Object -Maximum).Maximum $unit))"
}
function Verdict-Lower($rust, $amq) {
    if ($null -eq $rust -or $null -eq $amq) { return 'n/a' }
    if ($rust -lt $amq) { return 'met' } else { return 'not met' }
}
function Verdict-Higher($rust, $amq) {
    if ($null -eq $rust -or $null -eq $amq) { return 'n/a' }
    if ($rust -ge $amq) { return 'met' } else { return 'not met' }
}
function Verdict-Fifth($rust, $amq) {
    if ($null -eq $rust -or $null -eq $amq) { return 'n/a' }
    if ($rust -le $amq / 5) { return 'met' } else { return 'not met' }
}
function Combine([string[]]$v) {
    if ($v -contains 'n/a') { return 'n/a' }
    if ($v -contains 'not met') { return 'not met' }
    return 'met'
}

$metrics = [ordered]@{
    'idle_ws' = @('Idle Working Set (steady)', 'MB'); 'idle_pb' = @('Idle Private Bytes (steady)', 'MB')
    'hold_ws' = @('Hold Working Set (steady)', 'MB'); 'hold_pb' = @('Hold Private Bytes (steady)', 'MB')
    'peak_ws' = @('Peak Working Set', 'MB'); 'peak_pb' = @('Peak Private Bytes', 'MB')
    'produce_ms' = @('Produce time', 'ms'); 'consume_ms' = @('Consume time', 'ms')
    'produce_msgs_s' = @('Produce msgs/s', 'n'); 'consume_msgs_s' = @('Consume msgs/s', 'n')
    'produce_mb_s' = @('Produce MB/s', 'x'); 'consume_mb_s' = @('Consume MB/s', 'x')
    'startup_ms' = @('Start-up time (process start to port listening)', 'ms')
}
$metricsFor = @{
    'a' = @('idle_ws', 'idle_pb', 'peak_ws', 'peak_pb', 'startup_ms')
    'bc' = @('hold_ws', 'hold_pb', 'peak_ws', 'peak_pb', 'produce_ms', 'consume_ms', 'produce_msgs_s', 'consume_msgs_s', 'produce_mb_s', 'consume_mb_s')
    'd-async' = @('produce_ms', 'consume_ms', 'produce_msgs_s', 'consume_msgs_s', 'produce_mb_s', 'consume_mb_s', 'peak_ws', 'peak_pb')
    'd-sync' = @('produce_ms', 'consume_ms', 'produce_msgs_s', 'consume_msgs_s', 'produce_mb_s', 'consume_mb_s', 'peak_ws', 'peak_pb')
    'e' = @('hold_ws', 'hold_pb', 'peak_ws', 'peak_pb', 'produce_ms', 'consume_ms', 'produce_msgs_s', 'consume_msgs_s', 'produce_mb_s', 'consume_mb_s')
    'f-async' = @('produce_ms', 'consume_ms', 'produce_msgs_s', 'consume_msgs_s', 'produce_mb_s', 'consume_mb_s', 'peak_ws', 'peak_pb')
    'f-sync' = @('produce_ms', 'consume_ms', 'produce_msgs_s', 'consume_msgs_s', 'produce_mb_s', 'consume_mb_s', 'peak_ws', 'peak_pb')
}
function Label($setupName) {
    if ($setupName -like 'activemq-*-tuned') { return 'ActiveMQ tuned' }
    if ($setupName -like 'activemq-*-default') { return 'ActiveMQ default (reference)' }
    if ($setupName -like '*-nocompress') { return 'ActiveMQRust, broker compression off' }
    return 'ActiveMQRust defaults'
}
function Compression-Label($setupName, $m) {
    $l = @($rows | Where-Object { -not $_.warmup -and $_.measurement -eq $m -and (Get-SetupName $_) -eq $setupName } | ForEach-Object { $_.broker_compression } | Sort-Object -Unique)
    if ($l.Count -eq 0) { return 'n/a' }
    return ($l -join '/')
}

$md = New-Object System.Text.StringBuilder
function W([string]$s = '') { [void]$md.AppendLine($s) }

W "# ActiveMQRust vs ActiveMQ comparison ($date)"
W
W "Generated by ``scripts\compare-activemq.ps1``$(if ($Quick) { ' with **-Quick** (smoke test: small message counts, 1 run; not a valid comparison)' } else { '' }). Results apply to this machine only."
W
W '## Machine'
W
W '| Item | Value |'
W '|---|---|'
foreach ($k in $machine.Keys) { W "| $k | $($machine[$k]) |" }
foreach ($a in $amqHomes) { W "| ActiveMQ ($($a.Profile)) | $(Get-AmqVersion $a.Home) |" }
W "| ActiveMQRust | $mqrustVersion |"
W "| Pre-flight | $preflight |"
W "| Started / finished | $($started.ToString('yyyy-MM-dd HH:mm:ss')) / $((Get-Date).ToString('yyyy-MM-dd HH:mm:ss')) |"
W
W '## Method'
W
W "- Runs per broker setup and measurement: $(if ($Quick) { 'no warm-up run,' } else { '1 warm-up run (discarded) +' }) $Runs measured; the broker is restarted with a fresh data directory for every run. Tables list every measured value, then mean, median, min and max."
W "- Inside each run: $IdleSeconds s idle sampling, then $WarmupMessages warm-up messages on a separate queue (1,000 distinct documents cycled), excluded from timing."
W "- Memory sampled every 250 ms with Get-Process (Working Set, Private Bytes). Steady values are medians over the last 5 s of the idle period and of the $HoldSeconds s hold window."
W "- Resource usage per phase (start-up, idle, warm-up, produce, hold, consume or throughput): broker and client CPU time from TotalProcessorTime, shown as % of one core and of the machine ($logical logical processors), machine CPU from GetSystemTimes, other processes = machine - broker - client, broker Working Set and Private Bytes average and peak, broker CPU ms per 1,000 messages and per MB. CPU deltas use the 250 ms samples around each phase."
W "- A run is invalid and repeated (at most $MaxRetries times) when other processes used more than $MaxCpuPercent% of the machine's CPU during it, or when a tuned ActiveMQ run shows spooling or memory-limit messages. Invalid runs are excluded from the medians."
W '- Messages: XML TextMessage, an `id`, 20 random fields and a base64 buffer of random bytes padded to the exact size; NON_PERSISTENT, AUTO_ACKNOWLEDGE, prefetch 1000, client compression off, same fixed seeds for every broker.'
W '- 1 KB, 10 KB and 12 KB documents are below the 32 KB ActiveMQRust compression threshold. The `broker compression` label of every ActiveMQRust run is read from the broker (admin JSON API) at the end of the hold window, not inferred from the size.'
W "- Credentials ``admin`` / ``admin`` for both brokers (benchmark only)."
W "- Client command line (same for every broker): ``$benchCommandLine``"
W

$profilesDone = @($amqHomes | ForEach-Object { $_.Profile })
foreach ($p in $profilesDone) {
    $amq = "activemq-$p-tuned"; $rust = "mqrust-$p"; $rustNc = "mqrust-$p-nocompress"; $amqDef = "activemq-$p-default"
    $ver = Get-AmqVersion (($amqHomes | Where-Object { $_.Profile -eq $p }).Home)
    W "## ActiveMQ $ver vs ActiveMQRust (client $p)"
    W
    foreach ($m in $Measurements) {
        $def = $measureDefs[$m]
        W "### $($def.Title)"
        W
        W "$(Fmt $def.Messages 'n') messages of $(Fmt $def.Size 'n') bytes, send $($def.Send). Broker compression: ActiveMQRust defaults = $(Compression-Label $rust $m)$(if ($m -eq 'e') { ", compression-off run = $(Compression-Label $rustNc $m)" })."
        W
        W '| Metric | Setup | Runs | Mean | Median | Min | Max | Ratio to ActiveMQ tuned (median) |'
        W '|---|---|---|---|---|---|---|---|'
        $list = @($amq, $rust)
        if ($m -eq 'e') { $list += $rustNc }
        if (-not $SkipDefault) { $list += $amqDef }
        foreach ($f in $metricsFor[$m]) {
            foreach ($s in $list) {
                $v = Values $s $m $f
                $ratio = if ($s -eq $amq) { '1.00' } else { Ratio (Median $v) (Med $amq $m $f) }
                W "| $($metrics[$f][0]) | $(Label $s) | $(StatsCells $v $metrics[$f][1]) | $ratio |"
            }
        }
        W
        # Resource usage per phase: median (mean; min - max) of the measured runs, every value in the CSV.
        $phases = if ($m -eq 'a') { @('startup', 'idle') } elseif ($def.Scenario -eq 'hold') { @('startup', 'idle', 'warmup', 'produce', 'hold', 'consume') } else { @('startup', 'idle', 'warmup', 'throughput') }
        W "Resource usage per phase, median (mean; min - max):"
        W
        W "| Phase | Metric | $(($list | ForEach-Object { Label $_ }) -join ' | ') |"
        W "|---|---|$(($list | ForEach-Object { '---' }) -join '|')|"
        foreach ($ph in $phases) {
            foreach ($k in $phaseMetrics.Keys) {
                $cells = foreach ($s in $list) { Compact (Values $s $m "${ph}_$k") $phaseMetrics[$k][1] }
                if (@($cells | Where-Object { $_ -ne 'n/a' }).Count -eq 0) { continue }
                W "| $ph | $($phaseMetrics[$k][0]) | $($cells -join ' | ') |"
            }
        }
        W
    }

    # Start-up time across every run of each setup.
    W '### Start-up time (all runs)'
    W
    W '| Setup | Runs | Mean | Median | Min | Max |'
    W '|---|---|---|---|---|---|'
    foreach ($s in @($amq, $rust, $amqDef)) {
        $v = @($rows | Where-Object { -not $_.warmup -and (Get-SetupName $_) -eq $s -and $_.startup_ms } | ForEach-Object { [double]$_.startup_ms })
        if ($v.Count -gt 0) { W "| $(Label $s) | $(StatsCells $v 'ms') |" }
    }
    W

    # Compression in (e).
    $mmOn = Med $rust 'e' 'hold_message_memory'; $mmOff = Med $rustNc 'e' 'hold_message_memory'
    $defl = Med $rust 'e' 'deflate_ratio'
    $compOn = Compression-Label $rust 'e'
    W '### Broker compression in (e)'
    W
    W "- ActiveMQRust defaults: broker compression **$compOn** (compressed-message count read from the broker); compression-off run: **$(Compression-Label $rustNc 'e')**."
    W "- Message memory at the end of the hold window: $(Fmt $mmOn 'MB') with compression, $(Fmt $mmOff 'MB') without; ratio **$(if ($mmOn -and $mmOff -and $compOn -eq 'active') { ($mmOn / $mmOff).ToString('P1', $inv) } else { 'not available' })**."
    W "- Cross-check, Deflater level 1 on the sampled documents: **$(if ($null -ne $defl) { $defl.ToString('P1', $inv) } else { 'n/a' })** of the original size."
    W

    # Per-message overhead above payload for (b) and (e).
    W '### Memory overhead per message above payload'
    W
    W '| Measurement | Setup | Memory per held message (hold WS - idle WS) / messages | Overhead above payload per message |'
    W '|---|---|---|---|'
    foreach ($pair in @(@('bc', @($amq, $rust)), @('e', @($amq, $rustNc, $rust)))) {
        $m = $pair[0]
        if ($Measurements -notcontains $m) { continue }
        foreach ($s in $pair[1]) {
            $h = Med $s $m 'hold_ws'; $i = Med $s 'a' 'idle_ws'
            if ($null -eq $i) { $i = Med $s $m 'idle_ws' }
            $n = $measureDefs[$m].Messages; $sz = $measureDefs[$m].Size
            $o = if ($null -ne $h -and $null -ne $i) { (($h - $i - [double]$n * $sz) / $n).ToString('N0', $inv) + ' bytes' } else { 'n/a' }
            $per = Values $s $m 'memory_per_held_msg'
            W "| $($measureDefs[$m].Title) | $(Label $s) | $(if ($per.Count -gt 0) { (Compact $per 'n') + ' bytes' } else { 'n/a' }) | $o |"
        }
    }
    W

    # Verdicts, computed here and never edited by hand.
    W '### Verdicts (medians, against ActiveMQ tuned)'
    W
    W '| Criterion | ActiveMQ | ActiveMQRust | Ratio | Result |'
    W '|---|---|---|---|---|'
    function VRow([string]$name, $amqV, $rustV, [string]$unit, [string]$verdict) {
        W "| $name | $(Fmt $amqV $unit) | $(Fmt $rustV $unit) | $(Ratio $rustV $amqV) | **$verdict** |"
    }
    $memCrit = @(
        @('(a) less idle memory', 'a', 'idle_ws', 'idle_pb', $rust),
        @('(b) less memory holding the 10 KB messages', 'bc', 'hold_ws', 'hold_pb', $rust),
        @('(e) less memory holding the 50 KB messages, compression off (like-for-like)', 'e', 'hold_ws', 'hold_pb', $rustNc),
        @('(e) less memory holding the 50 KB messages, ActiveMQRust defaults', 'e', 'hold_ws', 'hold_pb', $rust))
    foreach ($c in $memCrit) {
        if ($Measurements -notcontains $c[1]) { continue }
        $aw = Med $amq $c[1] $c[2]; $rw = Med $c[4] $c[1] $c[2]; $ap = Med $amq $c[1] $c[3]; $rp = Med $c[4] $c[1] $c[3]
        VRow "$($c[0]): Working Set" $aw $rw 'MB' (Verdict-Lower $rw $aw)
        VRow "$($c[0]): Private Bytes" $ap $rp 'MB' (Verdict-Lower $rp $ap)
    }
    $timeCrit = @(@('(c) shorter', 'bc', $rust), @('(e) shorter, compression off (like-for-like)', 'e', $rustNc), @('(e) shorter, ActiveMQRust defaults', 'e', $rust))
    foreach ($c in $timeCrit) {
        if ($Measurements -notcontains $c[1]) { continue }
        foreach ($f in @('produce_ms', 'consume_ms')) {
            $a = Med $amq $c[1] $f; $r = Med $c[2] $c[1] $f
            VRow "$($c[0]) $($metrics[$f][0].ToLower())" $a $r 'ms' (Verdict-Lower $r $a)
        }
    }
    W
    W '§14.3 targets:'
    W
    W '| Target | ActiveMQ | ActiveMQRust | Ratio | Result |'
    W '|---|---|---|---|---|'
    foreach ($c in @(@('(a) idle', 'a', 'idle_ws', $rust), @('(b) 10 KB messages held', 'bc', 'hold_ws', $rust),
                     @('(e) 50 KB messages held, compression off', 'e', 'hold_ws', $rustNc), @('(e) 50 KB messages held, ActiveMQRust defaults', 'e', 'hold_ws', $rust))) {
        if ($Measurements -notcontains $c[1]) { continue }
        $a = Med $amq $c[1] $c[2]; $r = Med $c[3] $c[1] $c[2]
        VRow "memory <= 1/5 of ActiveMQ, $($c[0]), steady Working Set" $a $r 'MB' (Verdict-Fifth $r $a)
    }
    foreach ($c in @(@('(c)', 'bc', $rust), @('(d) async', 'd-async', $rust), @('(d) sync', 'd-sync', $rust),
                     @('(e) compression off', 'e', $rustNc), @('(e) ActiveMQRust defaults', 'e', $rust))) {
        if ($Measurements -notcontains $c[1]) { continue }
        foreach ($f in @('produce_msgs_s', 'consume_msgs_s')) {
            $a = Med $amq $c[1] $f; $r = Med $c[2] $c[1] $f
            VRow "throughput >= ActiveMQ, $($c[0]), $($metrics[$f][0].ToLower())" $a $r 'n' (Verdict-Higher $r $a)
        }
    }
    if ($Measurements -contains 'a') {
        $r = Med $rust 'a' 'idle_ws'
        W "| idle Working Set < 20 MB (optimize-broker-performance) | - | $(Fmt $r 'MB') | - | **$(if ($null -eq $r) { 'n/a' } elseif ($r -lt 20MB) { 'met' } else { 'not met' })** |"
    }
    W
}

$bad = @($rows | Where-Object { -not $_.valid })
W '## Invalid, failed and timed-out runs'
W
if ($bad.Count -eq 0) { W 'None.' } else {
    W '| Setup | Measurement | Run | Attempt | Status | Reason |'
    W '|---|---|---|---|---|---|'
    foreach ($r in $bad) { W "| $(Get-SetupName $r) | $($r.measurement) | $($r.run)$(if ($r.warmup) { ' (warm-up)' }) | $($r.attempt) | $($r.status) | $($r.reason -replace '\|', '/') |" }
}
$spooled = @($rows | Where-Object { $_.spooling })
if ($spooled.Count -gt 0) {
    W
    W 'ActiveMQ log lines about spooling, memory limits or flow control:'
    W
    foreach ($r in $spooled) { W "- $(Get-SetupName $r) $($r.measurement) run $($r.run): $($r.spooling -replace '\|', '/')" }
}
W
W '## Configuration used'
W
W "ActiveMQ tuned: ``scripts/activemq-bench/activemq-tuned.xml`` with the OpenWire port replaced by $Port; JVM:"
W '```'
W ("java -Xmx4g -Djava.util.logging.config.file=logging.properties -Djava.security.auth.login.config=<home>\conf\login.config -Dactivemq.home=<home> -Dactivemq.base=<home> -Dactivemq.conf=<home>\conf -Dactivemq.data=<run dir> -Djava.io.tmpdir=<run dir>\tmp -jar <home>\bin\activemq.jar start xbean:file:<run dir>/activemq-tuned.xml")
W '```'
W '```xml'
W ((Get-Content (Join-Path $benchDir 'activemq-tuned.xml') -Raw).TrimEnd())
W '```'
if (-not $SkipDefault) {
    W "ActiveMQ default (reference only): the distribution's ``conf`` directory copied for each run, with only the listening ports changed (OpenWire $Port, web console $AdminPort, other transports $($Port + 1)-$($Port + 4)); JVM memory options from ``bin\activemq.bat``: $((($amqHomes | ForEach-Object { "$($_.Profile) $((Get-AmqDefaultMemoryOpts $_.Home) -join ' ')" })) -join ', ')."
    W
}
W "ActiveMQRust: ``mqrust.exe --config <file> --port $Port --admin-port $AdminPort``"
W '```toml'
W ((Get-Content (Join-Path $benchDir 'mqrust-bench.toml') -Raw).TrimEnd())
W '```'
W '```toml'
W ((Get-Content (Join-Path $benchDir 'mqrust-bench-nocompress.toml') -Raw).TrimEnd())
W '```'
W
W 'Every individual run, including warm-up runs and retries, is in the CSV file next to this report.'
W
W '## Observations'
W
W '(Free text, added by hand after the run.)'
Set-Content -Path $mdTmp -Value $md.ToString() -Encoding UTF8

# Only now, with the whole comparison finished, the results reach the output directory.
New-Item -ItemType Directory -Force $OutDir | Out-Null
$mdPath = Join-Path $OutDir "activemq-comparison-$date.md"
$csvPath = Join-Path $OutDir "activemq-comparison-$date.csv"
Move-Item -Force $mdTmp $mdPath
if (Test-Path $csvTmp) { Move-Item -Force $csvTmp $csvPath }
if (-not $KeepData) { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
Log "report: $mdPath"
Log "csv:    $csvPath"
if ($script:anyFailure) {
    Log 'at least one broker failed to start or one bench run failed (see the report)'
    exit 1
}
exit 0
