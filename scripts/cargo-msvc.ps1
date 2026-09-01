#Requires -Version 5.1
<#
.SYNOPSIS
    Helper that runs a cargo command inside a Visual Studio 2022 x64 dev shell.

.DESCRIPTION
    Kova links against the Windows C++ runtime, so cargo needs the LIB and PATH
    environment set by vcvars64.bat. This script discovers a VS2022 install
    (preferring Community, then Professional, then Enterprise) and runs the
    requested cargo command with the environment initialized.

.EXAMPLE
    .\scripts\cargo-msvc.ps1 cargo test --workspace
    .\scripts\cargo-msvc.ps1 cargo build --release
#>
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

function Find-VsVarsBatch {
    $editions = @("Community", "Professional", "Enterprise")
    foreach ($edition in $editions) {
        $candidate = "C:\Program Files\Microsoft Visual Studio\2022\$edition\VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path $candidate) {
            return $candidate
        }
    }
    throw "vcvars64.bat not found. Install Visual Studio 2022 with the Desktop development with C++ workload."
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$vcvars = Find-VsVarsBatch

# Import the environment set by vcvars64.bat into the current PowerShell session.
$cmd = '"' + $vcvars + '" && set'
$envLines = cmd /c $cmd
foreach ($line in $envLines) {
    if ($line -match "^(\w+)=(.*)$") {
        [Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
    }
}

Write-Host "Visual Studio 2022 x64 environment loaded from: $vcvars" -ForegroundColor Cyan

# If the user typed `cargo-msvc.ps1 cargo test ...`, drop the leading "cargo".
if ($CargoArgs[0] -eq "cargo") {
    $CargoArgs = $CargoArgs[1..($CargoArgs.Length - 1)]
}

& cargo @CargoArgs
exit $LASTEXITCODE
