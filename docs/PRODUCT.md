# Kova Product Document — M0

## Vision

Kova is a fast, native-first, open-source file manager for Windows.

## Current Status

M0 foundation. A usable desktop shell with navigation, tabs, sorting,
selection, and safe file operations in a sandbox.

## What Works

- Directory enumeration off the UI thread.
- Back / Forward / Parent / Refresh / direct path navigation.
- Tabs with independent history.
- Sorting by Name, Type, Size, Modified in both directions with header indicators.
- Selection: single, Ctrl multi-select, Shift range-select, select all, clear.
- New Folder and Rename through an in-app dialog (empty names rejected).
- Windows known folder resolution for the initial location and sidebar.
- Dynamic local drive enumeration in the sidebar.
- File open via `ShellExecuteExW`.
- Stale async result protection via per-tab generation IDs.

## Known Limitations

- Generic placeholder icons instead of real shell icons.
- No Copy / Move / Delete UI (only sandbox-tested core operations).
- No global search.
- No preview pane.
- No custom context menu.
- Keyboard shortcuts require focus in the file-list scope.
- No session persistence.

## Roadmap (post-M0)

1. Real Windows shell icons and thumbnails.
2. Recycle-bin-safe delete, copy, move with progress.
3. Global search via USN / MFT in a dedicated milestone.
4. Preview pane and split view.
5. Session persistence.
