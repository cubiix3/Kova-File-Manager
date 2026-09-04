use kova_core::domain::Location;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Profile, KNOWN_FOLDER_FLAG,
    SHGetKnownFolderPath,
};
use windows::core::GUID;

/// A known folder the platform can resolve at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownFolder {
    Home,
    Desktop,
    Documents,
    Downloads,
}

impl KnownFolder {
    fn guid(&self) -> GUID {
        match self {
            KnownFolder::Home => FOLDERID_Profile,
            KnownFolder::Desktop => FOLDERID_Desktop,
            KnownFolder::Documents => FOLDERID_Documents,
            KnownFolder::Downloads => FOLDERID_Downloads,
        }
    }
}

/// Resolve a known folder to a `Location`. Returns `None` if the call fails,
/// allowing the caller to fall back to a sensible default.
pub fn resolve_known_folder(folder: KnownFolder) -> Option<Location> {
    // SAFETY: SHGetKnownFolderPath takes the known folder GUID, zero flags,
    // and a null token. It allocates a PWSTR that we must free with
    // CoTaskMemFree. We CoInitialize first on this thread; if already
    // initialized it returns S_FALSE which is fine.
    unsafe {
        crate::com::ensure_sta();

        let path_ptr = match SHGetKnownFolderPath(&folder.guid(), KNOWN_FOLDER_FLAG(0), None) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("SHGetKnownFolderPath failed: {:?}", e);
                return None;
            }
        };

        let wide = std::slice::from_raw_parts(path_ptr.0, wcslen(path_ptr.0));
        let os_string = std::ffi::OsString::from_wide(wide);
        CoTaskMemFree(Some(path_ptr.0 as *const _));
        Some(Location::new(PathBuf::from(os_string)))
    }
}

/// Resolve the best initial location for Kova. Falls back to the user profile,
/// then to `C:\` if nothing else works.
pub fn initial_location() -> Location {
    resolve_known_folder(KnownFolder::Home)
        .or_else(|| resolve_known_folder(KnownFolder::Desktop))
        .or_else(|| resolve_known_folder(KnownFolder::Documents))
        .unwrap_or_else(|| Location::new(PathBuf::from("C:\\")))
}

unsafe fn wcslen(ptr: *const u16) -> usize {
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_folders_resolve_or_none() {
        for folder in [
            KnownFolder::Home,
            KnownFolder::Desktop,
            KnownFolder::Documents,
            KnownFolder::Downloads,
        ] {
            let loc = resolve_known_folder(folder);
            if let Some(loc) = loc {
                assert!(loc.path.exists(), "{} should exist", loc.display());
            }
        }
    }

    #[test]
    fn initial_location_exists() {
        let loc = initial_location();
        assert!(
            loc.path.exists(),
            "initial location must exist: {}",
            loc.display()
        );
    }
}
