use super::entry::FileEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortColumn {
    Name,
    Type,
    Size,
    Modified,
}

impl SortColumn {
    /// Return a stable zero-based index used by the UI header.
    pub fn as_index(self) -> usize {
        match self {
            SortColumn::Name => 0,
            SortColumn::Type => 1,
            SortColumn::Size => 2,
            SortColumn::Modified => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(self) -> Self {
        match self {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SortDescriptor {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl SortDescriptor {
    pub fn new(column: SortColumn, direction: SortDirection) -> Self {
        Self { column, direction }
    }

    pub fn by_name() -> Self {
        Self::new(SortColumn::Name, SortDirection::Ascending)
    }

    pub fn by_modified_desc() -> Self {
        Self::new(SortColumn::Modified, SortDirection::Descending)
    }
}

/// Sort entries in place according to the descriptor.
///
/// Directories and files are grouped consistently: the primary grouping is by
/// the chosen column, but a hidden tie-breaker always uses name so the result
/// is stable and predictable.
pub fn sort_entries(entries: &mut [FileEntry], descriptor: SortDescriptor) {
    let compare = |a: &FileEntry, b: &FileEntry| {
        // Folders stay together at the top in either sort direction.
        let grouping = b.is_directory().cmp(&a.is_directory());
        if grouping != std::cmp::Ordering::Equal {
            return grouping;
        }
        let ord = match descriptor.column {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Type => {
                let by_kind = a.kind_order().cmp(&b.kind_order());
                if by_kind != std::cmp::Ordering::Equal {
                    by_kind
                } else {
                    a.extension_lower().cmp(&b.extension_lower())
                }
            }
            SortColumn::Size => {
                // Folders have no size and sort before files with size.
                match (a.metadata.size, b.metadata.size) {
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (Some(sa), Some(sb)) => sa.cmp(&sb),
                }
            }
            SortColumn::Modified => a.metadata.modified.cmp(&b.metadata.modified),
        };

        // Stable tie-breaker by name.
        let ord = if ord == std::cmp::Ordering::Equal {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            ord
        };

        match descriptor.direction {
            SortDirection::Ascending => ord,
            SortDirection::Descending => ord.reverse(),
        }
    };

    entries.sort_by(compare);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entry::{FileEntry, FileKind, FileMetadata, IconHandle};
    use std::path::PathBuf;

    fn entry(name: &str, kind: FileKind, size: Option<u64>) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("C:\\test\\{name}")),
            kind,
            metadata: FileMetadata {
                size,
                modified: None,
                is_hidden: false,
                is_system: false,
                raw_attributes: 0,
            },
            icon_handle: Some(IconHandle(0)),
        }
    }

    #[test]
    fn sort_by_name_ascending() {
        let mut entries = vec![
            entry("zebra.txt", FileKind::File, Some(10)),
            entry("alpha", FileKind::Directory, None),
            entry("beta.txt", FileKind::File, Some(20)),
        ];
        sort_entries(&mut entries, SortDescriptor::by_name());
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta.txt", "zebra.txt"]
        );
    }

    #[test]
    fn sort_by_size_directories_before_files() {
        let mut entries = vec![
            entry("big.txt", FileKind::File, Some(2000)),
            entry("dir", FileKind::Directory, None),
            entry("small.txt", FileKind::File, Some(10)),
        ];
        sort_entries(
            &mut entries,
            SortDescriptor::new(SortColumn::Size, SortDirection::Ascending),
        );
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["dir", "small.txt", "big.txt"]);
    }

    #[test]
    fn sort_by_type_groups_kind_then_extension() {
        let mut entries = vec![
            entry("file.b", FileKind::File, Some(1)),
            entry("file.a", FileKind::File, Some(1)),
            entry("folder", FileKind::Directory, None),
        ];
        sort_entries(
            &mut entries,
            SortDescriptor::new(SortColumn::Type, SortDirection::Ascending),
        );
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["folder", "file.a", "file.b"]);
    }

    #[test]
    fn sort_column_index_is_stable() {
        assert_eq!(SortColumn::Name.as_index(), 0);
        assert_eq!(SortColumn::Type.as_index(), 1);
        assert_eq!(SortColumn::Size.as_index(), 2);
        assert_eq!(SortColumn::Modified.as_index(), 3);
    }
}
