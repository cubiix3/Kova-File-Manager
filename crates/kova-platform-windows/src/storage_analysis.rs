//! Logical storage accounting. Enumeration is cancellable, bounded, and never
//! deliberately follows a reparse point or opens file contents.
use std::{
    collections::{BinaryHeap, HashMap},
    fs,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub bytes: u64,
    pub files: u64,
    pub skipped: u64,
    pub folders: Vec<(u64, PathBuf)>,
    pub largest_files: Vec<(u64, PathBuf)>,
    pub finished: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}

pub fn scan(root: &Path, generation: &AtomicU64, id: u64, mut emit: impl FnMut(Analysis)) {
    let mut result = Analysis::default();
    let mut pending = vec![(root.to_path_buf(), None::<PathBuf>)];
    let mut folders = HashMap::<PathBuf, u64>::new();
    let mut files = BinaryHeap::<std::cmp::Reverse<(u64, PathBuf)>>::new();
    let mut last = Instant::now();
    let snapshot = |result: &mut Analysis,
                    folders: &HashMap<PathBuf, u64>,
                    files: &BinaryHeap<std::cmp::Reverse<(u64, PathBuf)>>| {
        result.folders = folders.iter().map(|(p, b)| (*b, p.clone())).collect();
        result
            .folders
            .sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        result.folders.truncate(30);
        result.largest_files = files.iter().map(|entry| entry.0.clone()).collect();
        result
            .largest_files
            .sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    };
    while let Some((directory, bucket)) = pending.pop() {
        if generation.load(Ordering::Relaxed) != id {
            result.cancelled = true;
            break;
        }
        let entries = (|| -> std::io::Result<_> {
            let metadata = fs::symlink_metadata(&directory)?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x0044_1400 != 0 {
                return Err(std::io::Error::other(
                    "Link, offline file, or unavailable directory skipped",
                ));
            }
            fs::read_dir(&directory)
        })();
        let entries = match entries {
            Ok(entries) => entries,
            Err(error) => {
                result.skipped += 1;
                if directory == root {
                    result.error = Some(error.to_string());
                }
                continue;
            }
        };
        for entry in entries {
            if generation.load(Ordering::Relaxed) != id {
                result.cancelled = true;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    result.skipped += 1;
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(_) => {
                    result.skipped += 1;
                    continue;
                }
            };
            if metadata.file_attributes() & 0x0044_1400 != 0 {
                result.skipped += 1;
                continue;
            }
            if metadata.is_dir() {
                if pending.len() >= 100_000 {
                    result.skipped += 1;
                    continue;
                }
                let next_bucket = bucket.clone().unwrap_or_else(|| path.clone());
                folders.entry(next_bucket.clone()).or_default();
                pending.push((path, Some(next_bucket)));
            } else if metadata.is_file() {
                let bytes = metadata.len();
                result.bytes = result.bytes.saturating_add(bytes);
                result.files += 1;
                if let Some(bucket) = &bucket {
                    let total = folders.entry(bucket.clone()).or_default();
                    *total = total.saturating_add(bytes);
                }
                files.push(std::cmp::Reverse((bytes, path)));
                if files.len() > 30 {
                    files.pop();
                }
            }
            if last.elapsed() >= Duration::from_millis(200) {
                snapshot(&mut result, &folders, &files);
                emit(result.clone());
                last = Instant::now();
            }
        }
        if result.cancelled {
            break;
        }
    }
    result.finished = true;
    snapshot(&mut result, &folders, &files);
    emit(result);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn totals_biggest_items_missing_roots_and_cancellation() {
        let root = std::env::temp_dir().join(format!("kova-analysis-{}", std::process::id()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("one"), [0; 12]).unwrap();
        fs::write(root.join("nested/two"), [0; 31]).unwrap();
        let generation = AtomicU64::new(1);
        let mut output = Analysis::default();
        scan(&root, &generation, 1, |result| output = result);
        assert_eq!((output.bytes, output.files, output.skipped), (43, 2, 0));
        assert_eq!(output.folders[0].0, 31);
        assert_eq!(output.largest_files[0].0, 31);
        scan(&root, &generation, 2, |result| output = result);
        assert!(output.cancelled);
        scan(&root.join("missing"), &generation, 1, |result| {
            output = result
        });
        assert!(output.error.is_some());
        fs::remove_dir_all(root).unwrap();
    }
}
