//! Thread-safe transfer state shared by the Shell worker and the desktop.
//! No COM interfaces cross the apartment boundary.
use crate::shell_ops::ShellOpCommand;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    Replace,
    Skip,
    KeepBoth,
    Cancel,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub incoming: PathBuf,
    pub existing: PathBuf,
    pub incoming_info: String,
    pub existing_info: String,
    pub reply: mpsc::SyncSender<(ConflictChoice, bool)>,
}

#[derive(Clone, Debug)]
pub struct TransferState {
    pub id: u64,
    pub label: String,
    pub source: String,
    pub destination: String,
    pub current: String,
    pub status: String,
    pub progress: Option<f32>,
    pub bytes: u64,
    pub files: u64,
    pub remaining: usize,
    pub finished: bool,
    pub error: String,
    pub conflict: Option<Conflict>,
}

#[derive(Clone, Debug)]
pub struct TransferHandle {
    pub undo: Arc<crate::undo::History>,
    pub state: Arc<Mutex<TransferState>>,
    pub cancelled: Arc<AtomicBool>,
    pub moved: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
}
impl TransferHandle {
    pub fn update(&self, update: impl FnOnce(&mut TransferState)) {
        if let Ok(mut state) = self.state.lock() {
            update(&mut state);
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct ShellRequest {
    pub command: ShellOpCommand,
    pub handle: TransferHandle,
}

#[derive(Default)]
pub struct TransferQueue {
    pub undo: Arc<crate::undo::History>,
    next: AtomicU64,
    entries: Mutex<Vec<TransferHandle>>,
    moved: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
}
impl TransferQueue {
    pub fn enqueue(&self, command: ShellOpCommand) -> Result<ShellRequest, String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Transfer state unavailable")?;
        if command.sources().is_empty() {
            return Err("No files selected".into());
        }
        if entries
            .iter()
            .filter(|entry| entry.state.lock().is_ok_and(|state| !state.finished))
            .count()
            >= 100
        {
            return Err("The transfer queue is full. Wait for an operation to finish.".into());
        }
        while entries.len() >= 64 {
            if let Some(index) = entries
                .iter()
                .position(|entry| entry.state.lock().is_ok_and(|state| state.finished))
            {
                entries.remove(index);
            } else {
                break;
            }
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let sources = command.sources();
        let source = sources
            .iter()
            .take(3)
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("; ")
            + if sources.len() > 3 { "; …" } else { "" };
        let destination = match &command {
            ShellOpCommand::Copy { dest, .. } | ShellOpCommand::Move { dest, .. } => {
                dest.display().to_string()
            }
            ShellOpCommand::Delete { .. } => "Recycle Bin".into(),
        };
        let state = TransferState {
            id,
            label: command.label().into(),
            source,
            destination,
            current: String::new(),
            status: "Queued".into(),
            progress: None,
            bytes: 0,
            files: 0,
            remaining: sources.len(),
            finished: false,
            error: String::new(),
            conflict: None,
        };
        let handle = TransferHandle {
            undo: self.undo.clone(),
            state: Arc::new(Mutex::new(state)),
            cancelled: Arc::new(AtomicBool::new(false)),
            moved: self.moved.clone(),
        };
        entries.push(handle.clone());
        Ok(ShellRequest { command, handle })
    }
    pub fn snapshots(&self) -> Vec<TransferState> {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.state.lock().ok().map(|state| state.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn cancel(&self, id: u64) {
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.iter() {
                if entry
                    .state
                    .lock()
                    .is_ok_and(|state| state.id == id && !state.finished)
                {
                    entry.cancelled.store(true, Ordering::Relaxed);
                }
            }
        }
    }
    pub fn resolve(&self, id: u64, choice: ConflictChoice, all: bool) {
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.iter() {
                if let Ok(mut state) = entry.state.lock() {
                    if state.id == id {
                        if let Some(conflict) = state.conflict.take() {
                            let _ = conflict.reply.try_send((choice, all));
                        }
                    }
                }
            }
        }
    }
    pub fn take_moves(&self) -> Vec<(PathBuf, PathBuf)> {
        self.moved
            .lock()
            .map(|mut moves| std::mem::take(&mut *moves))
            .unwrap_or_default()
    }
    pub fn clear_finished(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|entry| !entry.state.lock().is_ok_and(|state| state.finished));
        }
    }
}
