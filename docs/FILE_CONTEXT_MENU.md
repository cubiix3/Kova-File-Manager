# File context actions

Right-click and the toolbar's **More** button open a file menu using the existing
app palette, icons and typography. The menu groups common actions and displays
their shortcuts. It offers file/folder-specific actions for single selections and
Cut, Copy, Copy paths, Delete and Windows options for multiple selections.

The native Windows **IContextMenu** implementation is retained unchanged. Choose
**More Windows options** or press **Shift+F10** in the file list to use installed
Shell extensions. Normal themed actions use the existing Kova clipboard and
operation dispatchers, including conflict handling and Recycle Bin deletion.

## Interaction and accessibility

- Up/Down, Home/End and Enter choose visible commands. Escape, Tab and outside
  clicks dismiss the menu and return focus to the file list.
- Menu placement is clamped in logical pixels to the current window; resizing
  cannot leave its actions outside the viewport.
- UI Automation exposes a named File actions group and named action buttons.
  Command rows retain stable accessibility identities when selection changes.
  Focus moves out before menu nodes are hidden; opening multi-selection chooses
  an applicable command before exposing the accessibility tree.
- Navigation, selection changes, operation dialogs, window deactivation and snapshot replacement
  dismiss the menu so its row context cannot act on a different file.
- Copy path(s) writes full paths as text, separated by Windows line breaks for
  multiple selections. File Copy/Cut retains Explorer-compatible clipboard data.

## Verification

`scripts/test-ui.ps1` exercises toolbar and mouse opening, viewport bounds, full
path copying, keyboard rename, Undo, multiple selections, Escape, native menu
handoff, refresh invalidation and Gallery targeting. The existing drag/drop,
recursive search and restored-session flows run afterward. The Windows CI job
runs these against the actual application and uploads logs and screenshots.

`scripts/capture-demo.ps1` captures the themed menu and verifies a real native
Windows menu through its process-owned menu window before recording the extension
fallback. Product captures use disposable files and a separate preferences profile.

The initial interactive run exposed a focus error when a menu entry disappeared
for multi-selection. Focus is now changed before showing/hiding those nodes;
that transition remains covered by the multi-selection interaction test.

The [Windows CI run](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34483172250)
passed format/check, 90 tests, warning-free Clippy and all actual UI interactions
for application source `866b88a`. Build/run evidence is recorded alongside the [product captures](DEMO.md).
The broader [hardening verification and performance baseline](DAILY_DRIVER_VERIFICATION.md)
remains available with its original build provenance.
