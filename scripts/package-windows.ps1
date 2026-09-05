#Requires -Version 5.1
param(
    [string]$Iscc,
    [string]$CrtDirectory,
    [string]$CargoAbout = 'cargo-about',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
if (-not $SkipBuild) {
    & cargo build --locked --release --workspace
    if ($LASTEXITCODE) { throw 'Release build failed.' }
}
$metadata = (& cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json)
if ($LASTEXITCODE) { throw 'Cargo metadata failed.' }
$version = ($metadata.packages | Where-Object name -eq 'kova-desktop').version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a numeric release version.' }
if ($env:GITHUB_REF_TYPE -eq 'tag' -and $env:GITHUB_REF_NAME -ne "v$version") {
    throw 'Release tag does not match Cargo.toml.'
}
if (-not $Iscc) { $Iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
if (-not (Test-Path -LiteralPath $Iscc)) { throw 'Install Inno Setup 6 or supply -Iscc.' }
if (-not $CrtDirectory) {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $crt = Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Redist\MSVC') -Filter 'Microsoft.VC*.CRT' -Directory -Recurse |
        Where-Object { $_.Parent.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $crt) { throw 'The x64 Visual C++ redistributable directory is missing; supply -CrtDirectory.' }
    $CrtDirectory = $crt.FullName
}
foreach ($dll in @('msvcp140.dll', 'vcruntime140.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $CrtDirectory $dll))) { throw "Missing runtime: $dll" }
}
# Use a fresh staging directory; never bundle the developer's configuration or logs.
$stage = Join-Path $repo ('target\package-' + [Guid]::NewGuid().ToString('N'))
$dist = Join-Path $repo 'dist'
New-Item -ItemType Directory -Force $stage, $dist | Out-Null
Copy-Item -LiteralPath (Join-Path $metadata.target_directory 'release\kova-desktop.exe') -Destination (Join-Path $stage 'Kova.exe')
Copy-Item -Path (Join-Path $CrtDirectory '*.dll') -Destination $stage
Copy-Item -LiteralPath 'LICENSE-MIT', 'LICENSE-APACHE', 'scripts/default-file-manager.ps1', 'apps/kova-desktop/assets/kova.ico', 'apps/kova-desktop/ui/third-party/FILES-ICONS-LICENSE.txt' -Destination $stage
Copy-Item -LiteralPath 'packaging/licenses/SLINT-LICENSE.md' -Destination $stage
& $CargoAbout generate --locked --workspace packaging/licenses.hbs -o (Join-Path $stage 'THIRD-PARTY-LICENSES.html')
if ($LASTEXITCODE) { throw 'Third-party license generation failed.' }
# Skia ships native third-party code whose complete notices accompany skia-bindings.
$skiaLicense = Get-ChildItem (Join-Path $metadata.target_directory 'release\build') -Filter 'LICENSE_SKIA' -File -Recurse | Select-Object -First 1
if (-not $skiaLicense) { throw 'Skia native license notices are missing.' }
Copy-Item -LiteralPath $skiaLicense.FullName -Destination (Join-Path $stage 'SKIA-LICENSE.txt')
& $Iscc "/DAppVersion=$version" "/DStageDir=$stage" packaging/kova.iss
if ($LASTEXITCODE) { throw 'Setup compilation failed.' }
$setup = Join-Path $dist "Kova-Setup-$version-x64.exe"
$zip = Join-Path $dist "Kova-$version-x64.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
$checksums = @($setup, $zip) | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + (Split-Path $_ -Leaf) }
[IO.File]::WriteAllLines((Join-Path $dist 'SHA256SUMS.txt'), $checksums)
Write-Host "Packages ready in $dist"
