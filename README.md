# Kova

Kova is an experimental, open-source, native-first file manager for Windows.

> **Status:** M0 foundation — complete with deferred UX items.

## Goals

- Fast, native-first file manager for Windows.
- Clean separation between UI, core domain logic, and platform-specific operations.
- Data integrity over flashy features.
- Built from scratch; not a fork of existing file managers.

## What Works in M0

- Basic directory enumeration (off the UI thread).
- Navigation: back, forward, parent, direct path entry, refresh.
- Tabs: create, switch, close with independent state.
- Sorting by name, type, size, modified date with header indicators.
- Selection model (single, multi via Ctrl, range via Shift, select all, clear).
- New folder and rename through a simple in-app dialog.
- Windows known folder resolution for the initial location and sidebar.
- Dynamic local drive discovery in the sidebar.
- File open via the Windows default handler (`ShellExecuteExW`).
- Stale-result protection using per-tab generation/request IDs.
- Keyboard shortcuts wired in Slint: F5 (refresh), F2 (rename selected),
  Ctrl+A (select all), Enter (open selected), Alt+Left/Right/Up (back/forward/parent),
  Ctrl+L (focus address bar). Focus must be in the file list for the global
  shortcuts to trigger.

## What Does Not Work Yet / Deferred

- Global search (MFT/USN/everything-style search).
- Preview pane, split view, Git integration.
- Cloud paths, network-specific handling beyond basic UNC paths.
- Bulk copy/move/delete; only safe sandbox tests for these are prepared.
- Real Windows shell icons / thumbnails; generic icons are used with a well-defined interface for future replacement.
- Custom window chrome, auto updater, telemetry, plugins.
- Rich keyboard focus management: shortcuts depend on the file-list focus scope.

## Build Prerequisites

- Windows 10/11 x64
- Rust stable (1.85+) with MSVC toolchain
- Visual Studio 2022 Build Tools or full Visual Studio with C++ workload

## Build Commands

Use the provided helper so the MSVC environment is set automatically:

```powershell
.\scripts\cargo-msvc.ps1 cargo fmt --all -- --check
.\scripts\cargo-msvc.ps1 cargo check --workspace
.\scripts\cargo-msvc.ps1 cargo clippy --workspace --all-targets --all-features -- -D warnings
.\scripts\cargo-msvc.ps1 cargo test --workspace
.\scripts\cargo-msvc.ps1 cargo build --workspace --release
```

## Run

```powershell
.\scripts\cargo-msvc.ps1 cargo run --bin kova-desktop
```

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Kova shall be dual licensed as above, without any additional
terms or conditions.
