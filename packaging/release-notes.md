Kova 0.2 adds workspace restoration, recursive folder search, native Windows drag
& drop, safer conflict handling and a reviewable Undo for supported renames/file moves.
Large folders use background natural sorting/filtering and a lazy UI model.

## Included

- Restored tabs, searches, window state, columns, gallery and inspector.
- Plain search with Type / Size / Date controls and cancellable Include subfolders.
- Themed file context actions with icons and shortcuts; native extensions through More Windows options.
- Explorer-compatible drag/drop and clipboard; Copy/Move feedback before dropping.
- Incoming/existing conflict comparison with Replace, Skip, Keep Both and apply to remaining.
- Recent folders and favorites on Home, clearer actions, copyable inspector paths,
  keyboard/accessibility improvements, locale-aware values and drive recovery with Retry.
- Existing Windows Shell extensions, IFileOperation, Recycle Bin deletion, previews,
  thumbnails, tags/collections, storage analysis and operation center remain integrated.

## Downloads

- **Setup EXE:** per-user installation, Start menu shortcut and uninstaller.
- **ZIP:** extract all files together and start **Kova.exe**.
- **SHA256SUMS.txt:** checksums for both downloads.

Requires Windows 10 version 1809 or later, or Windows 11, on x64. Visual C++
runtime files are included; Rust and Visual Studio are not needed. Packages are unsigned.

## Limits

Undo is session-only and covers validated non-replacing renames and same-volume
file moves. Copies, deletions, replacements, cross-volume moves and folder moves
are not offered as undoable. Recursive search skips directory links and observed
offline placeholders; it is not a system-wide index. Cloud virtual-file drag formats,
split panes, batch rename, full German translations and light theme are not included.
Windows provider/path and network timeouts can still apply. Physical USB removal,
real monitor switching and a clean Windows 10 installation need additional device testing.

See the repository's daily-driver verification report for measurements and runtime coverage.

<a href="https://slint.dev"><img src="https://raw.githubusercontent.com/slint-ui/slint/v1.13.1/logo/MadeWithSlint-logo-whitebg.png" width="150" alt="Made with Slint"></a>
