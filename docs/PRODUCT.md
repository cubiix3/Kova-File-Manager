# Product scope

Kova is a native-first, open-source Windows file manager built around responsive
browsing, familiar interactions and visible file-operation outcomes.

## Current preview

- Home with drive capacity, free space, file system and usage bars.
- Tabbed browsing, breadcrumbs, history, known folders and direct folder launches.
- Native Windows Shell menus, associated application launch and file clipboard.
- Copy, move and Recycle Bin deletion through Windows `IFileOperation`.
- Inline New Folder and Rename, including filename validation.
- Sorting, resizable columns, Ctrl/Shift selection and mouse selection rectangle.
- Asynchronous Shell icons and thumbnails with fallback type icons.
- Image, text and PDF previews; GIF, WebP and APNG animation playback.
- View preferences and optional bounded background folder-size calculations.
- Reversible, per-user folder-opening integration.
- Windows setup and portable ZIP packaging.
- Virtualized Details/Gallery views with small, medium and large thumbnails.
- Snapshot search by name, extension, type, size and date.
- Inspector metadata, transfer center, explicit file conflicts and storage analysis.
- Persistent local tags, collections and reorderable pinned folders.

See [the next-generation feature guide](NEXTGEN.md) for exact behavior.

The primary interface labels and GitHub documentation are English. Native Shell menus follow the Windows language.
The [README](../README.md) introduces the current product; the
[view guide](VIEW_AND_PREVIEW.md) records preview limits and runtime evidence.

## Planned work

Global search, drag & drop, split panes, application-level undo, batch rename,
full session restoration and dedicated cloud/network handling remain future work.
These are directions, not promised release dates.

## Boundaries

Kova uses Rust, Slint and native Windows APIs. It does not use Electron, a WebView,
React or Tauri. Filesystem and preview work run outside the UI thread. File
identity, selection consistency and data integrity take precedence over visual
effects. See [data safety](SECURITY_AND_DATA_SAFETY.md).

Historical milestone reports describe their original build, not the current
feature set. See the [documentation index](README.md).
