# Next-generation verification — historical report

This is the September 6 milestone, not the 0.2 release verification. Current test
counts, runtime coverage and performance are in the [daily-driver report](DAILY_DRIVER_VERIFICATION.md).

Windows 11 x64, September 6, 2026. Tests used isolated app preferences and local
demonstration files; normal personal folders were not modified. The current
feature contract and limitations are in [NEXTGEN.md](NEXTGEN.md).

## Automated checks

- `cargo fmt --all -- --check`: pass.
- `cargo check --workspace --all-targets`: pass.
- `cargo test --workspace`: 72 passed, 4 environment/performance tests ignored.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- Release workspace build: pass.
- The normally ignored native transfer test was run explicitly on the desktop:
  two-file Keep Both, Skip, Replace and Cancel with apply-to-all; source and
  destination contents checked before and after; native move and relocation
  notifications checked. Pass.

Regression coverage includes 10,000-entry snapshot filtering and selection,
gallery selection rectangles, virtual-folder destination guards, unavailable
references, atomic library replacement, exact-file and folder relocation, stale
snapshots, preview cancellation, malformed clipboard data and bounded storage scans.

## Real desktop flow

The actual Slint window was driven through mouse and keyboard input, with
filesystem checks in addition to screenshots:

- Home, folder navigation, Details/Gallery switching with selection retained,
  all three gallery sizes, first/last item navigation, responsive layout at 760 x 500 and 1280 x 800.
- Native bottom-right drag resize changed the window from 1280 x 800 to
  1400 x 860 through pointer input.
- Current-directory filters by name, extension, size and local date; 1,000 and
  10,000 synthetic files; End reached the last gallery tile and combined filters
  reduced the cached entries without a new directory enumeration.
- Image, text and PDF inspector; GIF playback verified by nine screen samples
  containing three distinct preview frames. MP4 thumbnail and Windows metadata
  reported 1280 x 800 and duration 0:00:28 for the 28-second demonstration clip.
- Multi-file copy/paste; file conflict with different source/destination contents;
  Keep Both; inline rename; deletion verified in the actual Recycle Bin.
- Copy and Keep Both output SHA-256 hashes matched their sources. A live copy
  of a 10,000-file folder was cancelled after approximately 1,985 completed files;
  the UI remained responsive and completed files were retained.
- Closing during a waiting conflict kept the process/window alive. Resolving
  or cancelling allowed normal shutdown.
- Collections and tags created through Organize, persisted across restart,
  and displayed references without moving files. Kova rename updated references;
  an externally missing file remained visible and recovered after restoration.
- Arbitrary folder pins, reordering and removal checked against persisted paths;
  drive context navigation checked on G: rather than the first drive; native file context menus with installed extensions,
  including 7-Zip. The Open/More controls use the current selection.
- An EXE launched by double-click verified its working directory by reading an
  adjacent relative asset and writing a local result file.
- Storage analysis of the demo tree, plus a self-referencing NTFS junction:
  the safety fixture returned 13 bytes, 1 file and 1 skipped link, without looping.
- Navigation to an unavailable drive produced a recoverable folder error.

## Large-folder timing

A final local release run logged 13.57 ms to enumerate 1,000 entries and 66.92 ms
for 10,000 entries. These are single warm local-disk enumeration measurements,
not end-to-end rendering latency or a cross-machine benchmark. The same real
window remained usable for End navigation, selection and snapshot filtering.

## Screenshots

- [Home and drive capacity](images/home-overview.png)
- [Gallery and inspector](images/nextgen-gallery.png)
- [Details and PDF preview](images/file-preview.png)
- [File conflict comparison](images/nextgen-conflict.png)
- [Storage analysis](images/nextgen-storage.png)
- [Native Shell menu with extensions](images/nextgen-native.png)

## Issues found and corrected during verification

- Virtualized gallery End scrolling could stop short of the last tile; defer
  visibility adjustment until the layout settles.
- Search text needed a two-way binding to clear correctly on navigation.
- Exact file relocation appended a directory separator; preserve the exact new
  path when the relative suffix is empty, with an OS-string regression test.
- Closing overlay panels could leave keyboard focus behind; return it to the list.
- Ctrl+Shift+N delivered an uppercase N; accept both cases.
- More needed the actual primary row rather than the activation sentinel.
- Nested sidebar/drive input areas needed explicit secondary-click forwarding
  to open their context menus while retaining mouse Back/Forward. Popup actions
  capture the invocation target explicitly instead of a repeated menu binding.
- Ctrl+T now consistently creates a new tab; Ctrl+Enter opens the selected folder
  in a new tab and Ctrl+W closes the current tab.

## Limits of the evidence

The large-folder checks are synthetic local-disk checks, not a claim about every
NAS, USB controller or third-party Shell extension. No physical drive was removed
during an active write. Preview handler support varies by machine. Kova does not
implement global indexing, external-move identity tracking, video playback or
application-level rollback. Existing native Windows conflict and recovery
semantics remain authoritative.

The README and accompanying screenshots describe this source update; the older
v0.1.0 installer is not represented as containing these features.
