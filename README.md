# Kova

Kova is an experimental, open-source, native-first file manager for Windows.

> **Status:** Early development / M0 foundation.

## Goals

- Fast, native-first file manager for Windows.
- Clean separation between UI, core domain logic, and platform-specific operations.
- Data integrity over flashy features.
- Built from scratch; not a fork of existing file managers.

## What Works in M0

- Basic directory enumeration (off the UI thread).
- Navigation: back, forward, parent, direct path entry, refresh.
- Tabs: create, switch, close with independent state.
- Sorting by name, type, size, modified date.
- Selection model (single, multi, range, select all, clear).
- New folder and rename in a safe temporary sandbox.
- Windows known folder resolution for the initial location.
- Keyboard shortcuts: Enter, Arrow keys, Ctrl+A, F2, Ctrl+L, Alt+Left/Right/Up, F5.

## What Does Not Work Yet

- Global search (MFT/USN/everything-style search).
- Preview pane, split view, Git integration.
- Cloud paths, network-specific handling beyond basic UNC paths.
- Bulk copy/move/delete; only safe sandbox tests for these are prepared.
- Real Windows shell icons / thumbnails; generic icons are used with a well-defined interface for future replacement.
- Custom window chrome, auto updater, telemetry, plugins.

## Build Prerequisites

- Windows 10/11 x64
- Rust stable (1.85+) with MSVC toolchain
- Visual Studio 2022 Build Tools or full Visual Studio with C++ workload

## Build Commands

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

## Run

```powershell
cargo run --bin kova-desktop
```

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT)
at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Kova shall be dual licensed as above, without any additional
terms or conditions.
