//! Explorer-grade file operations executed off the UI thread.
//!
//! All operations run through `IFileOperation` (the same engine Explorer
//! uses), which provides native progress dialogs, conflict handling,
//! undo/Recycle Bin integration and correct long-path/attribute semantics.
//! A dedicated thread owns a COM apartment; the UI thread only enqueues
//! commands and receives outcomes, so the interface never blocks.

use crate::transfers::ShellRequest;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR, FileOperation, IFileOperation, ILFree, IShellItem,
    IShellItemArray, SHCreateItemFromParsingName, SHCreateShellItemArrayFromIDLists,
    SHParseDisplayName,
};

/// A file operation requested by the UI.
#[derive(Debug, Clone)]
pub enum ShellOpCommand {
    /// Copy sources into `dest` (paste of a copied clipboard selection).
    Copy {
        sources: Vec<PathBuf>,
        dest: PathBuf,
    },
    /// Move sources into `dest` (paste of a cut clipboard selection).
    Move {
        sources: Vec<PathBuf>,
        dest: PathBuf,
    },
    /// Send sources to the Recycle Bin.
    Delete { sources: Vec<PathBuf> },
}

impl ShellOpCommand {
    /// Short user-facing label for status messages.
    pub fn label(&self) -> &'static str {
        match self {
            ShellOpCommand::Copy { .. } => "copy",
            ShellOpCommand::Move { .. } => "move",
            ShellOpCommand::Delete { .. } => "delete",
        }
    }

    pub fn sources(&self) -> &[PathBuf] {
        match self {
            ShellOpCommand::Copy { sources, .. }
            | ShellOpCommand::Move { sources, .. }
            | ShellOpCommand::Delete { sources } => sources,
        }
    }
}

/// Result of a shell file operation, delivered back to the UI thread.
#[derive(Debug, Clone)]
pub enum ShellOpOutcome {
    /// The engine reports success for all items.
    Completed { summary: String },
    /// The engine failed; `message` is the user-relevant error text and
    /// `code` the raw HRESULT (0 when the failure was not a COM error), so
    /// the UI can distinguish user cancellations from real errors.
    Failed {
        summary: String,
        message: String,
        code: i32,
    },
}

/// A failure with its optional HRESULT.
struct OpFailure {
    message: String,
    code: i32,
}

impl OpFailure {
    fn com(error: windows::core::Error) -> Self {
        Self {
            message: error.to_string(),
            code: error.code().0,
        }
    }

    fn plain(message: String) -> Self {
        Self { message, code: 0 }
    }
}

/// Spawn the dedicated shell operations thread. It initializes its own COM
/// apartment and processes commands strictly one at a time so the native
/// progress UI is never interleaved.
pub fn spawn_shell_ops_thread(
    rx: Receiver<ShellRequest>,
    tx: Sender<ShellOpOutcome>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("kova-shell-ops".into())
        .spawn(move || {
            init_com_sta();
            while let Ok(command) = rx.recv() {
                let outcome = execute(&command);
                let _ = tx.send(outcome);
            }
        })
        .expect("spawn kova-shell-ops thread")
}

fn init_com_sta() {
    crate::com::ensure_sta();
}

/// Run one operation through IFileOperation. Never panics on shell errors;
/// failures become `ShellOpOutcome::Failed`.
fn execute(request: &ShellRequest) -> ShellOpOutcome {
    let command = &request.command;
    let handle = &request.handle;
    let summary = format!(
        "{} {} item(s)",
        capitalize(command.label()),
        command.sources().len()
    );
    handle.update(|state| state.status = "Preparing".into());
    let run = || -> Result<(), OpFailure> {
        let prepared = crate::conflicts::prepare(command, handle).map_err(OpFailure::plain)?;
        let total = (prepared.normal.len() + prepared.replace.len()).max(1) as f32;
        let normal_weight = prepared.normal.len() as f32 / total;
        handle.update(|state| state.status = "Running".into());
        execute_group(command, handle, &prepared.normal, false, 0.0, normal_weight)?;
        execute_group(
            command,
            handle,
            &prepared.replace,
            true,
            normal_weight,
            1.0 - normal_weight,
        )?;
        if handle.is_cancelled() {
            return Err(OpFailure {
                message: "Operation cancelled; some items may have completed".into(),
                code: 0x800704c7_u32 as i32,
            });
        }
        if let Some(error) = handle
            .state
            .lock()
            .ok()
            .map(|state| state.error.clone())
            .filter(|error| !error.is_empty())
        {
            return Err(OpFailure::plain(error));
        }
        Ok(())
    };
    match run() {
        Ok(()) => {
            handle.update(|state| {
                state.status = "Completed".into();
                state.finished = true;
                state.remaining = 0;
                state.progress = Some(1.0);
                state.conflict = None;
            });
            ShellOpOutcome::Completed { summary }
        }
        Err(mut failure) => {
            let cancelled = handle.is_cancelled()
                || matches!(failure.code as u32, 0x800704c7 | 0x80270000 | 0x80004004);
            if cancelled {
                failure.message = "Cancelled. Files already completed are retained.".into();
            }
            handle.update(|state| {
                state.status = if cancelled { "Cancelled" } else { "Failed" }.into();
                state.error = failure.message.clone();
                state.finished = true;
                state.conflict = None;
            });
            ShellOpOutcome::Failed {
                summary,
                message: failure.message,
                code: if cancelled {
                    0x800704c7_u32 as i32
                } else {
                    failure.code
                },
            }
        }
    }
}

fn execute_group(
    command: &ShellOpCommand,
    handle: &crate::transfers::TransferHandle,
    items: &[(PathBuf, Option<std::ffi::OsString>)],
    replace: bool,
    base: f32,
    weight: f32,
) -> Result<(), OpFailure> {
    use windows::Win32::UI::Shell::{
        FOF_NOCONFIRMATION, FOF_SILENT, FOF_WANTNUKEWARNING, FOFX_ADDUNDORECORD,
        FOFX_RECYCLEONDELETE, IFileOperationProgressSink,
    };
    use windows::core::PCWSTR;
    if items.is_empty() {
        return Ok(());
    }
    if handle.is_cancelled() {
        return Err(OpFailure::plain("Operation cancelled".into()));
    }
    // SAFETY: every interface is created, used and released on this STA worker.
    unsafe {
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(OpFailure::com)?;
        let mut flags = FOF_ALLOWUNDO
            | FOF_NOCONFIRMMKDIR
            | FOF_SILENT
            | FOF_WANTNUKEWARNING
            | FOFX_ADDUNDORECORD;
        if matches!(command, ShellOpCommand::Delete { .. }) {
            flags |= FOFX_RECYCLEONDELETE;
        }
        if replace {
            flags |= FOF_NOCONFIRMATION;
        }
        operation.SetOperationFlags(flags).map_err(OpFailure::com)?;
        let sources = items
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        let sink: IFileOperationProgressSink = crate::transfer_progress::ProgressSink::new(
            handle.clone(),
            &sources,
            base,
            weight,
            !replace,
        )
        .into();
        let cookie = operation.Advise(&sink).map_err(OpFailure::com)?;
        struct Advice {
            operation: IFileOperation,
            cookie: u32,
        }
        impl Drop for Advice {
            fn drop(&mut self) {
                unsafe {
                    let _ = self.operation.Unadvise(self.cookie);
                }
            }
        }
        let _advice = Advice {
            operation: operation.clone(),
            cookie,
        };
        match command {
            ShellOpCommand::Delete { .. } => {
                operation
                    .DeleteItems(&shell_item_array(&sources).map_err(OpFailure::plain)?)
                    .map_err(OpFailure::com)?;
            }
            ShellOpCommand::Copy { dest, .. } | ShellOpCommand::Move { dest, .. } => {
                let destination = shell_item(dest).map_err(OpFailure::plain)?;
                if items.iter().all(|(_, name)| name.is_none()) {
                    let array = shell_item_array(&sources).map_err(OpFailure::plain)?;
                    if matches!(command, ShellOpCommand::Copy { .. }) {
                        operation
                            .CopyItems(&array, &destination)
                            .map_err(OpFailure::com)?;
                    } else {
                        operation
                            .MoveItems(&array, &destination)
                            .map_err(OpFailure::com)?;
                    }
                } else {
                    for (source, name) in items {
                        let item = shell_item(source).map_err(OpFailure::plain)?;
                        let wide = name
                            .as_ref()
                            .map(|name| name.encode_wide().chain(Some(0)).collect::<Vec<_>>());
                        let name = wide
                            .as_ref()
                            .map(|wide| PCWSTR(wide.as_ptr()))
                            .unwrap_or(PCWSTR::null());
                        if matches!(command, ShellOpCommand::Copy { .. }) {
                            operation
                                .CopyItem(&item, &destination, name, None)
                                .map_err(OpFailure::com)?;
                        } else {
                            operation
                                .MoveItem(&item, &destination, name, None)
                                .map_err(OpFailure::com)?;
                        }
                    }
                }
            }
        }
        let performed = operation.PerformOperations();
        let aborted = operation
            .GetAnyOperationsAborted()
            .map_err(OpFailure::com)?;
        performed.map_err(OpFailure::com)?;
        if aborted.as_bool() {
            return Err(OpFailure {
                message: "Operation cancelled; some items may have completed".into(),
                code: 0x800704c7_u32 as i32,
            });
        }
    }
    Ok(())
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// SAFETY: COM STA must be initialized on the calling thread.
unsafe fn shell_item(path: &Path) -> Result<IShellItem, String> {
    unsafe {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        SHCreateItemFromParsingName::<_, _, IShellItem>(windows::core::PCWSTR(wide.as_ptr()), None)
            .map_err(|e| format!("shell item {}: {e}", path.display()))
    }
}

/// SAFETY: COM STA must be initialized on the calling thread.
unsafe fn shell_item_array(paths: &[PathBuf]) -> Result<IShellItemArray, String> {
    unsafe {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
        for path in paths {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            if let Err(error) = SHParseDisplayName(
                windows::core::PCWSTR(wide.as_ptr()),
                None,
                &mut pidl,
                0,
                None,
            ) {
                for allocated in &pidls {
                    ILFree(Some(*allocated as *const _));
                }
                return Err(format!("resolve {}: {error}", path.display()));
            }
            pidls.push(pidl);
        }
        let refs: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
        let result = SHCreateShellItemArrayFromIDLists(&refs);
        for pidl in &pidls {
            ILFree(Some(*pidl as *const _));
        }
        result.map_err(|e| format!("shell item array: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_labels_are_user_facing() {
        let cmd = ShellOpCommand::Copy {
            sources: vec![PathBuf::from("C:\\a")],
            dest: PathBuf::from("C:\\b"),
        };
        assert_eq!(cmd.label(), "copy");
        assert_eq!(cmd.sources().len(), 1);

        let cmd = ShellOpCommand::Delete {
            sources: Vec::new(),
        };
        assert_eq!(cmd.label(), "delete");
    }

    #[test]
    fn capitalize_builds_status_words() {
        assert_eq!(capitalize("copy"), "Copy");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    #[ignore = "runs real IFileOperation copy/move on an interactive Windows session"]
    fn native_transfers_preserve_contents_and_explicit_conflict_decisions() {
        use crate::transfers::{ConflictChoice, TransferQueue};
        use std::{
            sync::Arc,
            time::{Duration, Instant},
        };
        let root = std::env::temp_dir().join(format!(
            "kova-native-transfers-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = root.join("source");
        let dest = root.join("destination");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let queue = Arc::new(TransferQueue::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let (out_tx, out_rx) = std::sync::mpsc::channel();
        let worker = spawn_shell_ops_thread(rx, out_tx);
        for choice in [
            ConflictChoice::KeepBoth,
            ConflictChoice::Skip,
            ConflictChoice::Replace,
            ConflictChoice::Cancel,
        ] {
            for name in ["first.txt", "second.txt"] {
                std::fs::write(source.join(name), b"incoming").unwrap();
                std::fs::write(dest.join(name), b"existing").unwrap();
            }
            let request = queue
                .enqueue(ShellOpCommand::Copy {
                    sources: vec![source.join("first.txt"), source.join("second.txt")],
                    dest: dest.clone(),
                })
                .unwrap();
            let id = request.handle.state.lock().unwrap().id;
            tx.send(request).unwrap();
            let start = Instant::now();
            while !queue
                .snapshots()
                .iter()
                .any(|state| state.id == id && state.conflict.is_some())
            {
                assert!(start.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(10));
            }
            // Waiting for the user's choice must never mutate either side.
            assert_eq!(std::fs::read(dest.join("first.txt")).unwrap(), b"existing");
            queue.resolve(id, choice, true);
            let outcome = out_rx.recv_timeout(Duration::from_secs(20)).unwrap();
            assert_eq!(
                matches!(outcome, ShellOpOutcome::Completed { .. }),
                choice != ConflictChoice::Cancel,
                "{outcome:?}"
            );
            for name in ["first.txt", "second.txt"] {
                assert_eq!(std::fs::read(source.join(name)).unwrap(), b"incoming");
                assert_eq!(
                    std::fs::read(dest.join(name)).unwrap(),
                    if choice == ConflictChoice::Replace {
                        b"incoming"
                    } else {
                        b"existing"
                    }
                );
            }
            if choice == ConflictChoice::KeepBoth {
                assert_eq!(
                    std::fs::read(dest.join("first (1).txt")).unwrap(),
                    b"incoming"
                );
                assert_eq!(
                    std::fs::read(dest.join("second (1).txt")).unwrap(),
                    b"incoming"
                );
            }
        }
        std::fs::write(source.join("move.txt"), b"move intact").unwrap();
        tx.send(
            queue
                .enqueue(ShellOpCommand::Move {
                    sources: vec![source.join("move.txt")],
                    dest: dest.clone(),
                })
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            out_rx.recv_timeout(Duration::from_secs(20)).unwrap(),
            ShellOpOutcome::Completed { .. }
        ));
        assert!(!source.join("move.txt").exists());
        assert_eq!(
            std::fs::read(dest.join("move.txt")).unwrap(),
            b"move intact"
        );
        assert!(
            queue
                .take_moves()
                .contains(&(source.join("move.txt"), dest.join("move.txt")))
        );
        drop(tx);
        worker.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
