# Windows installation and releases

## Install

Download the Setup EXE from [GitHub Releases](https://github.com/cubiix3/Kova-File-Manager/releases).
It installs Kova for the current user in `%LOCALAPPDATA%\Kova`, creates a Start
menu shortcut and offers an optional desktop shortcut. Windows 10 version 1809
or later, or Windows 11, is required; packages target x64.

The matching ZIP contains the same application and runtime files. Extract all
files together before launching `Kova.exe`. No Rust toolchain is needed.

## Update and uninstall

Close Kova before updating, then run the newer installer. Setup reuses the same
installation identity and directory. User preferences are preserved.

Uninstall through Windows Settings. If the stable installation has an association
backup, the uninstaller first runs Kova's existing restoration helper. A failed
restore stops removal so folder-opening commands are not knowingly left pointing
at a removed executable. Unrelated user files and preferences are retained.

Folder integration remains opt-in. Read its [scope and limits](INTERACTION_INTEGRATION.md).
The ZIP does not create an uninstall entry; restore any enabled folder integration
from Kova's logo menu before manually removing its stable copy.

## Build packages

Use a Windows x64 developer shell with Rust/MSVC, Inno Setup 6, cargo-about 0.9.2,
and the Visual C++ x64 redistributable files installed with Visual Studio.

```powershell
.\scripts\package-windows.ps1
```

Use `-Iscc`, `-CargoAbout` or `-CrtDirectory` for explicit tool paths. The script
builds with `--locked`, reads the version from Cargo metadata, creates a fresh
allowlisted staging directory and writes these files to ignored `dist/`:

- `Kova-Setup-<version>-x64.exe`
- `Kova-<version>-x64.zip`
- `SHA256SUMS.txt`

Visual C++ DLLs are deployed beside the executable from Visual Studio's x64
redistribution directory, following Microsoft's
[local deployment guidance](https://learn.microsoft.com/en-us/cpp/windows/determining-which-dlls-to-redistribute?view=msvc-170).
Packages include Kova, Files icon, Rust dependency, Slint and Skia license notices.
The download page carries Slint's attribution badge under its
[royalty-free desktop license](../packaging/licenses/SLINT-LICENSE.md).

## Automated release

The **Windows packages** workflow builds and tests on a Windows runner, compiles
the setup, installs it in an isolated directory, checks application startup and
uninstalls it. It uploads the three package artifacts. Tags matching the Cargo
version, such as `v0.1.0`, additionally publish a GitHub prerelease.

The first preview is not code-signed. SHA-256 checksums verify downloaded file
integrity; they do not replace publisher authentication. A clean Windows 10
machine and upgrade from a previous installer remain separate verification targets.
