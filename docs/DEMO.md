# Product captures

The README uses unedited window captures of the 0.2 candidate taken on
10 September 2026 on an interactive Windows Server 2025 hosted desktop at 100%
scaling (984 × 680 physical window pixels). The application source is `866b88a`.
Files and preferences use isolated fixtures on a temporary SUBST drive.

The [Windows interaction run](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34483172250)
passed the menu, clipboard, keyboard, refresh, Gallery and native-extension flows
and retained its debug-build captures. The matching
[candidate package workflow](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34483751031)
builds Setup and ZIP from the same application source.

- [Details](images/daily-driver-details.png): tabs, readable search/filter controls and details.
- [Text inspector](images/daily-driver-inspector.png): text preview and Copy Path.
- [Gallery](images/daily-driver-gallery.png): gallery and inspector with copyable metadata.
- [Themed file actions](images/daily-driver-context.png): matching icons, grouped actions and shortcuts.
- [Native Windows menu](images/daily-driver-native.png): the real Windows Shell context menu.
- [Home](images/daily-driver-home.png): recent folders, favorites and drives.
- [File conflicts](images/daily-driver-conflict.png): incoming/existing comparison and explicit choices.

`runtime-window.ps1` captures pixels directly from the specified test window.
No generated UI, recoloring, image editing or product framing is applied.

To reproduce the captures on an interactive Windows desktop, build the application
and run the repository capture script:

```powershell
.\scripts\cargo-msvc.ps1 build --locked --workspace --release
.\scripts\capture-demo.ps1 -Executable target/release/kova-desktop.exe
```

The script creates demonstration files and a separate preferences profile under
`target/runtime/demo-<id>`, uses a temporary drive letter and removes that mapping
on successful cleanup. It exercises real clipboard and file operations in its
fixtures. Screenshots are saved in that directory's `screenshots` folder. The
native-menu capture requires an actual visible Windows menu.

Older `product-*`, `current-*`, `nextgen-*` images and videos preserve earlier
milestones; their interfaces and feature scope are historical.
