# Approved desktop design

The implemented design follows the user's approved mockup: a standalone K mark,
tabs beside it, a framed breadcrumb bar, full-width file commands, a graphite
Places/Drives sidebar and a folder heading above the details table. Primary
labels are German, sizes use decimal commas and dates use day/month/year order.
The existing application functions remain connected to these controls.

File names use 14px text and 20px icons. Compact rows are 34px and comfortable
rows 40px. Metadata is quieter than names; alternating backgrounds are subtle.
Selection has a blue fill and accent edge. Header and row columns share one
geometry model; dragging separators changes widths. At narrow widths the table
hides type/date columns before compromising the name column. Column widths
are session state, not persisted settings.

Folder-size work remains asynchronous. A pending calculation is shown explicitly;
an incomplete result retains its lower-bound marker. It is not presented as a
finished exact size or as indefinitely running work. The status bar shows the
selected item's known size, including lower-bound wording when necessary.

## Window resizing

The previous frameless window relied on Slint's 5px resize border. Explicit
logical-pixel input regions now cover all four edges and corners; the lower-right
grip is visible. Mouse-down delegates to winit's native `drag_resize_window`.
The regions are disabled while maximized. This uses the existing window and
requires no Win32 subclass, input hook or synthetic window-size loop.

Reference: [winit native drag resize](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.drag_resize_window).

## Verification on Windows 11

Actual mouse input changed window bounds at every edge and corner. Examples:
1280×820 became 1370×880 at the lower-right corner; dragging the left edge then
increased width from 1370 to 1420. Maximize/Restore were exercised. Moving the
Name/Type separator by 40px moved the Type column by 40px without sorting it.
The layout was visually inspected at 1280×820 and 780×600.

Toolbar folder creation and F2 rename changed real fixture paths and retained
the text file's extension. An EXE fixture opened and found its relative asset.
PDF preview remained visible and usable with the new table layout.
The installed release matched the build's SHA-256, opened Home without arguments,
and resized from 1280×820 to 1330×850 through a real corner drag. README captures
were taken from that installed release.

Local checks: fmt, workspace/all-target check, 60 passing tests (3 intentionally
ignored), Clippy with warnings denied, and an optimized release build.

Not verified: Windows 10, mixed-DPI monitor transitions, touch input and screen
readers. Remaining Windows/provider error messages may use their original
language; this is not a complete application translation framework.
