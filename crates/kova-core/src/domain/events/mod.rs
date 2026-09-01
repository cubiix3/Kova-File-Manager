use super::entry::DirectorySnapshot;
use super::location::Location;
use super::tab::TabId;

/// Events emitted by the core / operations layer back to the UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KovaEvent {
    /// A directory snapshot has been produced for a tab.
    DirectoryLoaded {
        tab_id: TabId,
        snapshot: DirectorySnapshot,
    },

    /// A directory request was cancelled or replaced; the UI may keep showing
    /// whatever it has until a newer result arrives.
    DirectoryCancelled { tab_id: TabId, request_id: u64 },

    /// Enumeration failed.
    DirectoryError {
        tab_id: TabId,
        location: Location,
        request_id: u64,
        error_message: String,
    },

    /// The active tab should display this location.
    LocationChanged { tab_id: TabId, location: Location },

    /// A new folder was created.
    FolderCreated { parent: Location, name: String },

    /// An item was renamed.
    ItemRenamed {
        old_path: std::path::PathBuf,
        new_path: std::path::PathBuf,
    },

    /// A user-facing operation failed.
    OperationError {
        context: String,
        error_message: String,
    },

    /// Status message for the status bar.
    StatusMessage { text: String },
}
