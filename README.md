<p align="center">
  <img src="apps/kova-desktop/assets/kova.svg" width="88" height="88" alt="Kova logo">
</p>

<h1 align="center">Kova</h1>
<p align="center">A native Windows file manager. Your files, with a clearer view.</p>

<p align="center">
  <a href="https://github.com/cubiix3/Kova-File-Manager/releases">Download for Windows</a> ·
  <a href="#a-closer-look">Screenshots</a> ·
  <a href="docs/VIEW_AND_PREVIEW.md">User guide</a> ·
  <a href="https://github.com/cubiix3/Kova-File-Manager/issues">Report an issue</a>
</p>

![Kova: tabs, search and native file browsing](docs/images/daily-driver-details.png)

Kova combines tabbed browsing, a thumbnail gallery, native Windows context menus
and an inspector in a compact desktop interface. Built with **Rust, Slint and Win32/Shell APIs** for
Windows 10/11 x64. No Electron or WebView.

> **0.2 release candidate:** The current source includes the daily-driver changes below.
> The published v0.1.0 installer predates these changes. The verified 0.2.0 Setup
> and ZIP are available in the [candidate workflow artifacts](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34473650758)
> (GitHub sign-in required). No new public release has been published.
> See [verification and limits](docs/DAILY_DRIVER_VERIFICATION.md) and the [changelog](CHANGELOG.md).

## Download & install

1. Open [GitHub Releases](https://github.com/cubiix3/Kova-File-Manager/releases).
2. Download **Kova-Setup-<version>-x64.exe** from the release's **Assets** section.
3. Run Setup, then launch Kova from the Start menu. A desktop shortcut is optional.

The installer includes the required Visual C++ runtime files and installs for
your Windows account. Rust and Visual Studio are not required. To remove Kova,
use **Windows Settings → Apps → Installed apps → Kova → Uninstall**.

Prefer a ZIP? Download **Kova-<version>-x64.zip**, extract the entire archive and run
**Kova.exe**. Keep the included DLLs beside the executable. Package checksums
are available in **SHA256SUMS.txt**. The preview packages are not code-signed.

Folder-opening integration is optional and can be enabled or restored from the
Kova logo menu. See [what it covers](docs/INTERACTION_INTEGRATION.md); it does not
replace Win+E, Windows file pickers or every explicit Explorer invocation.

<a href="https://slint.dev"><img src="docs/images/made-with-slint.png" width="150" alt="Made with Slint"></a>

## What you can do

| Feature | Included |
| --- | --- |
| **Browse** | Tabs with independent history, breadcrumbs, automatic folder updates, familiar shortcuts and mouse Back/Forward |
| **Manage files** | Native copy, move and Recycle Bin deletion; transfer history, progress and cancellation; explicit Replace / Skip / Keep Both conflicts |
| **See more** | Details and three gallery sizes; image, text and PDF inspector; animated GIF, WebP and APNG; media metadata and Shell thumbnails |
| **Stay organized** | Local tags and collections, pinned folders with reordering; sortable columns and rectangular multi-selection |
| **Find files** | Name search with optional Type / Size / Date filters; cancellable Include subfolders; natural filename ordering |
| **Check storage** | Drive type, file system and capacity; cancellable background analysis with largest folders/files and proportional bars |
| **Resume work** | Restore tabs, active location, searches, window state, columns, gallery and inspector; debounced atomic preferences |
| **Undo safely** | Review supported renames and same-volume file moves; identity checks refuse changed items or occupied destinations |
| **Adjust the view** | Hidden/system files, file extensions, row density, alternating rows and a resizable preview pane |
| **Use Windows tools** | Native Shell menus with installed extensions, associated applications, Explorer-compatible clipboard and bidirectional drag & drop |

## A closer look

### See more. Stay in flow.

Switch between Details and Gallery without changing the folder or selection.
Choose small, medium or large tiles, then inspect a file alongside the gallery.

![Gallery and inspector](docs/images/daily-driver-gallery.png)

Select a file and press **Space**. Read text, inspect images or page through a PDF
alongside your files. GIF, animated WebP and APNG support Play/Pause.

### Feels new. Works native.

Use your installed Shell extensions, associated applications and familiar
Windows commands directly from Kova's native context menu.

![Native Windows context menu](docs/images/daily-driver-native.png)

### Every drive. One place.

Compare capacity and free space in Home, then open a drive or analyze a folder's
storage. Keep your regular destinations close with pinned folders.

![Home: recent folders, favorites and drives](docs/images/daily-driver-home.png)

These are unedited captures of the 0.2 release candidate with demonstration files.
[About the images](docs/DEMO.md) · [Feature guide and limitations](docs/NEXTGEN.md).

### Stay in control of transfers

Review source and destination, progress and completed work in Transfers. File
collisions wait for your choice; folder merges retain native Windows handling.

![Kova file conflict comparison](docs/images/daily-driver-conflict.png)

## Familiar shortcuts

| Action | Shortcut |
| --- | --- |
| New tab / close tab | `Ctrl+T` / `Ctrl+W` |
| Edit address / refresh | `Ctrl+L` / `F5` |
| Back / forward / parent | `Alt+Left` / `Alt+Right` / `Alt+Up` |
| New folder / rename | `Ctrl+Shift+N` / `F2` |
| Copy / cut / paste | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` |
| Select all / delete | `Ctrl+A` / `Delete` |
| Toggle preview | `Space` |
| Details / Gallery | `Ctrl+1` / `Ctrl+2` |
| Search the current folder/tree | `Ctrl+F` |
| Review the last supported Undo | `Ctrl+Z` |
| Switch tabs | `Ctrl+Tab` / `Ctrl+Shift+Tab` |

## Build from source

Use Windows x64, Rust stable with the MSVC target, and Visual Studio with the
**Desktop development with C++** workload. The first build downloads dependencies
and may download prebuilt Skia libraries.

```powershell
git clone https://github.com/cubiix3/Kova-File-Manager.git
cd Kova-File-Manager
.\scripts\cargo-msvc.ps1 build --locked --workspace --release
.\target\release\kova-desktop.exe
```

See [Contributing](CONTRIBUTING.md) for quality checks and
[Windows packaging](docs/WINDOWS_RELEASE.md) for building the installer.

## Project documentation

- [Gallery, search, transfers and local organization](docs/NEXTGEN.md)
- [User guide: views, previews and storage](docs/VIEW_AND_PREVIEW.md)
- [Windows integration and restoration](docs/INTERACTION_INTEGRATION.md)
- [Daily-driver verification and performance](docs/DAILY_DRIVER_VERIFICATION.md)
- [Current design](docs/APPROVED_DESIGN.md)
- [Product scope and planned work](docs/PRODUCT.md)
- [Architecture](docs/ARCHITECTURE.md) · [Data safety](docs/SECURITY_AND_DATA_SAFETY.md)
- [Documentation index and historical reports](docs/README.md)

Current source includes recursive folder search, native Windows drag & drop,
session restoration and identity-checked Undo for supported renames/file moves.
System-wide indexing, split panes and batch rename remain future work. See the
[feature guide](docs/NEXTGEN.md) for exact limits.

## License & acknowledgments

Kova's source is dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE). Dependencies retain their own licenses; distributed
packages include third-party notices.

Built with [Slint](https://slint.dev). [Files](https://github.com/files-community/Files)
was studied as a UX reference; adapted MIT-licensed icon geometry is credited in
[the icon notices](apps/kova-desktop/ui/third-party/README.md). Kova is an
independent implementation with its own branding.
