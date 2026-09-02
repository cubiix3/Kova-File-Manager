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

    #[tokio::test]
    async fn enumerate_project_root_contains_cargo_toml() {
        // Resolve the repository root relative to this crate so the test
        // never depends on a machine-specific absolute path.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let loc = Location::new(root);
        let snap = enumerate_directory(loc, 1).await.unwrap();
        let names: Vec<&str> = snap.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Cargo.toml"));
        assert!(names.contains(&"README.md"));
    }

    #[tokio::test]
    #[ignore = "slow performance baseline"]
    async fn enumerate_directory_performance_baseline() {
        use std::time::Instant;
        let root = std::env::temp_dir().join("kova-perf");
        let _ = tokio::fs::remove_dir_all(&root).await;

        let sizes = vec![100usize, 1_000, 10_000];
        let runs = 5usize;
        for size in sizes {
            let dir = root.join(format!("entries_{}", size));
            tokio::fs::create_dir_all(&dir).await.unwrap();
            for i in 0..size {
                let path = dir.join(format!("file_{:08}.txt", i));
                tokio::fs::write(&path, b"x").await.unwrap();
            }

            let mut times = Vec::new();
            for _ in 0..runs {
                let loc = Location::new(dir.clone());
                let start = Instant::now();
                let snap = enumerate_directory(loc, 1).await.unwrap();
                let elapsed = start.elapsed();
                assert_eq!(snap.entries.len(), size);
                times.push(elapsed.as_secs_f64() * 1000.0);
            }

            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let min = times[0];
            let median = times[runs / 2];
            let max = times[times.len() - 1];
            println!("entries={size} min={min:.2}ms median={median:.2}ms max={max:.2}ms");
        }

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
