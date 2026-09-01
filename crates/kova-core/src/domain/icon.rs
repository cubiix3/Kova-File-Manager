use super::entry::IconHandle;

/// Generic icon ids used while a proper shell icon provider is not yet wired
/// up. The UI maps these to actual image resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericIcon {
    Folder = 0,
    File = 1,
    Symlink = 2,
    Drive = 3,
    Unknown = 4,
}

impl GenericIcon {
    pub fn as_icon_handle(self) -> IconHandle {
        IconHandle(self as u32)
    }
}
