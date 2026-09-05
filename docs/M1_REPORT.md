# M1 report — Windows interaction, icons and UI

Date: September 2, 2026 · Branch: `main` · Baseline: `1ebdabb`

> Historical report. Results and limitations below apply to the M1 build.
> See [current documentation](README.md) for subsequent changes.

## Changes

### Mouse Back/Forward

`NavTouchArea` extends Slint `TouchArea` and dispatches Back/Forward through the
same commands as the toolbar. Navigation is disabled while a dialog is open.
It covers the toolbar, tabs, sidebar, file rows and status bar through Slint's
normal input pipeline, without window subclassing, raw-pointer hooks or
`unsafe impl Send`. Row handling preserves right-click and Ctrl/Shift selection.

### Shell icons

The UI-thread `IconStore` deduplicates requests and uses a dedicated `kova-icons`
worker outside Tokio, serialized through `SHELL_ICON_LOCK`. Generic folder, file,
symlink, drive and unknown icons are seeded in slots 0–4. Known folders and drives
use Shell icons; extension icons load asynchronously, with EXE/LNK keyed by path.

Fixed an ID mismatch: `next_id` started at 8 although the model contained only
five rows. IDs now derive from `model.row_count()`, so they always match row indices.
Fallback order is Shell icon, generic type icon, then no image. `is_dir` controls
whether Open in New Tab is available.

### Menus and interaction

The M1 row menu offered Open, Open in New Tab, Rename, Copy Path, New Folder and
Refresh through Slint `ContextMenuArea`. Blank space offered New Folder/Refresh.
Right-click selected an unselected row while preserving an existing selected group.
Open in New Tab activated a separate tab/history and rejected files. Copy Path
used the Windows clipboard; action failures appeared in the status line.

### Visual polish and cleanup

Central `Theme`/`Metrics` tokens defined quieter surfaces, subtle borders,
pressed states, 16px icons, row heights and a future danger color. The status bar
showed status, item/selection counts and loading. Empty/loading states were centered.
Dialogs blocked background input, supported Escape and selected their input on open.
Headers gained accent sort indicators; tabs gained active, hover and pressed states.

Removed `debug_pointer`, temporary navigation/activation/rename traces, icon timing
logs and snapshot debug logs. Removed UTF-8 BOMs and repaired double-encoded arrows,
close symbols, ellipses and sort glyphs in `main.slint`.

## Quality gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace` | PASS: 32 passed, 1 ignored performance baseline |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS: MSVC, 29.7 seconds |

New tests: `item_and_selection_counts_track_active_tab` and
`file_list_rows_expose_generic_icon_and_dir_flag`, covering fallback IDs,
directory flags and Shell icon precedence.

## Runtime verification

Release build, UI Automation and real input in `%TEMP%\kova-m1-final`, using
`M1Target`, `M1Back`, `M1Created`/`M1Renamed` and `note.txt`.

| Test | Result | Evidence |
| --- | --- | --- |
| Open in New Tab through context menu | PASS | Native menu found; target address; `enumerate tab=TabId(2)`; closing returned to one tab |
| New Folder through dialog | PASS | Folder existed on disk; `entries=3`; autofocus and select-all |
| F2 Rename through dialog | PASS | `M1Renamed` existed and `M1Created` no longer existed |
| File-list icons | PASS | 126 colored pixels in each 20×20 row icon area, separate from text |
| Single-click selection | PASS | Status showed `1 selected` |
| Folder double-click | PASS | Address and enumeration changed to target folder |
| Right-click popup | PASS | Native `#32768` popup found through UIA |
| Close tab | PASS | Original address restored, `tabs=1` |
| Physical mouse Back/Forward | USER VERIFICATION | Synthetic events were unreliable |

Local screenshots covered the main view and centered New Folder dialog; the
dialog's OK button occupied client X=536–615, verified by pixel analysis.

### Remaining mouse verification

Both `SendInput` with `MOUSEEVENTF_XDOWN/XUP` and direct `WM_XBUTTONDOWN/UP`
messages failed to produce reliable events in automation. The code path was
verified: winit 0.30.13 maps XBUTTON1/2 to `MouseButton::Back/Forward`, and Slint
1.13.1 maps these to its pointer events. Both dispatches are wired in Kova.

Physical-device check: open a folder, press Back, then Forward. Repeat with the
pointer over the toolbar, tabs, sidebar and blank list area. A physical failure
would require a focused event-delivery fix; automated synthesis was not proof
of real mouse behavior.

## Deferred at M1

Batch rename, copy/cut/paste, drag & drop and undo were deferred to later milestones.
Menu icons and keyboard navigation followed native Win32 behavior. Icon failures
were cached per process to prevent retry loops. The long performance test remained
ignored by default.

## Commits and outcome

Feature commit: `b12485c` — `feat: m1 shell icons, context menus, mouse navigation, ui polish`.
Affected desktop files: `main.rs`, `app_state.rs`, `bridges.rs`, `main.slint`;
platform file: `shell_icons.rs`. The working tree was reported clean at closeout.

M1 functionality was implemented with the runtime evidence above. Physical mouse
Back/Forward remained explicitly unverified; no success claim was based solely
on the verified Slint API mapping.
