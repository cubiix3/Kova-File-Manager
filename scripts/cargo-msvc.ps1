#Requires -Version 5.1
<#
.SYNOPSIS
    Helper that runs a cargo command inside an installed Visual Studio x64 dev shell.

.DESCRIPTION
    Kova links against the Windows C++ runtime, so cargo needs the LIB/PATH
    environment set by vcvars64.bat. vswhere discovers the newest suitable
    installation, including standalone Build Tools in Program Files (x86).

    It avoids calling the system `cmd` command because some environments have a
    Node wrapper at `cmd` that breaks argument parsing.

.EXAMPLE
    .\scripts\cargo-msvc.ps1 test --workspace
    .\scripts\cargo-msvc.ps1 build --release
    .\scripts\cargo-msvc.ps1 -CargoArgs @('clippy','--workspace','--','-D','warnings')
#>
param(
    [string[]]$CargoArgs = @()
)

# Stay a simple script: advanced PowerShell binding consumes Cargo's -p as
# the common -PipelineVariable parameter. Preserve all remaining Cargo flags.
$CargoArgs = @($CargoArgs) + @($args)
if ($CargoArgs.Count -eq 0) { throw 'Pass a Cargo command, for example: build --workspace' }

$ErrorActionPreference = "Stop"

function Find-VsVarsBatch {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $installations = & $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -sort -property installationPath
        foreach ($installation in $installations) {
            $candidate = Join-Path $installation 'VC\Auxiliary\Build\vcvars64.bat'
            if (Test-Path -LiteralPath $candidate) { return $candidate }
        }
    }
    throw "vcvars64.bat not found. Install Visual Studio with the Desktop development with C++ workload."
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

Write-Host "Visual Studio x64 environment loaded from: $vcvars" -ForegroundColor Cyan

# If the user typed `cargo-msvc.ps1 cargo test ...`, drop the leading "cargo".
if ($CargoArgs[0] -eq "cargo") {
    if ($CargoArgs.Length -eq 1) { throw 'Pass a Cargo subcommand after cargo.' }
    $CargoArgs = $CargoArgs[1..($CargoArgs.Length - 1)]
}

& cargo @CargoArgs
exit $LASTEXITCODE
