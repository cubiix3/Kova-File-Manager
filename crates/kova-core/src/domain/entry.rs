use chrono::{DateTime, Local};
use std::path::PathBuf;

/// High-level classification of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    Junction,
    Unknown,
}

impl FileKind {
    pub fn display_name(&self) -> &str {
        match self {
            FileKind::Directory => "Folder",
            FileKind::File => "File",
            FileKind::Symlink => "Symlink",
            FileKind::Junction => "Junction",
            FileKind::Unknown => "Unknown",
        }
    }
}

/// Platform-independent view of a single filesystem entry.
///
/// UI-specific state (hover, pixel position) does **not** belong here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FileKind,
    pub metadata: FileMetadata,
    pub icon_handle: Option<IconHandle>,
}

impl FileEntry {
    pub fn is_directory(&self) -> bool {
        self.kind == FileKind::Directory
    }

    pub fn is_file(&self) -> bool {
        self.kind == FileKind::File
    }

    pub fn extension_lower(&self) -> String {
        std::path::Path::new(&self.name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    /// Sort key for file type: directories sort before files before other.
    pub fn kind_order(&self) -> u8 {
        match self.kind {
            FileKind::Directory => 0,
            FileKind::File => 1,
            FileKind::Symlink => 2,
            FileKind::Junction => 3,
            FileKind::Unknown => 4,
        }
    }
}

/// Metadata that is cheap to obtain during a normal directory read.
///
/// Folder size is intentionally **not** stored here; computing recursive folder
/// sizes is a separate, opt-in operation performed outside the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadata {
    /// Size in bytes. Directories have `None` to avoid implying a computed size.
    pub size: Option<u64>,

    /// Last modified time in local time, when available.
    pub modified: Option<DateTime<Local>>,

    /// True if the entry is hidden.
    pub is_hidden: bool,

    /// True if the entry is a system file.
    pub is_system: bool,

    /// Raw platform attributes; the UI must not interpret these directly.
    pub raw_attributes: u32,
}

impl FileMetadata {
    pub fn empty() -> Self {
        Self {
            size: None,
            modified: None,
            is_hidden: false,
            is_system: false,
            raw_attributes: 0,
        }
    }
}

/// A complete snapshot of a directory, identified by the location and a request
/// generation so stale results can be discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySnapshot {
    pub location: super::Location,
    pub request_id: u64,
    pub entries: Vec<FileEntry>,
}

impl DirectorySnapshot {
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Handle to an icon resource managed by the platform / UI layer.
///
/// The core stores only a token so sorting and selection logic remain platform
/// independent. The actual image data lives in the platform or UI crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconHandle(pub u32);
