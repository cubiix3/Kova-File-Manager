use super::FileEntry;
use std::{collections::HashMap, path::PathBuf};

/// A measured size may be a lower bound when a scan was bounded or inaccessible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveSize {
    pub bytes: u64,
    pub complete: bool,
}

pub type FolderSizes = HashMap<PathBuf, Option<EffectiveSize>>;

pub fn effective_size(
    entry: &FileEntry,
    folders: &FolderSizes,
    enabled: bool,
) -> Option<EffectiveSize> {
    if entry.is_directory() {
        enabled
            .then(|| folders.get(&entry.path).copied().flatten())
            .flatten()
    } else {
        entry.metadata.size.map(|bytes| EffectiveSize {
            bytes,
            complete: true,
        })
    }
}
