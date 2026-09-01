use kova_core::domain::{
    DirectorySnapshot, FileEntry, Location, SelectionState, SortColumn, SortDescriptor,
    SortDirection, TabCollection, TabId,
};
use std::collections::HashMap;

/// UI-facing representation of a single file list row.
#[derive(Debug, Clone, Default)]
pub struct FileListItem {
    pub name: String,
    pub type_name: String,
    pub size_text: String,
    pub modified_text: String,
    pub icon_id: i32,
    pub selected: bool,
}

/// Application controller that keeps the UI state in sync with the core
/// domain. All mutation happens on the main thread; filesystem I/O is
/// delegated to the worker.
pub struct AppController {
    tabs: TabCollection,
    snapshots: HashMap<TabId, DirectorySnapshot>,
    request_ids: HashMap<TabId, u64>,
    status_text: String,
}

impl AppController {
    pub fn new(initial: Location) -> Self {
        Self {
            tabs: TabCollection::new(initial),
            snapshots: HashMap::new(),
            request_ids: HashMap::new(),
            status_text: "Ready".into(),
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
        self.status_text.clone()
    }

    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status_text = text.into();
    }

    pub fn tab_labels(&self) -> Vec<String> {
        self.tabs.tabs().iter().map(|t| t.label.clone()).collect()
    }

    pub fn apply_snapshot(&mut self, tab_id: TabId, snapshot: DirectorySnapshot) {
        let tab = match self.tabs.get_mut(tab_id) {
            Some(t) => t,
            None => return,
        };

        let mut entries = snapshot.entries.clone();
        kova_core::domain::sort_entries(&mut entries, tab.sort);

        let snap = DirectorySnapshot {
            location: snapshot.location,
            request_id: snapshot.request_id,
            entries,
        };
        self.snapshots.insert(tab_id, snap);
        let count = snapshot.entries.len();
        self.status_text = format!("{} items", count);
    }

    pub fn is_current_request(&self, tab_id: TabId, request_id: u64) -> bool {
        self.request_ids
            .get(&tab_id)
            .map(|&id| id == request_id)
            .unwrap_or(true)
    }

    pub fn record_request(&mut self, tab_id: TabId, request_id: u64) {
        self.request_ids.insert(tab_id, request_id);
    }

    pub fn navigate(&mut self, location: Location) {
        if let Some(tab) = self.tabs.active_mut() {
            tab.history.navigate(location);
        }
    }

    pub fn back(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        tab.history.back()
    }

    pub fn forward(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        tab.history.forward()
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
        self.tabs.close(id)
    }

    pub fn switch_tab(&mut self, index: usize) -> bool {
        let id = match self.tabs.tabs().get(index) {
            Some(t) => t.id,
            None => return false,
        };
        self.tabs.switch_to(id)
    }

    pub fn selection_mut(&mut self) -> Option<&mut SelectionState> {
        self.tabs.active_mut().map(|t| &mut t.selection)
    }

    pub fn snapshot(&self) -> Option<&DirectorySnapshot> {
        self.snapshots.get(&self.tabs.active_id())
    }

    pub fn file_list_items(&self) -> Vec<FileListItem> {
        let Some(snapshot) = self.snapshots.get(&self.tabs.active_id()) else {
            return Vec::new();
        };
        let selection = match self.tabs.active() {
            Some(t) => &t.selection,
            None => return Vec::new(),
        };

        snapshot
            .entries
            .iter()
            .enumerate()
            .map(|(idx, e)| FileListItem {
                name: e.name.clone(),
                type_name: kind_text(e),
                size_text: size_text(e),
                modified_text: modified_text(e),
                icon_id: e.icon_handle.map(|h| h.0 as i32).unwrap_or(-1),
                selected: selection.is_selected(idx),
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
            kova_core::domain::sort_entries(&mut snap.entries, tab.sort);
        }
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
