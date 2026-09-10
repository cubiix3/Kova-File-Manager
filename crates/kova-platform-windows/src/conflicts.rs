//! Explicit decisions for file collisions. Folder merges and unusual Shell
//! objects remain with Windows' native conflict UI and semantics.
use crate::{
    shell_ops::ShellOpCommand,
    transfers::{Conflict, ConflictChoice, TransferHandle},
};
use std::{
    collections::HashSet,
    ffi::OsString,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

#[derive(Default)]
pub struct Prepared {
    pub normal: Vec<(PathBuf, Option<OsString>)>,
    pub replace: Vec<(PathBuf, Option<OsString>)>,
}

pub fn prepare(command: &ShellOpCommand, handle: &TransferHandle) -> Result<Prepared, String> {
    let mut prepared = Prepared::default();
    let destination = match command {
        ShellOpCommand::Copy { dest, .. } | ShellOpCommand::Move { dest, .. } => Some(dest),
        ShellOpCommand::Delete { .. } => None,
    };
    let mut apply_all = None;
    let mut reserved = HashSet::new();
    for source in command.sources() {
        if handle.is_cancelled() {
            return Err("Operation cancelled".into());
        }
        let Some(dest) = destination else {
            prepared.normal.push((source.clone(), None));
            continue;
        };
        let Some(name) = source.file_name() else {
            prepared.normal.push((source.clone(), None));
            continue;
        };
        let existing = dest.join(name);
        let incoming_meta = std::fs::symlink_metadata(source);
        let existing_meta = std::fs::symlink_metadata(&existing);
        let (incoming_meta, existing_meta) = match (incoming_meta, existing_meta) {
            (Ok(incoming), Ok(existing))
                if incoming.is_file()
                    && existing.is_file()
                    && incoming.file_attributes() & 0x400 == 0
                    && existing.file_attributes() & 0x400 == 0 =>
            {
                (incoming, existing)
            }
            _ => {
                reserved.insert(existing);
                prepared.normal.push((source.clone(), None));
                continue;
            }
        };
        let choice = if let Some(choice) = apply_all {
            choice
        } else {
            let (tx, rx) = mpsc::sync_channel(1);
            let conflict = Conflict {
                incoming: source.clone(),
                existing: existing.clone(),
                incoming_info: describe(&incoming_meta),
                existing_info: describe(&existing_meta),
                reply: tx,
            };
            handle.update(|state| {
                state.status = "Waiting for a decision".into();
                state.conflict = Some(conflict);
            });
            let decision = loop {
                if handle.is_cancelled() {
                    break (ConflictChoice::Cancel, false);
                }
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(choice) => break choice,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break (ConflictChoice::Cancel, false);
                    }
                }
            };
            handle.update(|state| {
                state.status = "Preparing".into();
                state.conflict = None;
            });
            if decision.1 {
                apply_all = Some(decision.0);
            }
            decision.0
        };
        match choice {
            ConflictChoice::Cancel => {
                handle
                    .cancelled
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                return Err("Operation cancelled".into());
            }
            ConflictChoice::Skip => {
                handle.update(|state| {
                    state.remaining = state.remaining.saturating_sub(1);
                });
            }
            ConflictChoice::KeepBoth => {
                let name = unique_name(source, dest, &reserved)?;
                reserved.insert(dest.join(&name));
                prepared.normal.push((source.clone(), Some(name)));
            }
            ConflictChoice::Replace => {
                // Replacing an item with itself must not delete or truncate it.
                if source == &existing {
                    handle.update(|state| state.remaining = state.remaining.saturating_sub(1));
                } else {
                    prepared.replace.push((source.clone(), None));
                }
            }
        }
    }
    Ok(prepared)
}

fn describe(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .map(chrono::DateTime::<chrono::Local>::from)
        .map(|date| crate::formatting::date(date, true))
        .unwrap_or_else(|| "Unknown date".into());
    format!(
        "{} bytes · {modified}",
        crate::formatting::integer(metadata.len())
    )
}

fn unique_name(
    source: &Path,
    destination: &Path,
    reserved: &HashSet<PathBuf>,
) -> Result<OsString, String> {
    let stem = source.file_stem().ok_or("File has no name")?;
    for number in 1..=100_000 {
        let mut name = stem.to_os_string();
        name.push(format!(" ({number})"));
        if let Some(extension) = source.extension() {
            name.push(".");
            name.push(extension);
        }
        let candidate = destination.join(&name);
        if reserved.contains(&candidate) {
            continue;
        }
        match std::fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(name),
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
    }
    Err("Could not find an unused copy name".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keep_both_preserves_extension_and_accounts_for_reserved_names() {
        let root = std::env::temp_dir().join(format!("kova-conflict-names-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("photo (1).jpg"), b"existing").unwrap();
        let reserved = HashSet::from([root.join("photo (2).jpg")]);
        assert_eq!(
            unique_name(Path::new("photo.jpg"), &root, &reserved).unwrap(),
            "photo (3).jpg"
        );
        assert_eq!(
            std::fs::read(root.join("photo (1).jpg")).unwrap(),
            b"existing"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
