# Kova Research Reference — Files

Current follow-up: Kova now adapts specific MIT-marked vector geometry from
Files' `Icons.Common.xaml`, pinned to commit
`c6c05c6cc84137ed554c28b2b6521c3a9ae4b049`. See the current
[icon attribution](../../apps/kova-desktop/ui/third-party/README.md).
The older no-copy statements below describe the historical runtime rescue,
not the later icon adaptation. Upstream also contains other licenses and
third-party resources; the MIT label must not be assumed for every asset.

Source repository studied:

```text
https://github.com/files-community/Files
Cloned at: <local clone of the upstream repository>
License: MIT (see LICENSE-MIT in the upstream repository)
```

This document captures concrete source paths and ideas from Files that are relevant to the current Kova runtime/UI rescue. Kova remains an independent Rust/Slint project; no large code blocks are copied.

---

## 1. Pointer / focus / hit testing — why buttons feel alive

### Kova problem
All toolbar/sidebar/file-list buttons appear dead.

### Files reference
Files does **not** rely on a generic "everything is clickable" container. It explicitly wires pointer events per control and separates visual state from the command:

- `src/Files.App.Controls/Sidebar/SidebarItem.cs` lines ~390–470
  - `ItemBorder_PointerPressed` sets `isClicking = true` and immediately calls `UpdatePointerState(true)`.
  - `Item_PointerReleased` checks `isClicking`, resets state, then calls `Clicked(pointerUpdateKind)`.
  - `Clicked` invokes `Owner.RaiseItemInvoked(...)`.
  - Visual states (`Pressed`, `PointerOver`, `Normal`, `Selected`) are explicitly transitioned with `VisualStateManager.GoToState(...)`.

- `src/Files.App/Views/Shells/BaseShellPage.cs` line ~270
  - `PointerPressed += CoreWindow_PointerPressed;`
  - `CoreWindow_PointerPressed` handles mouse X-buttons and routes them to `Back_Click()` / `Forward_Click()`.

- `src/Files.App/UserControls/NavigationToolbar.xaml`
  - Every navigation button is a concrete `Button`/`ToggleButton` with a `Command` binding and explicit visual states, not a generic rectangle.

### Relevant idea
Pointer feedback needs three explicit pieces:
1. Pressed state while the pointer is down.
2. Released handler that decides whether to act (e.g., left release only).
3. Command / callback invocation from the released handler.

In Slint this maps to: use `TouchArea`/`FocusScope`, but the callback must be **non-pure** and the visual background must change with the button state (`touch-area.pressed` property). Slint's `TouchArea` already provides `clicked`, but we must ensure the Rust callback is actually invoked and has a visible pressed state.

### What Kova should adopt conceptually
- Remove `pure` from all UI callbacks that have side effects.
- Add explicit pressed/hover visual states to every button so the user sees immediate feedback even before the Rust side responds.
- Keep the command dispatcher simple and synchronous where possible; async work should happen after the click has been acknowledged.

### What we should NOT copy
- Do not copy the WinUI `VisualStateManager` XAML; Slint has its own state-driven styling.
- Do not copy the full `ItemManipulationModel` class yet; adopt the event-publisher idea only if needed for multi-selection.

---

## 2. Active tab / tab switching

### Kova problem
No active tab visible; tab switching does not work.

### Files reference
- `src/Files.App/UserControls/TabBar/BaseTabBar.cs` lines ~70–110
  - `TabView_SelectionChanged` updates `CurrentSelectedAppInstance` and raises `CurrentInstanceChanged`.
  - `CurrentInstanceChanged` listeners (in `BaseTabBar`) set `IsCurrentInstance = true/false` per tab content.

- `src/Files.App/Views/Shells/BaseShellPage.cs` lines ~100–150
  - `IsCurrentInstance` toggles background visual state, starts/stops timers, and notifies bindings.

- `src/Files.App/Helpers/Navigation/NavigationHelpers.cs` lines ~40–80
  - `AddNewTabByPathAsync` inserts the tab into `MainPageViewModel.AppInstances` and then sets `App.AppModel.TabStripSelectedIndex = index` to switch to it immediately.

### Relevant idea
Tab switching has two independent updates:
1. Update the model (`active_tab`).
2. Notify the view so it re-renders immediately, not only after a directory finishes loading.

### What Kova should adopt conceptually
- When the user clicks a tab, switch the active tab **now** and refresh the UI immediately with whatever snapshot is cached. Then request a re-enumeration in the background.
- When creating a new tab, make it active immediately and request enumeration.
- The active-tab highlight must not wait for the async filesystem call.

### What we should NOT copy
- Do not copy the full `TabBarItem` drag/drop or window-movement code.
- Do not copy `App.AppModel.TabStripSelectedIndex` global state; Kova can keep it in `AppState`.

---

## 3. Blank initial file list

### Kova problem
File list starts empty and may stay empty.

### Files reference
- `src/Files.App/ViewModels/ShellViewModel.cs` line ~2040
  - `RefreshItems(previousDir, postLoadCallback)` starts `RapidAddItemsToCollectionAsync`.
  - It clears the internal backing list but keeps the **displayed** list until the first batch arrives.
  - It raises `ItemLoadStatusChanged` with `Status = Starting`.

- `src/Files.App/Views/Layouts/BaseLayoutPage.cs` lines ~490–540
  - `OnNavigatedTo` calls `shellViewModel.RefreshItems(previousDir, SetSelectedItemsOnNavigation)` after `SetWorkingDirectoryAsync`.

- `src/Files.App/Data/Models/ItemManipulationModel.cs`
  - `FocusFileList`, `SetSelectedItem`, `ScrollIntoView`, etc. are publisher events invoked by the layout page after the directory loads.

### Relevant idea
The file list is populated by an explicit refresh cycle:
1. Navigate / change working directory.
2. Call `RefreshItems`.
3. Enumeration produces snapshots.
4. View layer applies the snapshot and updates selection/focus.

Crucially, Files shows a status/progress indicator and clears stale items only when new data arrives, never leaving the user with a permanently blank list because of a missed event.

### What Kova should adopt conceptually
- On startup, after requesting the initial enumeration, immediately call `update_ui` so the address bar, tabs, and empty file model are visible.
- If a `DirectoryLoaded` event is ignored because `is_current_request` is false, log it and surface a status message instead of silently leaving the list blank.
- Add a "Loading..." status text while enumeration is in flight.

### What we should NOT copy
- Do not copy the `BulkConcurrentObservableCollection` or cloud-drive checks; Kova can use a simple `VecModel` for now.

---

## 4. Navigation state (Back / Forward / Parent / Refresh)

### Kova problem
Toolbar navigation buttons do nothing.

### Files reference
- `src/Files.App/Views/Shells/BaseShellPage.cs` lines ~30–50
  - `CanNavigateForward => ItemDisplay.CanGoForward`
  - `CanNavigateBackward => ItemDisplay.CanGoBack`
  - `ForwardStack` / `BackwardStack` are the Frame navigation stacks.

- `src/Files.App/Views/Shells/ModernShellPage.xaml.cs` lines ~120–160
  - `Back_Click()` / `Forward_Click()` first disable the command (`ToolbarViewModel.CanGoBack = false`), check the stack, then call `base.Back_Click()`.
  - `ItemDisplayFrame.Navigate(...)` is the single source of truth for location changes.

- `src/Files.App/ViewModels/UserControls/NavigationToolbarViewModel.cs` lines ~150–170
  - `CanGoBack`, `CanGoForward`, `CanNavigateToParent`, `CanRefresh` are observable properties bound to the toolbar buttons.

### Relevant idea
Navigation state is derived from the model and surfaced as booleans. The buttons bind to these booleans, so they are visually disabled when the operation is impossible.

### What Kova should adopt conceptually
- Ensure `AppState.can_go_back`, `can_go_forward`, and `can_go_parent` are updated on **every** `update_ui` call.
- Make the Rust dispatcher validate the operation and return an error; surface the error in the status text so silent failures are visible.
- Refresh must re-request enumeration for the current location, not just re-sort.

### What we should NOT copy
- Do not copy the `Frame` page-navigation stack; Kova uses its own `NavigationHistory`.

---

## 5. Sidebar selection / drives

### Kova problem
Sidebar entries (Home, Desktop, Drives) do not navigate.

### Files reference
- `src/Files.App.Controls/Sidebar/SidebarView.xaml.cs` lines ~30–50
  - `RaiseItemInvoked(SidebarItem item, PointerUpdateKind pointerUpdateKind)`:
    - Sets `SelectedItem = item.Item` so the row highlights.
    - Raises `ItemInvoked` event.

- `src/Files.App.Controls/Sidebar/SidebarItem.cs` lines ~260–300
  - `ReevaluateSelection()` checks `Item == Owner.SelectedItem` and updates `IsSelected`.
  - Selection is visual state, not just a model flag.

- `src/Files.App/ViewModels/UserControls/SidebarViewModel.cs`
  - Provides `Favorites`, `Drives`, `Network` sections as observable collections.

### Relevant idea
Sidebar click handling has two responsibilities:
1. Mark the clicked item as selected (visual feedback).
2. Emit an event that the main shell page handles by navigating.

### What Kova should adopt conceptually
- Add a `selected` flag to each sidebar row and update it when clicked.
- Ensure the Rust callback is actually invoked (see point 1 about `pure` callbacks).
- Resolve known-folder paths robustly; if a folder cannot be resolved, show it disabled rather than making the button silently inactive.

### What we should NOT copy
- Do not copy the `SidebarViewModel.FlatTree.cs` tree-expansion logic for now; Kova only needs a flat favorites + drives list.

---

## 6. Selection and keyboard activation in the file list

### Kova problem
File list items may not activate on double-click or Enter.

### Files reference
- `src/Files.App/Data/Models/ItemManipulationModel.cs`
  - Events: `FocusFileListInvoked`, `AddSelectedItemInvoked`, `SetSelectedItem`, `ScrollIntoViewInvoked`, etc.

- `src/Files.App/Views/Layouts/BaseLayoutPage.cs` lines ~220–290
  - `SelectedItems` setter updates `IsItemSelected`, `SelectedItem`, and pushes selection to the toolbar.
  - After a single selection it schedules a rename-on-double-click guard.

### Relevant idea
Selection is a first-class state machine. Double-click/Enter activation is separate from single-click selection.

### What Kova should adopt conceptually
- Keep the existing `request_select`, `request_toggle`, `request_range`, `request_activate` callbacks but make them non-pure.
- Ensure `FocusScope` keyboard handling uses correct Slint key names (`Key.Return`, `Key.F5`, `Key.Left` with `event.modifiers.control`, not `event.text == "return"`).

### What we should NOT copy
- Do not copy the complex rename-on-slow-double-click logic yet.

---

## 7. Error visibility

### Kova problem
Operations may fail silently (e.g., navigation to a non-existent path).

### Files reference
- `src/Files.App/ViewModels/ShellViewModel.cs` lines ~300–340
  - `ShowLocationUnavailable(kind, message)` clears the file list, sets an info bar, and categorizes errors (`AccessDenied`, `NotFound`, `DriveUnplugged`, `PasswordRequired`).
  - `ShowLocationInaccessibleOrMissing(path)` distinguishes `Directory.Exists(path)` true (access denied) from false (not found).

### Relevant idea
Every failed operation must produce a visible status / info message. The UI must never swallow errors.

### What Kova should adopt conceptually
- In `dispatch_navigate`, when `resolve_input` fails or the path does not exist, set `status_text` and call `update_ui`.
- In the event pump, update `status_text` for `DirectoryError` even if the request is not current.

---

## 8. Kova action table

| Kova problem | How Files solves the equivalent | Kova action |
|-------------|--------------------------------|-------------|
| All buttons appear dead | Explicit pointer states + non-pure command callbacks bound to concrete controls | Remove `pure` from `AppState` callbacks; add pressed/hover visuals; surface dispatcher errors |
| Blank initial file list | `RefreshItems` cycle + progress status + immediate UI update on navigation | Call `update_ui` on startup; show "Loading..." status while enumerating; log stale-snapshot drops |
| Active tab not visible | Tab selection updated immediately, not after async load | Update `active_tab` and call `update_ui` immediately in `dispatch_switch_tab` / `dispatch_new_tab` |
| Sidebar drive selection | `RaiseItemInvoked` + `SelectedItem` visual state + shell navigation | Mark clicked sidebar row selected; ensure callback is non-pure |
| Back/Forward state | Observable `CanGoBack`/`CanGoForward` bound to toolbar | Re-derive these booleans on every `update_ui`; disable buttons visually |
| Pointer/Focus handling | `PointerPressed` at page level, explicit focus management | Use Slint `TouchArea` correctly; remove broken `FocusScope` key strings |
| Multi-selection | `ItemManipulationModel` event publisher | Keep existing selection code but ensure callbacks fire |

---

## 9. License note

Files is MIT-licensed. Concepts (callback wiring, selection state machine, navigation stack, sidebar item invocation) are adapted conceptually. No substantial code blocks are copied into Kova. Any future direct reuse of code from Files must retain the MIT copyright notice and document the source.

---

## 10. Runtime rescue findings (verified 2026-09)

These are the root causes found while rescuing the Kova desktop runtime,
with the Files concept that each fix corresponds to.

### R1. UI updates from a worker thread

Kova problem: `update_ui` was called from a tokio task. Slint property
access off the UI thread panics or silently drops, so tabs, address bar
and file model never rendered - the app looked like an empty prototype.

Files reference: all WPF/WinUI updates happen on the dispatcher thread
(`DispatcherQueue`, `CoreDispatcher`); background enumeration posts
results through observable collections bound on the UI thread.

Kova fix: worker events are forwarded into a `std::sync::mpsc` channel
that a `slint::Timer` drains on the UI thread (`UI-thread event pump`).

### R2. Percentages inside layouts resolve against the window

Kova problem: `width: 100%` on sidebar buttons inflated the sidebar's
preferred width to the full window, and the GridLayout content row
(height: 100% of the window) overflowed the window. The whole file list
was laid out past the right window edge; the status bar was below the
window. The list looked empty although the model had 97 entries.

Files reference: WinUI uses star-sizing (`Grid ColumnDefinition
Width="*"`) which is always relative to the layout container.

Kova fix: absolute window skeleton (tabs/toolbar/content/status placed
with x/y/width/height against `parent.width/height`), fixed pixel
columns, no `width: 100%` inside layouts, fixed heights for sidebar
section headers (Texts otherwise absorb all extra vertical space).

### R3. Recreating models breaks double-click

Kova problem: every selection click rebuilt the whole files model.
Slint recreated all row delegates, so the second click of a double-click
landed on a new `TouchArea` and `double-clicked` never fired - folders
could not be opened.

Files reference: `ItemManipulationModel` selection updates mutate
collection items in place instead of replacing the collection.

Kova fix: keep one `VecModel` per list installed for the app lifetime;
on update, compare rows and use `set_row_data` (identity preserved).

### R4. Keyboard focus after mouse clicks

Kova problem: F2/F5/Enter/Ctrl+A never fired because the list
`FocusScope` never had keyboard focus; row clicks did not move focus.

Files reference: `BaseLayoutPage` explicitly calls `FocusFileList`
after operations.

Kova fix: `forward-focus` on the window plus `list-scope.focus()` in
the row pointer handler, so keyboard shortcuts work after clicks.

### R5. Slint API verification process

The previous agent invented `event.key == Key.A` which does not exist
in Slint 1.13.1 (the project did not compile). Verified against the
registry sources: `KeyEvent { text, modifiers, repeat }`, the `Key`
namespace (Key.Return, Key.F5, Key.LeftArrow, ...), and that winit
removes Ctrl before computing `logical_key`, so Ctrl+A arrives as
`event.text == "a"` with `modifiers.control`. Slint's own widgets
(`combobox-base.slint`, `listview.slint`) use `event.text == Key.X`.
