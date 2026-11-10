<#
.SYNOPSIS
    Installs, upgrades and uninstalls the MSI, checking the files, Excel's OPEN values and
    the value UPDATECHECK=0 writes.

.DESCRIPTION
    This changes the current user's Excel add-in list, so it runs only in CI, where
    no Excel is installed. A copy of ping.exe named EXCEL.EXE stands in for a running Excel.

.PARAMETER Stage
    The folder Package-AddIn.ps1 staged.

.PARAMETER Msi
    The MSI that folder went into. Its next version is built here.
#>
param(
    [Parameter(Mandatory)] [string] $Stage,
    [Parameter(Mandatory)] [string] $Msi
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw "This changes the user's Excel add-in list: it runs only in CI." }

$cargo = Get-Content -Raw (Join-Path $PSScriptRoot '..\..\Cargo.toml')
$version = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
$parts = [version]$version
$nextVersion = '{0}.{1}.{2}' -f $parts.Major, $parts.Minor, ($parts.Build + 1)

$optionsKey = 'HKCU:\Software\Microsoft\Office\16.0\Excel\Options'
$root = Join-Path $env:LOCALAPPDATA 'Convex\Excel'
$otherA = '/R "C:\Tools\other-a.xll"'
$otherB = '/R "C:\Tools\other-b.xll"'
$logs = Join-Path $env:RUNNER_TEMP 'msi-logs'
New-Item -ItemType Directory -Force $logs | Out-Null
$script:step = 0

function Check([bool] $ok, [string] $what) {
    if (-not $ok) { throw "Check failed: $what" }
    Write-Output "ok: $what"
}

function Run-Msi([string[]] $Arguments) {
    $script:step++
    $script:log = Join-Path $logs "step$script:step.log"
    (Start-Process msiexec.exe -ArgumentList ($Arguments + @('/qn', '/l*v', "`"$script:log`"")) -Wait -PassThru).ExitCode
}

function Open-Values {
    $values = [ordered]@{}
    if (Test-Path $optionsKey) {
        $key = Get-Item $optionsKey
        foreach ($name in $key.GetValueNames() | Where-Object { $_ -match '^OPEN([1-9][0-9]*)?$' } | Sort-Object { [int]($_ -replace 'OPEN', '0') }) {
            $values[$name] = $key.GetValue($name)
        }
    }
    $values
}

function Ours { @((Open-Values).GetEnumerator() | Where-Object { $_.Value -like "*$root\*" }) }

# What UPDATECHECK=0 writes: null when there is no value.
$convexKey = 'HKCU:\Software\Convex\Excel'
function Update-Check {
    if (-not (Test-Path $convexKey)) { return $null }
    $key = Get-Item $convexKey
    if ($key.GetValueNames() -notcontains 'UpdateCheck') { return $null }
    [pscustomobject]@{ Kind = $key.GetValueKind('UpdateCheck').ToString(); Value = $key.GetValue('UpdateCheck') }
}

function Entries {
    @(Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -eq 'Convex Excel Add-in' })
}

function Check-Installed([string] $expected, [string] $when) {
    $folder = Join-Path $root $expected
    Check (Test-Path (Join-Path $folder 'Convex.Excel64.xll')) "$when`: the .xll is in the $expected folder"
    Check (Test-Path (Join-Path $folder 'convex_ffi.dll')) "$when`: convex_ffi.dll is beside it"
    $folders = @(Get-ChildItem $root -Directory | Where-Object { $_.Name -ne 'logs' } | ForEach-Object Name)
    Check ($folders.Count -eq 1 -and $folders[0] -eq $expected) "$when`: only the $expected folder exists (found: $($folders -join ', '))"
    $ours = @(Ours)
    Check ($ours.Count -eq 1) "$when`: exactly one OPEN value is ours (found $($ours.Count))"
    Check ($ours[0].Value -eq "/R `"$folder\Convex.Excel64.xll`"") "$when`: it points at the $expected .xll (it is $($ours[0].Value))"
    Check ((Entries).Count -eq 1) "$when`: Apps & features has one entry"
}

$excel = $null
try {
    # Signed after packing: the two files inside the MSI, and the MSI.
    foreach ($file in (Join-Path $Stage 'Convex.Excel64.xll'), (Join-Path $Stage 'convex_ffi.dll'), $Msi) {
        $status = (Get-AuthenticodeSignature $file).Status
        Check ($status -eq 'Valid') "$file is signed (status $status)"
    }

    # Another add-in on either side of a value left by an older install.
    New-Item -Path $optionsKey -Force | Out-Null
    Set-ItemProperty $optionsKey -Name OPEN -Value $otherA
    Set-ItemProperty $optionsKey -Name OPEN1 -Value "/R `"$root\0.0.1\Convex.Excel64.xll`""
    Set-ItemProperty $optionsKey -Name OPEN2 -Value $otherB

    $code = Run-Msi @('/i', "`"$Msi`"")
    Check ($code -eq 0) "install exits 0 (exit $code)"
    Check-Installed $version 'install'
    $values = Open-Values
    Check ($values['OPEN'] -eq $otherA) 'install: the add-in before ours is untouched'
    Check ($values['OPEN1'] -eq $otherB) 'install: the add-in after the stale value moved down'
    Check ($values.Contains('OPEN2')) 'install: ours took the next index'
    Check ($null -eq (Update-Check)) 'install: no UpdateCheck value without UPDATECHECK'

    $nextStage = Join-Path $env:RUNNER_TEMP 'next-stage'
    $nextMsi = Join-Path $env:RUNNER_TEMP 'next.msi'
    & (Join-Path $PSScriptRoot '..\Package-AddIn.ps1') -OutDir $nextStage -Msi $nextMsi -Version $nextVersion

    $code = Run-Msi @('/i', "`"$nextMsi`"", 'UPDATECHECK=0')
    Check ($code -eq 0) "upgrade exits 0 (exit $code)"
    Check-Installed $nextVersion 'upgrade'
    Check ((Open-Values).Count -eq 3) 'upgrade: the other add-ins are still listed'
    $update = Update-Check
    Check ($null -ne $update -and $update.Kind -eq 'DWord' -and $update.Value -eq 0) "upgrade with UPDATECHECK=0: UpdateCheck is DWORD 0 (found $($update.Kind) $($update.Value))"

    $code = Run-Msi @('/i', "`"$Msi`"")
    Check ($code -ne 0) "a downgrade is refused (exit $code)"
    Check-Installed $nextVersion 'refused downgrade'

    $excelCopy = Join-Path $env:RUNNER_TEMP 'EXCEL.EXE'
    Copy-Item (Join-Path $env:WINDIR 'System32\ping.exe') $excelCopy
    $excel = Start-Process $excelCopy -ArgumentList '-n', '600', '127.0.0.1' -WindowStyle Hidden -PassThru
    $code = Run-Msi @('/x', "`"$nextMsi`"")
    Check ($code -eq 1603) "uninstall is refused while Excel runs (exit $code)"
    Check-Installed $nextVersion 'refused uninstall'
    Stop-Process -Id $excel.Id -Force
    $excel.WaitForExit()

    $code = Run-Msi @('/x', "`"$nextMsi`"")
    Check ($code -eq 0) "uninstall exits 0 (exit $code)"
    Check (-not (Test-Path (Join-Path $root $nextVersion))) 'uninstall: the version folder is gone'
    Check ((Ours).Count -eq 0) 'uninstall: no OPEN value is ours'
    $values = Open-Values
    Check ($values.Count -eq 2 -and $values['OPEN'] -eq $otherA -and $values['OPEN1'] -eq $otherB) 'uninstall: the other add-ins stay, with no gap'
    Check ((Entries).Count -eq 0) 'uninstall: Apps & features has no entry'
    Check ($null -eq (Update-Check)) 'uninstall: the UpdateCheck value is gone'

    $excel = Start-Process $excelCopy -ArgumentList '-n', '600', '127.0.0.1' -WindowStyle Hidden -PassThru
    $code = Run-Msi @('/i', "`"$Msi`"")
    Check ($code -eq 1603) "install is refused while Excel runs (exit $code)"
    Check (-not (Test-Path (Join-Path $root $version))) 'refused install: nothing was installed'
    Check ((Ours).Count -eq 0) 'refused install: no OPEN value was written'
}
catch {
    if ($script:log -and (Test-Path $script:log)) { Get-Content $script:log -Tail 60 }
    throw
}
finally {
    if ($excel -and -not $excel.HasExited) { Stop-Process -Id $excel.Id -Force }
}
