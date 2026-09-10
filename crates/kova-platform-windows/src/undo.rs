//! Session-only, identity-checked undo. Never replaces an existing destination.
use std::{
    fs::File,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::*};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    volume: u32,
    index: u64,
    size: u64,
    modified: u64,
    directory: bool,
}
fn open(path: &Path, rename: bool) -> Result<File, String> {
    File::options()
        .access_mode(FILE_READ_ATTRIBUTES.0 | if rename { DELETE.0 } else { 0 })
        .share_mode(if rename {
            FILE_SHARE_READ.0
        } else {
            FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0
        })
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|e| e.to_string())
}
fn identity(file: &File) -> Result<Identity, String> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: file owns this live handle; info is a correctly sized writable structure.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(|e| e.to_string())?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("Link undo is unavailable".into());
    }
    Ok(Identity {
        volume: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        size: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        modified: (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(info.ftLastWriteTime.dwLowDateTime),
        directory: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
    })
}
#[derive(Clone, Debug)]
struct Entry {
    id: u64,
    original: PathBuf,
    current: PathBuf,
    identity: Identity,
    label: String,
}
#[derive(Default, Debug)]
pub struct History {
    entries: Mutex<Vec<Entry>>,
    next: AtomicU64,
}
impl History {
    /// Only call after a confirmed non-replacing rename or completed native move.
    pub fn record(&self, original: &Path, current: &Path, rename: bool) {
        let capture = || -> Result<Entry, String> {
            let identity = identity(&open(current, false)?)?;
            if identity.directory && !rename {
                return Err("Folder move undo is unavailable".into());
            }
            let parent = identity_of_parent(original)?;
            if parent.volume != identity.volume {
                return Err("Cross-volume undo is unavailable".into());
            }
            Ok(Entry {
                id: self.next.fetch_add(1, Ordering::Relaxed) + 1,
                original: original.into(),
                current: current.into(),
                identity,
                label: format!(
                    "Undo {}: {} → {}",
                    if rename { "rename" } else { "move" },
                    current.file_name().unwrap_or_default().to_string_lossy(),
                    if rename {
                        original
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        original.display().to_string()
                    }
                ),
            })
        };
        if let Ok(entry) = capture() {
            if let Ok(mut entries) = self.entries.lock() {
                entries.push(entry);
                if entries.len() > 32 {
                    entries.remove(0);
                }
            }
        }
    }
    pub fn next(&self) -> Option<(u64, String)> {
        self.entries
            .lock()
            .ok()?
            .last()
            .map(|e| (e.id, e.label.clone()))
    }
    pub fn apply(&self, id: u64) -> Result<(PathBuf, PathBuf), String> {
        let entry = {
            let mut entries = self
                .entries
                .lock()
                .map_err(|_| "Undo history unavailable")?;
            if entries.last().is_none_or(|e| e.id != id) {
                return Err("The next undo action changed; review it again".into());
            }
            entries.pop().unwrap()
        };
        let file = open(&entry.current, true)?;
        if identity(&file)? != entry.identity {
            return Err(
                "Undo unavailable: this item was changed or replaced after the operation".into(),
            );
        }
        let mut destination = crate::path_resolver::extended_path(&entry.original);
        destination.pop(); // FILE_RENAME_INFO uses an explicit length, without NUL.
        let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
        let length = offset + destination.len() * 2;
        let mut buffer = vec![
            0u64;
            length
                .div_ceil(8)
                .max(std::mem::size_of::<FILE_RENAME_INFO>().div_ceil(8))
        ];
        // SAFETY: u64 allocation provides FILE_RENAME_INFO alignment and enough bytes
        // for its variable UTF-16 tail. The handle locks the validated identity against
        // replacement. Zeroed ReplaceIfExists explicitly forbids overwriting any item.
        unsafe {
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            (*info).FileNameLength = (destination.len() * 2)
                .try_into()
                .map_err(|_| "Path too long")?;
            std::ptr::copy_nonoverlapping(
                destination.as_ptr(),
                buffer.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
                destination.len(),
            );
            SetFileInformationByHandle(
                HANDLE(file.as_raw_handle()),
                FileRenameInfo,
                info.cast(),
                length.try_into().map_err(|_| "Path too long")?,
            )
            .map_err(|e| {
                format!("Undo could not restore the original path; nothing was overwritten: {e}")
            })?;
        }
        Ok((entry.current, entry.original))
    }
}
fn identity_of_parent(path: &Path) -> Result<Identity, String> {
    identity(&open(path.parent().ok_or("No parent folder")?, false)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_preserves_replacements_and_restores_the_same_file() {
        let root = std::env::temp_dir().join(format!("kova-undo-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("original.txt");
        let renamed = root.join("renamed.txt");
        std::fs::write(&original, b"original").unwrap();
        std::fs::rename(&original, &renamed).unwrap();
        let history = History::default();
        history.record(&original, &renamed, true);
        let id = history.next().unwrap().0;
        history.apply(id).unwrap();
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
        std::fs::rename(&original, &renamed).unwrap();
        history.record(&original, &renamed, true);
        std::fs::write(&original, b"new occupant").unwrap();
        assert!(history.apply(history.next().unwrap().0).is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"new occupant");
        assert_eq!(std::fs::read(&renamed).unwrap(), b"original");
        std::fs::remove_dir_all(root).unwrap();
    }
}
