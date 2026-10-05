# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Starts a local Apache ActiveMQ (5.18.x or 6.x) in the foreground, for reference runs of the
# Java acceptance and integration suites, with a fresh temporary data directory.
# The OpenWire port (and, for the default configuration, the web console port) can be changed;
# the configuration files of the distribution are never modified.
#
#   pwsh scripts\start-activemq.ps1 -ActiveMQHome C:\tools\apache-activemq-6.3.2 [-Config reference|tuned|default] [-Port 61616] [-AdminPort 8161]

param(
    [Parameter(Mandatory = $true)][string]$ActiveMQHome,
    [ValidateSet('reference', 'tuned', 'default')][string]$Config = 'reference',
    [string]$Java = 'java',
    [string]$Xmx = '4g',
    [int]$Port = 61616,
    [int]$AdminPort = 8161
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path (Join-Path $ActiveMQHome 'bin\activemq.jar'))) { throw "ActiveMQ installation not found: $ActiveMQHome" }
$busy = [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners() | Where-Object { $_.Port -eq $Port }
if ($busy) { throw "port $Port is already in use" }

$data = Join-Path ([System.IO.Path]::GetTempPath()) ("activemq-data-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force "$data\tmp" | Out-Null
$conf = Join-Path $ActiveMQHome 'conf'
$target = @()
if ($Config -eq 'default') {
    # Copy of the distribution's conf directory with only the listening ports changed.
    $conf = Join-Path $data 'conf'
    Copy-Item -Recurse (Join-Path $ActiveMQHome 'conf') $conf
    $xmlPath = Join-Path $conf 'activemq.xml'
    $xml = Get-Content $xmlPath -Raw
    $ports = @{ '61616' = $Port; '5672' = $Port + 1; '61613' = $Port + 2; '1883' = $Port + 3; '61614' = $Port + 4 }
    foreach ($k in $ports.Keys) { $xml = $xml.Replace("0.0.0.0:$k", "127.0.0.1:$($ports[$k])") }
    Set-Content -Path $xmlPath -Value $xml -Encoding UTF8
    foreach ($f in @('jetty.xml', 'jetty-spring.properties')) {
        $p = Join-Path $conf $f
        if (Test-Path $p) {
            $t = (Get-Content $p -Raw).Replace('name="port" value="8161"', "name=`"port`" value=`"$AdminPort`"").Replace('jetty.http.port=8161', "jetty.http.port=$AdminPort")
            Set-Content -Path $p -Value $t -Encoding UTF8
        }
    }
} else {
    $xmlPath = Join-Path $data "activemq-$Config.xml"
    $xml = (Get-Content (Join-Path $PSScriptRoot "activemq-bench\activemq-$Config.xml") -Raw).Replace('127.0.0.1:61616', "127.0.0.1:$Port")
    Set-Content -Path $xmlPath -Value $xml -Encoding UTF8
    $target = @('xbean:file:' + $xmlPath.Replace('\', '/'))
}
$jvm = @(
    "-Xmx$Xmx",
    '-Djava.util.logging.config.file=logging.properties',
    "-Djava.security.auth.login.config=$conf\login.config",
    "-Dactivemq.home=$ActiveMQHome", "-Dactivemq.base=$ActiveMQHome", "-Dactivemq.conf=$conf",
    "-Dactivemq.data=$data", "-Djava.io.tmpdir=$data\tmp",
    '-jar', "$ActiveMQHome\bin\activemq.jar", 'start') + $target
Write-Host "ActiveMQ data directory: $data (OpenWire port $Port)"
Push-Location $ActiveMQHome
try { & $Java @jvm } finally { Pop-Location }
