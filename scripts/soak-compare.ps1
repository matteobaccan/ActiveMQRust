# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Soak comparison: the `soak` scenario of the Java bench client (steady producers, consumers in
# parallel, checks for order, losses and duplicates) against ActiveMQRust, ActiveMQ 5.x and 6.x,
# one broker after the other, with the same tuned in-RAM ActiveMQ configuration as the comparison.
# Measures startup time and, over the run, broker and client CPU and broker memory.
#
# Example (10 producers x 10 msg/s for 240 s, 10 consumers, 1 KB):
#   pwsh scripts\soak-compare.ps1 -ActiveMQ5 C:\tools\apache-activemq-5.18.7 -ActiveMQ6 C:\tools\apache-activemq-6.3.2 -Duration 240
param(
    [Parameter(Mandatory = $true)][string]$ActiveMQ5,
    [Parameter(Mandatory = $true)][string]$ActiveMQ6,
    [int]$Port = 61616, [int]$AdminPort = 8161, [int]$Duration = 300, [int]$Queues = 1,
    [int]$Producers = 10, [int]$Consumers = 10, [int]$Size = 1024, [int]$Rate = 10,
    [string[]]$Only,
    [string]$MqrustConfig,
    [switch]$ClientCompression,
    [ValidateSet('amq5', 'amq6')][string]$ClientProfile = 'amq5',
    [string]$AmqHeap = '4g',
    [int]$AmqMemoryLimitGB = 3,
    # Fixed memory for every broker: ActiveMQ gets -Xms = -Xmx with AlwaysPreTouch (the whole heap is
    # committed at start), ActiveMQRust gets its allocator to reserve this many GB at start.
    [switch]$PreallocateMemory,
    [int]$MqrustReserveGB = 4,
    [string]$Results = (Join-Path $PSScriptRoot '..\docs\benchmarks\soak-results.csv')
)
$ErrorActionPreference = 'Stop'
[System.Threading.Thread]::CurrentThread.CurrentCulture = [cultureinfo]::InvariantCulture
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("mqrust-soak-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $scratch | Out-Null
function Version($h) { (Get-ChildItem "$h\lib" -Filter 'activemq-broker-*.jar' | Select-Object -First 1).BaseName -replace 'activemq-broker-', '' }
$homes = [ordered]@{
    "ActiveMQ $(Version $ActiveMQ5)" = @{ Home = $ActiveMQ5 }
    "ActiveMQ $(Version $ActiveMQ6)" = @{ Home = $ActiveMQ6 }
}
$xml = Join-Path $scratch 'soak-tuned.xml'
(Get-Content "$root\scripts\activemq-bench\activemq-tuned.xml" -Raw).Replace('127.0.0.1:61616', "127.0.0.1:$Port").Replace('<memoryUsage limit="3 gb"/>', "<memoryUsage limit=`"$AmqMemoryLimitGB gb`"/>") | Set-Content $xml -Encoding utf8
$ncpu = [Environment]::ProcessorCount
$rows = [System.Collections.Generic.List[object]]::new()

function Listening([int]$p) { [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners() | Where-Object Port -eq $p }
function CpuMs($p) { $p.Refresh(); $p.TotalProcessorTime.TotalMilliseconds }
Add-Type -Namespace Win32 -Name SysTimes -MemberDefinition '[DllImport("kernel32.dll")] public static extern bool GetSystemTimes(out long idle, out long kernel, out long user);'
# Busy and total CPU time of the whole machine (100 ns units); kernel time includes idle time.
function MachineTimes { $i = 0L; $k = 0L; $u = 0L; [Win32.SysTimes]::GetSystemTimes([ref]$i, [ref]$k, [ref]$u) | Out-Null; @{ Busy = $k + $u - $i; Total = $k + $u } }

function Start-Broker($name) {
    if ($name -eq 'ActiveMQRust') {
        $margs = @('--port', $Port, '--admin-port', $AdminPort)
        if ($MqrustConfig) { $margs += @('--config', (Resolve-Path $MqrustConfig).Path) }
        # mimalloc reads its options from the environment at start; the child inherits them.
        if ($PreallocateMemory) { $env:MIMALLOC_RESERVE_OS_MEMORY = "${MqrustReserveGB}GiB" } else { Remove-Item Env:MIMALLOC_RESERVE_OS_MEMORY -ErrorAction SilentlyContinue }
        return Start-Process "$root\target\release\mqrust.exe" -PassThru -WindowStyle Hidden -ArgumentList $margs `
            -RedirectStandardOutput "$scratch\soak-broker.out" -RedirectStandardError "$scratch\soak-broker.err"
    }
    $h = $homes[$name].Home
    $script:data = Join-Path $scratch ("soakdata-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Force "$data\tmp" | Out-Null
    $heapArgs = if ($PreallocateMemory) { @("-Xms$AmqHeap", "-Xmx$AmqHeap", '-XX:+AlwaysPreTouch') } else { @("-Xmx$AmqHeap") }
    $jvm = $heapArgs + @("-Djava.security.auth.login.config=$h\conf\login.config", "-Dactivemq.home=$h", "-Dactivemq.base=$h",
        "-Dactivemq.conf=$h\conf", "-Dactivemq.data=$data", "-Djava.io.tmpdir=$data\tmp", '-jar', "$h\bin\activemq.jar", 'start', "xbean:file:$($xml.Replace('\','/'))")
    Start-Process java -PassThru -WindowStyle Hidden -WorkingDirectory $h -ArgumentList $jvm -RedirectStandardOutput "$scratch\soak-broker.out" -RedirectStandardError "$scratch\soak-broker.err"
}

foreach ($name in @('ActiveMQRust') + @($homes.Keys)) {
    if ($Only -and -not ($Only | Where-Object { $name -like "*$_*" })) { continue }
    $script:data = $null
    if (Listening $Port) { throw "port $Port busy" }
    $b = Start-Broker $name
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Listening $Port)) { if ($b.HasExited) { throw "$name exited" }; Start-Sleep -Milliseconds 5 }
    $startupMs = [int]$sw.Elapsed.TotalMilliseconds
    Start-Sleep -Seconds 5
    $jar = $ClientProfile   # the same client build for every broker: only the server changes
    $out = "$scratch\soak-client.out"
    $client = Start-Process java -PassThru -WindowStyle Hidden -RedirectStandardOutput $out -RedirectStandardError "$out.err" -ArgumentList @(
        '-Xmx4g', '-jar', "$root\tests\java-it\target\$jar\mqrust-acceptance.jar", 'bench', '--url', "tcp://127.0.0.1:$Port",
        '--user', 'admin', '--password', 'admin', '--scenario', 'soak', '--duration-seconds', $Duration, '--queues', $Queues, '--size', $Size, '--producers', $Producers, '--consumers', $Consumers, '--rate', $Rate, '--client-compression', ([bool]$ClientCompression).ToString().ToLower())
    $b0 = CpuMs $b; $m0 = MachineTimes; $t0 = Get-Date; $c0 = 0
    $ws = [System.Collections.Generic.List[double]]::new(); $pb = [System.Collections.Generic.List[double]]::new(); $cws = 0
    while (-not $client.HasExited) {
        $b.Refresh(); $ws.Add($b.WorkingSet64); $pb.Add($b.PrivateMemorySize64)
        try { $client.Refresh(); $cws = [math]::Max($cws, $client.WorkingSet64); $c0 = $client.TotalProcessorTime.TotalMilliseconds } catch {}
        Start-Sleep -Milliseconds 500
    }
    $secs = ((Get-Date) - $t0).TotalSeconds
    $bcpu = (CpuMs $b) - $b0
    $m1 = MachineTimes; $machinePct = if ($m1.Total -gt $m0.Total) { ($m1.Busy - $m0.Busy) / ($m1.Total - $m0.Total) * 100 } else { 0 }
    Stop-Process -Id $b.Id -Force; $b.WaitForExit(15000) | Out-Null
    Start-Sleep -Seconds 3
    $res = (Get-Content $out | Select-String 'RESULT').Line
    # ActiveMQ must keep everything in RAM: memory-limit, flow-control or spooling lines invalidate the run.
    $amqWarnings = ''
    if ($script:data -and (Test-Path "$script:data\activemq.log")) {
        $amqWarnings = (Select-String -Path "$script:data\activemq.log" -Pattern 'Usage Manager|memory limit|limit reached|Spool|tmp_storage|flow control' | Select-Object -First 3 | ForEach-Object { $_.Line.Trim() }) -join ' / '
    }
    $get = { param($k) if ($res -match "$k=([^ ]+)") { $Matches[1] } else { '' } }
    $row = [pscustomobject]@{
        Broker = $name; Status = (& $get 'status'); AmqRamWarnings = $amqWarnings; Reason = (& $get 'reason'); StartupMs = $startupMs
        Sent = (& $get 'sent'); Received = (& $get 'received'); Missing = (& $get 'missing'); Duplicates = (& $get 'duplicates'); OutOfOrder = (& $get 'out_of_order')
        SendMsgS = (& $get 'send_msgs_s'); RecvMsgS = (& $get 'recv_msgs_s'); RecvMBs = (& $get 'recv_mb_s'); ProduceMs = (& $get 'produce_ms'); ConsumeMs = (& $get 'consume_ms')
        P50us = (& $get 'p50_us'); P99us = (& $get 'p99_us'); MaxUs = (& $get 'max_us'); MaxSendLagMs = (& $get 'max_send_lag_ms')
        BrokerCpuPctOneCore = [math]::Round($bcpu / ($secs * 1000) * 100, 2)
        BrokerCpuPctMachine = [math]::Round($bcpu / ($secs * 1000 * $ncpu) * 100, 3)
        BrokerCpuMsPer1000Msg = if ((& $get 'received')) { [math]::Round($bcpu / ([double](& $get 'received') / 1000), 1) } else { '' }
        ClientCpuPctOneCore = [math]::Round($c0 / ($secs * 1000) * 100, 2)
        MachineCpuPct = [math]::Round($machinePct, 1)
        BrokerWsAvgMB = [math]::Round(($ws | Measure-Object -Average).Average / 1MB, 1); BrokerWsPeakMB = [math]::Round(($ws | Measure-Object -Maximum).Maximum / 1MB, 1)
        BrokerPbAvgMB = [math]::Round(($pb | Measure-Object -Average).Average / 1MB, 1); BrokerPbPeakMB = [math]::Round(($pb | Measure-Object -Maximum).Maximum / 1MB, 1)
        ClientWsPeakMB = [math]::Round($cws / 1MB, 1); Seconds = [math]::Round($secs, 1)
    }
    $rows.Add($row)
    $row | Format-List | Out-String | Write-Host
    New-Item -ItemType Directory -Force (Split-Path -Parent ([System.IO.Path]::GetFullPath($Results))) | Out-Null
    $rows | Export-Csv $Results -NoTypeInformation
}
Remove-Item $scratch -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "results: $Results"
Write-Host 'soak done'
