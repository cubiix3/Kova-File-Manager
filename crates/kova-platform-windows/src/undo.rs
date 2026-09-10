//! Session-only undo for renames, file moves and captured Recycle Bin items.
//! Identity checks and collision-safe Shell flags prevent silent replacement.
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
#[derive(Clone, Debug)]
enum Action {
    Relocate(Entry),
    Recycle {
        id: u64,
        items: Vec<Recycled>,
        label: String,
    },
}
impl Action {
    fn id(&self) -> u64 {
        match self {
            Self::Relocate(e) => e.id,
            Self::Recycle { id, .. } => *id,
        }
    }
    fn label(&self) -> &str {
        match self {
            Self::Relocate(e) => &e.label,
            Self::Recycle { label, .. } => label,
        }
    }
}

/// Only owned data crosses the Shell worker's apartment boundary.
#[derive(Clone, Debug)]
pub(crate) struct Recycled {
    original: PathBuf,
    current: PathBuf,
    identity: Identity,
    pidl: Vec<u8>,
}
impl Recycled {
    pub(crate) fn capture(
        original: PathBuf,
        item: &windows::Win32::UI::Shell::IShellItem,
    ) -> Option<Self> {
        use windows::Win32::UI::Shell::{ILFree, ILGetSize, SHGetIDListFromObject};
        let current = crate::transfer_progress::item_path(Some(item))?;
        let identity = identity(&open(&current, false).ok()?).ok()?;
        // SAFETY: item belongs to this STA. The Shell allocates the PIDL; copy
        // exactly its reported size and release it before leaving the apartment.
        let pidl = unsafe {
            let raw = SHGetIDListFromObject(item).ok()?;
            if raw.is_null() {
                return None;
            }
            let length = ILGetSize(Some(raw)) as usize;
            let bytes = if (2..=1024 * 1024).contains(&length) {
                Some(std::slice::from_raw_parts(raw.cast::<u8>(), length).to_vec())
            } else {
                None
            };
            ILFree(Some(raw));
            bytes?
        };
        Some(Self {
            original,
            current,
            identity,
            pidl,
        })
    }

    fn restore(&self) -> Result<PathBuf, String> {
        // Never mistake a newly deleted item of the same name for this record.
        if identity(&open(&self.current, false)?)? != self.identity {
            return Err("This Recycle Bin item changed or is no longer available".into());
        }
        match std::fs::symlink_metadata(&self.original) {
            Ok(_) => {
                return Err(format!(
                    "{} already exists. Move or rename it before retrying Undo; nothing was overwritten.",
                    self.original.display()
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        crate::com::ensure_sta();
        // SAFETY: the private PIDL is a complete owned copy returned by the
        // Shell, including its terminator. All interfaces remain on this thread.
        let item =
            unsafe { windows::Win32::UI::Shell::SHCreateItemFromIDList(self.pidl.as_ptr().cast()) }
                .map_err(|e| e.to_string())?;
        crate::shell_ops::restore_recycled(&item, &self.current, &self.original)
    }
}
#[derive(Default, Debug)]
pub struct History {
    entries: Mutex<Vec<Action>>,
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
                entries.push(Action::Relocate(entry));
                if entries.len() > 32 {
                    entries.remove(0);
                }
            }
        }
    }
    pub(crate) fn record_recycled(&self, items: Vec<Recycled>) {
        if items.is_empty() {
            return;
        }
        let label = if items.len() == 1 {
            format!("Restore from Recycle Bin: {}", items[0].original.display())
        } else {
            format!("Restore {} items from Recycle Bin", items.len())
        };
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(Action::Recycle {
                id: self.next.fetch_add(1, Ordering::Relaxed) + 1,
                items,
                label,
            });
            if entries.len() > 32 {
                entries.remove(0);
            }
        }
    }
    pub fn is_recycle(&self, id: u64) -> bool {
        self.entries.lock().is_ok_and(|entries| matches!(entries.last(), Some(Action::Recycle { id: next, .. }) if *next == id))
    }
    pub fn next(&self) -> Option<(u64, String)> {
        self.entries
            .lock()
            .ok()?
            .last()
            .map(|e| (e.id(), e.label().to_owned()))
    }
    pub fn apply(&self, id: u64) -> Result<Vec<(PathBuf, PathBuf)>, String> {
        let entry = {
            let entries = self
                .entries
                .lock()
                .map_err(|_| "Undo history unavailable")?;
            if entries.last().is_none_or(|e| e.id() != id) {
                return Err("The next undo action changed; review it again".into());
            }
            entries.last().cloned().ok_or("Undo history is empty")?
        };
        let result = match entry {
            Action::Relocate(entry) => Self::relocate(entry).map(|paths| vec![paths]),
            Action::Recycle { items, .. } => {
                let mut restored = Vec::new();
                for item in items {
                    let destination = item
                        .restore()
                        .map_err(|e| format!("Restored {} item(s). {e}", restored.len()))?;
                    restored.push((item.current.clone(), destination));
                    // Keep only pending items after a partial failure/cancel, so
                    // retry never restores a successful item a second time.
                    if let Ok(mut entries) = self.entries.lock() {
                        if let Some(Action::Recycle { items, .. }) =
                            entries.iter_mut().find(|e| e.id() == id)
                        {
                            items.retain(|pending| pending.current != item.current);
                        }
                    }
                }
                Ok(restored)
            }
        };
        if result.is_ok() {
            if let Ok(mut entries) = self.entries.lock() {
                entries.retain(|e| e.id() != id);
            }
        }
        result
    }
    fn relocate(entry: Entry) -> Result<(PathBuf, PathBuf), String> {
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

    #[test]
    fn recycled_batch_restores_files_and_folder_without_overwriting() {
        use crate::{
            shell_ops::{ShellOpCommand, ShellOpOutcome, spawn_shell_ops_thread},
            transfers::TransferQueue,
        };
        let root = std::env::temp_dir().join(format!(
            "kova-recycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("Nested")).unwrap();
        let file = root.join("Résumé.txt");
        let folder = root.join("Nested");
        std::fs::write(&file, b"preserved file").unwrap();
        std::fs::write(folder.join("child.txt"), b"preserved child").unwrap();
        let queue = TransferQueue::default();
        let (send, receive) = std::sync::mpsc::channel();
        let (result_send, result_receive) = std::sync::mpsc::channel();
        let worker = spawn_shell_ops_thread(receive, result_send);
        send.send(
            queue
                .enqueue(ShellOpCommand::Delete {
                    sources: vec![file.clone(), folder.clone()],
                })
                .unwrap(),
        )
        .unwrap();
        let outcome = result_receive
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
        drop(send);
        worker.join().unwrap();
        assert!(
            matches!(outcome, ShellOpOutcome::Completed { .. }),
            "{outcome:?}"
        );
        assert!(!file.exists() && !folder.exists());
        let (id, _) = queue
            .undo
            .next()
            .expect("Recycle Bin undo must be recorded");
        assert!(queue.undo.is_recycle(id));
        std::fs::write(&file, b"new occupant").unwrap();
        assert!(queue.undo.apply(id).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"new occupant");
        assert_eq!(
            queue.undo.next().unwrap().0,
            id,
            "failed restore remains retryable"
        );
        std::fs::remove_file(&file).unwrap();
        // A later conflict must preserve earlier successful restorations and
        // leave only the remaining item to retry.
        std::fs::create_dir(&folder).unwrap();
        assert!(queue.undo.apply(id).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"preserved file");
        std::fs::remove_dir(&folder).unwrap();
        queue.undo.apply(id).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"preserved file");
        assert_eq!(
            std::fs::read(folder.join("child.txt")).unwrap(),
            b"preserved child"
        );
        assert!(queue.undo.next().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
