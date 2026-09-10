# File context actions

Commands highlight on mouse hover without pressing a button, using a short
90 ms transition. Keyboard focus follows the hovered command, so Enter invokes
the highlighted action. Ctrl+Z in the file view restores the last selection
deleted by Kova into the Recycle Bin during this session.

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
- Navigation, selection changes, operation dialogs and window deactivation dismiss
  the menu so its row context cannot act on a different file. Background refreshes
  and folder-size updates wait while the menu is open, then reconcile queued changes.
- Copy path(s) writes full paths as text, separated by Windows line breaks for
  multiple selections. File Copy/Cut retains Explorer-compatible clipboard data.

## Verification

`scripts/test-ui.ps1` exercises toolbar and mouse opening, viewport bounds, full
path copying, keyboard rename, Undo, multiple selections, Escape, native menu
handoff, deferred refresh and Gallery targeting. The existing drag/drop,
recursive search and restored-session flows run afterward. The Windows CI job
runs these against the actual application and uploads logs and screenshots.

`scripts/capture-demo.ps1` captures the themed menu and verifies a real native
Windows menu through its process-owned menu window before recording the extension
fallback. Product captures use disposable files and a separate preferences profile.

The initial interactive run exposed a focus error when a menu entry disappeared
for multi-selection. Focus is now changed before showing/hiding those nodes;
that transition remains covered by the multi-selection interaction test.

The [Windows CI run](https://github.com/cubiix3/Kova-File-Manager/actions/runs/34486247256)
passed format/check, 90 tests plus 3 isolated clipboard tests, warning-free Clippy and all actual UI interactions
for application source `a527789`. Build/run evidence is recorded alongside the [product captures](DEMO.md).
The broader [hardening verification and performance baseline](DAILY_DRIVER_VERIFICATION.md)
remains available with its original build provenance.

A repeated UI run exposed a briefly occupied Windows clipboard during Copy path.
Clipboard opening now retries access-denied errors for up to approximately 100 ms,
before modifying any data. Persistent locks still produce an error. Isolated
Windows CI tests exercise both transient recovery and persistent contention, plus
text and Explorer-compatible file round trips. This follows the exclusive-access
behavior documented for [OpenClipboard](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openclipboard).

Native-menu handoff requests a redraw and briefly defers the blocking Shell call,
so the dismissed themed menu is painted away first. The captured tab, target path
and selection must still match before Windows options opens. The final captures
verify that the two menus are no longer drawn over one another.

The 0.2.1 regression check holds the menu open during an external timestamp change.
A local Windows test also reproduced the dismissal in 0.2.0, then kept a real
right-click menu open in the patched build through eight file/subfolder writes
with folder sizes enabled. Choosing Preview afterwards displayed the latest file
contents. Controller tests cover unchanged filtered views, concurrent background
filter work and real metadata changes; native watcher tests cover both shallow
and recursive modes.
