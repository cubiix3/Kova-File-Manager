use std::path::{Path, PathBuf};

/// A resolved, canonical location in the filesystem.
///
/// It is intentionally not just a `String`; the platform layer is responsible
/// for turning user input into a valid `PathBuf`. The core treats a `Location`
/// as an opaque token it can compare by absolute path and hand back to the
/// platform for enumeration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location {
    /// Absolute path as reported by the platform layer. Must be canonical
    /// enough to identify a directory uniquely within Kova.
    pub path: PathBuf,
}

impl Location {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns the display text for an address bar. The platform layer may
    /// choose a prettier representation, but the core uses the raw path.
    pub fn display(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    /// Returns the parent location, or `None` if this is already a root.
    pub fn parent(&self) -> Option<Location> {
        self.path.parent().and_then(|p| {
            if p.as_os_str().is_empty() {
                // Preserve drive roots, e.g. C:\
                None
            } else {
                Some(Location::new(p.to_path_buf()))
            }
        })
    }

    pub fn root_name(&self) -> Option<String> {
        self.path
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
    }

    pub fn is_root(&self) -> bool {
        self.path.parent().is_none()
            || self.path.as_os_str().len() <= 3
                && self.path.as_os_str().to_string_lossy().ends_with(":\\")
    }
}

impl AsRef<Path> for Location {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// A location that the user typed or otherwise requested. It may not yet be
/// validated. The platform layer resolves it into a `Location`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationInput {
    pub raw: String,
}

impl LocationInput {
    pub fn new(raw: impl Into<String>) -> Self {
        Self { raw: raw.into() }
    }

    pub fn as_path(&self) -> PathBuf {
        PathBuf::from(&self.raw)
    }
}
