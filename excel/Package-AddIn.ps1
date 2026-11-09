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
#>
param(
    [Parameter(Mandatory)] [string] $OutDir,
    [string] $Zip
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
$version = $found.Groups[1].Value

New-Item -ItemType Directory -Force $OutDir | Out-Null
Copy-Item $xll (Join-Path $OutDir 'Convex.Excel64.xll')
Copy-Item $native $OutDir
$demo = Join-Path $PSScriptRoot 'ConvexDemo.xlsx'
if (Test-Path $demo) { Copy-Item $demo $OutDir }

@"
Convex Excel add-in $version

Needs Microsoft 365 Apps for Windows, 64-bit.

1. Extract all the files to one folder. Keep convex_ffi.dll beside Convex.Excel64.xll.
2. Unblock the files in PowerShell, or Excel refuses a downloaded add-in:
       Get-ChildItem <folder> | Unblock-File
3. In Excel: File > Options > Add-ins > Manage: Excel Add-ins > Go > Browse,
   and pick Convex.Excel64.xll.

=CX.DIAG() shows what loaded.
"@ | Set-Content (Join-Path $OutDir 'README.txt') -Encoding utf8

if ($Zip) { Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $Zip -Force }

Write-Output "Staged the add-in $version in $OutDir"
