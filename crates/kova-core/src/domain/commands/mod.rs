use super::location::{Location, LocationInput};
use super::tab::TabId;

/// Commands emitted by the UI, dispatched by the application, and executed by
/// the platform / operations layer. The UI never performs filesystem work
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KovaCommand {
    NavigateTo(LocationInput),
    NavigateBack,
    NavigateForward,
    NavigateParent,
    NavigateRefresh,
    NavigateLocation {
        tab_id: TabId,
        location: Location,
    },

    TabNew,
    TabSwitch(TabId),
    TabClose(TabId),

    SelectionSingle {
        tab_id: TabId,
        index: usize,
    },
    SelectionToggle {
        tab_id: TabId,
        index: usize,
    },
    SelectionRange {
        tab_id: TabId,
        index: usize,
    },
    SelectionAll {
        tab_id: TabId,
    },
    SelectionClear {
        tab_id: TabId,
    },
    SelectionMoveFocus {
        tab_id: TabId,
        delta: isize,
        extend: bool,
    },

    Sort {
        tab_id: TabId,
        column: super::sort::SortColumn,
    },

    NewFolder {
        parent: Location,
        name: String,
    },
    Rename {
        path: std::path::PathBuf,
        new_name: String,
    },
    OpenItem {
        path: std::path::PathBuf,
    },
}
