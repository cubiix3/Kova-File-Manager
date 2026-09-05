# M2 report — Native Shell, file operations and UI

Date: September 2, 2026 · Branch: `main` · Baseline: M1 (`b12485c`)

> Historical report. Results and limitations below apply to the M2 build.
> See [current documentation](README.md) for subsequent changes.

## Native Windows context menus

`kova-platform-windows::shell_menu` uses `IContextMenu`, resolving all selected
PIDLs through the Desktop Shell folder and `GetUIObjectOf`. `IContextMenu2/3`
messages (`WM_INITMENUPOPUP`, `WM_DRAWITEM`, `WM_MEASUREITEM`, `WM_MENUCHAR`)
are forwarded through a dedicated hidden `KovaShellMenuHost` window. This avoids
subclassing Slint's window, hooks and `unsafe impl Send`.

Commands use `CMINVOKECOMMANDINFOEX`, MAKEINTRESOURCE offsets in
`CMD_FIRST..=0x7FFF`, raw IDs outside that range, and `CMIC_MASK_PTINVOKE` with
cursor coordinates. Right-click inside the selection targets the whole group;
otherwise it targets the clicked row. Invoking a command triggers refresh.

Blank space retains Kova's New Folder/Paste/Refresh menu without an extra overlay.
`ensure_com_sta` initializes the UI thread's COM apartment.

## Copy, cut, paste, move and delete

- Clipboard: unit-tested `CF_HDROP` encoder/parser plus Preferred DropEffect
  (`COPY=1`, `MOVE=2`), interoperable with Explorer in both directions.
- Operations: dedicated COM STA thread using `IFileOperation` CopyItems,
  MoveItems and DeleteItems with `FOF_ALLOWUNDO`, native progress/conflict UI and
  `catch_unwind` to report failures instead of terminating the worker.
- Shortcuts: Ctrl+C/X/V and Delete, with Ctrl/Shift multi-selection.
- Cancellation codes `0x800704C7`, `0x800703E3` and
  `COPYENGINE_E_USER_CANCELLED` produce cancellation status; other failures
  produce status and an error dialog. Views refresh after completion.

## UI changes and runtime fixes

Vector toolbar icons, separators and an accent address-field focus border replaced
text glyphs. Tabs used an accent underline and showed Close on hover/active state.
The denser sidebar grouped Quick Access and Drives; usage bars used
`GetDiskFreeSpaceExW`, free/total labels and a danger color above 90% use.

File rows were 26px high with aligned columns and quieter hover/selection colors.
The status bar showed status and item/selection counts. Dialogs had rounded corners.

Fixed selection indices leaking across navigation by clearing selection on
Navigate/Back/Forward. Ctrl+L now focused and selected the address field instead
of navigating again. Submitting an address attempted to return focus to the list.

## Quality gates

| Gate | Result |
| --- | --- |
| Formatting | PASS |
| Workspace/all-target check | PASS |
| Workspace tests | PASS: 38 passed, 1 ignored performance baseline |
| Clippy with `-D warnings` | PASS |
| Release build | PASS: approximately 30 seconds |

New tests covered HDROP roundtrips and malformed/ANSI buffer rejection, live
clipboard file roundtrips, selected-path ordering, drive capacity and operation
labels. Clipboard tests were serialized after parallel access caused
`STATUS_HEAP_CORRUPTION`.

## Runtime verification

Release build, UIA and SendInput in `%TEMP%\kova-m2-run` with `alpha.txt`,
`beta.log`, `SubFolder` and a separate paste-source folder. Filesystem state and
application logs were used as evidence.

| Test | Result |
| --- | --- |
| Ctrl+L navigation into sandbox | PASS: log |
| Ctrl+C produces CF_HDROP for alpha.txt | PASS: PowerShell `GetFileDropList` |
| Ctrl+click multi-selection | PASS: UIA status `3 items · 2 selected` |
| Ctrl+X sets MOVE(2) | PASS |
| Ctrl+V moves beta.log into SubFolder | PASS: disk and `entries=2` |
| Paste Explorer CF_HDROP source as paste_me.txt | PASS: disk and `entries=3` |
| Delete paste_me.txt to Recycle Bin | PASS: disk and `entries=2` |
| Native Shell right-click menu | PASS: `#32768` popup |
| Screenshots | Local PrintWindow captures verified dark content; not committed |

7-Zip and Git were installed. Menus came from the real Shell context object, but
item-level UIA inspection was unavailable through the popup HWND. Individual
extension entries therefore still needed visual confirmation.

M1 navigation, icons, tabs, New Folder, Rename and selection retained their
existing pipelines. Open/Rename/Copy Path moved into the Shell menu or existing
keyboard commands; this was an intentional interaction change.

## Deferred at M2

The dedicated This PC overview was evaluated and deferred; sidebar usage bars
covered drive access at that stage. Drag & drop, undo and batch rename remained
future work. Conflict dialogs were provided by Windows. Clipboard-mutating tests
were guarded when existing clipboard files were present.

## Commits

- `cec0b7d`: native Shell menu, operation thread, Explorer clipboard and capacity.
- `377b553`: desktop wiring and visual polish.
