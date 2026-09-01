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
    Enumerate {
        tab_id: TabId,
        location: Location,
        request_id: u64,
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
pub fn spawn_worker(mut rx: mpsc::Receiver<WorkerCommand>, tx: mpsc::Sender<KovaEvent>) {
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            use WorkerCommand::*;
            match cmd {
                Enumerate {
                    tab_id,
                    location,
                    request_id,
                } => {
                    tracing::info!(
                        "worker: enumerate tab={:?} loc={} request={}",
                        tab_id,
                        location.display(),
                        request_id
                    );
                    match crate::enumerate::enumerate_directory(location.clone(), request_id).await
                    {
                        Ok(snapshot) => {
                            tracing::info!(
                                "worker: loaded tab={:?} request={} entries={}",
                                tab_id,
                                request_id,
                                snapshot.entries.len()
                            );
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
                }
                NewFolder { parent, name } => {
                    match crate::file_ops::new_folder(&parent, &name).await {
                        Ok(_) => {
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
                    let display = path.display().to_string();
                    if let Err(error) = crate::file_ops::open_with_default_handler(&path) {
                        let _ = tx
                            .send(KovaEvent::OperationError {
                                context: format!("open {}", display),
                                error_message: error.to_string(),
                            })
                            .await;
                    }
                }
            }
        }
    });
}
