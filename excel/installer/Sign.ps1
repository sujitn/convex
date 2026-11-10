<#
.SYNOPSIS
    Signs files with a code-signing certificate in the current user's store.

.DESCRIPTION
    The one place that knows how the release is signed, so choosing a certificate
    vendor changes one step. A CA's cloud HSM client also presents its certificate
    in the store. A timestamp keeps a signature valid after the certificate expires.

.PARAMETER Path
    The files to sign, in place.

.PARAMETER Thumbprint
    The SHA-1 thumbprint of the certificate.

.PARAMETER TimestampUrl
    An RFC 3161 timestamp server. Leave it out only for a throwaway test certificate.
#>
param(
    [Parameter(Mandatory)] [string[]] $Path,
    [Parameter(Mandatory)] [string] $Thumbprint,
    [string] $TimestampUrl
)

$ErrorActionPreference = 'Stop'

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1
if (-not $signtool) { throw 'signtool.exe was not found: install the Windows SDK.' }

$arguments = @('sign', '/sha1', $Thumbprint, '/fd', 'SHA256')
if ($TimestampUrl) { $arguments += @('/tr', $TimestampUrl, '/td', 'SHA256') }
& $signtool.FullName @arguments @Path
if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE." }
