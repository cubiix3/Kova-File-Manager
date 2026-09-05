# Runtime rescue report

Date: September 1, 2026 · Branch: `main` · Baseline: `91ad5b4`

> Historical report. This records the original rescue investigation and its
> verification limits. See [current documentation](README.md) for later fixes.

## Starting state

The working tree contained four modified files (`main.rs`, `main.slint`,
`worker.rs`, `cargo-msvc.ps1`) and untracked local linker configuration,
reference material and a debug-build helper. `cargo check` failed: unsupported
`event.key == Key.A`, invalid percentage conversion and three duplicate `ta` IDs.
The user reported a blank file list, no active tab and unresponsive buttons.
Earlier completion claims were not supported by this build state.

## Root causes and fixes

### UI access from the worker thread

The Tokio event consumer called `update_ui`/`Weak::upgrade()` directly from a
worker task. Slint properties must be updated on the UI thread. The replacement
forwards events through `std::sync::mpsc`; a 50ms `slint::Timer` drains them on the
UI thread. This removed the cross-thread UI access.

### Layout outside the visible window

Percentage geometry resolved against the window instead of the intended layout
row. Grid rows totaled 578px in a 480px window, placing status/drives below it.
`SidebarButton` requested approximately 892px, moving file rows to X=3300 beyond
the window edge at X=3292. Enumeration had correctly returned 97 entries; their
delegates were rendered outside the visible area. Unconstrained text headers
absorbed another 104–194px and displaced drive controls.

Explicit geometry and bounded header/sidebar dimensions corrected the layout.

### Recreated models broke double-click

Every selection click created a fresh `VecModel`, replacing row delegates.
The second click hit a different `TouchArea`, so `double-clicked` did not fire.
The fix retains one model per list and updates rows through `set_row_data`.

### Keyboard focus and API misuse

Row touch areas did not acquire focus, leaving the file-list `FocusScope`
inactive. The fix uses `forward-focus: list-scope` and focuses it on row input.
Keyboard checks use verified Slint 1.13.1 APIs: `event.text`, modifiers and
`Key.F5`/`Return`/`LeftArrow`. For Ctrl+A, winit's logical key is `a`, not a
control character. An initial UI update before `app.run()` supplies tab, address
and status values before asynchronous enumeration completes.

## Data flow and reference study

The enumeration pipeline was already producing valid data:

```text
start_location=<user profile>
worker: enumerate tab=TabId(1) request=1
worker: loaded entries=97
controller: snapshot accepted files=97 tabs=1
update_ui → Slint Model
```

The [Files reference study](research/FILES_REFERENCE.md) examined pointer states
in `SidebarItem.cs`, commands in `BaseShellPage.cs`, immediate tab updates in
`BaseTabBar.cs`, refresh behavior in `ShellViewModel.RefreshItems`, navigation
availability in `NavigationToolbarViewModel`, and `ShowLocationUnavailable`.
Kova adopted these concepts: non-pure action callbacks, visible errors, updated
navigation flags, immediate tab/startup state and virtualized lists.

```text
Slint UI → AppState callbacks → CommandDispatcher / per-tab GenerationCounter
  → WorkerCommand → Tokio worker → KovaEvent
  → mpsc forwarding → UI-thread Timer
  → AppController (stale checks, sorting, selection)
  → update_ui (persistent row models) → Slint
```

## Runtime evidence

| Control | Callback | Result and evidence |
| --- | --- | --- |
| New tab | request_new_tab | PASS: new active Home tab |
| Close tab | request_close_tab | PASS: removed tab, adjacent tab activated |
| Switch tab | request_switch_tab | PASS: address and list changed |
| Home, Desktop, Documents, Downloads | request_navigate | PASS: Known Folder paths |
| Drives C:, G:, D:, I: | request_navigate | PASS: dynamic drive roots |
| Back, Forward, Parent | dispatch_back/forward/parent | PASS: history |
| Refresh | request_refresh | PASS: enumeration repeated |
| Address submit | request_navigate | PASS: canonical path; invalid input surfaced an error |
| Header sort | request_sort | PASS: callback and state; live reorder needed visual confirmation |
| Select, Ctrl-toggle, Shift-range | request_select/toggle/range | PASS: controller state |
| Folder double-click | request_activate | PASS: target address and enumeration |
| File double-click | request_activate → Open | PASS: Notepad++ opened file-a.txt |
| New Folder | request_new_folder + dialog | PASS: disk creation, refreshed list, AlreadyExists error |
| F2 Rename | request_rename + dialog | PASS: TestFolder1 became TestFolder2 on disk |
| Ctrl+A, Enter, Alt+arrows | FocusScope key-pressed | PASS: dispatch; full live-input suite remained incomplete |

Requests 1–14 were observed. Older results were rejected; tests included
`stale_snapshot_is_rejected_after_newer_request` and
`out_of_order_results_keep_latest_navigation_visible`. Drives came from
`GetLogicalDriveStringsW`; known folders used `SHGetKnownFolderPath`.

GUI automation used UIA and mouse/keyboard input in a temporary runtime sandbox.
Live Shift-range/Ctrl+A selection, scrolling with 10,000 entries and detailed
column behavior during resize were not verified in that session.

The femtovg renderer produced a transparent client area in the automation session.
`SLINT_BACKEND=winit-software` rendered correctly, with pixel-verified tab
selection `#094771`, sidebar `#252526` and list `#1e1e1e`. Earlier user captures
showed GPU rendering working interactively. Final visual judgment, sort order
and resize behavior therefore remained user-verification items. A local
PrintWindow capture was retained during verification but not committed.

## Visual changes

The rescue added a visible initial tab, close/plus buttons and active-tab styling;
disabled toolbar states; a flexible address bar; a 180px sidebar with fixed
headers; virtualized sortable file rows; and a full-width neutral status bar.
Shared `Theme` and `Metrics` globals replaced scattered values. Loading, empty
and error states were differentiated, including operation-error dialogs.

## Quality gates

| Gate | Result |
| --- | --- |
| Formatting | PASS after formatting |
| Workspace check | PASS |
| Workspace tests | PASS: 23 tests; performance baseline ignored |
| Clippy, all targets/features, `-D warnings` | PASS |
| Workspace release build | PASS |

The test distribution was 14 core, 2 desktop controller, 4 operations and
3 platform tests, plus the ignored performance baseline. Coverage included
stale/out-of-order results, sandbox path rejection, temporary New Folder/Rename,
Known Folders and drive enumeration.

## Remaining issues at that milestone

- P0: none known after the fixes.
- P1: transparent GPU rendering in sessions without a working graphics context;
  documented software fallback, with automatic fallback left for future work.
- P1: fixed 320/120/90/140px column widths without responsive adaptation.
- P2: background refresh could normalize address text during editing;
  `last_address` reduced but did not eliminate the issue.
- P2: context-menu Rename was missing; F2 existed.
- P3: placeholder icons, missing drive labels and English status text.

## Commits

| Commit | Change |
| --- | --- |
| `3a4bbf1` | Route UI updates through the UI-thread event pump |
| `f637f7e` | Rebuild window skeleton with explicit geometry |
| `ebebe2f` | Improve MSVC helper and exclude local Cargo overrides |
| `6f6e9e8` | Add Files reference research |
| `e35e8e3` | Record verified root causes in the reference study |

No secrets, build output, reference repositories or machine-specific paths were
committed. Local `.cargo/` overrides were ignored and the hardcoded debug helper
was removed. The report followed these commits on `main`.

## Outcome

**Runtime baseline partial.** Static gates passed and the functional evidence
above was collected, but the full visual/input suite was not complete. The
original closeout requested interactive verification of appearance, sorting and
resize, with the software backend as a fallback for a blank window.
