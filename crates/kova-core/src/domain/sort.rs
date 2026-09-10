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
    sort_entries_by_size(entries, descriptor, |e| e.metadata.size);
}

/// Precompute keys once per entry, never allocate lowercase names in comparisons.
pub fn sort_entries_by_size(
    entries: &mut [FileEntry],
    descriptor: SortDescriptor,
    size: impl Fn(&FileEntry) -> Option<u64>,
) {
    entries.sort_by_cached_key(|e| comparison_key(e, descriptor, &size));
}

/// An ordering can be reused when only the filter changes for a snapshot.
pub fn sorted_indices(
    entries: &[FileEntry],
    descriptor: SortDescriptor,
    size: impl Fn(&FileEntry) -> Option<u64>,
) -> Vec<usize> {
    let mut indices: Vec<_> = (0..entries.len()).collect();
    indices.sort_by_cached_key(|&i| comparison_key(&entries[i], descriptor, &size));
    indices
}

type EntryKey = (
    bool,
    Directed<(Primary, NaturalKey, String, std::path::PathBuf)>,
);

fn comparison_key(
    e: &FileEntry,
    descriptor: SortDescriptor,
    size: &impl Fn(&FileEntry) -> Option<u64>,
) -> EntryKey {
    let name = NaturalKey::new(&e.name);
    let primary = match descriptor.column {
        SortColumn::Name => Primary::Name,
        SortColumn::Type => Primary::Type(e.kind_order(), e.extension_lower()),
        SortColumn::Size => Primary::Size(size(e)),
        SortColumn::Modified => {
            Primary::Modified(e.metadata.modified.map(|d| d.timestamp_millis()))
        }
    };
    let key = (primary, name, e.name.clone(), e.path.clone());
    (
        !e.is_directory(),
        Directed {
            key,
            descending: descriptor.direction == SortDirection::Descending,
        },
    )
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Primary {
    Name,
    Type(u8, String),
    Size(Option<u64>),
    Modified(Option<i64>),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct NaturalKey(Vec<Part>);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(usize, String, usize),
    Text(String),
}
impl NaturalKey {
    fn new(name: &str) -> Self {
        let lower = name.to_lowercase();
        let mut parts = Vec::new();
        let mut rest = lower.as_str();
        while !rest.is_empty() {
            let numeric = rest.as_bytes()[0].is_ascii_digit();
            let end = rest
                .char_indices()
                .find(|(_, c)| c.is_ascii_digit() != numeric)
                .map_or(rest.len(), |(i, _)| i);
            let (part, tail) = rest.split_at(end);
            parts.push(if numeric {
                let significant = part.trim_start_matches('0');
                Part::Number(significant.len(), significant.into(), part.len())
            } else {
                Part::Text(part.into())
            });
            rest = tail;
        }
        Self(parts)
    }
}
#[derive(PartialEq, Eq)]
struct Directed<T> {
    key: T,
    descending: bool,
}
impl<T: Ord> Ord for Directed<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let order = self.key.cmp(&other.key);
        if self.descending {
            order.reverse()
        } else {
            order
        }
    }
}
impl<T: Ord> PartialOrd for Directed<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entry::{FileEntry, FileKind, FileMetadata, IconHandle};
    use std::path::PathBuf;

    #[test]
    fn natural_sort_handles_numbers_without_integer_overflow_and_keeps_folders_first() {
        let mut entries = vec![
            entry("Image 10.png", FileKind::File, None),
            entry("Image 2.png", FileKind::File, None),
            entry("Image 0002.png", FileKind::File, None),
            entry(
                "Image 999999999999999999999999999999.png",
                FileKind::File,
                None,
            ),
            entry("Z", FileKind::Directory, None),
        ];
        sort_entries(&mut entries, SortDescriptor::by_name());
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            [
                "Z",
                "Image 2.png",
                "Image 0002.png",
                "Image 10.png",
                "Image 999999999999999999999999999999.png"
            ]
        );
    }

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
