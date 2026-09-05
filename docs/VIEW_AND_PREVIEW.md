# View options and previews

The **View** menu at the right of the command bar contains preview visibility,
storage overview, folder-size calculation, hidden files, system files, file extensions, compact rows, alternating row
colors and the loading-logo animation. The top-left menu now uses only the
Kova logo. It gently pulses during directory loading; animation can be disabled.

Hidden and system flags use Windows file attributes. Files carrying both flags
require both options. Extension hiding changes the displayed name only; opening,
renaming and other operations keep the complete path. Filtering reuses cached
entries in all tabs, remaps selection by path and excludes invisible items from
Select All and subsequent operations. Icons are queued when entries become visible.
Preferences are stored on normal app exit in `%LOCALAPPDATA%\Kova\view-options.txt`.

## Preview pane

Select one file and choose **View > Preview pane**, or press **Space** in the file
list. The pane supports common raster images, plain text/source files, and PDF
pages with previous/next controls. Unsupported, unreadable and binary files show
a message. It is read-only; scripts and document content are not executed.

One worker performs filesystem reads and Windows Runtime decoding. Its pending
queue is bounded to one request; obsolete results are rejected by generation,
selected path and page. Images are rendered at up to 1024 pixels on the long edge,
respecting EXIF orientation. Inputs above 128 MiB or images above 80 million pixels
are not previewed. Text reads at most 64 KiB and supports UTF-8 and BOM-marked
UTF-16; invalid UTF-8 uses replacement characters. PDFs use Windows.Data.Pdf,
without a WebView, and display one page at a time. Password-protected PDFs show
the decoder error; password entry is not included.

In narrow windows, lower-priority details columns disappear to leave readable
file names beside the preview. This is a preview pane, not a second file pane.

## Storage and folder sizes

**Home** is the start page when Kova launches without a folder argument and when
creating a new tab. Click Home, **Overview** beside Devices & Drives, or
**View > Drives and storage** to return to it.
The storage overview shows Windows volume labels, file system, free/total bytes
and usage percentages. Its usage bars animate into place, unless animations are
disabled. Double-click opens a drive; Back/Forward traverse Home and folders in
each tab's own history. Home is a virtual location with no filesystem target;
file creation and paste are disabled there. Explicit folder launches bypass Home.
Drive capacity is read on startup and refreshed with F5 on Home, off the UI thread.
Arrow keys select a drive and Enter opens it.

**View > Calculate folder sizes** enables a separate worker for local fixed disks.
The Size column fills with logical file-byte totals, including nested entries.
Scans skip observed reparse points, offline files and recall-on-data-access items;
they stop after two seconds or 50,000 entries per folder. `≥` marks a lower bound,
never a falsely complete size. These totals are not allocated size on disk, and
hard-linked file paths can count separately. Network/removable roots display
Local Only. Refresh starts a new calculation; navigation and disabling the option
invalidate pending results. Both request and result queues are bounded.

When Size sorting is requested, known folder totals are used and selection is
remapped by path. Background totals do not continuously reorder rows under the
mouse: click the Size header again to re-sort with later results.

## Verification

Real release mouse/keyboard verification on Windows 11:

- PNG image and UTF-8 text with umlaut/Japanese characters rendered.
- Two-page PDF rendered; clicking Next changed the displayed page and page count.
- Binary text fixture produced the expected message.
- Actual Hidden/System attribute fixtures appeared when their View switches were
  enabled (5 to 6 to 7 visible items); hiding extensions changed labels only.
- Preview remained usable at 1140 × 780 and 780 × 600 without overlapping controls.
- Background Select All selected all seven visible fixtures; Clear Selection
  cleared them. Open This Folder created a second tab at the same path.
- Sort submenu changed to descending and visibly reordered the rows.
- Native file context menu retained installed extensions including 7-Zip.
- Clean window close wrote the expected view preferences.
- Reopening restored preview visibility. Empty text files showed an explicit hint.
- Storage overview rendered all four local NTFS drives at normal and compact sizes;
  double-clicking the G: row navigated to its actual root.
- Starting the final release without arguments opened Home. Drive double-click,
  Alt+Left/Right, Home Ctrl+T/Ctrl+W, arrow selection/Enter and F5 were exercised.
  Home remained usable at 1140 × 780 and 780 × 600. Deferred focus avoids a Slint
  accessibility-tree reentrancy panic during Home/folder transitions.
- The installed executable's hash matched the final release. Windows Shell Open
  launched it directly at the fixture folder; a separate no-argument launch
  opened Home. Folder registration status reported complete.
- Folder fixtures displayed 0 B (empty), 3.0 KB (nested files), 2.0 MB (larger file)
  and ≥ 0 B for a directory containing a junction back to the fixture root.
  Size sorting placed the 3 KiB folder before the 2 MiB folder.

Automated tests cover visibility/selection remapping, retained operation paths
when hiding extensions, text encodings and binary detection, alongside the
existing navigation/selection/operations suite.
The five local quality gates passed with 53 tests passing and 3 intentionally ignored.

NOT VERIFIED: Windows 10 runtime, reachable UNC shares, password-protected PDFs,
huge/malformed image and PDF fixtures, EXIF-rotated photos, optional installed
image codecs and screen-reader behavior. These are not reported as runtime passes.

![Home in the installed release](images/storage-overview.png)

![PDF preview in the release](images/pdf-preview.png)

Microsoft references: [PDF rendering](https://learn.microsoft.com/en-us/uwp/api/windows.data.pdf.pdfdocument.loadfromstreamasync)
and [bitmap transforms](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.imaging.bitmaptransform).
