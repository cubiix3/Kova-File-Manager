#Requires -Version 5.1
<#
.SYNOPSIS
    Helper that runs a cargo command inside a Visual Studio 2022 x64 dev shell.

.DESCRIPTION
    Kova links against the Windows C++ runtime, so cargo needs the LIB/PATH
    environment set by vcvars64.bat. This script discovers a VS2022 install
    (preferring Community, then Professional, then Enterprise) and runs the
    requested cargo command with the environment initialized.

    It avoids calling the system `cmd` command because some environments have a
    Node wrapper at `cmd` that breaks argument parsing.

.EXAMPLE
    .\scripts\cargo-msvc.ps1 test --workspace
    .\scripts\cargo-msvc.ps1 build --release
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

# Capture environment *before* running vcvars so we can diff it afterwards.
$before = @{}
foreach ($var in [Environment]::GetEnvironmentVariables("Process").Keys) {
    $before[$var] = [Environment]::GetEnvironmentVariable($var, "Process")
}

# Use the legacy Windows command processor directly via its absolute path.
$cmdExe = "$env:SystemRoot\system32\cmd.exe"
$envDump = & $cmdExe /c """$vcvars"" 1>nul 2>nul & set" 2>$null
$after = @{}
foreach ($line in $envDump) {
    if ($line -match "^(\w+)=(.*)$") {
        $after[$matches[1]] = $matches[2]
    }
}

# Apply all new or changed variables to the current PowerShell process.
foreach ($key in $after.Keys) {
    if ($before[$key] -ne $after[$key]) {
        [Environment]::SetEnvironmentVariable($key, $after[$key], "Process")
    }
}

Write-Host "Visual Studio 2022 x64 environment loaded from: $vcvars" -ForegroundColor Cyan

# If the user typed `cargo-msvc.ps1 cargo test ...`, drop the leading "cargo".
if ($CargoArgs[0] -eq "cargo") {
    $CargoArgs = $CargoArgs[1..($CargoArgs.Length - 1)]
}

& cargo @CargoArgs
exit $LASTEXITCODE
