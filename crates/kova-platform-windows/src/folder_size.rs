//! Cancellable logical-size scans. Never follow reparse points or hydrate files.
use std::{
    fs,
    os::windows::fs::MetadataExt,
    path::{Component, Path, Prefix},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use windows::{Win32::Storage::FileSystem::GetDriveTypeW, core::PCWSTR};

#[derive(Debug, Clone, Copy)]
pub struct FolderSize {
    pub bytes: u64,
    pub complete: bool,
}

pub fn is_local_fixed(path: &Path) -> bool {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        _ => return false,
    };
    let root = [u16::from(letter), b':' as u16, b'\\' as u16, 0];
    // SAFETY: root is a NUL-terminated local drive prefix, valid for this call.
    unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) == 3 }
}

pub fn calculate(path: &Path, generation: &AtomicU64, id: u64) -> std::io::Result<FolderSize> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_attributes() & 0x0040_1400 != 0 || !metadata.is_dir() {
        return Ok(FolderSize {
            bytes: 0,
            complete: false,
        });
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut remaining = vec![path.to_path_buf()];
    let mut total = FolderSize {
        bytes: 0,
        complete: true,
    };
    let mut visited = 0;
    while let Some(directory) = remaining.pop() {
        // Recheck just before opening: navigation cancels work, and a directory
        // may have been replaced with a junction since it was first enumerated.
        if generation.load(Ordering::Relaxed) != id {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let Ok(meta) = fs::symlink_metadata(&directory) else {
            total.complete = false;
            continue;
        };
        if meta.file_attributes() & 0x0040_1400 != 0 {
            total.complete = false;
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                total.complete = false;
                continue;
            }
        };
        for entry in entries {
            if generation.load(Ordering::Relaxed) != id {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            if visited >= 50_000 || Instant::now() >= deadline {
                total.complete = false;
                return Ok(total);
            }
            visited += 1;
            let Ok(entry) = entry else {
                total.complete = false;
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                total.complete = false;
                continue;
            };
            // Reparse point, offline, or recall-on-data-access: skip without
            // following links or triggering cloud content hydration.
            if metadata.file_attributes() & 0x0040_1400 != 0 {
                total.complete = false;
                continue;
            }
            if metadata.is_dir() {
                remaining.push(entry.path());
            } else {
                total.bytes = total.bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_files_are_summed_and_cancelled_work_returns_no_total() {
        let root = std::env::temp_dir().join(format!(
            "kova-size-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("a"), [0; 17]).unwrap();
        fs::write(root.join("nested/b"), [0; 23]).unwrap();
        let generation = AtomicU64::new(1);
        let size = calculate(&root, &generation, 1).unwrap();
        assert_eq!(size.bytes, 40);
        assert!(size.complete);
        assert_eq!(
            calculate(&root, &generation, 2).unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
        fs::remove_dir_all(root).unwrap();
    }
}
