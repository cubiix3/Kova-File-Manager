use kova_core::domain::{
    DirectorySnapshot, FileEntry, Location, SelectionState, SortColumn, SortDescriptor,
    SortDirection, TabCollection, TabId,
};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
use kova_core::domain::{FileKind, FileMetadata};

/// UI-facing representation of a single file list row.
#[derive(Debug, Clone, Default)]
pub struct FileListItem {
    pub name: String,
    pub type_name: String,
    pub size_text: String,
    pub modified_text: String,
    pub icon_id: i32,
    pub is_dir: bool,
    pub selected: bool,
}

/// Application controller that keeps the UI state in sync with the core
/// domain. All mutation happens on the main thread; filesystem I/O is
/// delegated to the worker.
pub struct AppController {
    tabs: TabCollection,
    snapshots: HashMap<TabId, DirectorySnapshot>,
    excluded: HashMap<TabId, Vec<FileEntry>>,
    pub show_hidden: bool,
    pub show_system: bool,
    pub show_extensions: bool,
    pub folder_sizes_enabled: bool,
    pub folder_sizes: HashMap<std::path::PathBuf, (Option<u64>, String)>,
    request_ids: HashMap<TabId, u64>,
    status_text: String,
    pending: HashSet<TabId>,
    errors: HashMap<TabId, String>,
}

impl AppController {
    pub fn new(initial: Location) -> Self {
        Self {
            tabs: TabCollection::new(initial),
            snapshots: HashMap::new(),
            excluded: HashMap::new(),
            show_hidden: false,
            show_system: false,
            show_extensions: true,
            folder_sizes_enabled: false,
            folder_sizes: HashMap::new(),
            request_ids: HashMap::new(),
            status_text: "Ready".into(),
            pending: HashSet::new(),
            errors: HashMap::new(),
        }
    }

    pub fn active_tab_id(&self) -> TabId {
        self.tabs.active_id()
    }

    pub fn active_tab_index(&self) -> usize {
        let active = self.tabs.active_id();
        self.tabs
            .tabs()
            .iter()
            .position(|t| t.id == active)
            .unwrap_or(0)
    }

    pub fn current_location(&self) -> Option<&Location> {
        self.tabs.active().and_then(|t| t.history.current())
    }

    pub fn current_directory(&self) -> Option<&Location> {
        self.current_location()
            .filter(|location| !location.is_home())
    }

    pub fn tab_locations(&self) -> Vec<(TabId, Location)> {
        self.tabs
            .tabs()
            .iter()
            .filter_map(|t| t.current_location().cloned().map(|l| (t.id, l)))
            .collect()
    }

    pub fn can_go_back(&self) -> bool {
        self.tabs
            .active()
            .map(|t| t.history.can_go_back())
            .unwrap_or(false)
    }

    pub fn can_go_forward(&self) -> bool {
        self.tabs
            .active()
            .map(|t| t.history.can_go_forward())
            .unwrap_or(false)
    }

    pub fn can_go_parent(&self) -> bool {
        self.current_location().and_then(|l| l.parent()).is_some()
    }

    pub fn address_path(&self) -> String {
        self.current_location()
            .map(|l| l.display())
            .unwrap_or_default()
    }

    pub fn status_text(&self) -> String {
        if self.is_loading() {
            return "Loading…".into();
        }
        if !self.directory_error().is_empty() {
            return "Folder unavailable".into();
        }
        self.status_text.clone()
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status_text = text.into();
    }

    /// True while an enumeration for the active tab is in flight.
    pub fn is_loading(&self) -> bool {
        self.pending.contains(&self.active_tab_id())
    }

    pub fn directory_error(&self) -> String {
        self.errors
            .get(&self.active_tab_id())
            .cloned()
            .unwrap_or_default()
    }

    pub fn apply_error(&mut self, tab_id: TabId, request_id: u64, message: String) {
        if !self.is_current_request(tab_id, request_id) {
            return;
        }
        self.pending.remove(&tab_id);
        self.snapshots.remove(&tab_id);
        self.excluded.remove(&tab_id);
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.selection.clear();
        }
        self.errors.insert(tab_id, message);
    }

    pub fn tab_labels(&self) -> Vec<String> {
        self.tabs
            .tabs()
            .iter()
            .map(|t| {
                t.current_location()
                    .map(|l| {
                        if l.is_home() {
                            return "Home".into();
                        }
                        l.path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| l.display())
                    })
                    .unwrap_or_else(|| t.label.clone())
            })
            .collect()
    }

    /// Number of entries in the active tab's snapshot.
    pub fn item_count(&self) -> usize {
        self.snapshot().map(|s| s.entries.len()).unwrap_or(0)
    }

    pub fn filtered_count(&self) -> usize {
        self.excluded.get(&self.active_tab_id()).map_or(0, Vec::len)
    }

    /// Number of selected rows in the active tab.
    pub fn selected_count(&self) -> usize {
        self.tabs.active().map(|t| t.selection.count()).unwrap_or(0)
    }

    pub fn selected_indices(&self) -> HashSet<usize> {
        self.tabs
            .active()
            .map(|t| t.selection.selected().iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn needs_enumeration(&self) -> bool {
        !self.pending.contains(&self.active_tab_id())
            && self.snapshot().is_none()
            && self.directory_error().is_empty()
    }

    pub fn apply_snapshot(&mut self, tab_id: TabId, snapshot: DirectorySnapshot) {
        if !self.is_current_request(tab_id, snapshot.request_id) {
            return;
        }

        let tab = match self.tabs.get_mut(tab_id) {
            Some(t) => t,
            None => return,
        };

        let old_paths: Vec<_> = self
            .snapshots
            .get(&tab_id)
            .map(|s| s.entries.iter().map(|e| e.path.clone()).collect())
            .unwrap_or_default();
        let (mut entries, excluded) =
            partition_visible(snapshot.entries, self.show_hidden, self.show_system);
        self.excluded.insert(tab_id, excluded);
        kova_core::domain::sort_entries(&mut entries, tab.sort);
        let new_indices: HashMap<_, _> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.clone(), i))
            .collect();
        tab.selection
            .remap(|i| old_paths.get(i).and_then(|p| new_indices.get(p).copied()));

        let snap = DirectorySnapshot {
            location: snapshot.location,
            request_id: snapshot.request_id,
            entries,
        };
        self.snapshots.insert(tab_id, snap);
        self.pending.remove(&tab_id);
        self.errors.remove(&tab_id);
        if tab_id == self.active_tab_id() {
            self.status_text = "Ready".into();
        }
    }

    pub fn is_current_request(&self, tab_id: TabId, request_id: u64) -> bool {
        self.request_ids
            .get(&tab_id)
            .map(|id| *id == request_id)
            .unwrap_or(false)
    }

    pub fn record_request(&mut self, tab_id: TabId, request_id: u64) {
        self.request_ids.insert(tab_id, request_id);
        self.pending.insert(tab_id);
        self.errors.remove(&tab_id);
        if self.snapshots.get(&tab_id).is_some_and(|s| {
            self.tabs.get(tab_id).and_then(|t| t.current_location()) != Some(&s.location)
        }) {
            self.snapshots.remove(&tab_id);
            self.excluded.remove(&tab_id);
        }
    }

    pub fn navigate(&mut self, location: Location) {
        if let Some(tab) = self.tabs.active_mut() {
            tab.history.navigate(location);
            // Entering a different directory invalidates index-based
            // selection state (Explorer behavior).
            tab.selection.clear();
        }
    }

    pub fn back(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        let location = tab.history.back();
        if location.is_some() {
            tab.selection.clear();
        }
        location
    }

    pub fn forward(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        let location = tab.history.forward();
        if location.is_some() {
            tab.selection.clear();
        }
        location
    }

    pub fn parent(&self) -> Option<Location> {
        let tab = self.tabs.active()?;
        let current = tab.history.current()?.clone();
        current.parent()
    }

    pub fn refresh_current(&self) -> Option<Location> {
        self.current_location().cloned()
    }

    pub fn new_tab(&mut self, initial: Location) -> TabId {
        self.tabs.create(initial.clone())
    }

    pub fn close_tab(&mut self, index: usize) -> Option<TabId> {
        let id = self.tabs.tabs().get(index)?.id;
        let active = self.tabs.close(id)?;
        self.snapshots.remove(&id);
        self.excluded.remove(&id);
        self.request_ids.remove(&id);
        self.pending.remove(&id);
        self.errors.remove(&id);
        Some(active)
    }

    pub fn switch_tab(&mut self, index: usize) -> bool {
        let id = match self.tabs.tabs().get(index) {
            Some(t) => t.id,
            None => return false,
        };
        self.tabs.switch_to(id)
    }

    /// Full paths of all selected rows in the active tab, in selection order.
    pub fn selected_paths(&self) -> Vec<std::path::PathBuf> {
        let Some(tab) = self.tabs.active() else {
            return Vec::new();
        };
        let Some(snapshot) = self.snapshots.get(&tab.id) else {
            return Vec::new();
        };
        tab.selection
            .selected()
            .iter()
            .filter_map(|&idx| snapshot.entries.get(idx))
            .map(|e| e.path.clone())
            .collect()
    }

    /// Path of the entry at `index` in the active tab's snapshot.
    pub fn path_at(&self, index: usize) -> Option<std::path::PathBuf> {
        self.snapshot()
            .and_then(|s| s.entries.get(index))
            .map(|e| e.path.clone())
    }

    pub fn selection_mut(&mut self) -> Option<&mut SelectionState> {
        self.tabs.active_mut().map(|t| &mut t.selection)
    }

    pub fn snapshot(&self) -> Option<&DirectorySnapshot> {
        self.snapshots.get(&self.tabs.active_id())
    }

    pub fn snapshots_mut(&mut self) -> impl Iterator<Item = &mut DirectorySnapshot> {
        self.snapshots.values_mut()
    }

    pub fn sort_descriptor(&self) -> SortDescriptor {
        self.tabs
            .active()
            .map(|t| t.sort)
            .unwrap_or_else(SortDescriptor::by_name)
    }

    pub fn primary_selection(&self) -> Option<usize> {
        self.tabs.active().and_then(|t| t.selection.primary())
    }

    pub fn file_list_items(&self) -> Vec<FileListItem> {
        let Some(snapshot) = self.snapshots.get(&self.tabs.active_id()) else {
            return Vec::new();
        };
        let selection: HashSet<usize> = match self.tabs.active() {
            Some(t) => t.selection.selected().iter().copied().collect(),
            None => return Vec::new(),
        };

        snapshot
            .entries
            .iter()
            .enumerate()
            .map(|(idx, e)| FileListItem {
                name: if self.show_extensions || e.is_directory() {
                    e.name.clone()
                } else {
                    std::path::Path::new(&e.name)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                },
                type_name: kind_text(e),
                size_text: if e.is_directory() && self.folder_sizes_enabled {
                    self.folder_sizes
                        .get(&e.path)
                        .map(|(_, label)| label.clone())
                        .unwrap_or_else(|| "…".into())
                } else {
                    size_text(e)
                },
                modified_text: modified_text(e),
                icon_id: effective_icon_id(e),
                is_dir: e.is_directory(),
                selected: selection.contains(&idx),
            })
            .collect()
    }

    pub fn set_sort(&mut self, column: SortColumn) {
        let Some(tab) = self.tabs.active_mut() else {
            return;
        };
        tab.sort = if tab.sort.column == column {
            SortDescriptor::new(column, tab.sort.direction.toggle())
        } else {
            SortDescriptor::new(column, SortDirection::Ascending)
        };

        // Re-sort the currently cached snapshot.
        let id = tab.id;
        if let Some(snap) = self.snapshots.get_mut(&id) {
            let old_paths: Vec<_> = snap.entries.iter().map(|e| e.path.clone()).collect();
            kova_core::domain::sort_entries(&mut snap.entries, tab.sort);
            if self.folder_sizes_enabled && column == SortColumn::Size {
                let folders = snap.entries.iter().take_while(|e| e.is_directory()).count();
                snap.entries[..folders].sort_by(|a, b| {
                    let size = |e: &FileEntry| {
                        self.folder_sizes.get(&e.path).and_then(|(bytes, _)| *bytes)
                    };
                    let order = size(a)
                        .cmp(&size(b))
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    if tab.sort.direction == SortDirection::Ascending {
                        order
                    } else {
                        order.reverse()
                    }
                });
            }
            let indices: HashMap<_, _> = snap
                .entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.path.clone(), i))
                .collect();
            tab.selection
                .remap(|i| old_paths.get(i).and_then(|p| indices.get(p).copied()));
        }
    }

    /// Refilter cached entries and remap selection by path, so hidden items
    /// cannot accidentally become targets of a later file operation.
    pub fn set_visibility(&mut self, hidden: bool, system: bool) {
        self.show_hidden = hidden;
        self.show_system = system;
        for (id, snapshot) in &mut self.snapshots {
            let Some(tab) = self.tabs.get_mut(*id) else {
                continue;
            };
            let old_paths: Vec<_> = snapshot.entries.iter().map(|e| e.path.clone()).collect();
            let mut all = std::mem::take(&mut snapshot.entries);
            all.extend(self.excluded.remove(id).unwrap_or_default());
            let (mut visible, excluded) = partition_visible(all, hidden, system);
            kova_core::domain::sort_entries(&mut visible, tab.sort);
            let indices: HashMap<_, _> = visible
                .iter()
                .enumerate()
                .map(|(i, e)| (e.path.clone(), i))
                .collect();
            tab.selection
                .remap(|i| old_paths.get(i).and_then(|p| indices.get(p).copied()));
            snapshot.entries = visible;
            self.excluded.insert(*id, excluded);
        }
    }
}

fn partition_visible(
    entries: Vec<FileEntry>,
    hidden: bool,
    system: bool,
) -> (Vec<FileEntry>, Vec<FileEntry>) {
    entries
        .into_iter()
        .partition(|e| (!e.metadata.is_hidden || hidden) && (!e.metadata.is_system || system))
}

/// Icon id for a row: the resolved shell icon when present, otherwise the
/// generic kind icon (pre-seeded in the UI icon store).
fn effective_icon_id(entry: &FileEntry) -> i32 {
    entry
        .icon_handle
        .map(|h| h.0 as i32)
        .unwrap_or_else(|| generic_icon_id(entry))
}

fn generic_icon_id(entry: &FileEntry) -> i32 {
    if entry.is_directory() {
        0 // GenericIcon::Folder
    } else {
        1 // GenericIcon::File
    }
}

fn kind_text(entry: &FileEntry) -> String {
    if entry.is_directory() {
        "Folder".into()
    } else {
        let ext = entry.extension_lower();
        if ext.is_empty() {
            "File".into()
        } else {
            format!("{} file", ext.to_uppercase())
        }
    }
}

fn size_text(entry: &FileEntry) -> String {
    match entry.metadata.size {
        Some(bytes) if bytes < 1024 => format!("{} B", bytes),
        Some(bytes) if bytes < 1024 * 1024 => format!("{:.1} KB", bytes as f64 / 1024.0),
        Some(bytes) if bytes < 1024 * 1024 * 1024 => {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        }
        Some(bytes) => format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0)),
        None => String::new(),
    }
}

fn modified_text(entry: &FileEntry) -> String {
    entry
        .metadata
        .modified
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
fn dummy_snapshot(request_id: u64, name: &str) -> DirectorySnapshot {
    DirectorySnapshot {
        location: Location::new(std::path::PathBuf::from("C:\\dummy")),
        request_id,
        entries: vec![FileEntry {
            name: name.into(),
            path: std::path::PathBuf::from("C:\\dummy").join(name),
            kind: FileKind::Directory,
            metadata: FileMetadata {
                size: None,
                modified: None,
                is_hidden: false,
                is_system: false,
                raw_attributes: 0,
            },
            icon_handle: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_a_history_location_without_a_filesystem_target() {
        let mut ctrl = AppController::new(Location::home());
        assert_eq!(ctrl.tab_labels(), ["Home"]);
        assert!(ctrl.current_directory().is_none());
        assert!(!ctrl.can_go_parent());
        ctrl.navigate(Location::new("C:\\dummy".into()));
        assert!(ctrl.current_directory().is_some());
        assert!(ctrl.back().unwrap().is_home());
        assert!(ctrl.current_directory().is_none());
        assert_eq!(
            ctrl.forward().unwrap().path,
            std::path::PathBuf::from("C:\\dummy")
        );
    }

    #[test]
    fn visibility_remaps_selection_and_keeps_hidden_entries_available() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let id = ctrl.active_tab_id();
        let mut snap = dummy_snapshot(1, "visible");
        let mut hidden = dummy_snapshot(1, "hidden").entries.remove(0);
        hidden.metadata.is_hidden = true;
        let mut system = dummy_snapshot(1, "system").entries.remove(0);
        system.metadata.is_system = true;
        snap.entries.extend([hidden, system]);
        ctrl.record_request(id, 1);
        ctrl.apply_snapshot(id, snap);
        assert_eq!(ctrl.item_count(), 1);
        ctrl.selection_mut().unwrap().select_single(0);
        let selected = ctrl.selected_paths();
        ctrl.set_visibility(true, true);
        assert_eq!(ctrl.item_count(), 3);
        assert_eq!(ctrl.selected_paths(), selected);
        ctrl.selection_mut().unwrap().select_all(3);
        ctrl.set_visibility(false, false);
        assert_eq!(ctrl.selected_paths(), selected);
        assert_eq!(ctrl.selected_count(), 1);
        ctrl.set_visibility(true, false);
        assert_eq!(ctrl.item_count(), 2);
        assert!(
            ctrl.snapshot()
                .unwrap()
                .entries
                .iter()
                .all(|e| !e.metadata.is_system)
        );
    }

    #[test]
    fn hiding_extensions_does_not_change_operation_paths_or_folder_names() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let id = ctrl.active_tab_id();
        let mut snap = dummy_snapshot(1, "archive.txt");
        snap.entries[0].kind = FileKind::File;
        snap.entries
            .extend(dummy_snapshot(1, "folder.name").entries);
        ctrl.record_request(id, 1);
        ctrl.apply_snapshot(id, snap);
        ctrl.show_extensions = false;
        assert_eq!(ctrl.file_list_items()[0].name, "folder.name");
        assert_eq!(ctrl.file_list_items()[1].name, "archive");
        assert!(ctrl.path_at(1).unwrap().ends_with("archive.txt"));
    }

    #[test]
    fn sort_and_refresh_preserve_selected_file_identity() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let tab = ctrl.active_tab_id();
        let mut snap = dummy_snapshot(1, "b");
        snap.entries.extend(dummy_snapshot(1, "a").entries);
        ctrl.record_request(tab, 1);
        ctrl.apply_snapshot(tab, snap.clone());
        ctrl.selection_mut().unwrap().select_single(0);
        let selected = ctrl.selected_paths();
        ctrl.set_sort(SortColumn::Name);
        assert_eq!(ctrl.selected_paths(), selected);
        snap.request_id = 2;
        snap.entries.extend(dummy_snapshot(2, "c").entries);
        ctrl.record_request(tab, 2);
        ctrl.apply_snapshot(tab, snap);
        assert_eq!(ctrl.selected_paths(), selected);
    }

    #[test]
    fn stale_errors_and_closed_tab_results_cannot_replace_current_state() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let first = ctrl.active_tab_id();
        ctrl.record_request(first, 2);
        ctrl.apply_snapshot(first, dummy_snapshot(2, "current"));
        ctrl.apply_error(first, 1, "stale failure".into());
        assert!(ctrl.directory_error().is_empty());
        assert_eq!(ctrl.item_count(), 1);
        let second = ctrl.new_tab(Location::new("C:\\other".into()));
        ctrl.record_request(second, 1);
        ctrl.close_tab(1);
        assert!(!ctrl.is_current_request(second, 1));
        ctrl.apply_snapshot(second, dummy_snapshot(1, "closed"));
        assert_eq!(ctrl.snapshots.len(), 1);
    }

    #[test]
    fn background_results_and_loading_belong_to_their_tab() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let first = ctrl.active_tab_id();
        ctrl.record_request(first, 1);
        let second = ctrl.new_tab(Location::new("C:\\other".into()));
        ctrl.record_request(second, 1);
        ctrl.apply_snapshot(first, dummy_snapshot(1, "first"));
        assert!(ctrl.is_loading());
        ctrl.switch_tab(0);
        assert!(!ctrl.is_loading());
        assert_eq!(ctrl.item_count(), 1);
        ctrl.navigate(Location::new("C:\\next".into()));
        ctrl.record_request(first, 2);
        assert_eq!(ctrl.item_count(), 0);
        assert_eq!(ctrl.tab_labels()[0], "next");
        ctrl.apply_error(first, 2, "Access denied".into());
        assert_eq!(ctrl.directory_error(), "Access denied");
        assert!(!ctrl.is_loading());
        ctrl.back();
        assert_eq!(ctrl.tab_labels()[0], "dummy");
    }

    #[test]
    fn stale_snapshot_is_rejected_after_newer_request() {
        let initial = Location::new(std::path::PathBuf::from("C:\\dummy"));
        let mut ctrl = AppController::new(initial);
        let tab_id = ctrl.active_tab_id();

        // Simulate request #1, then request #2 for the same tab.
        ctrl.record_request(tab_id, 1);
        ctrl.record_request(tab_id, 2);

        // Snapshot from request #1 must be ignored.
        let stale = dummy_snapshot(1, "stale-folder");
        ctrl.apply_snapshot(tab_id, stale);
        assert!(ctrl.snapshot().is_none());

        // Snapshot from request #2 must be applied.
        let current = dummy_snapshot(2, "current-folder");
        ctrl.apply_snapshot(tab_id, current);
        let snap = ctrl.snapshot().unwrap();
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.entries[0].name, "current-folder");
    }

    #[test]
    fn out_of_order_results_keep_latest_navigation_visible() {
        // Scenario: navigate A -> request 1, navigate B -> request 2.
        // B completes, then A completes. The visible snapshot must remain B.
        let initial = Location::new(std::path::PathBuf::from("C:\\dummy"));
        let mut ctrl = AppController::new(initial);
        let tab_id = ctrl.active_tab_id();

        ctrl.record_request(tab_id, 1);
        ctrl.record_request(tab_id, 2);

        let b = dummy_snapshot(2, "current-b");
        ctrl.apply_snapshot(tab_id, b);

        let a = dummy_snapshot(1, "stale-a");
        ctrl.apply_snapshot(tab_id, a);

        let snap = ctrl.snapshot().unwrap();
        assert_eq!(snap.entries[0].name, "current-b");
    }

    #[test]
    fn item_and_selection_counts_track_active_tab() {
        let initial = Location::new(std::path::PathBuf::from("C:\\dummy"));
        let mut ctrl = AppController::new(initial);
        let tab_id = ctrl.active_tab_id();

        assert_eq!(ctrl.item_count(), 0);
        assert_eq!(ctrl.selected_count(), 0);

        ctrl.record_request(tab_id, 1);
        ctrl.apply_snapshot(tab_id, dummy_snapshot(1, "FolderA"));
        assert_eq!(ctrl.item_count(), 1);

        if let Some(sel) = ctrl.selection_mut() {
            sel.select_single(0);
        }
        assert_eq!(ctrl.selected_count(), 1);

        // Second tab starts with its own empty snapshot.
        let second = ctrl.new_tab(ctrl.current_location().cloned().unwrap());
        assert_eq!(ctrl.item_count(), 0, "new tab has no snapshot yet");
        assert_eq!(ctrl.selected_count(), 0);

        ctrl.switch_tab(0);
        let _ = second;
        assert_eq!(ctrl.item_count(), 1);
        assert_eq!(ctrl.selected_count(), 1);
    }

    #[test]
    fn selected_paths_follow_selection_order() {
        let initial = Location::new(std::path::PathBuf::from("C:\\dummy"));
        let mut ctrl = AppController::new(initial);
        let tab_id = ctrl.active_tab_id();

        ctrl.record_request(tab_id, 1);
        ctrl.apply_snapshot(
            tab_id,
            DirectorySnapshot {
                location: Location::new(std::path::PathBuf::from("C:\\dummy")),
                request_id: 1,
                entries: vec![
                    dummy_snapshot(1, "one").entries.remove(0),
                    dummy_snapshot(1, "two").entries.remove(0),
                    dummy_snapshot(1, "three").entries.remove(0),
                ],
            },
        );

        if let Some(sel) = ctrl.selection_mut() {
            sel.select_single(2);
            sel.toggle(0);
        }
        let paths = ctrl.selected_paths();
        assert_eq!(paths.len(), 2);
        // The snapshot is re-sorted by name ("one", "three", "two"), so index
        // 2 is "two" and index 0 is "one".
        assert!(paths[0].ends_with("two"));
        assert!(paths[1].ends_with("one"));
        assert!(ctrl.path_at(1).unwrap().ends_with("three"));
    }

    #[test]
    fn file_list_rows_expose_generic_icon_and_dir_flag() {
        let initial = Location::new(std::path::PathBuf::from("C:\\dummy"));
        let mut ctrl = AppController::new(initial);
        let tab_id = ctrl.active_tab_id();

        ctrl.record_request(tab_id, 1);
        ctrl.apply_snapshot(tab_id, dummy_snapshot(1, "FolderA"));

        let rows = ctrl.file_list_items();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_dir);
        assert_eq!(
            rows[0].icon_id, 0,
            "folder rows fall back to the folder icon"
        );

        // A resolved shell icon must win over the generic fallback.
        ctrl.snapshots_mut().next().unwrap().entries[0].icon_handle =
            Some(kova_core::domain::IconHandle(9));
        let rows = ctrl.file_list_items();
        assert_eq!(rows[0].icon_id, 9);
    }
}
