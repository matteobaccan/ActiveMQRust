# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Starts a local Apache ActiveMQ (5.18.x or 6.x) in the foreground, for reference runs of the
# Java acceptance and integration suites, with a fresh temporary data directory.
#
#   pwsh scripts\start-activemq.ps1 -ActiveMQHome C:\tools\apache-activemq-6.3.2 [-Config reference|tuned|default]

param(
    [Parameter(Mandatory = $true)][string]$ActiveMQHome,
    [ValidateSet('reference', 'tuned', 'default')][string]$Config = 'reference',
    [string]$Java = 'java',
    [string]$Xmx = '4g'
)

$data = Join-Path ([System.IO.Path]::GetTempPath()) ("activemq-data-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force "$data\tmp" | Out-Null
$jvm = @(
    "-Xmx$Xmx",
    '-Djava.util.logging.config.file=logging.properties',
    "-Djava.security.auth.login.config=$ActiveMQHome\conf\login.config",
    "-Dactivemq.home=$ActiveMQHome", "-Dactivemq.base=$ActiveMQHome", "-Dactivemq.conf=$ActiveMQHome\conf",
    "-Dactivemq.data=$data", "-Djava.io.tmpdir=$data\tmp",
    '-jar', "$ActiveMQHome\bin\activemq.jar", 'start')
if ($Config -ne 'default') {
    $xml = (Join-Path $PSScriptRoot "activemq-bench\activemq-$Config.xml").Replace('\', '/')
    $jvm += "xbean:file:$xml"
}
Write-Host "ActiveMQ data directory: $data"
Push-Location $ActiveMQHome
try { & $Java @jvm } finally { Pop-Location }
