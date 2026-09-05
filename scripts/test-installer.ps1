#Requires -Version 5.1
# Run only on an ephemeral GitHub Actions runner: this exercises HKCU uninstall registration.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Run installer smoke tests on an ephemeral GitHub Actions runner.' }
$setups = @(Get-ChildItem (Join-Path $PSScriptRoot '..\dist\Kova-Setup-*-x64.exe'))
if ($setups.Count -ne 1) { throw 'Expected exactly one setup executable.' }
$setup = $setups[0]
$install = Join-Path $env:RUNNER_TEMP ('Kova-Install-Test-' + [Guid]::NewGuid().ToString('N'))
$process = Start-Process -FilePath $setup.FullName -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/NOICONS', "/DIR=`"$install`"") -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode -ne 0) { throw "Setup failed: $($process.ExitCode)" }
$exe = Join-Path $install 'Kova.exe'
foreach ($file in @('Kova.exe', 'msvcp140.dll', 'vcruntime140.dll', 'unins000.exe', 'THIRD-PARTY-LICENSES.html')) {
    if (-not (Test-Path -LiteralPath (Join-Path $install $file))) { throw "Missing installed file: $file" }
}
$app = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
try {
    Start-Sleep -Seconds 6
    $app.Refresh()
    if ($app.HasExited -or $app.MainWindowHandle -eq 0) { throw 'Installed application did not create a window.' }
    Write-Host 'Installed app created a native window.'
} finally {
    if (-not $app.HasExited) {
        $null = $app.CloseMainWindow()
        if (-not $app.WaitForExit(5000)) { $app.Kill() }
    }
}
$uninstall = Start-Process -FilePath (Join-Path $install 'unins000.exe') -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -WindowStyle Hidden -PassThru -Wait
if ($uninstall.ExitCode -ne 0) { throw 'Uninstall failed.' }
if (Test-Path -LiteralPath $exe) { throw 'Uninstall left Kova.exe behind.' }
Write-Host 'Setup, installed app startup and uninstall passed.'
