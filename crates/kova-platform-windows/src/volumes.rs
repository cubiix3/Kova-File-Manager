use kova_core::domain::{FileEntry, FileKind, FileMetadata, IconHandle};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDriveStringsW};
use windows::core::PCWSTR;

const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_RAMDISK: u32 = 6;

/// A logical drive entry shown in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveInfo {
    pub letter: String,
    pub path: PathBuf,
}

/// Enumerate local logical drives as returned by Windows. Only fixed and
/// removable local drives are returned; CD/DVD/network drives are filtered out
/// for M0 to keep the UI simple.
pub fn list_local_drives() -> Vec<DriveInfo> {
    let mut buffer = vec![0u16; (MAX_PATH * 26) as usize];
    let count = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) };
    if count == 0 {
        return Vec::new();
    }

    let drives: Vec<DriveInfo> = buffer[..count as usize]
        .split(|c| *c == 0)
        .filter(|s| !s.is_empty())
        .filter_map(|wide| {
            let os_string = std::ffi::OsString::from_wide(wide);
            let path = PathBuf::from(&os_string);
            if is_included_drive_type(&path) {
                let letter = os_string.to_string_lossy().into_owned();
                Some(DriveInfo { letter, path })
            } else {
                None
            }
        })
        .collect();

    drives
}

fn is_included_drive_type(path: &std::path::Path) -> bool {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
    drive_type == DRIVE_FIXED || drive_type == DRIVE_REMOVABLE || drive_type == DRIVE_RAMDISK
}

/// Convert drive information into a `FileEntry` suitable for the sidebar model.
pub fn drive_to_entry(drive: &DriveInfo) -> FileEntry {
    FileEntry {
        name: drive.letter.clone(),
        path: drive.path.clone(),
        kind: FileKind::Directory,
        metadata: FileMetadata::empty(),
        icon_handle: Some(IconHandle(3)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_listing_does_not_panic() {
        let drives = list_local_drives();
        // At least the system drive should be present on a real Windows box.
        assert!(!drives.is_empty(), "expected at least one local drive");
    }
}
