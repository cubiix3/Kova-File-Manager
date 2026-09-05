//! Local organization stores references only. No method in this module performs
//! file operations; deleting a group can never delete a referenced file.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Library {
    pub pins: Vec<PathBuf>,
    pub tags: BTreeMap<String, Vec<PathBuf>>,
    pub collections: BTreeMap<String, Vec<PathBuf>>,
    #[serde(skip)]
    pub revision: u64,
}

impl Library {
    pub fn pin(&mut self, path: PathBuf) {
        if path.is_absolute() && !self.pins.contains(&path) {
            self.pins.push(path);
            self.changed();
        }
    }
    pub fn unpin(&mut self, index: usize) {
        if index < self.pins.len() {
            self.pins.remove(index);
            self.changed();
        }
    }
    pub fn reorder_pin(&mut self, index: usize, delta: isize) {
        let next = index.saturating_add_signed(delta);
        if index < self.pins.len() && next < self.pins.len() && index != next {
            self.pins.swap(index, next);
            self.changed();
        }
    }
    pub fn add(
        &mut self,
        tag: bool,
        name: &str,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("Use a name with 1–64 characters.".into());
        }
        let groups = if tag {
            &mut self.tags
        } else {
            &mut self.collections
        };
        let entries = groups.entry(name.into()).or_default();
        for path in paths {
            if path.is_absolute() && !entries.contains(&path) {
                entries.push(path);
            }
        }
        self.changed();
        Ok(())
    }
    pub fn remove_group(&mut self, tag: bool, name: &str) {
        let groups = if tag {
            &mut self.tags
        } else {
            &mut self.collections
        };
        if groups.remove(name).is_some() {
            self.changed();
        }
    }
    pub fn remove_references(&mut self, key: &str, paths: &[PathBuf]) {
        if let Some(entries) = self.entries_mut(key) {
            entries.retain(|p| !paths.contains(p));
            self.changed();
        }
    }
    pub fn entries(&self, key: &str) -> Option<&[PathBuf]> {
        if let Some(name) = key.strip_prefix("tag:") {
            self.tags.get(name).map(Vec::as_slice)
        } else {
            self.collections
                .get(key.strip_prefix("collection:")?)
                .map(Vec::as_slice)
        }
    }
    fn entries_mut(&mut self, key: &str) -> Option<&mut Vec<PathBuf>> {
        if let Some(name) = key.strip_prefix("tag:") {
            self.tags.get_mut(name)
        } else {
            self.collections.get_mut(key.strip_prefix("collection:")?)
        }
    }
    /// A confirmed rename/move performed by Kova follows references, including
    /// children of moved folders. External changes remain explicit unavailable
    /// references instead of silently attaching to another same-named file.
    pub fn relocate(&mut self, old: &Path, new: &Path) {
        let mut changed = false;
        for path in self
            .pins
            .iter_mut()
            .chain(self.tags.values_mut().flatten())
            .chain(self.collections.values_mut().flatten())
        {
            if let Ok(suffix) = path.strip_prefix(old) {
                *path = if suffix.as_os_str().is_empty() {
                    new.to_path_buf()
                } else {
                    new.join(suffix)
                };
                changed = true;
            }
        }
        if changed {
            self.changed();
        }
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_file_rename_does_not_append_a_directory_separator() {
        let mut library = Library::default();
        let old = PathBuf::from("C:\\Assets\\old.png");
        let new = PathBuf::from("C:\\Assets\\new.png");
        library.add(false, "Assets", [old.clone()]).unwrap();
        library.relocate(&old, &new);
        assert_eq!(
            library.entries("collection:Assets").unwrap()[0].as_os_str(),
            new.as_os_str()
        );
    }
    #[test]
    fn groups_are_deduplicated_references_and_follow_confirmed_renames() {
        let root = std::env::temp_dir().join("kova-library-test");
        let old = root.join("folder");
        let file = old.join("photo.png");
        let new = root.join("renamed");
        let mut library = Library::default();
        library.pin(old.clone());
        library.pin(old.clone());
        library
            .add(false, "Assets", [file.clone(), file.clone()])
            .unwrap();
        library.add(true, "Work", [file.clone()]).unwrap();
        assert_eq!(library.pins.len(), 1);
        assert_eq!(library.entries("collection:Assets").unwrap(), &[file]);
        library.relocate(&old, &new);
        assert_eq!(
            library.entries("tag:Work").unwrap(),
            &[new.join("photo.png")]
        );
        library.remove_group(false, "Assets");
        assert!(library.entries("collection:Assets").is_none());
        assert_eq!(library.entries("tag:Work").unwrap().len(), 1);
        assert!(library.add(true, "  ", []).is_err());
    }
}
