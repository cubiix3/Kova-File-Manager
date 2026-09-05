# Approved desktop design

The implemented design follows the user's approved mockup: a standalone K mark,
tabs beside it, a framed breadcrumb bar, full-width file commands, a graphite
Places/Drives sidebar and a folder heading above the details table. Primary
labels are now English, sizes use decimal points and dates use `YYYY-MM-DD` order.
The existing application functions remain connected to these controls.

## English interface and demo

Kova's own navigation, commands, menus, file-type labels, counts, loading states
and storage headings now use English consistently. New-folder creation starts
with `New folder`; existing file names and paths are not translated. Dates are
shown as `YYYY-MM-DD HH:MM`, and formatted sizes use a decimal point. Native
Windows menus, provider names and OS error messages retain their own language.
There is no language selector or automatic locale selection in this version.

The English release-mode build was exercised on Windows 11: Home, a second tab,
image/GIF/PDF previews, native file context menus, toolbar folder creation and F2
inline rename. Both file operations produced the expected paths in an isolated
fixture. A 28-second, 420-frame recording captures real inputs; decoded video
frames confirm GIF motion. The README screenshots were refreshed from this
build. The v0.1.0 installer still predates these changes. See [the demo](DEMO.md).

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
and resized from 1280×820 to 1330×850 through a real corner drag. Those checks
used the earlier installed release. The current README screenshots and demo
use the English development build; see [recording details](DEMO.md).

Local checks: fmt, workspace/all-target check, 60 passing tests (3 intentionally
ignored), Clippy with warnings denied, and an optimized release build.

Not verified: Windows 10, mixed-DPI monitor transitions, touch input and screen
readers. Remaining Windows/provider error messages may use their original
language; this is not a complete application translation framework.
