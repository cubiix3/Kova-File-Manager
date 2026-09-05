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

# Exercise the real per-user directory and optional associations only on this runner.
$stable = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Kova'
if (Test-Path -LiteralPath $stable) { throw 'Refusing to overwrite an existing stable installation.' }
$process = Start-Process -FilePath $setup.FullName -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/TASKS=desktopicon', "/DIR=`"$stable`"") -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode) { throw 'Stable-directory installation failed.' }
$shell = New-Object -ComObject WScript.Shell
foreach ($folder in @('Programs', 'DesktopDirectory')) {
    $shortcut = Join-Path ([Environment]::GetFolderPath($folder)) 'Kova.lnk'
    if (-not (Test-Path $shortcut) -or $shell.CreateShortcut($shortcut).TargetPath -ne (Join-Path $stable 'Kova.exe')) { throw "Invalid shortcut: $shortcut" }
}
& (Join-Path $stable 'default-file-manager.ps1') -Mode Enable -Executable (Join-Path $stable 'Kova.exe')
$defaults = Get-Content (Join-Path $stable 'folder-associations.json') -Raw | ConvertFrom-Json
$commands = Get-Content (Join-Path $stable 'folder-commands.json') -Raw | ConvertFrom-Json
$sentinel = Join-Path $stable 'installer-user-data-test.txt'
Set-Content -LiteralPath $sentinel -Value 'Preserve user data'
# Reinstall the same version to verify upgrades preserve user data and backups.
$update = Start-Process -FilePath $setup.FullName -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-' -WindowStyle Hidden -PassThru -Wait
if ($update.ExitCode -or -not (Test-Path $sentinel) -or -not (Test-Path (Join-Path $stable 'folder-associations.json'))) { throw 'Reinstall did not preserve user data and integration backup.' }
$uninstall = Start-Process -FilePath (Join-Path $stable 'unins000.exe') -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -WindowStyle Hidden -PassThru -Wait
if ($uninstall.ExitCode -or (Test-Path (Join-Path $stable 'Kova.exe')) -or -not (Test-Path $sentinel)) { throw 'Stable uninstall failed or removed user data.' }
function Assert-RestoredValue($key, $name, $saved) {
    $present = $key -and ($key.GetValueNames() -contains $name)
    if ([bool]$present -ne [bool]$saved.Present) { throw "Registry presence mismatch: $name" }
    if ($present -and ($key.GetValue($name) -ne $saved.Value -or $key.GetValueKind($name).ToString() -ne $saved.Kind)) { throw "Registry value mismatch: $name" }
}
foreach ($class in @('Directory','Drive')) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Software\Classes\$class\shell")
    try { Assert-RestoredValue $key '' $defaults.$class } finally { if ($key) { $key.Dispose() } }
}
foreach ($saved in $commands) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(($saved.Path + '\command'))
    try {
        Assert-RestoredValue $key '' $saved.Default
        Assert-RestoredValue $key 'DelegateExecute' $saved.Delegate
    } finally { if ($key) { $key.Dispose() } }
}
if (Test-Path (Join-Path $stable 'folder-associations.json')) { throw 'Association backup was not consumed.' }
Write-Host 'Shortcuts, reinstall, user-data preservation and association restoration passed.'
