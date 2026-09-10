//! Only visible/accessed rows become Slint strings and images. The immutable
//! snapshot is shared with the controller; this model never locks it during rendering.
use crate::{FileListItem, app_state::display_row};
use kova_core::domain::{DirectorySnapshot, FolderSizes};
use slint::{Model, ModelNotify, ModelTracker};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};
type IconLookup = std::rc::Rc<dyn Fn(&std::path::Path, bool) -> i32>;

#[derive(Default)]
struct Content {
    snapshot: Option<Arc<DirectorySnapshot>>,
    extensions: bool,
    sizes_enabled: bool,
    sizes: FolderSizes,
    selected: HashSet<usize>,
}
#[derive(Default)]
pub struct FileModel {
    content: RefCell<Content>,
    rows: RefCell<HashMap<usize, FileListItem>>,
    notify: ModelNotify,
    icon_lookup: RefCell<Option<IconLookup>>,
}
impl Content {
    fn row(&self, index: usize) -> Option<FileListItem> {
        let entry = self.snapshot.as_ref()?.entries.get(index)?;
        let row = display_row(
            entry,
            self.extensions,
            self.sizes_enabled,
            &self.sizes,
            self.selected.contains(&index),
        );
        Some(FileListItem {
            path: entry.path.to_string_lossy().as_ref().into(),
            name: row.name.into(),
            type_name: row.type_name.into(),
            size_text: row.size_text.into(),
            modified_text: row.modified_text.into(),
            icon_id: row.icon_id,
            is_dir: row.is_dir,
            selected: row.selected,
            ..Default::default()
        })
    }
}
impl FileModel {
    pub fn set_icon_lookup(&self, lookup: IconLookup) {
        *self.icon_lookup.borrow_mut() = Some(lookup);
    }
    fn icon(&self, row: &mut FileListItem) {
        if let Some(lookup) = self.icon_lookup.borrow().as_ref() {
            row.icon_id = lookup(std::path::Path::new(row.path.as_str()), row.is_dir);
        }
    }
    pub fn refresh_icons(&self) {
        let mut changed = Vec::new();
        for (&index, row) in self.rows.borrow_mut().iter_mut() {
            let old = row.icon_id;
            self.icon(row);
            if row.icon_id != old {
                changed.push(index);
            }
        }
        for index in changed {
            self.notify.row_changed(index);
        }
    }
    pub fn replace(
        &self,
        snapshot: Option<Arc<DirectorySnapshot>>,
        extensions: bool,
        sizes_enabled: bool,
        sizes: FolderSizes,
        selected: HashSet<usize>,
    ) {
        let next = Content {
            snapshot,
            extensions,
            sizes_enabled,
            sizes,
            selected,
        };
        let old = self.content.replace(next);
        let content = self.content.borrow();
        let same_order = match (&old.snapshot, &content.snapshot) {
            (Some(a), Some(b)) => {
                a.entries.len() == b.entries.len()
                    && a.entries
                        .iter()
                        .zip(&b.entries)
                        .all(|(a, b)| a.path == b.path)
            }
            (None, None) => true,
            _ => false,
        };
        let previous = self.rows.take();
        let mut changed = Vec::new();
        if same_order {
            let mut rows = self.rows.borrow_mut();
            for (index, previous) in previous {
                if let Some(mut row) = content.row(index) {
                    self.icon(&mut row);
                    if old
                        .snapshot
                        .as_ref()
                        .zip(content.snapshot.as_ref())
                        .is_some_and(|(a, b)| {
                            a.entries[index].metadata == b.entries[index].metadata
                        })
                    {
                        row.thumbnail = previous.thumbnail.clone();
                        row.has_thumbnail = previous.has_thumbnail;
                    }
                    if row != previous {
                        changed.push(index);
                    }
                    rows.insert(index, row);
                }
            }
        }
        drop(content);
        if same_order {
            for index in changed {
                self.notify.row_changed(index);
            }
        } else {
            self.notify.reset();
        }
    }
    pub fn set_selection(&self, selected: HashSet<usize>) {
        let mut changed = Vec::new();
        for (&index, row) in self.rows.borrow_mut().iter_mut() {
            let value = selected.contains(&index);
            if row.selected != value {
                row.selected = value;
                changed.push(index);
            }
        }
        self.content.borrow_mut().selected = selected;
        for index in changed {
            self.notify.row_changed(index);
        }
    }
}
impl Model for FileModel {
    type Data = FileListItem;
    fn row_count(&self) -> usize {
        self.content
            .borrow()
            .snapshot
            .as_ref()
            .map_or(0, |s| s.entries.len())
    }
    fn row_data(&self, index: usize) -> Option<FileListItem> {
        if let Some(row) = self.rows.borrow().get(&index) {
            let mut row = row.clone();
            self.icon(&mut row);
            return Some(row);
        }
        let mut row = self.content.borrow().row(index)?;
        self.icon(&mut row);
        let mut rows = self.rows.borrow_mut();
        // Bound memory even when accessibility clients inspect the entire folder.
        if rows.len() >= 1024 {
            rows.clear();
        }
        rows.insert(index, row.clone());
        Some(row)
    }
    fn set_row_data(&self, index: usize, row: FileListItem) {
        if index >= self.row_count() {
            return;
        }
        self.rows.borrow_mut().insert(index, row);
        self.notify.row_changed(index);
    }
    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kova_core::domain::{FileEntry, FileKind, FileMetadata, Location};
    #[test]
    fn rows_are_lazy_bounded_and_changed_metadata_invalidates_thumbnails() {
        let mut snapshot = Arc::new(DirectorySnapshot {
            location: Location::new("C:\\fixture".into()),
            request_id: 1,
            entries: (0..3000)
                .map(|i| FileEntry {
                    name: format!("Image {i}.txt"),
                    path: format!("C:\\fixture\\Image {i}.txt").into(),
                    kind: FileKind::File,
                    metadata: FileMetadata::empty(),
                    icon_handle: None,
                })
                .collect(),
        });
        let model = FileModel::default();
        model.replace(
            Some(snapshot.clone()),
            true,
            false,
            FolderSizes::new(),
            HashSet::from([0]),
        );
        assert_eq!(model.row_count(), 3000);
        assert!(model.rows.borrow().is_empty());
        assert!(model.row_data(0).unwrap().selected);
        model.set_selection(HashSet::from([1]));
        assert!(!model.row_data(0).unwrap().selected);
        assert!(model.row_data(1).unwrap().selected);
        for i in 0..3000 {
            assert!(model.row_data(i).is_some());
        }
        assert!(model.rows.borrow().len() <= 1024);
        let mut row = model.row_data(1).unwrap();
        row.has_thumbnail = true;
        model.set_row_data(1, row);
        Arc::make_mut(&mut snapshot).entries[1].metadata.size = Some(7);
        model.replace(
            Some(snapshot),
            true,
            false,
            FolderSizes::new(),
            HashSet::from([1]),
        );
        assert!(!model.row_data(1).unwrap().has_thumbnail);
        assert_eq!(model.row_data(1).unwrap().size_text, "7 B");
        assert!(model.row_data(1).unwrap().selected);
    }
}
