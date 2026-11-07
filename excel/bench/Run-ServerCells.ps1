<#
.SYNOPSIS
    Drives real Excel against a running convex-server to check live server cells (Excel X3).

.DESCRIPTION
    Starts a private Excel over COM, loads the packed add-in, writes CX.LIVE, CX.SNAP and
    CX.STATUS cells for one instrument and samples them while the server's feed moves it.
    With -StopServer and -StartServer it also stops the server, checks that values stay
    with quality Stale, starts it again and checks that the cells follow it once more.

    The add-in reads %APPDATA%\Convex\server.json. For the run the script writes one that
    points at -Server with sign-in None (a server in local development mode), and restores
    the previous file (or removes the new one) afterwards.

    The showcase serves INSTRUMENT-001 with a quote a second (deploy/showcase). The RTD
    throttle is left at Excel's setting.

.EXAMPLE
    excel/bench/Run-ServerCells.ps1 -Server http://127.0.0.1:8081 -StopServer { Stop-Process -Id $pid } -StartServer { ... }
#>
param(
    [string] $Server = 'http://127.0.0.1:8081',
    [string] $Instrument = 'INSTRUMENT-001',
    [int] $Seconds = 12,
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

Add-Type -Namespace ServerCells -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
'@

$configDir = Join-Path $env:APPDATA 'Convex'
$configPath = Join-Path $configDir 'server.json'
$backup = $null
if (Test-Path $configPath) { $backup = Get-Content -Raw $configPath }
New-Item -ItemType Directory -Force $configDir | Out-Null
[IO.File]::WriteAllText($configPath, (@{ url = $Server; sign_in = 'none' } | ConvertTo-Json -Compress))

$app = $null
[uint32] $excelPid = 0
$result = [ordered]@{ server = $Server; instrument = $Instrument }
try {
    $app = New-Object -ComObject Excel.Application
    $app.Visible = [bool]$Visible
    $app.DisplayAlerts = $false
    [void][ServerCells.Win32]::GetWindowThreadProcessId([IntPtr]$app.Hwnd, [ref]$excelPid)
    $result.excel_build = $app.Build
    $result.rtd_throttle_ms = $app.RTD.ThrottleInterval
    if (-not (Invoke-Com { $app.RegisterXLL((Resolve-Path $AddInXll).Path) })) { throw "RegisterXLL failed: $AddInXll" }

    $wb = Invoke-Com { $app.Workbooks.Add() }
    $ws = $wb.Worksheets.Item(1)
    $id = '"' + $Instrument + '"'
    $cells = [ordered]@{
        A1 = '=CX.STATUS()'
        A2 = "=CX.LIVE($id,""clean_price_mid"")"
        A3 = "=CX.LIVE($id,""clean_price_mid"",""quality"")"
        A4 = "=CX.LIVE($id,""clean_price_mid"",""revision"")"
        A5 = '=A2*2'
        A6 = '=CX.LIVE("UNKNOWN-1","clean_price_mid")'
        A7 = '=CX.LASTERROR(A6)'
        A8 = "=CX.LIVE($id,""no_such_mid"")"
        A9 = '=CX.LASTERROR(A8)'
        A10 = "=CX.SNAP($id,""clean_price_mid"")"
        A11 = '=CX.STATUS("runtime")'
        A12 = '=CX.STATUS("calculation")'
        A13 = "=CX.LIVE($id,""ytm_mid"")"
    }
    foreach ($k in $cells.Keys) { Invoke-Com { $ws.Range($k).Formula = $cells[$k] } | Out-Null }
    function Read([string] $a) { Invoke-Com { $ws.Range($a).Value2 } }
    function Text([string] $a) { Invoke-Com { $ws.Range($a).Text } }

    $result.connect_s = Wait-Until { (Read 'A1') -eq 'Connected' } 20 'Connected'
    $result.first_value_s = Wait-Until { (Read 'A2') -is [double] } 20 'a first CX.LIVE value'

    # Sample while the feed moves the price.
    $samples = [System.Collections.Generic.List[object]]::new()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
        $samples.Add([ordered]@{ t = [math]::Round($sw.Elapsed.TotalSeconds, 2); clean = Read 'A2'; revision = Read 'A4'; doubled = Read 'A5' })
        Start-Sleep -Milliseconds 500
    }
    $distinct = @($samples | ForEach-Object { $_.clean } | Select-Object -Unique)
    $result.samples = $samples.Count
    $result.distinct_prices = $distinct.Count
    $result.revision_first = $samples[0].revision
    $result.revision_last = $samples[-1].revision
    $result.downstream_follows = [bool](@($samples | Where-Object { [math]::Abs($_.doubled - 2 * $_.clean) -gt 1e-9 }).Count -eq 0)
    $result.quality = Read 'A3'
    $result.unknown_text = Text 'A6'
    $result.unknown_detail = Read 'A7'
    $result.no_measure_detail = Read 'A9'
    $result.snap = Read 'A10'
    $result.runtime = Read 'A11'
    $result.calculation = Read 'A12'
    $result.ytm = Read 'A13'

    if ($StopServer -and $StartServer) {
        & $StopServer
        $result.stale_after_s = Wait-Until { (Read 'A3') -eq 'Stale' } 60 'quality Stale after the server stopped'
        # The last price stays while the server is away.
        $held = Read 'A2'
        Start-Sleep -Seconds 3
        $result.value_kept = [bool](($held -is [double]) -and ((Read 'A2') -eq $held))
        $result.status_while_down = Read 'A1'
        & $StartServer
        $result.reconnect_s = Wait-Until { (Read 'A1') -eq 'Connected' -and (Read 'A3') -ne 'Stale' } 120 'reconnect'
        $after = Read 'A4'
        $result.moves_after_reconnect_s = Wait-Until { (Read 'A4') -ne $after } 30 'a new revision after reconnect'
        $result.quality_after = Read 'A3'
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
    if ($null -ne $backup) { [IO.File]::WriteAllText($configPath, $backup) } else { Remove-Item -Force $configPath -ErrorAction SilentlyContinue }
}
$result | ConvertTo-Json -Depth 4
