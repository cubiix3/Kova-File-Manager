//! Explorer-grade file operations executed off the UI thread.
//!
//! All operations run through `IFileOperation` (the same engine Explorer
//! uses), which provides native progress dialogs, conflict handling,
//! undo/Recycle Bin integration and correct long-path/attribute semantics.
//! A dedicated thread owns a COM apartment; the UI thread only enqueues
//! commands and receives outcomes, so the interface never blocks.

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
    rx: Receiver<ShellOpCommand>,
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
fn execute(command: &ShellOpCommand) -> ShellOpOutcome {
    let summary = format!(
        "{} {} item(s)",
        capitalize(command.label()),
        command.sources().len()
    );

    let run = || -> Result<(), OpFailure> {
        // SAFETY: COM STA was initialized on this thread; every handle is
        // valid for the duration of the call and freed on scope exit.
        unsafe {
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(OpFailure::com)?;

            operation
                .SetOperationFlags(FOF_ALLOWUNDO | FOF_NOCONFIRMMKDIR)
                .map_err(OpFailure::com)?;

            let item_array = shell_item_array(command.sources()).map_err(OpFailure::plain)?;

            match command {
                ShellOpCommand::Copy { dest, .. } | ShellOpCommand::Move { dest, .. } => {
                    let destination = shell_item(dest).map_err(OpFailure::plain)?;
                    if matches!(command, ShellOpCommand::Copy { .. }) {
                        operation
                            .CopyItems(&item_array, &destination)
                            .map_err(OpFailure::com)?;
                    } else {
                        operation
                            .MoveItems(&item_array, &destination)
                            .map_err(OpFailure::com)?;
                    }
                }
                ShellOpCommand::Delete { .. } => {
                    operation.DeleteItems(&item_array).map_err(OpFailure::com)?;
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
                    code: 0x8007_04C7u32 as i32,
                });
            }
            Ok(())
        }
    };

    match run() {
        Ok(()) => ShellOpOutcome::Completed { summary },
        Err(failure) => ShellOpOutcome::Failed {
            summary,
            message: failure.message,
            code: failure.code,
        },
    }
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
}
