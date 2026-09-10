# Kova 0.2 daily-driver verification

Implementation and interactive Windows verification: 10 September 2026.
This report records the hardening baseline built from `ea92807` (application
changes through `46da38a`). Its measurements and captures refer to that build.
The subsequent [file-menu refinement](FILE_CONTEXT_MENU.md) adds themed actions
and further Windows interaction coverage. Current candidate downloads are linked
from the [README](../README.md). Public publication remains a separate step.

## Scope delivered

Effective folder sizes now drive display, filtering and sorting consistently.
Workspace preferences use versioned, debounced atomic background writes. Tabs,
active location, per-tab searches/scopes/filters/sort, window state, columns,
gallery and inspector restore across restarts, including unavailable locations.

Large snapshots filter/sort on a worker with generation checks. Natural ordering
is reused for query-only changes. The UI model shares snapshots and formats only
accessed rows; cached row updates retain thumbnails and selection where valid.
Recursive search runs asynchronously, supports cancellation and skips observed
directory links/offline placeholders. It does not build a system-wide index.

Native OLE drag/drop supports folder, tab, sidebar and Explorer transfers. Copy/Move
feedback remains visible inside the Windows drag loop. Kova destinations use the
existing IFileOperation queue and explicit conflict decisions. Undo reviews a
captured operation ID and validates file identity before a non-replacing rename.

Home shows recent/favorite locations; toolbar actions, search filters, inspector
copy actions, conflict metadata, operation summaries and keyboard/accessibility
labels have been improved. Windows drive-map changes refresh affected tabs, errors
offer Retry, and metadata uses Windows regional formats. Coherent Rust/Slint
components were extracted while retaining the native Shell operation layer.

## Automated checks

- `cargo fmt --all -- --check`
- `cargo check --locked --workspace --all-targets`
- `cargo test --locked --workspace`: 90 passing tests; five explicitly ignored
  tests cover interactive clipboard/native operations or large filesystem benchmarks.
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- `cargo build --locked --release --workspace`

The native IFileOperation integration test was also run explicitly on the local
interactive Windows 11 desktop. Copy/move contents and Replace/Skip/Keep Both/Cancel,
including apply-to-remaining conflicts, passed. Clipboard tests that overwrite
arbitrary user formats remain excluded from ordinary CI.

The new real-filesystem interaction tests exercise open/select/rename/refresh,
conflicting rename preservation, recursive descendants/missing root and paths over
260 characters through rename, refresh and identity-checked Undo. Controller tests
cover stale navigation/search, selection identity and computed folder size sorting
after filter changes on both small and background-sized snapshots. The lazy model
test covers bounded row materialization and stale-thumbnail invalidation.

`scripts/test-ui.ps1` drives the actual app through keyboard input and Windows UI
Automation. It exercises open/select/F2 rename/F5 refresh/Undo, Ctrl-drag copy of
an already selected item, recursive search, global address focus from search, tab creation/switching, debounced save before
shutdown and active-tab restoration after restart. Its UUID fixture and separate
LOCALAPPDATA profile remain available for inspection. It requires an unlocked
interactive Windows desktop. The optional **Windows packages** verification job
runs it on a hosted Windows desktop against a checksum-verified package and
retains logs, fixtures, screenshots and package-source provenance.

## Runtime matrix

| Flow | Result / evidence |
| --- | --- |
| Startup/session | Two tabs, active location, search/scope, gallery, inspector, normal size and maximized state restored. Restoring a maximized window returns its saved normal size. |
| Columns/inspector | Mouse-resized type column and inspector width persisted before shutdown and after restart (140 → about 161 logical pixels; inspector 340 → 440). |
| Navigation/tabs | Address from focused search, Ctrl+T/W/Tab, breadcrumbs, refresh and unavailable locations exercised. |
| List/gallery/search | Natural Image 2/Image 10 ordering, name/type filters, recursive child image, gallery switching and inspector image/text previews exercised. Final packaged UI also passes Ctrl+1/Ctrl+2 and named Details/Gallery actions. |
| Large folders | Real 1k/10k/100k flat directories; search, sort, refresh, scrolling and memory measurements described below. |
| Inspector | Long path wraps; Copy Path and metadata are accessible; no-file Copy Path is disabled. |
| Internal drag/drop | List → folder Move, list → tab Move and Ctrl+list → sidebar Copy verified against actual source/destination contents. |
| Explorer interoperability | Explorer → Kova Ctrl+Copy/Shift+Move and Kova → Explorer Shift+Move verified. Explorer clipboard copy → Kova paste and Kova cut/paste across tabs verified. |
| Conflicts | Final packaged UI Keep Both preserves both contents; incoming/existing name, path, size/date and operation Action required state visible. Native integration test covers all decisions. |
| Undo | F2 rename and same-volume native file move restored. Confirmation names the operation; changed identity/occupied destination are refused by tests. |
| Delete | A uniquely identified fixture was deleted through Kova and found in Windows Recycle Bin with its expected original directory. |
| Native menu | Shift+F10 opens the real Shell menu: NanaZip/Notepad++ extensions locally; final packaged capture and visible-menu assertion passed on CI with 7-Zip. |
| Network/drives | Unavailable localhost UNC share displays error/Retry. Temporary SUBST drive arrival/removal updates sidebar and affected tab automatically; restart retains missing tab and reappearance reloads it. |
| DPI/resize | 125% Windows scaling, minimum/normal resizing, maximize/restart/restore, columns and inspector tested. Per-monitor-v2 manifest included. |
| Accessibility | Windows UI Automation exposes named tabs, toolbar controls, sort buttons, search scope and file list items; UI test invokes list selection and toggles recursive scope through accessibility patterns. Keyboard focus rings and global navigation shortcuts exercised. |

SUBST validates logical drive transitions; it does not simulate USB hardware,
media eject errors or a blocked network server. Real cross-monitor DPI changes and
a full Narrator session were not available in this environment.

## Performance method

Final packaged UI benchmark: Windows Server 2025 (build 26100), AMD EPYC 7763
host with four guest logical processors, 16 GB RAM, 984 × 680 physical window
pixels at 100% scaling, Slint 1.13.1, release executable version 0.2.0.
The [successful interactive run](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34475306852)
contains logs, screenshots and the executable SHA-256.
Fixtures contain 1,000 / 10,000 / 100,000 zero-byte `Image <n>.txt` files. Filesystem
caches are warm. No build runs concurrently with the UI measurement.

Directory latency is F5 request to updated UI model (enumeration + natural sort +
delivery), median of three refreshes. Search latency is request to model, including
the 150 ms typing debounce; six samples alternate `Image 999` and clearing search.
Sort latency is header activation to updated model, median of three direction
changes. Scrolling sends 60 wheel events with a requested 16 ms interval (actual
injection spacing depends on the Windows scheduler) and measures input to
the next winit redraw request. **This is a responsiveness proxy, not measured
GPU presentation time or FPS**; the active Skia backend did not emit the optional
Slint after-rendering callback. Working set is sampled after scrolling; peak is
the process lifetime peak, not total system memory.

| Entries | Directory median | Search median | Sort median | Scroll request p95 | Working / peak set |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 17.98 ms | 151.87 ms | 1.56 ms | 11.84 ms | 54.14 / 54.18 MiB |
| 10,000 | 103.17 ms | 191.08 ms | 37.62 ms | 11.71 ms | 61.86 / 61.89 MiB |
| 100,000 | 448.62 ms | 251.89 ms | 273.57 ms | 12.14 ms | 118.87 / 149.93 MiB |

[Final raw samples and environment](measurements/daily-driver.json) describe this
synthetic workload, not cold-cache disks, network shares, hundreds of thousands
of media thumbnails or concurrent transfers.

An earlier [local UI measurement](measurements/daily-driver-baseline.json) used
Windows 11, Ryzen 9 8945HS, 32 GB RAM, Rust 1.98.1 MSVC, 1400 × 900 physical pixels
at 125% scaling. It predates the final modifier-drag/view-control changes, uses the
same filtering/model algorithms and is retained as a separate hardware baseline.
Its 100k medians were 575.10 ms directory, 265.85 ms search, 312.15 ms sort,
7.04 ms scroll request p95 and 118.23 MiB working set. These are not paired
before/after measurements against the hosted runner.

Before the lazy UI model, a 100k-row refresh spent 147–201 ms constructing/updating
UI rows alone. After the change, observed model replacement was about 3 ms in a
debug trace; the release benchmark below measures the complete request path.
Moving enumeration to a single blocking directory walk reduced the 100k backend
enumeration median from about 1,697 ms to 161 ms on the same filesystem fixture.

Reproduce on an interactive desktop:

```powershell
.\scripts\cargo-msvc.ps1 build --locked --workspace --release
.\scripts\benchmark-ui.ps1 -PrepareFixtures
.\scripts\test-ui.ps1 -Executable target/release/kova-desktop.exe
.\scripts\cargo-msvc.ps1 -CargoArgs @('test','--release','-p','kova-ops','--test','daily_driver','daily_driver_performance','--','--ignored','--nocapture')
```

Logs, unedited benchmark screenshots and process measurements are written under
`target/runtime/ui-benchmark`. The ignored filesystem benchmark is separate: its
enumeration/sort/matching timings do not include the UI or typing debounce.
The separate local release backend run measured 1.175 / 12.342 / 131.773 ms enumeration,
0.610 / 11.115 / 124.044 ms natural sorting and 0.106 / 1.329 / 15.352 ms
matching for 1k / 10k / 100k entries ([raw medians](measurements/backend-release.json)).

## Remaining limits

- Undo is session-only (32 entries), covering confirmed non-replacing renames and
  same-volume top-level file moves, one confirmed item at a time. Copies, deletes,
  replacements, cross-volume
  moves, folder moves, links and externally executed actions are excluded.
- Recursive search retains the enumerated tree in memory and displays final results
  after traversal. Cancellation discards stale work; an already-blocked provider
  I/O call or third-party Shell handler cannot be forcibly interrupted.
- Incoming drag/drop supports filesystem paths (CF_HDROP), not virtual-file streams
  from Outlook/cloud providers. Link-creation gestures are rejected. External target
  operations have their own Windows feedback and are not entered into Kova Undo.
- Long-path-aware native entry points and a >260-character interaction test improve
  support; Windows configuration, Shell extensions and filesystem providers can
  still impose their own path limits. No system registry policy is changed.
- Multiple processes share the last saved workspace. A crash can lose changes
  still within the debounce interval; corrupt/newer settings are preserved.
- English UI, Windows locale formatting and translation boundaries are present;
  complete German catalogs, light/system themes, split view and batch rename are
  future work. Optical drives are not listed.
- Hardware USB removal, a real multi-monitor DPI switch, a full Narrator pass and
  a clean Windows 10 installation remain separate verification targets.

## Release provenance

The [Windows package run](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34473650758)
passed formatting, tests, warning-free Clippy, release compilation and installer
install/launch/remove checks, including reinstall, user-data preservation and
folder-association restoration. Its application source is `ea92807`.
The [packaged interaction and performance run](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34475306852)
verified checksums and passed UI interactions, the 1k/10k/100k benchmark, view
shortcuts and conflict preservation. Its first native-menu capture was rejected
on visual inspection; the capture harness now requires a visible process-owned
Windows menu before saving that image. The
[follow-up verification](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34476071198)
passed the complete UI/benchmark/capture sequence again, including Shift+F10 and
the visible native menu assertion. Its menu screenshot was visually reviewed.

The packaged Kova.exe SHA-256 is
`F52ED2B90254DC222C8A874E9AA662106A9D99EB4ECABE385B821E0D4036D7F2`.
Setup and portable ZIP checksums ship in `SHA256SUMS.txt` alongside the artifacts.
The PR retains protected main and uses normal commits/pushes; no public release
or tag is created as part of this work.
