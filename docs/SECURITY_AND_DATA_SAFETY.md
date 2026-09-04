# Security and data safety

File identity and data integrity take priority over visual changes. Mutating tests
use unique temporary directories or a designated ignored runtime sandbox.

Copy, Move and Delete run on a dedicated COM thread through Windows
IFileOperation, with native conflict/progress UI and undo/Recycle Bin support.
Windows controls whether a destination supports recycling; Kova does not silently
substitute a recursive permanent-delete implementation. Cancellation may mean
partial completion, so all open directory views are reconciled afterwards.

New Folder and Rename accept one validated Windows filename. Paths, alternate
streams, reserved device names, control characters and trailing dots/spaces are
rejected. Rename uses MoveFileW on a worker thread, preventing silent replacement
of an existing destination. Case-only and unusual-filesystem behavior still
requires verification on the relevant target volumes.

Selection indices are remapped by full path after sorting/refresh. Rename dialogs
capture the source path. Navigating removes mismatched snapshots before they can
be acted upon. Success and failure must match the tab's latest request generation.

Native menus bind the selected items' parent and child PIDLs. Installed extensions
remain native; their execution, UI and responsiveness are outside Kova's control.
Failed resolution must not invoke a partial selection.

Windows-specific unsafe calls belong in the platform crate; the legacy
ShellExecuteExW launcher currently lives in kova-ops. New unsafe blocks require a
SAFETY explanation. COM initialization is balanced per thread, clipboard allocations
remain owned until transfer, and clipboard reads use allocation bounds.

The core uses PathBuf. Normalization does not query the filesystem or follow
junctions. Long paths, UNC shares, permissions and disconnected devices remain
subject to Windows/filesystem capabilities. See [verification limits](PRODUCT_AUDIT.md).

Default tests do not mutate the live desktop clipboard. Interactive clipboard
tests require a controlled session and are explicitly ignored. Runtime Explorer
checks use sandbox files and restore captured clipboard formats afterwards; this
is not a claim of clipboard-history restoration.

Logs may contain paths, but do not intentionally contain file contents. Builds,
logs and machine-specific configuration are ignored by Git.