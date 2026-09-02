# Kova Architecture — M0

## Goals

- Native-first Windows file manager.
- UI thread never blocked by filesystem work.
- Domain logic is platform and UI independent.
- Data integrity over feature count.

## Workspace Layout

```text
kova/
├── Cargo.toml
├── crates/
│   ├── kova-core          → platform-independent domain logic
│   ├── kova-platform-windows → Windows-specific APIs
│   └── kova-ops           → filesystem operation execution
├── apps/
│   └── kova-desktop       → Slint desktop application
├── docs/
│   ├── ARCHITECTURE.md
│   ├── PRODUCT.md
│   ├── SECURITY_AND_DATA_SAFETY.md
│   ├── PERFORMANCE_BASELINE.md
│   └── M0_REPORT.md
├── scripts/
│   └── cargo-msvc.ps1     → helper that loads the VS dev shell before cargo
└── tests/
    └── fixtures/
```

## Crate Responsibilities

### kova-core

- `FileEntry`, `FileMetadata`, `DirectorySnapshot`
- `Location`, `NavigationHistory`, `TabState`, `TabCollection`
- `SelectionState`, `SortDescriptor`
- `KovaCommand`, `KovaEvent`, `OperationError`

Contains no `unsafe`, no Slint, no Win32 calls. Unit-testable in isolation.

### kova-platform-windows

- Known folder resolution via `SHGetKnownFolderPath`
- Path canonicalization and error classification
- Logical drive enumeration (`GetLogicalDriveStringsW` / `GetDriveTypeW`)
- Default application launching via `ShellExecuteExW`

All `unsafe` blocks have a `SAFETY:` comment.

### kova-ops

- `enumerate_directory`: Tokio-based async directory read
- `new_folder`, `rename`, `open_with_default_handler`
- `TestSandbox`: integration test root guard
- `worker`: command/event bridge

Filesystem I/O happens on the Tokio runtime, not the UI thread.

### kova-desktop

- Slint `.slint` UI files
- `app_state`: UI-facing controller and view model
- `bridges`: command dispatcher with generation IDs
- `main.rs`: event loop wiring

Contains no direct `std::fs` calls from callbacks.

## Concurrency Model

```text
UI Thread (Slint)
    │
    │ UI callbacks
    ▼
CommandDispatcher (main thread, fast)
    │
    │ WorkerCommand
    ▼
Tokio worker task (filesystem I/O)
    │
    │ KovaEvent
    ▼
Event consumer (Tokio task, calls Slint on main thread via Weak upgrade)
    │
    ▼
UI state update
```

## Dependency Rationale

| Dependency | Purpose |
|------------|---------|
| slint | Native UI without webview; cross-platform if needed later. |
| windows-rs | Official Rust bindings for Win32/COM/Shell APIs. |
| tokio | Async runtime for filesystem worker; single runtime choice. |
| tracing | Structured logging with environment-filtered levels. |
| thiserror | Concise, maintainable error enum definitions. |
| chrono | Localized date/time formatting for file metadata. |
| bitflags | Reserved for future attribute flags. |
| uuid | Unique sandbox directory names in tests. |

## Safety

See `docs/SECURITY_AND_DATA_SAFETY.md`.

## Build Helper

`scripts/cargo-msvc.ps1` locates Visual Studio 2022, imports the `vcvars64.bat`
environment, and runs the requested cargo command. This removes the need to
start a dedicated VS developer shell.

## Not in M0

- MFT / USN global search
- Shell icons / thumbnails
- Copy / Move / permanent Delete
- Preview pane, split view, Git integration, cloud paths
- Plugins, auto updater, telemetry
