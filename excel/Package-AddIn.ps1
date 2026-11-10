<#
.SYNOPSIS
    Stages the add-in for release: the packed .xll and convex_ffi.dll beside it.

.DESCRIPTION
    The packed .xll carries the .dna, Convex.Excel.dll, Newtonsoft.Json.dll and
    ExcelDna.IntelliSense.dll, so it and convex_ffi.dll are all Excel needs. The
    unpacked Convex.Excel64.xll in the build output does not work on its own.

    Build first:
        cargo build --profile excel -p convex-ffi
        dotnet build excel/Convex.Excel/Convex.Excel.csproj --configuration Release

.PARAMETER OutDir
    The folder to stage the files in. It is created if needed.

.PARAMETER Zip
    Also write the folder's contents to this zip.

.PARAMETER Msi
    Also build the per-user installer to this path.

.PARAMETER Version
    The installer's version, if not the workspace's. CI uses it to test an upgrade.

.PARAMETER Thumbprint
    Sign the .xll, the DLL and the MSI with this certificate. Without it they stay unsigned.

.PARAMETER TimestampUrl
    The timestamp server for signing.
#>
param(
    [Parameter(Mandatory)] [string] $OutDir,
    [string] $Zip,
    [string] $Msi,
    [string] $Version,
    [string] $Thumbprint,
    [string] $TimestampUrl
)

$ErrorActionPreference = 'Stop'

$publish = Join-Path $PSScriptRoot 'Convex.Excel\bin\Release\net472\publish'
$xll = Join-Path $publish 'Convex.Excel64-packed.xll'
$native = Join-Path $publish 'convex_ffi.dll'
foreach ($required in $xll, $native) {
    if (-not (Test-Path $required)) { throw "$required is missing: build the FFI and the add-in first." }
}

$cargo = Get-Content -Raw (Join-Path $PSScriptRoot '..\Cargo.toml')
$found = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $found.Success) { throw 'No version found in the workspace Cargo.toml.' }
$workspaceVersion = $found.Groups[1].Value
if (-not $Version) { $Version = $workspaceVersion }

function Sign([string[]] $Files) {
    if (-not $Thumbprint) { return }
    & (Join-Path $PSScriptRoot 'installer\Sign.ps1') -Path $Files -Thumbprint $Thumbprint -TimestampUrl $TimestampUrl
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
Copy-Item $xll (Join-Path $OutDir 'Convex.Excel64.xll')
Copy-Item $native $OutDir
$demo = Join-Path $PSScriptRoot 'ConvexDemo.xlsx'
if (Test-Path $demo) { Copy-Item $demo $OutDir }

@"
Convex Excel add-in $Version

Needs Microsoft 365 Apps for Windows, 64-bit.

The MSI on the release page installs and registers the add-in for you. To load it by hand:

1. Extract all the files to one folder. Keep convex_ffi.dll beside Convex.Excel64.xll.
2. Unblock the files in PowerShell, or Excel refuses a downloaded add-in:
       Get-ChildItem <folder> | Unblock-File
3. In Excel: File > Options > Add-ins > Manage: Excel Add-ins > Go > Browse,
   and pick Convex.Excel64.xll.

=CX.DIAG() shows what loaded.
"@ | Set-Content (Join-Path $OutDir 'README.txt') -Encoding utf8

Sign @((Join-Path $OutDir 'Convex.Excel64.xll'), (Join-Path $OutDir 'convex_ffi.dll'))

if ($Zip) { Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $Zip -Force }

if ($Msi) {
    $project = Join-Path $PSScriptRoot 'installer\Convex.Excel.Installer.wixproj'
    $stage = (Resolve-Path $OutDir).Path.TrimEnd('\')
    # Rebuilt each time: only the version may differ between two runs.
    dotnet build $project --configuration Release --no-incremental "-p:Version=$Version" "-p:StageDir=$stage"
    if ($LASTEXITCODE -ne 0) { throw "The installer build failed with exit code $LASTEXITCODE." }
    Copy-Item (Join-Path $PSScriptRoot 'installer\bin\Release\convex-excel-addin.msi') $Msi -Force
    Sign @($Msi)
}

Write-Output "Staged the add-in $Version in $OutDir"
