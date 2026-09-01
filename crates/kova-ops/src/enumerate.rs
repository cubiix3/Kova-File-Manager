use kova_core::domain::{
    DirectorySnapshot, FileEntry, FileKind, FileMetadata, IconHandle, Location,
};
use kova_core::error::Result;
use kova_platform_windows::path_resolver::{map_io_error, require_directory};
use std::os::windows::fs::MetadataExt;
use tokio::fs;

/// Enumerate a directory asynchronously, off the UI thread.
///
/// A single broken entry must not abort the whole listing. The returned
/// snapshot carries a `request_id` so callers can discard stale results.
pub async fn enumerate_directory(location: Location, request_id: u64) -> Result<DirectorySnapshot> {
    let path = location.path.clone();
    require_directory(&path)?;

    let mut entries = Vec::new();
    let mut read_dir = fs::read_dir(&path)
        .await
        .map_err(|e| map_io_error(&path, e))?;

    while let Some(item) = read_dir
        .next_entry()
        .await
        .map_err(|e| map_io_error(&path, e))?
    {
        let path = item.path();
        let name = match item.file_name().into_string() {
            Ok(n) => n,
            Err(_os) => {
                tracing::warn!("skipping non-Unicode entry at {}", path.display());
                continue;
            }
        };

        let metadata = match item.metadata().await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("metadata failed for {}: {e}", path.display());
                continue;
            }
        };

        let kind = classify_kind(&metadata, &path);
        let size = if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| chrono::DateTime::<chrono::Local>::from(t).into());

        // Placeholder generic icon until shell icons are wired.
        let icon_handle = Some(match kind {
            FileKind::Directory => IconHandle(0),
            _ => IconHandle(1),
        });

        entries.push(FileEntry {
            name,
            path,
            kind,
            metadata: FileMetadata {
                size,
                modified,
                is_hidden: metadata.file_attributes() & 0x2 != 0,
                is_system: metadata.file_attributes() & 0x4 != 0,
                raw_attributes: metadata.file_attributes(),
            },
            icon_handle,
        });
    }

    Ok(DirectorySnapshot {
        location,
        request_id,
        entries,
    })
}

fn classify_kind(metadata: &std::fs::Metadata, _path: &std::path::Path) -> FileKind {
    let attrs = metadata.file_attributes();
    // FILE_ATTRIBUTE_REPARSE_POINT
    if attrs & 0x400 != 0 {
        // If it points to a directory, treat as junction; otherwise symlink.
        if metadata.is_dir() {
            return FileKind::Junction;
        } else {
            return FileKind::Symlink;
        }
    }

    if metadata.is_dir() {
        FileKind::Directory
    } else if metadata.is_file() {
        FileKind::File
    } else {
        FileKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio;

    #[tokio::test]
    async fn enumerate_project_root_contains_cargo_toml() {
        let loc = Location::new(PathBuf::from("<repo-root>"));
        let snap = enumerate_directory(loc, 1).await.unwrap();
        let names: Vec<&str> = snap.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Cargo.toml"));
        assert!(names.contains(&"README.md"));
    }
}
