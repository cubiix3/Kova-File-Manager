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
    sources: HashMap<TabId, std::sync::Arc<DirectorySnapshot>>,
    view_generations: HashMap<TabId, std::sync::Arc<std::sync::atomic::AtomicU64>>,
    view_worker: Option<crate::search::Worker>,
    view_pending: HashSet<TabId>,
    pub revision: u64,
    pub library: kova_core::domain::Library,
    pub(crate) tabs: TabCollection,
    snapshots: HashMap<TabId, std::sync::Arc<DirectorySnapshot>>,
    excluded: HashMap<TabId, Vec<FileEntry>>,
    pub(crate) searches: HashMap<TabId, String>,
    pub recent_folders: Vec<std::path::PathBuf>,
    pub filters: HashMap<TabId, [usize; 3]>,
    pub recursive_tabs: HashSet<TabId>,
    pub show_hidden: bool,
    pub show_system: bool,
    pub show_extensions: bool,
    pub folder_sizes_enabled: bool,
    pub folder_sizes: kova_core::domain::FolderSizes,
    request_ids: HashMap<TabId, u64>,
    completed_enumerations: HashMap<TabId, u64>,
    status_text: String,
    pending: HashSet<TabId>,
    background_pending: HashSet<TabId>,
    errors: HashMap<TabId, String>,
}

impl AppController {
    pub fn new(initial: Location) -> Self {
        Self {
            sources: HashMap::new(),
            view_generations: HashMap::new(),
            view_worker: None,
            view_pending: HashSet::new(),
            revision: 0,
            library: kova_core::domain::Library::default(),
            tabs: TabCollection::new(initial),
            snapshots: HashMap::new(),
            excluded: HashMap::new(),
            searches: HashMap::new(),
            recent_folders: Vec::new(),
            filters: HashMap::new(),
            recursive_tabs: HashSet::new(),
            show_hidden: false,
            show_system: false,
            show_extensions: true,
            folder_sizes_enabled: false,
            folder_sizes: HashMap::new(),
            request_ids: HashMap::new(),
            completed_enumerations: HashMap::new(),
            status_text: "Ready".into(),
            pending: HashSet::new(),
            background_pending: HashSet::new(),
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
            .filter(|location| !location.is_virtual())
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
        (self.pending.contains(&self.active_tab_id())
            && !self.background_pending.contains(&self.active_tab_id()))
            || (self.view_pending.contains(&self.active_tab_id()) && self.snapshot().is_none())
    }

    pub fn request_in_flight(&self, tab: TabId) -> bool {
        self.pending.contains(&tab)
    }

    pub fn background_in_flight(&self, tab: TabId) -> bool {
        self.background_pending.contains(&tab)
    }

    pub fn record_background_request(&mut self, tab: TabId, request: u64) {
        // A notification is not a visible change. Keep the current view/source
        // and its pending filter work until fresh metadata actually differs.
        self.request_ids.insert(tab, request);
        self.pending.insert(tab);
        self.background_pending.insert(tab);
    }

    pub fn folder_scan_generation(&self) -> Option<u64> {
        self.completed_enumerations
            .get(&self.active_tab_id())
            .copied()
    }

    fn unchanged_entries(&self, tab: TabId, next: &DirectorySnapshot) -> bool {
        let Some(current) = self.sources.get(&tab).or_else(|| self.snapshots.get(&tab)) else {
            return false;
        };
        if current.location != next.location {
            return false;
        }
        let excluded = if self.sources.contains_key(&tab) {
            None
        } else {
            self.excluded.get(&tab)
        };
        if current.entries.len() + excluded.map_or(0, Vec::len) != next.entries.len() {
            return false;
        }
        let previous: HashMap<_, _> = current
            .entries
            .iter()
            .chain(excluded.into_iter().flatten())
            .map(|entry| (&entry.path, entry))
            .collect();
        next.entries.iter().all(|entry| {
            previous.get(&entry.path).is_some_and(|old| {
                old.name == entry.name && old.kind == entry.kind && old.metadata == entry.metadata
            })
        })
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
        self.cancel_view(tab_id);
        self.sources.remove(&tab_id);
        self.revision = self.revision.wrapping_add(1);
        self.pending.remove(&tab_id);
        self.background_pending.remove(&tab_id);
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
                        if let Some(key) = l.virtual_key() {
                            return key
                                .split_once(':')
                                .map(|(_, name)| name)
                                .unwrap_or(key)
                                .into();
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

        self.completed_enumerations
            .insert(tab_id, snapshot.request_id);
        if self.background_pending.contains(&tab_id) && self.unchanged_entries(tab_id, &snapshot) {
            self.pending.remove(&tab_id);
            self.background_pending.remove(&tab_id);
            self.errors.remove(&tab_id);
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        self.cancel_view(tab_id);
        if snapshot.entries.len() >= crate::search::BACKGROUND_THRESHOLD {
            self.sources.insert(tab_id, std::sync::Arc::new(snapshot));
            self.schedule_view(tab_id);
            return;
        }
        self.sources.remove(&tab_id);

        let query_text = self.query_text(tab_id);
        let tab = match self.tabs.get_mut(tab_id) {
            Some(t) => t,
            None => return,
        };

        let old_paths: Vec<_> = self
            .snapshots
            .get(&tab_id)
            .map(|s| s.entries.iter().map(|e| e.path.clone()).collect())
            .unwrap_or_default();
        let query = kova_core::domain::SearchQuery::parse(&query_text, chrono::Local::now());
        let (mut entries, excluded) = partition_query(
            snapshot.entries,
            self.show_hidden,
            self.show_system,
            &query,
            &self.folder_sizes,
            self.folder_sizes_enabled,
        );
        self.excluded.insert(tab_id, excluded);
        kova_core::domain::sort_entries_by_size(&mut entries, tab.sort, |e| {
            kova_core::domain::effective_size(e, &self.folder_sizes, self.folder_sizes_enabled)
                .map(|s| s.bytes)
        });
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
        self.snapshots.insert(tab_id, std::sync::Arc::new(snap));
        self.pending.remove(&tab_id);
        self.background_pending.remove(&tab_id);
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
        crate::diagnostics::begin(tab_id, "directory");
        self.revision = self.revision.wrapping_add(1);
        self.cancel_view(tab_id);
        self.sources.remove(&tab_id);
        self.background_pending.remove(&tab_id);
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
        if !location.is_virtual() {
            self.recent_folders.retain(|path| path != &location.path);
            self.recent_folders.insert(0, location.path.clone());
            self.recent_folders.truncate(12);
        }

        self.filters.remove(&self.active_tab_id());
        self.searches.remove(&self.active_tab_id());
        if let Some(tab) = self.tabs.active_mut() {
            tab.history.navigate(location);
            // Entering a different directory invalidates index-based
            // selection state (Explorer behavior).
            tab.selection.clear();
        }
    }

    pub fn back(&mut self) -> Option<Location> {
        self.filters.remove(&self.active_tab_id());
        self.searches.remove(&self.active_tab_id());
        let tab = self.tabs.active_mut()?;
        let location = tab.history.back();
        if location.is_some() {
            tab.selection.clear();
        }
        location
    }

    pub fn forward(&mut self) -> Option<Location> {
        self.filters.remove(&self.active_tab_id());
        self.searches.remove(&self.active_tab_id());
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
        self.cancel_view(id);
        self.sources.remove(&id);
        self.view_generations.remove(&id);
        self.snapshots.remove(&id);
        self.searches.remove(&id);
        self.excluded.remove(&id);
        self.request_ids.remove(&id);
        self.completed_enumerations.remove(&id);
        self.pending.remove(&id);
        self.background_pending.remove(&id);
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
        self.snapshots
            .get(&self.tabs.active_id())
            .map(AsRef::as_ref)
    }

    pub fn snapshot_shared(&self) -> Option<std::sync::Arc<DirectorySnapshot>> {
        self.snapshots.get(&self.tabs.active_id()).cloned()
    }

    #[cfg(test)]
    pub fn snapshots_mut(&mut self) -> impl Iterator<Item = &mut DirectorySnapshot> {
        self.snapshots.values_mut().map(std::sync::Arc::make_mut)
    }

    /// Include filtered-out rows: their icon slots must remain valid when a
    /// search or visibility filter is cleared without another disk read.
    pub fn folder_size_paths(&self) -> Vec<std::path::PathBuf> {
        let id = self.active_tab_id();
        if let Some(source) = self.sources.get(&id) {
            return source
                .entries
                .iter()
                .filter(|e| e.is_directory())
                .map(|e| e.path.clone())
                .collect();
        }
        self.snapshot()
            .into_iter()
            .flat_map(|s| &s.entries)
            .chain(self.excluded.get(&id).into_iter().flatten())
            .filter(|e| e.is_directory())
            .map(|e| e.path.clone())
            .collect()
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

    #[cfg(test)]
    pub fn file_list_items(&self) -> Vec<FileListItem> {
        self.snapshot()
            .into_iter()
            .flat_map(|s| s.entries.iter())
            .enumerate()
            .map(|(idx, e)| {
                display_row(
                    e,
                    self.show_extensions,
                    self.folder_sizes_enabled,
                    &self.folder_sizes,
                    self.selected_indices().contains(&idx),
                )
            })
            .collect()
    }

    pub fn set_sort(&mut self, column: SortColumn) {
        crate::diagnostics::begin(self.active_tab_id(), "sort");
        self.revision = self.revision.wrapping_add(1);
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
        if self.sources.contains_key(&id) {
            self.schedule_view(id);
            return;
        }
        if let Some(snap) = self.snapshots.get_mut(&id) {
            let snap = std::sync::Arc::make_mut(snap);
            let old_paths: Vec<_> = snap.entries.iter().map(|e| e.path.clone()).collect();
            kova_core::domain::sort_entries_by_size(&mut snap.entries, tab.sort, |e| {
                kova_core::domain::effective_size(e, &self.folder_sizes, self.folder_sizes_enabled)
                    .map(|s| s.bytes)
            });
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
        self.refilter(None);
    }

    pub fn refilter(&mut self, only_tab: Option<TabId>) {
        self.revision = self.revision.wrapping_add(1);
        let (hidden, system) = (self.show_hidden, self.show_system);
        let large: Vec<_> = self
            .sources
            .keys()
            .copied()
            .filter(|id| only_tab.is_none_or(|tab| tab == *id))
            .collect();
        for id in large {
            self.schedule_view(id);
        }
        let queries: HashMap<_, _> = self
            .snapshots
            .keys()
            .map(|id| (*id, self.query_text(*id)))
            .collect();
        for (id, snapshot) in &mut self.snapshots {
            if self.sources.contains_key(id) || only_tab.is_some_and(|tab| tab != *id) {
                continue;
            }
            let Some(tab) = self.tabs.get_mut(*id) else {
                continue;
            };
            let snapshot = std::sync::Arc::make_mut(snapshot);
            let old_paths: Vec<_> = snapshot.entries.iter().map(|e| e.path.clone()).collect();
            let mut all = std::mem::take(&mut snapshot.entries);
            all.extend(self.excluded.remove(id).unwrap_or_default());
            let query = kova_core::domain::SearchQuery::parse(&queries[id], chrono::Local::now());
            let (mut visible, excluded) = partition_query(
                all,
                hidden,
                system,
                &query,
                &self.folder_sizes,
                self.folder_sizes_enabled,
            );
            kova_core::domain::sort_entries_by_size(&mut visible, tab.sort, |e| {
                kova_core::domain::effective_size(e, &self.folder_sizes, self.folder_sizes_enabled)
                    .map(|s| s.bytes)
            });
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

    pub fn query_text(&self, id: TabId) -> String {
        let filters = self.filters.get(&id).copied().unwrap_or_default();
        let kind = [
            "",
            "type:image",
            "type:document",
            "type:video",
            "type:audio",
            "type:folder",
            "type:archive",
        ]
        .get(filters[0])
        .copied()
        .unwrap_or_default();
        let size = ["", "size:<1MiB", "size:>100MiB", "size:>1GiB"]
            .get(filters[1])
            .copied()
            .unwrap_or_default();
        let date = [
            "",
            "modified:today",
            "modified:this-week",
            "modified:this-month",
        ]
        .get(filters[2])
        .copied()
        .unwrap_or_default();
        format!(
            "{} {kind} {size} {date}",
            self.searches
                .get(&id)
                .map(String::as_str)
                .unwrap_or_default()
        )
    }

    pub fn search_text(&self) -> &str {
        self.searches
            .get(&self.active_tab_id())
            .map(String::as_str)
            .unwrap_or_default()
    }

    pub fn view_in_flight(&self) -> bool {
        self.view_pending.contains(&self.active_tab_id())
    }

    fn cancel_view(&mut self, id: TabId) {
        if let Some(generation) = self.view_generations.get(&id) {
            generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.view_pending.remove(&id);
    }

    fn schedule_view(&mut self, id: TabId) {
        let Some(source) = self.sources.get(&id).cloned() else {
            return;
        };
        let Some(tab) = self.tabs.get(id) else { return };
        if self.view_worker.is_none() {
            match crate::search::Worker::new() {
                Ok(worker) => self.view_worker = Some(worker),
                Err(error) => {
                    self.errors
                        .insert(id, format!("Search worker unavailable: {error}"));
                    self.pending.remove(&id);
                    return;
                }
            }
        }
        let latest = self.view_generations.entry(id).or_default().clone();
        let generation = latest.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let job = crate::search::Job {
            tab: id,
            generation,
            latest,
            source,
            query: self.query_text(id),
            hidden: self.show_hidden,
            system: self.show_system,
            sizes: self.folder_sizes.clone(),
            sizes_enabled: self.folder_sizes_enabled,
            sort: tab.sort,
        };
        if self.view_worker.as_ref().unwrap().sender.send(job).is_ok() {
            self.view_pending.insert(id);
        }
    }

    pub fn poll_views(&mut self) -> bool {
        let results: Vec<_> = self
            .view_worker
            .as_ref()
            .map(|w| w.results.try_iter().collect())
            .unwrap_or_default();
        let mut dirty = false;
        for result in results {
            if self
                .sources
                .get(&result.tab)
                .is_none_or(|source| source.request_id != result.snapshot.request_id)
                || self.view_generations.get(&result.tab).is_none_or(|g| {
                    g.load(std::sync::atomic::Ordering::Relaxed) != result.generation
                })
            {
                continue;
            }
            let Some(tab) = self.tabs.get_mut(result.tab) else {
                continue;
            };
            if let Some(old) = self.snapshots.get(&result.tab) {
                tab.selection.remap(|i| {
                    old.entries
                        .get(i)
                        .and_then(|e| result.indices.get(&e.path).copied())
                });
            } else {
                tab.selection.clear();
            }
            self.revision = self.revision.wrapping_add(1);
            let completes_enumeration =
                self.is_current_request(result.tab, result.snapshot.request_id);
            self.snapshots
                .insert(result.tab, std::sync::Arc::new(result.snapshot));
            self.excluded.insert(result.tab, result.excluded);
            self.view_pending.remove(&result.tab);
            if completes_enumeration {
                self.pending.remove(&result.tab);
                self.background_pending.remove(&result.tab);
            }
            self.errors.remove(&result.tab);
            if result.tab == self.active_tab_id() {
                self.status_text = "Ready".into();
            }
            dirty |= result.tab == self.active_tab_id();
        }
        dirty
    }

    pub fn set_search(&mut self, text: String) {
        if self.search_text() == text {
            return;
        }
        self.searches.insert(self.active_tab_id(), text);
        self.refilter(Some(self.active_tab_id()));
    }
}

pub(crate) fn display_row(
    e: &FileEntry,
    show_extensions: bool,
    folder_sizes_enabled: bool,
    folder_sizes: &kova_core::domain::FolderSizes,
    selected: bool,
) -> FileListItem {
    FileListItem {
        name: if show_extensions || e.is_directory() {
            e.name.clone()
        } else {
            std::path::Path::new(&e.name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        },
        type_name: kind_text(e),
        size_text: if e.is_directory() && folder_sizes_enabled {
            folder_sizes
                .get(&e.path)
                .map(|size| {
                    size.map(|size| {
                        format!(
                            "{}{}",
                            if size.complete { "" } else { "≥ " },
                            crate::format_bytes(size.bytes)
                        )
                    })
                    .unwrap_or_else(|| "Unavailable".into())
                })
                .unwrap_or_else(|| "…".into())
        } else {
            size_text(e)
        },
        modified_text: modified_text(e),
        icon_id: effective_icon_id(e),
        is_dir: e.is_directory(),
        selected,
    }
}

fn partition_query(
    entries: Vec<FileEntry>,
    hidden: bool,
    system: bool,
    query: &kova_core::domain::SearchQuery,
    sizes: &kova_core::domain::FolderSizes,
    sizes_enabled: bool,
) -> (Vec<FileEntry>, Vec<FileEntry>) {
    entries.into_iter().partition(|e| {
        (!e.metadata.is_hidden || hidden)
            && (!e.metadata.is_system || system)
            && query.matches_with_size(
                e,
                kova_core::domain::effective_size(e, sizes, sizes_enabled),
            )
    })
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
    if entry.kind == kova_core::domain::FileKind::Unknown {
        return "Unavailable".into();
    }
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
    entry
        .metadata
        .size
        .map(crate::format_bytes)
        .unwrap_or_default()
}

fn modified_text(entry: &FileEntry) -> String {
    entry
        .metadata
        .modified
        .map(|dt| kova_platform_windows::formatting::date(dt, false))
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
    fn unchanged_background_notifications_preserve_view_and_filter_work() {
        for count in [3, 2500] {
            let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
            let id = ctrl.active_tab_id();
            let mut source = dummy_snapshot(1, "unused");
            source.entries = (0..count)
                .map(|i| dummy_snapshot(1, &format!("Folder {i}")).entries.remove(0))
                .collect();
            ctrl.record_request(id, 1);
            ctrl.apply_snapshot(id, source.clone());
            settle(&mut ctrl);
            ctrl.set_search("Folder 1".into());
            settle(&mut ctrl);
            ctrl.selection_mut().unwrap().select_single(0);
            let selected = ctrl.selected_paths();
            let original = ctrl.snapshot_shared().unwrap();
            let revision = ctrl.revision;
            ctrl.record_background_request(id, 2);
            assert_eq!(
                ctrl.revision, revision,
                "starting a refresh must not redraw or dismiss menus"
            );
            source.request_id = 2;
            source.entries.reverse(); // Enumeration order is not visible sort order.
            ctrl.apply_snapshot(id, source.clone());
            assert_eq!(ctrl.revision, revision);
            assert!(std::sync::Arc::ptr_eq(
                &original,
                &ctrl.snapshot_shared().unwrap()
            ));
            assert_eq!(ctrl.selected_paths(), selected);
            assert!(!ctrl.request_in_flight(id));
            assert_eq!(ctrl.folder_scan_generation(), Some(2));

            ctrl.set_search("Folder 2".into());
            ctrl.record_background_request(id, 3);
            settle(&mut ctrl); // A cached transform may finish while enumeration is pending.
            assert!(ctrl.request_in_flight(id));
            source.request_id = 3;
            ctrl.apply_snapshot(id, source.clone());
            settle(&mut ctrl);
            assert!(!ctrl.request_in_flight(id));
            assert!(!ctrl.snapshot().unwrap().entries.is_empty());
            assert!(
                ctrl.snapshot()
                    .unwrap()
                    .entries
                    .iter()
                    .all(|e| e.name.contains('2'))
            );

            let revision = ctrl.revision;
            ctrl.record_background_request(id, 4);
            source.request_id = 4;
            source
                .entries
                .iter_mut()
                .find(|e| e.name == "Folder 2")
                .unwrap()
                .metadata
                .modified = Some(chrono::Local::now());
            ctrl.apply_snapshot(id, source);
            settle(&mut ctrl);
            assert!(
                ctrl.revision > revision,
                "real metadata changes must still appear"
            );
        }
    }

    #[test]
    fn computed_folder_sizes_drive_display_filter_and_sort_after_queries() {
        use kova_core::domain::EffectiveSize;
        for count in [3, 2500] {
            let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
            let id = ctrl.active_tab_id();
            let mut snapshot = dummy_snapshot(1, "Folder 0");
            snapshot.entries = (0..count)
                .map(|i| {
                    let entry = dummy_snapshot(1, &format!("Folder {i}")).entries.remove(0);
                    ctrl.folder_sizes.insert(
                        entry.path.clone(),
                        Some(EffectiveSize {
                            bytes: (count - i) as u64 * 1024,
                            complete: true,
                        }),
                    );
                    entry
                })
                .collect();
            ctrl.folder_sizes_enabled = true;
            ctrl.record_request(id, 1);
            ctrl.apply_snapshot(id, snapshot);
            settle(&mut ctrl);
            ctrl.set_sort(SortColumn::Size);
            settle(&mut ctrl);
            assert!(
                ctrl.path_at(0)
                    .unwrap()
                    .ends_with(format!("Folder {}", count - 1))
            );
            ctrl.set_search("size:>1KiB".into());
            settle(&mut ctrl);
            assert_eq!(ctrl.item_count(), count - 1);
            assert!(
                ctrl.path_at(0)
                    .unwrap()
                    .ends_with(format!("Folder {}", count - 2))
            );
            assert_eq!(
                ctrl.file_list_items()[0].size_text,
                crate::format_bytes(2048)
            );
            ctrl.set_search(String::new());
            settle(&mut ctrl);
            assert!(
                ctrl.path_at(0)
                    .unwrap()
                    .ends_with(format!("Folder {}", count - 1))
            );
        }
    }

    #[test]
    fn rapid_background_searches_cannot_replace_newer_navigation() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let id = ctrl.active_tab_id();
        let mut snapshot = dummy_snapshot(1, "old");
        snapshot.entries = (0..10000)
            .map(|i| dummy_snapshot(1, &format!("Folder {i}")).entries.remove(0))
            .collect();
        ctrl.record_request(id, 1);
        ctrl.apply_snapshot(id, snapshot);
        ctrl.set_search("123".into());
        ctrl.set_search("999".into());
        settle(&mut ctrl);
        assert!(
            ctrl.snapshot()
                .unwrap()
                .entries
                .iter()
                .all(|e| e.name.contains("999"))
        );
        ctrl.set_search(String::new());
        ctrl.record_request(id, 2);
        ctrl.apply_snapshot(id, dummy_snapshot(2, "New destination"));
        for _ in 0..20 {
            ctrl.poll_views();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(ctrl.snapshot().unwrap().entries[0].name, "New destination");
        assert_eq!(ctrl.item_count(), 1);
    }
    fn settle(ctrl: &mut AppController) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ctrl.view_pending.is_empty() {
            ctrl.poll_views();
            assert!(
                std::time::Instant::now() < deadline,
                "view worker timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn search_refilters_ten_thousand_cached_entries_without_new_requests() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let id = ctrl.active_tab_id();
        let mut snapshot = dummy_snapshot(1, "unused");
        snapshot.entries = (0..10_000)
            .map(|i| {
                let name = format!("photo-{i:05}.png");
                FileEntry {
                    name: name.clone(),
                    path: std::path::Path::new("C:\\dummy").join(name),
                    kind: FileKind::File,
                    metadata: FileMetadata {
                        size: Some(i * 1024),
                        ..FileMetadata::empty()
                    },
                    icon_handle: None,
                }
            })
            .collect();
        ctrl.record_request(id, 1);
        ctrl.apply_snapshot(id, snapshot);
        settle(&mut ctrl);
        ctrl.selection_mut().unwrap().select_single(9999);
        let selection = ctrl.selected_paths();
        ctrl.set_search("type:image size:>9MB".into());
        settle(&mut ctrl);
        assert_eq!(ctrl.item_count(), 783);
        assert_eq!(ctrl.selected_paths(), selection);
        assert_eq!(ctrl.snapshot().unwrap().request_id, 1);
        assert!(!ctrl.is_loading());
        ctrl.set_search(String::new());
        settle(&mut ctrl);
        assert_eq!(ctrl.item_count(), 10_000);
        assert_eq!(ctrl.selected_paths(), selection);
        ctrl.set_search("nothing-matches".into());
        settle(&mut ctrl);
        assert_eq!(ctrl.selected_count(), 0);
        assert!(ctrl.selected_paths().is_empty());
    }

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
    fn background_refresh_preserves_selection_filter_and_interactive_state() {
        let mut ctrl = AppController::new(Location::new("C:\\dummy".into()));
        let tab = ctrl.active_tab_id();
        ctrl.record_request(tab, 1);
        ctrl.apply_snapshot(tab, dummy_snapshot(1, "program.exe"));
        ctrl.set_search("program".into());
        ctrl.selection_mut().unwrap().select_single(0);
        let selected = ctrl.selected_paths();
        ctrl.record_background_request(tab, 2);
        assert!(ctrl.request_in_flight(tab));
        assert!(!ctrl.is_loading());
        assert_eq!(ctrl.item_count(), 1);
        let mut changed = dummy_snapshot(2, "program.exe");
        changed.entries[0].metadata.size = Some(2048);
        ctrl.apply_snapshot(tab, changed);
        assert!(!ctrl.request_in_flight(tab));
        assert_eq!(ctrl.selected_paths(), selected);
        assert_eq!(ctrl.search_text(), "program");
        assert_eq!(
            ctrl.snapshot().unwrap().entries[0].metadata.size,
            Some(2048)
        );
        ctrl.record_background_request(tab, 3);
        ctrl.record_request(tab, 4);
        assert!(ctrl.is_loading(), "navigation supersedes silent refresh");
        ctrl.apply_snapshot(tab, dummy_snapshot(3, "stale"));
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
