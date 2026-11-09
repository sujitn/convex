<#
.SYNOPSIS
    Drives real Excel against a running convex-server to check CX.LIMIT cells (Excel X4).

.DESCRIPTION
    Starts a private Excel over COM, loads the packed add-in, writes CX.LIMIT cells for one
    limit of one book and samples them while the server's feed moves the book's value. The
    book needs a limit set whose bounds the feed crosses, so its status changes while the
    script samples (see excel/bench/README.md). With -StopServer and -StartServer it also
    stops the server, checks that the limit stays with quality Stale, starts it again and
    checks that the cells are current once more.

    The add-in reads %APPDATA%\Convex\server.json. For the run the script writes one that
    points at -Server with sign-in None (a server in local development mode), and restores
    the previous file (or removes the new one) afterwards.

.EXAMPLE
    excel/bench/Run-LimitCells.ps1 -Server http://127.0.0.1:8081 -Book SHOWCASE-BOOK -Limit mv
#>
param(
    [string] $Server = 'http://127.0.0.1:8081',
    [string] $Book = 'SHOWCASE-BOOK',
    [string] $Limit = 'mv',
    [int] $Seconds = 30,
    [scriptblock] $StopServer,
    [scriptblock] $StartServer,
    [switch] $Visible,
    [string] $AddInXll = (Join-Path $PSScriptRoot '..\Convex.Excel\bin\Release\net472\publish\Convex.Excel64-packed.xll')
)
$ErrorActionPreference = 'Stop'

# Excel answers RPC_E_CALL_REJECTED while busy; retry.
function Invoke-Com([scriptblock] $Block) {
    for ($i = 0; ; $i++) {
        try { return & $Block }
        catch [System.Runtime.InteropServices.COMException] {
            if ($i -ge 100 -or $_.Exception.HResult -ne -2147418111) { throw }
            Start-Sleep -Milliseconds 100
        }
    }
}

function Wait-Until([scriptblock] $Cond, [int] $TimeoutSec, [string] $What) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $Cond)) {
        if ($sw.Elapsed.TotalSeconds -gt $TimeoutSec) { throw "timed out after $TimeoutSec s waiting for $What" }
        Start-Sleep -Milliseconds 250
    }
    return $sw.Elapsed.TotalSeconds
}

Add-Type -Namespace LimitCells -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
'@

$configDir = Join-Path $env:APPDATA 'Convex'
$configPath = Join-Path $configDir 'server.json'
$backup = $null
# Bytes, so the restore is exact whatever the file's encoding.
if (Test-Path $configPath) { $backup = [IO.File]::ReadAllBytes($configPath) }
$app = $null
[uint32] $excelPid = 0
$result = [ordered]@{ server = $Server; book = $Book; limit = $Limit }
try {
    # Inside try, so finally restores the user's server.json if anything below fails.
    New-Item -ItemType Directory -Force $configDir | Out-Null
    [IO.File]::WriteAllText($configPath, (@{ url = $Server; sign_in = 'none' } | ConvertTo-Json -Compress))
    $app = New-Object -ComObject Excel.Application
    $app.Visible = [bool]$Visible
    $app.DisplayAlerts = $false
    [void][LimitCells.Win32]::GetWindowThreadProcessId([IntPtr]$app.Hwnd, [ref]$excelPid)
    $result.excel_build = $app.Build
    $result.rtd_throttle_ms = $app.RTD.ThrottleInterval
    if (-not (Invoke-Com { $app.RegisterXLL((Resolve-Path $AddInXll).Path) })) { throw "RegisterXLL failed: $AddInXll" }

    $wb = Invoke-Com { $app.Workbooks.Add() }
    $ws = $wb.Worksheets.Item(1)
    $b = '"' + $Book + '"'
    $l = '"' + $Limit + '"'
    $cells = [ordered]@{
        A1 = '=CX.STATUS()'
        A2 = "=CX.LIMIT($b,$l)"
        A3 = "=CX.LIMIT($b,$l,""value"")"
        A4 = "=CX.LIMIT($b,$l,""utilisation"")"
        A5 = "=CX.LIMIT($b,$l,""evaluated_at"")"
        A6 = "=CX.LIMIT($b,$l,""record_id"")"
        A7 = "=CX.LIMIT($b,$l,""quality"")"
        A8 = '=IF(A2="breach",1,0)'
        A9 = "=CX.LIMIT($b,""no-such-limit"")"
        A10 = '=CX.LASTERROR(A9)'
        A11 = '=CX.LIMIT("NO-SET-BOOK","x")'
        A12 = '=CX.LASTERROR(A11)'
        A13 = '=CX.STATUS("books")'
        A14 = "=CX.LIMIT($b,$l,""measure"")"
        A15 = "=CX.LIMIT($b,$l,""upper"")"
    }
    foreach ($k in $cells.Keys) { Invoke-Com { $ws.Range($k).Formula = $cells[$k] } | Out-Null }
    function Read([string] $a) { Invoke-Com { $ws.Range($a).Value2 } }

    $result.connect_s = Wait-Until { (Read 'A1') -eq 'Connected' } 20 'Connected'
    $result.first_status_s = Wait-Until { (Read 'A2') -is [string] } 20 'a first CX.LIMIT status'

    # Sample while the feed moves the book's value across the limit's bounds.
    $samples = [System.Collections.Generic.List[object]]::new()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
        $samples.Add([ordered]@{
            t = [math]::Round($sw.Elapsed.TotalSeconds, 2); status = Read 'A2'; value = Read 'A3'
            record = Read 'A6'; flag = Read 'A8'
        })
        Start-Sleep -Milliseconds 500
    }
    $result.samples = $samples.Count
    $result.statuses = @($samples | ForEach-Object { $_.status } | Select-Object -Unique)
    $changes = 0
    for ($i = 1; $i -lt $samples.Count; $i++) { if ($samples[$i].status -ne $samples[$i - 1].status) { $changes++ } }
    $result.status_changes = $changes
    $result.record_ids = @($samples | ForEach-Object { $_.record } | Where-Object { $_ -is [double] } | Select-Object -Unique)
    $result.dependent_follows = [bool](@($samples | Where-Object { ($_.status -eq 'breach') -ne ($_.flag -eq 1) }).Count -eq 0)
    $result.value = Read 'A3'
    $result.utilisation = Read 'A4'
    $result.evaluated_at = [DateTime]::FromOADate((Read 'A5')).ToString('u')
    $result.quality = Read 'A7'
    $result.measure = Read 'A14'
    $result.upper = Read 'A15'
    $result.no_limit_detail = Read 'A10'
    $result.no_set_detail = Read 'A12'
    $result.books_followed = Read 'A13'

    if ($StopServer -and $StartServer) {
        & $StopServer
        $result.stale_after_s = Wait-Until { (Read 'A7') -eq 'Stale' } 60 'quality Stale after the server stopped'
        $held = Read 'A2'
        Start-Sleep -Seconds 3
        $result.status_kept = [bool](($held -is [string]) -and ((Read 'A2') -eq $held))
        $result.status_while_down = Read 'A1'
        & $StartServer
        $result.reconnect_s = Wait-Until { (Read 'A1') -eq 'Connected' -and (Read 'A7') -ne 'Stale' } 120 'reconnect'
        $result.quality_after = Read 'A7'
        $result.status_after = Read 'A2'
    }
}
finally {
    if ($app) {
        try { foreach ($w in @($app.Workbooks)) { $w.Close($false) } } catch { }
        try { $app.Quit() } catch { }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($app)
        [GC]::Collect(); [GC]::WaitForPendingFinalizers()
        if ($excelPid) {
            $p = Get-Process -Id $excelPid -ErrorAction SilentlyContinue
            if ($p -and -not $p.WaitForExit(5000)) { Stop-Process -Id $excelPid -Force }
        }
    }
    if ($null -ne $backup) { [IO.File]::WriteAllBytes($configPath, $backup) } else { Remove-Item -Force $configPath -ErrorAction SilentlyContinue }
}
$result | ConvertTo-Json -Depth 4
