use kova_core::domain::{KovaEvent, Location, TabId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

/// Per-tab generation counter used to discard stale async results.
#[derive(Debug, Default)]
pub struct GenerationCounter {
    current: Mutex<HashMap<TabId, AtomicU64>>,
}

impl Clone for GenerationCounter {
    fn clone(&self) -> Self {
        let inner = self.current.lock().unwrap();
        Self {
            current: Mutex::new(
                inner
                    .iter()
                    .map(|(&k, v)| (k, AtomicU64::new(v.load(Ordering::SeqCst))))
                    .collect(),
            ),
        }
    }
}

impl GenerationCounter {
    pub fn remove(&self, tab_id: TabId) {
        self.current.lock().unwrap().remove(&tab_id);
    }

    pub fn next(&self, tab_id: TabId) -> u64 {
        let mut inner = self.current.lock().unwrap();
        let counter = inner.entry(tab_id).or_insert_with(|| AtomicU64::new(0));
        counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current(&self, tab_id: TabId) -> u64 {
        let inner = self.current.lock().unwrap();
        inner
            .get(&tab_id)
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(0)
    }
}

/// Worker command type used internally by the ops runtime.
#[derive(Debug)]
pub enum WorkerCommand {
    CancelEnumeration {
        tab_id: TabId,
    },
    EnumerateReferences {
        tab_id: TabId,
        location: Location,
        request_id: u64,
        paths: Vec<PathBuf>,
    },
    Enumerate {
        tab_id: TabId,
        location: Location,
        request_id: u64,
        background: bool,
    },
    NewFolder {
        parent: Location,
        name: String,
    },
    Rename {
        path: PathBuf,
        new_name: String,
    },
    Open {
        path: PathBuf,
    },
}

/// Spawn a filesystem worker that receives commands and emits events.
///
/// The worker runs on a dedicated Tokio task and is the only place that
/// performs filesystem I/O for the UI.
pub fn spawn_worker(mut rx: mpsc::UnboundedReceiver<WorkerCommand>, tx: mpsc::Sender<KovaEvent>) {
    tokio::spawn(async move {
        let mut enumerations: HashMap<TabId, tokio::task::JoinHandle<()>> = HashMap::new();
        while let Some(cmd) = rx.recv().await {
            enumerations.retain(|_, task| !task.is_finished());
            use WorkerCommand::*;
            match cmd {
                CancelEnumeration { tab_id } => {
                    if let Some(task) = enumerations.remove(&tab_id) {
                        task.abort();
                    }
                }
                EnumerateReferences {
                    tab_id,
                    location,
                    request_id,
                    paths,
                } => {
                    if let Some(previous) = enumerations.remove(&tab_id) {
                        previous.abort();
                    }
                    let tx = tx.clone();
                    enumerations.insert(
                        tab_id,
                        tokio::spawn(async move {
                            let snapshot =
                                crate::enumerate::enumerate_references(location, request_id, paths)
                                    .await;
                            let _ = tx
                                .send(KovaEvent::DirectoryLoaded { tab_id, snapshot })
                                .await;
                        }),
                    );
                }
                Enumerate {
                    tab_id,
                    location,
                    request_id,
                    background,
                } => {
                    if let Some(previous) = enumerations.remove(&tab_id) {
                        previous.abort();
                    }
                    let tx = tx.clone();
                    enumerations.insert(
                        tab_id,
                        tokio::spawn(async move {
                            if !background {
                                tracing::info!(
                                    "worker: enumerate tab={:?} loc={} request={}",
                                    tab_id,
                                    location.display(),
                                    request_id
                                );
                            }
                            match crate::enumerate::enumerate_directory(
                                location.clone(),
                                request_id,
                            )
                            .await
                            {
                                Ok(snapshot) => {
                                    if !background {
                                        tracing::info!(
                                            "worker: loaded tab={:?} request={} entries={}",
                                            tab_id,
                                            request_id,
                                            snapshot.entries.len()
                                        );
                                    }
                                    let _ = tx
                                        .send(KovaEvent::DirectoryLoaded { tab_id, snapshot })
                                        .await;
                                }
                                Err(error) => {
                                    let _ = tx
                                        .send(KovaEvent::DirectoryError {
                                            tab_id,
                                            location,
                                            request_id,
                                            error_message: error.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }),
                    );
                }
                NewFolder { parent, name } => {
                    match crate::file_ops::new_folder_unique(&parent, &name).await {
                        Ok(path) => {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            let _ = tx.send(KovaEvent::FolderCreated { parent, name }).await;
                        }
                        Err(error) => {
                            let _ = tx
                                .send(KovaEvent::OperationError {
                                    context: "new folder".into(),
                                    error_message: error.to_string(),
                                })
                                .await;
                        }
                    }
                }
                Rename { path, new_name } => {
                    let old_path = path.clone();
                    match crate::file_ops::rename(&path, &new_name).await {
                        Ok(new_path) => {
                            let _ = tx.send(KovaEvent::ItemRenamed { old_path, new_path }).await;
                        }
                        Err(error) => {
                            let _ = tx
                                .send(KovaEvent::OperationError {
                                    context: "rename".into(),
                                    error_message: error.to_string(),
                                })
                                .await;
                        }
                    }
                }
                Open { path } => {
                    let path_label = path.display().to_string();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        tracing::info!("Opening {}", path_label);
                        let result = tokio::task::spawn_blocking(move || {
                            crate::file_ops::open_with_default_handler(&path)
                        })
                        .await;
                        let result = result
                            .map_err(|e| kova_core::error::OperationError::Shell(e.to_string()))
                            .and_then(|result| result);
                        if let Err(error) = result {
                            let _ = tx
                                .send(KovaEvent::OperationError {
                                    context: format!("open {}", path_label),
                                    error_message: error.to_string(),
                                })
                                .await;
                        }
                    });
                }
            }
        }
        for task in enumerations.into_values() {
            task.abort();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_tabs_do_not_retain_generations() {
        let generations = GenerationCounter::default();
        generations.next(TabId(1));
        for id in 2..10_002 {
            generations.next(TabId(id));
            generations.remove(TabId(id));
        }
        assert_eq!(generations.current.lock().unwrap().len(), 1);
        assert_eq!(generations.next(TabId(1)), 2);
    }
}
