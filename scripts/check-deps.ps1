# ActiveMQRust by Matteo Baccan
# SPDX-License-Identifier: MIT
#
# Lists the DLLs imported by an executable (via dumpbin, or by reading the PE import table
# when dumpbin is not available) and fails if any is outside the Windows system allow-list.

param([Parameter(Mandatory = $true)][string]$Exe)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Exe)) { Write-Error "not found: $Exe"; exit 2 }

$allowed = @(
    'kernel32.dll', 'ntdll.dll', 'ws2_32.dll', 'advapi32.dll', 'bcrypt.dll', 'bcryptprimitives.dll',
    'userenv.dll', 'user32.dll', 'shell32.dll', 'ole32.dll', 'oleaut32.dll', 'secur32.dll', 'crypt32.dll',
    'iphlpapi.dll', 'psapi.dll', 'dbghelp.dll', 'synchronization.dll', 'mswsock.dll', 'shlwapi.dll',
    'api-ms-win-core-synch-l1-2-0.dll', 'api-ms-win-core-path-l1-1-0.dll', 'api-ms-win-core-winrt-error-l1-1-0.dll'
)

function Get-ImportsFromPe([string]$path) {
    $b = [System.IO.File]::ReadAllBytes($path)
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    $numSections = [BitConverter]::ToUInt16($b, $pe + 6)
    $optSize = [BitConverter]::ToUInt16($b, $pe + 20)
    $opt = $pe + 24
    $magic = [BitConverter]::ToUInt16($b, $opt)
    $dirOffset = if ($magic -eq 0x20b) { $opt + 112 } else { $opt + 96 }
    $importRva = [BitConverter]::ToUInt32($b, $dirOffset + 8)
    $sections = @()
    $sec = $opt + $optSize
    for ($i = 0; $i -lt $numSections; $i++) {
        $o = $sec + $i * 40
        $sections += [pscustomobject]@{
            Va = [BitConverter]::ToUInt32($b, $o + 12); Size = [BitConverter]::ToUInt32($b, $o + 8)
            Raw = [BitConverter]::ToUInt32($b, $o + 20)
        }
    }
    function RvaToOff([uint32]$rva) {
        foreach ($s in $sections) { if ($rva -ge $s.Va -and $rva -lt ($s.Va + [Math]::Max($s.Size, 1))) { return $rva - $s.Va + $s.Raw } }
        return -1
    }
    $names = @()
    $off = RvaToOff $importRva
    while ($off -gt 0) {
        $nameRva = [BitConverter]::ToUInt32($b, $off + 12)
        if ($nameRva -eq 0) { break }
        $n = RvaToOff $nameRva
        $end = $n; while ($b[$end] -ne 0) { $end++ }
        $names += [System.Text.Encoding]::ASCII.GetString($b, $n, $end - $n)
        $off += 20
    }
    # Delay-load imports.
    $delayRva = [BitConverter]::ToUInt32($b, $dirOffset + 13 * 8)
    $off = RvaToOff $delayRva
    while ($delayRva -ne 0 -and $off -gt 0) {
        $nameRva = [BitConverter]::ToUInt32($b, $off + 4)
        if ($nameRva -eq 0) { break }
        $n = RvaToOff $nameRva
        $end = $n; while ($b[$end] -ne 0) { $end++ }
        $names += [System.Text.Encoding]::ASCII.GetString($b, $n, $end - $n)
        $off += 32
    }
    return $names
}

$dlls = $null
$dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
if ($dumpbin) {
    $out = & $dumpbin.Source /nologo /dependents $Exe
    $dlls = $out | Where-Object { $_ -match '^\s+\S+\.dll\s*$' } | ForEach-Object { $_.Trim() }
} else {
    $dlls = Get-ImportsFromPe $Exe
}

$bad = @()
Write-Host "Imported DLLs of ${Exe}:"
foreach ($d in ($dlls | Sort-Object -Unique)) {
    $ok = ($allowed -contains $d.ToLowerInvariant()) -or ($d.ToLowerInvariant().StartsWith('api-ms-win-'))
    Write-Host ("  {0,-40} {1}" -f $d, $(if ($ok) { 'system' } else { 'NOT ALLOWED' }))
    if (-not $ok) { $bad += $d }
}
if ($bad.Count -gt 0) {
    Write-Host "FAIL: non-system DLLs: $($bad -join ', ')"
    exit 1
}
Write-Host 'OK: only Windows system DLLs are imported'
exit 0
