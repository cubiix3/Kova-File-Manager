# Product audit — September 2026

Scope: current Windows desktop product, starting at `9442878`. Existing native
Rust/Slint/Shell architecture retained. Historical milestone reports are evidence
of earlier work, not verification of this build.

## Findings and changes

| Area | Finding | Correction |
| --- | --- | --- |
| Selection/data integrity | Sort and refresh reused selection indices for different files | Remap selection, anchor and focus by full path; rename dialogs capture the source path |
| Navigation | Old errors overrode newer requests; inactive-tab results were discarded | Check generation IDs for success and failure; cache results/errors/loading per tab |
| Tabs | Labels stayed at the initial location; closed tabs retained snapshots; switches reread directories | Labels follow history, closed state is removed, cached switches preserve state |
| Responsiveness | UI callbacks called `exists`; one slow directory delayed navigation | Validate by asynchronous enumeration; replace obsolete per-tab tasks |
| Errors | Old rows remained actionable beneath a new or failed address | Remove mismatched snapshots; explicit unavailable-folder state |
| Icons | Generic handles prevented asynchronous resolution | Leave unresolved handles unset; asynchronously seed fallbacks and resolve file icons |
| Performance | Selection reformatted every row; cache-hit lookup was quadratic | Update flags in place; deduplicate keys, hash cache hits and batch delivery |
| Rename/New Folder | Names accepted paths/streams; rename could overwrite destinations | Validate Windows filenames; non-replacing native rename and conflict-content tests |
| Shell operations | Aborted operations could report success; partial changes left stale views | Check `GetAnyOperationsAborted`; reconcile tabs after outcomes |
| Native menus | Desktop `GetUIObjectOf` received absolute PIDLs, producing incorrect file menus | Bind actual parent with `SHBindToParent`, pass child PIDLs and retain IContextMenu2/3 forwarding |
| Resources | Repeated COM initialization was unbalanced; failure paths leaked allocations | Thread-local COM lifetime; clipboard allocation guards; free partial PIDL arrays |
| Clipboard | Unbounded UTF-16 scan and unaligned external HDROP reads | Bound reads by allocation size; reject malformed offsets and read unaligned data safely |
| Test safety | Default tests overwrote live clipboard contents | Explicitly ignore interactive clipboard mutation tests; exercise actual runtime transfers separately |
| Rendering/startup | FemtoVG began producing white/transparent client areas, also reproduced with unchanged baseline; debug UI exceeded the default Windows stack | Enable Slint's Skia renderer; reserve 8 MiB main-thread stack on MSVC; suppress the console window |
| UI | Crowded drive labels, narrow-column overflow, uncentered/editable error dialogs | Shared tokens, separate capacity line, aligned compact columns, centered dialogs and wrapped errors |

Drive discovery and icon resolution run off the UI thread. Junctions behave as
folders, grouped first in both sort directions. No recursive production delete
was added: Copy/Move/Delete continue through IFileOperation.

The renderer comparison used the same UI and machine: Skia rendered all vector
and Shell icons correctly while FemtoVG failed. The underlying OpenGL/driver
cause was not established. No graphics-driver or system settings were changed.

## Verification

A PASS requires actual exercise, not inference from source or historical reports.
Runtime test mutations use `.rivet_temp/product-files`; clipboard formats were
saved and restored around the transfer checks. No personal files were mutated.

| Runtime exercise | Result / evidence |
| --- | --- |
| Startup, Home, Desktop, Documents, Downloads, drive G:, back/forward/parent, typed address | PASS: real release window and address/row changes |
| Tabs: create, switch, close, independent locations/history and labels | PASS: Documents and Archive tabs |
| Single/Ctrl selection, sorting with selection preserved | PASS: alpha/beta selection and copied HDROP paths agree after sorting |
| Shift/Ctrl+A, large-list scrolling | PASS: four-row range, 5000/5000 selected, wheel scrolling revealed later rows |
| Directory junction | PASS after attribute fix: DocumentsLink opens in Kova, with inside.txt listed |
| Double-click folder, New Folder, F2 rename, focused input and Enter | PASS: Created directory became Renamed on disk |
| Multi-file Copy/Paste | PASS: two files copied into Archive, contents checked |
| Cut/Paste (Move) | PASS: beta.txt moved to Documents; source absent, destination present |
| Explorer to Kova and Kova to Explorer clipboard | PASS: actual Explorer windows, Ctrl+C/Ctrl+V and destination content checks |
| Delete to Recycle Bin | PASS: removed Archive/inside.txt; matching original location and content found in Recycle Bin |
| Native file context menu, extension submenu | PASS after parent-PIDL fix: actual alpha.txt menu and expanded 7-Zip archive commands |
| Native folder and multi-file menus | PASS: Archive folder menu; two-file menu and expanded 7-Zip submenu retained both selected files |
| Properties | PASS: native alpha.txt Properties, correct path and size |
| Invalid path | PASS: error state, zero stale rows |
| Rename conflict | PASS: existing destination and source preserved |
| Release resize and dialogs | PASS: 1140x780 and 780x550 outer windows; centered focused input, wrapped error, Escape closes |

File/clipboard/Recycle Bin and Properties checks were performed during the
audit's release iterations. After enabling Skia, the final release was exercised
again for navigation, tabs, selection/scrolling, junctions, New Folder/Rename,
native folder/multi-file menus, 7-Zip, errors and resize. No backend file-operation
changes followed the earlier transfer checks.

Final release executable SHA-256:
`7ac3bbcd17ac59b97309ce145c054fe74d71db6f29f084b5289b87de5c7abde2`.

Actual final-build captures:
[desktop](images/product-audit-final.png),
[compact window](images/product-audit-compact.png),
[focused dialog](images/product-audit-dialog.png),
[conflict error](images/product-audit-error.png).

Automated gates: `cargo fmt --all -- --check`, `cargo check --workspace
--all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo build --workspace --release` all passed.
Tests: 43 passed, 3 deliberately ignored (one slow benchmark, two tests that
overwrite the interactive clipboard). The MSVC helper was used locally.

Generation races, closed-tab results, background-tab loading/error ownership,
selection identity, malformed clipboard offsets and non-overwriting rename
conflicts have automated regression coverage. This is not a substitute for
hardware/network failure testing.

## Remaining verification and limits

- NOT VERIFIED: live disconnected/removable drives, offline UNC shares,
  permission-denied mutations and paths longer than 260 characters.
- NOT VERIFIED: cancellation during a partially completed multi-file operation,
  native overwrite-conflict decisions, case-only rename and deletion on volumes
  without Recycle Bin support.
- NOT VERIFIED: every installed extension and every Open With handler. Native
  extensions execute their own code and may block while showing a menu.
- NOT VERIFIED: physical mouse XBUTTON2 and multiple monitor/DPI transitions.
- Drive discovery runs at startup; external filesystem changes require Refresh.
  Cached tabs retain their last snapshot until refreshed.
- Non-Unicode names and individual entries whose metadata cannot be read are
  skipped with a log message; no lossless display representation was added.
- Many tabs use horizontal scrolling; automatically revealing the active tab
  and resizable file columns remain future UX work.
- No File Pilot image attachment was accessible in this session. Visual QA used
  real Kova screenshots and the requested density/hierarchy criteria; no claim
  of a direct image-to-image comparison is made.

## API references

- [GetUIObjectOf child PIDL contract](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishellfolder-getuiobjectof)
- [SHBindToParent](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shbindtoparent)
- [Aborted shell operations](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-getanyoperationsaborted)
- [Clipboard ownership](https://learn.microsoft.com/en-us/windows/win32/dataxchg/using-the-clipboard)
- [Slint renderer selection](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/)
- [Slint generated UI and Windows stack](https://github.com/slint-ui/slint/discussions/5058)
