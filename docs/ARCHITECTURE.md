# Kova Architecture

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
- Shell icons, native context menus, Explorer-compatible clipboard and IFileOperation
- Balanced thread-local COM apartments and native non-replacing rename

New `unsafe` blocks require a `SAFETY:` comment. The legacy ShellExecuteExW launcher remains in kova-ops.

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
Tokio command receiver and cancellable per-tab enumeration tasks
    │
    │ KovaEvent
    ▼
Channel forwarding to a Slint timer (all UI/model updates on the UI thread)
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

## Deferred

- MFT / USN global search
- Thumbnails
- Explicit permanent-delete UI
- Preview pane, split view, Git integration, cloud paths
- Plugins, auto updater, telemetry


## Snapshot views and local organization

Details and Gallery consume one indexed Slint model. View changes do not enumerate
again. SearchQuery is a core-only parser/matcher; the controller filters cached
entries and remaps selection by path. Gallery rows are virtualized, and thumbnail
requests cover the visible range with a bounded cache and stale-result generation.
The inspector and storage scanner each own background workers. Storage reports
periodic partial results and skips observed reparse/offline entries.

Library stores pins and named reference groups, serialized by a dedicated worker
to a temporary file and atomically replaced. Collections/tags are virtual locations,
with no filesystem destination; the enumeration worker resolves their references.
Confirmed Kova moves/renames relocate matching references. External moves require
repair by the user; unavailable references stay visible.

TransferQueue is shared plain Rust state. Only the Shell STA worker holds COM
interfaces. File-to-file conflicts are prepared on that worker and wait on a
bounded decision channel. IFileOperation still performs the operations. Its
progress sink reports progress, completed file bytes and cancellation. An Advise
guard always calls Unadvise before the operation is released. No COM interface is
made Send or carried into the UI thread. Folder conflicts retain native handling.
