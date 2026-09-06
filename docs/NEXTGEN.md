# Working with Kova

These features describe the current source, after v0.1.0. Kova remains a native
Rust/Slint Windows application; no Spacedrive or File Browser code was imported.

## Views and inspector

Use the Details/Gallery buttons or **View**. **View > Thumbnail size** offers
Small, Medium and Large. Both views share the same directory snapshot and
selection. Switching views maps the first visible row to its corresponding
gallery row; exact pixel offsets can differ after resizing or changing tile size.
Ctrl/Shift selection, arrow keys, Home/End and rectangle selection work in both.
Sorting remains available in the gallery header. Ctrl+T creates a Home tab,
Ctrl+W closes the current tab, and Ctrl+Enter opens a selected folder in a new tab.

**Space** toggles the inspector. Images, bounded text and PDF pages use the
existing decoders; GIF, WebP and APNG retain playback controls. Other files use
Windows thumbnail providers or a large Shell icon. Created/modified dates, path,
extension and size appear below. Dimensions and duration depend on Windows
property handlers. Video playback is not included; video thumbnails are supported
when Windows has a provider. No claim is made that all codecs are available.

Thumbnail work runs off the UI thread, only for the visible range plus a margin.
The in-memory thumbnail cache is bounded to 256 entries. Navigation invalidates
stale work. Automatic previews avoid network locations and observed cloud
placeholders/reparse points. A slow third-party Shell handler cannot be forcibly
interrupted; stale results are discarded when it returns.

## Search

**Ctrl+F** focuses the current-folder filter. Typing filters the cached snapshot
without enumerating the directory again. Names are case-insensitive. Examples:

| Query | Meaning |
| --- | --- |
| `holiday` | Name contains holiday |
| `type:image` | Images |
| `type:video` / `type:audio` | Video / audio extensions |
| `type:document` / `type:archive` / `type:folder` | File groups / folders |
| `ext:png` | PNG extension |
| `size:>100MB` | Larger than 100 MiB |
| `size:<1GB modified:this-week` | Both conditions |
| `modified:today` / `modified:this-month` | Local calendar boundaries |
| `modified:2026-09-01` | Modified on or after that date |

Filters combine with name terms. Multiple types/extensions are alternatives;
multiple size bounds must all match. Folder sizes are unknown unless calculated.
Global search is not implemented. The toolkit-independent query matcher can be
reused by a future indexed provider without coupling the UI to MFT/USN traversal.

## Transfers and conflicts

Copy, cut/paste and deletion use Windows **IFileOperation**, on a dedicated STA
worker. **Transfers** in the status bar opens the queue and recent history.
It shows selected sources, destination, current item, progress, completed bytes
and files, remaining items in the current native stage, and errors. Windows progress units are not
assumed to be bytes; byte totals advance when file completion is reported.
Small operations do not open the panel automatically.

**Cancel** requests cancellation through the native progress sink. Completed work
is retained; cancellation is not rollback. Kova keeps its window open while an
operation is active, so it cannot terminate its own transfer worker mid-operation.
Let transfers finish or cancel them before closing.

File-to-file collisions show Existing and Incoming paths, sizes and dates before
any transfer starts. Choose **Replace**, **Skip**, or **Keep Both**, optionally
for all file conflicts in that operation. Keep Both selects an unused name such
as `photo (1).jpg`. Replace confirmation applies only to those colliding files.
Folder merges and exceptional Shell objects retain Windows conflict dialogs.
Races discovered by Windows after preparation use its native handling.
Deletion goes through the Recycle Bin where supported; Windows retains permanent
deletion warnings on volumes that cannot recycle. Operations invoked directly by
third-party Shell menu extensions retain their own UI and are not queue entries.

## Storage

**Home** compares local fixed/removable/RAM drives, file systems and capacity.
Refresh it with F5 after connecting a device. Network/CD drives are not listed;
explicit paths can still be navigated. Unavailable capacity is labeled accordingly.

Select a folder, or open one, then choose **View > Analyze storage**. Drive rows
also offer Analyze storage. Analysis runs in the background and reports logical
file bytes, the 30 largest immediate subfolders and 30 largest files across the
tree. It skips observed reparse/offline entries, records skipped/inaccessible
items, bounds the pending-directory queue and supports cancellation. These are
logical sizes, not physical allocation; hard links may be counted more than once.

## Tags, collections and Quick access

Select files and open **Organize**. Enter a name to create/add a collection or
apply a tag. Existing groups provide **Add**, **Open** and **Remove**. Groups
contain references only, including files from different drives. Removing a group
never deletes its files. In a group, **Remove selected references** removes only
membership. New Folder and Paste are disabled in these virtual locations.

Kova's confirmed renames/moves update references. Files moved externally remain
as unavailable references until removed or added from their new location; no
background global identity index is implied. Missing entries stay visible as
**Unavailable**. Operations on them report errors rather than acting elsewhere.

Pin the current/selected folder from Organize or View. Right-click a pin to remove
it or move it up/down. Pins, tags and collections are stored atomically in
`%LOCALAPPDATA%\Kova\library.json`. Corrupt/unreadable storage is left intact and
organization becomes read-only with an error. No account or network service is used.

Concurrent Kova windows cannot silently overwrite each other's library changes:
saving checks the loaded version under an exclusive file lock. If another window
has changed the library, its saved data is preserved and the stale window asks
you to restart before editing organization again. An unchanged window does not
save its old library snapshot on exit.

## Automatic updates

Open folders listen for native Windows file notifications, including changes
made by other applications. Short bursts are coalesced before a background
refresh; selection and search remain attached to file paths. Refresh is deferred
while an inline name is being edited. Recursive notifications also reconcile
changes inside displayed folders. Unsupported or temporarily unavailable paths
are retried in the background. This is not a hard real-time guarantee for network
providers or an automatic device-discovery service; Home still has manual refresh.
