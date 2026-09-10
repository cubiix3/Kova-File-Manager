//! CPU-heavy snapshot transforms own no UI objects and never hold the controller lock.
use kova_core::domain::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};

pub const BACKGROUND_THRESHOLD: usize = 2_000;
pub struct Job {
    pub tab: TabId,
    pub generation: u64,
    pub latest: Arc<AtomicU64>,
    pub source: Arc<DirectorySnapshot>,
    pub query: String,
    pub hidden: bool,
    pub system: bool,
    pub sizes: FolderSizes,
    pub sizes_enabled: bool,
    pub sort: SortDescriptor,
}
pub struct ResultView {
    pub tab: TabId,
    pub generation: u64,
    pub snapshot: DirectorySnapshot,
    pub excluded: Vec<FileEntry>,
    pub indices: HashMap<PathBuf, usize>,
}
struct CachedOrdering {
    source: std::sync::Weak<DirectorySnapshot>,
    sort: SortDescriptor,
    sizes_enabled: bool,
    sizes: FolderSizes,
    indices: Vec<usize>,
}
impl CachedOrdering {
    fn reusable(&self, job: &Job) -> bool {
        self.source.ptr_eq(&Arc::downgrade(&job.source))
            && self.sort == job.sort
            && (self.sort.column != SortColumn::Size
                || (self.sizes_enabled == job.sizes_enabled && self.sizes == job.sizes))
    }
}
pub struct Worker {
    pub sender: mpsc::Sender<Job>,
    pub results: mpsc::Receiver<ResultView>,
}
impl Worker {
    pub fn new() -> std::io::Result<Self> {
        let (sender, input) = mpsc::channel::<Job>();
        let (output, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("kova-search-sort".into())
            .spawn(move || {
                let mut ordering: Option<CachedOrdering> = None;
                while let Ok(job) = input.recv() {
                    if job.latest.load(Ordering::Relaxed) != job.generation {
                        continue;
                    }
                    let start = Instant::now();
                    let query = SearchQuery::parse(&job.query, chrono::Local::now());
                    let reusable = ordering.as_ref().is_some_and(|order| order.reusable(&job));
                    if !reusable {
                        let indices = sorted_indices(&job.source.entries, job.sort, |e| {
                            effective_size(e, &job.sizes, job.sizes_enabled).map(|s| s.bytes)
                        });
                        ordering = Some(CachedOrdering {
                            source: Arc::downgrade(&job.source),
                            sort: job.sort,
                            sizes_enabled: job.sizes_enabled,
                            sizes: job.sizes.clone(),
                            indices,
                        });
                    }
                    let mut visible = Vec::new();
                    let mut excluded = Vec::new();
                    for (index, &source_index) in
                        ordering.as_ref().unwrap().indices.iter().enumerate()
                    {
                        let entry = &job.source.entries[source_index];
                        if index % 256 == 0 && job.latest.load(Ordering::Relaxed) != job.generation
                        {
                            break;
                        }
                        if (!entry.metadata.is_hidden || job.hidden)
                            && (!entry.metadata.is_system || job.system)
                            && query.matches_with_size(
                                entry,
                                effective_size(entry, &job.sizes, job.sizes_enabled),
                            )
                        {
                            visible.push(entry.clone());
                        } else {
                            excluded.push(entry.clone());
                        }
                    }
                    if job.latest.load(Ordering::Relaxed) != job.generation {
                        continue;
                    }
                    let indices = visible
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (e.path.clone(), i))
                        .collect();
                    tracing::info!(
                        entries = job.source.entries.len(),
                        visible = visible.len(),
                        elapsed_ms = start.elapsed().as_secs_f64() * 1000.,
                        "snapshot transform"
                    );
                    if output
                        .send(ResultView {
                            tab: job.tab,
                            generation: job.generation,
                            snapshot: DirectorySnapshot {
                                location: job.source.location.clone(),
                                request_id: job.source.request_id,
                                entries: visible,
                            },
                            excluded,
                            indices,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self { sender, results })
    }
}
