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

- `enumerate_directory`: one cancellable blocking Windows read on the Tokio pool
- `enumerate_tree`: asynchronous, cancellable traversal without following reparse/offline directories
- `new_folder`, `rename`, `open_with_default_handler`
- `TestSandbox`: integration test root guard
- `worker`: command/event bridge

Filesystem I/O happens on the Tokio runtime, not the UI thread.

### kova-desktop

- Slint `.slint` UI files
- `app_state`: UI-facing controller and view model
- `bridges`: command dispatcher with generation IDs
- `main.rs`: event-loop and worker wiring
- `callbacks`, `sidebar`, `ui_sync`: navigation actions and UI synchronization
- `search`: background filtering with generations and reusable sort order
- `file_model`: shared immutable snapshots and bounded, lazy Slint rows
- `preferences`: versioned session capture and debounced atomic persistence
- `keyboard`, `drag_drop`, `operations`: native interaction boundaries
- `shared.slint`, `controls.slint`, `search.slint`, `inspector.slint`,
  `operations.slint`: coherent view components, with `main.slint` composing them

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
| chrono | Typed timestamps and local-calendar search boundaries; Windows APIs format metadata. |
| bitflags | Reserved for future attribute flags. |
| uuid | Unique sandbox directory names in tests. |

## Safety

See `docs/SECURITY_AND_DATA_SAFETY.md`.

## Build Helper

`scripts/cargo-msvc.ps1` uses vswhere to locate a C++-capable Visual Studio or
Build Tools installation (including Program Files (x86)), imports `vcvars64.bat`
environment, and runs the requested cargo command. This removes the need to
start a dedicated VS developer shell.

## Deferred

- MFT / USN global search
- Explicit permanent-delete UI
- Split view, batch rename, Git integration, virtual cloud-file streams
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
