<#
.SYNOPSIS
  X0 measurement driver (excel-plan). Drives a private Excel instance over COM.

.DESCRIPTION
  Scenarios:
    native  Loads only the packed bench .xll from a folder with no convex_ffi.dll
            beside it, and records CXB.NATIVE(): did the packed native DLL load?
    rtd     N synthetic RTD topics (=CXB.TOPIC) each with a CXB.SEEN cell, all
            topics pushed every 250 ms, at each RTD.ThrottleInterval. Records
            push-to-cell latency, refresh batches and main-thread delay.
    cxlive  The current add-in's CX.SPREAD.LIVE: N live cells (distinct marks,
            so N engine calls) reference a curve by name; the driver edits one
            curve input and records how long until all N live cells changed.

  RTD.ThrottleInterval is an Excel-wide, persistent per-user setting. The
  driver restores the original value in a finally block and prints it.

.EXAMPLE
  .\Run-ExcelBench.ps1 -OutDir C:\temp\x0 -Runs 2
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $OutDir,
    [string[]] $Scenario = @('native', 'rtd', 'cxlive'),
    [int[]] $Counts = @(1000, 10000),
    [int[]] $Throttles = @(2000, 250),
    [int[]] $LiveCounts = @(100, 1000, 10000),
    [double] $Seconds = 10,
    [int] $Runs = 1,
    [switch] $Visible,
    [string] $BenchXll = (Join-Path $PSScriptRoot '..\Convex.Excel.Bench\bin\Release\net472\publish\Convex.Excel.Bench64-packed.xll'),
    [string] $AddInXll = (Join-Path $PSScriptRoot '..\Convex.Excel\bin\Release\net472\publish\Convex.Excel64-packed.xll')
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

# Excel rejects COM calls while busy (RPC_E_CALL_REJECTED); retry them.
function Invoke-Com([scriptblock] $Block, [int] $Tries = 200) {
    for ($i = 0; ; $i++) {
        try { return & $Block }
        catch {
            $busy = $_.Exception.HResult -in @(-2147418111, -2147417846) -or
                    $_.Exception.Message -match 'RPC_E_CALL_REJECTED|0x80010001|0x8001010A'
            if (-not $busy -or $i -ge $Tries) { throw }
            Start-Sleep -Milliseconds 50
        }
    }
}

function Wait-Calc($App, [int] $TimeoutSec = 300) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ((Invoke-Com { $App.CalculationState }) -ne 0) {
        if ($sw.Elapsed.TotalSeconds -gt $TimeoutSec) { throw "calculation did not finish in $TimeoutSec s" }
        Start-Sleep -Milliseconds 100
    }
}

function Wait-Until([scriptblock] $Cond, [int] $TimeoutSec, [string] $What) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while (-not (& $Cond)) {
        if ($sw.Elapsed.TotalSeconds -gt $TimeoutSec) { throw "timed out after $TimeoutSec s waiting for $What" }
        Start-Sleep -Milliseconds 200
    }
}

function Ctl($App, [string] $Cmd, $A = [Type]::Missing, $B = [Type]::Missing) {
    $r = Invoke-Com { $App.Run('CXB.CTL', $Cmd, $A, $B) }
    if ("$r" -like 'FAIL*' -or "$r" -like 'unknown*') { throw "CXB.CTL $Cmd -> $r" }
    return $r
}

function New-Excel {
    $app = New-Object -ComObject Excel.Application
    $app.Visible = [bool]$Visible
    $app.DisplayAlerts = $false
    return $app
}

function Close-Excel($App) {
    if ($null -eq $App) { return }
    try { foreach ($wb in @($App.Workbooks)) { $wb.Close($false) } } catch { }
    try { $App.Quit() } catch { }
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($App)
    [GC]::Collect(); [GC]::WaitForPendingFinalizers()
}

function Register-Xll($App, [string] $Path) {
    $full = (Resolve-Path $Path).Path
    if (-not (Invoke-Com { $App.RegisterXLL($full) })) { throw "RegisterXLL failed: $full" }
}

$summary = [System.Collections.Generic.List[object]]::new()
$probeApp = New-Excel
$originalThrottle = $probeApp.RTD.ThrottleInterval
$build = $probeApp.Build
Close-Excel $probeApp
Write-Host "Excel build $build; original RTD.ThrottleInterval = $originalThrottle ms"

$app = $null
try {
    if ('native' -in $Scenario) {
        $dir = Join-Path $OutDir 'native-only'
        New-Item -ItemType Directory -Force $dir | Out-Null
        Get-ChildItem $dir | Remove-Item -Force
        Copy-Item $BenchXll $dir
        $app = New-Excel
        Register-Xll $app (Join-Path $dir (Split-Path $BenchXll -Leaf))
        $result = Invoke-Com { $app.Run('CXB.NATIVE') }
        Write-Host "native: $result"
        Set-Content (Join-Path $OutDir 'native.txt') "$result"
        $summary.Add([pscustomobject]@{ scenario = 'native'; n = 0; throttle = 0; run = 1; result = "$result" })
        Close-Excel $app; $app = $null
    }

    if ('rtd' -in $Scenario) {
        foreach ($run in 1..$Runs) {
            foreach ($n in $Counts) {
                foreach ($thr in $Throttles) {
                    $app = New-Excel
                    Register-Xll $app $BenchXll
                    $app.RTD.ThrottleInterval = $thr
                    $wb = $app.Workbooks.Add(); $ws = $wb.Worksheets.Item(1)
                    $sw = [Diagnostics.Stopwatch]::StartNew()
                    Invoke-Com { $ws.Range("A1:A$n").Formula = '=CXB.TOPIC(ROW())' } | Out-Null
                    Invoke-Com { $ws.Range("B1:B$n").Formula = '=CXB.SEEN(A1,ROW(),TRUE)' } | Out-Null
                    Wait-Calc $app
                    $setupMs = $sw.ElapsedMilliseconds
                    Ctl $app 'reset' | Out-Null
                    Ctl $app 'probe' 50 | Out-Null
                    $push = Ctl $app 'push' ($n * 4) $Seconds
                    Write-Host "rtd n=$n throttle=$thr run=$run setup=${setupMs}ms $push"
                    Start-Sleep -Milliseconds ([int]($Seconds * 1000 + 2 * $thr + 1000))
                    $file = Join-Path $OutDir "rtd-n$n-thr$thr-run$run.json"
                    Ctl $app 'dump' $file | Out-Null
                    $summary.Add([pscustomobject]@{ scenario = 'rtd'; n = $n; throttle = $thr; run = $run; setup_ms = $setupMs; file = $file })
                    Close-Excel $app; $app = $null
                }
            }
        }
    }

    if ('cxlive' -in $Scenario) {
        foreach ($run in 1..$Runs) {
            foreach ($n in $LiveCounts) {
              foreach ($thr in $Throttles) {
                $app = New-Excel
                Register-Xll $app $BenchXll
                Register-Xll $app $AddInXll
                $app.RTD.ThrottleInterval = $thr
                $wb = $app.Workbooks.Add(); $ws = $wb.Worksheets.Item(1)
                [double[]] $tenors = 0.5, 1, 2, 5, 10, 30
                for ($i = 0; $i -lt 6; $i++) {
                    $ws.Range("D$($i + 1)").Value2 = $tenors[$i]
                    $ws.Range("E$($i + 1)").Value2 = [double]0.04
                }
                $ws.Range('F1').Formula = '=CX.CURVE("USD.SOFR",DATE(2025,1,15),D1:D6,E1:E6)'
                $ws.Range('F2').Formula = '=CX.BOND("BENCHBOND",0.05,DATE(2035,1,15),DATE(2025,1,15))'
                Wait-Calc $app
                $sw = [Diagnostics.Stopwatch]::StartNew()
                # Distinct clean-price marks so CxCache cannot collapse the N requests.
                # CX.SPREAD.LIVE (Z off the named curve): CX.PRICE returns no z_spread.
                Invoke-Com { $ws.Range("A1:A$n").Formula = '=CX.SPREAD.LIVE("BENCHBOND","USD.SOFR",DATE(2025,4,15),FIXED(90+ROW()*0.0001,4,TRUE)&"C","Z")' } | Out-Null
                Invoke-Com { $ws.Range("B1:B$n").Formula = '=CXB.SEEN(A1,ROW(),FALSE)' } | Out-Null
                Invoke-Com { $ws.Range('G1').Formula = "=COUNT(A1:A$n)" } | Out-Null
                Wait-Until { (Invoke-Com { $ws.Range('G1').Value2 }) -eq $n } 300 "$n live cells to compute"
                $setupMs = $sw.ElapsedMilliseconds
                $a1 = Invoke-Com { $ws.Range('A1').Value2 }
                Ctl $app 'reset' | Out-Null
                Ctl $app 'probe' 50 | Out-Null
                Ctl $app 'mark' 'curve_edit' | Out-Null
                Invoke-Com { $ws.Range('E3').Value2 = 0.0401 } | Out-Null
                Wait-Until { [double](Ctl $app 'changed') -ge $n } 600 "$n live cells to change"
                Start-Sleep -Milliseconds 500
                $file = Join-Path $OutDir "cxlive-n$n-thr$thr-run$run.json"
                Ctl $app 'dump' $file | Out-Null
                $a1after = Invoke-Com { $ws.Range('A1').Value2 }
                Write-Host "cxlive n=$n throttle=$thr run=$run setup=${setupMs}ms A1 $a1 -> $a1after"
                $summary.Add([pscustomobject]@{ scenario = 'cxlive'; n = $n; throttle = $thr; run = $run; setup_ms = $setupMs; file = $file })
                Close-Excel $app; $app = $null
              }
            }
        }
    }
}
finally {
    Close-Excel $app
    $restore = New-Excel
    $restore.RTD.ThrottleInterval = $originalThrottle
    Write-Host "Restored RTD.ThrottleInterval = $($restore.RTD.ThrottleInterval) ms"
    Close-Excel $restore
    $summary | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $OutDir 'summary.json')
}
