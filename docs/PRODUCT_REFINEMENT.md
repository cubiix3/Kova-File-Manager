# Desktop refinement and Explorer-style naming

The September 2026 refinement reduces the combined tab/navigation/command-bar
height by 26 pixels. Breadcrumb segments follow their text width. The graphite
palette, compact Places/Storage sidebar, tab accents, file selection and Home
quick links share consistent spacing and color roles.

## Naming

- New Folder and Ctrl+Shift+N create `New folder`, then `(2)`, `(3)`, etc.
  Atomic creation handles existing files and simultaneous creation without
  replacing anything. The new row is selected and its name is editable inline.
- F2 and Rename edit the selected row. The initial selection excludes file
  extensions. With extensions hidden, the original suffix is retained.
- Enter confirms; Escape cancels the edit. Cancelling a newly created folder's
  name leaves the folder in place. Clicking another list row confirms the edit;
  navigation or refresh cancels it. Invalid names show an error in the status bar.
- Existing destinations are never overwritten by rename.

## Preview

Drag the divider to resize the inspector; double-click it to restore its width.
The pane adds file type/size and Fit or 25–400% zoom with scrollbars.
Zoom uses the decoded preview pixels, not full-resolution original pixels.
These controls do not enlarge the existing decoder memory limits.

## Program launch

Windows Shell launch runs off the UI and command-dispatch threads, with
`SEE_MASK_NOASYNC` because its worker does not run a Windows message loop.
EXE launches receive their parent folder as the working directory; shortcuts
keep their configured working directory. Shell failures remain visible in Kova.

A portable WinForms fixture launched by an actual file-row double-click recorded
the repository working directory and failed to find a relative asset before
the fix. After the fix it displayed its window, recorded its own folder and
found the asset. This verifies that concrete failure, not every third-party EXE.

## Verification

Windows 11 input checks covered inline creation, F2 stem selection, successful
rename retaining `.txt`, Escape cancellation and invalid-name rejection.
Zoom reached 125%; dragging the divider enlarged the pane by 100 pixels.
The layout was inspected at 1140 × 780 and 780 × 600.

Local gates: formatting, workspace check, tests, Clippy with warnings denied,
and release build. The suite has 60 passing tests and three intentionally ignored
tests, including concurrent folder-name allocation and Home breadcrumb regression
coverage. The final release repeated inline creation, file rename and the EXE
double-click check. Home and PDF screenshots were captured from that release.
The installed executable's SHA-256 matched the release; its no-argument launch
opened Home.

Not verified: Windows 10 runtime, mixed-DPI displays, screen readers and arbitrary
third-party executable installers/elevation flows. Preview zoom and pane width
are session controls, not persisted preferences.
