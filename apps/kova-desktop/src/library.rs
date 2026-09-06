//! Local library persistence and UI binding. Loading precedes the event loop;
//! serialization and atomic replacement are owned by a background worker.
use crate::{AppState, LibraryItem, MainWindow, bridges::CommandDispatcher};
use kova_core::domain::Library;
use slint::ComponentHandle;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn save(path: &Path, library: &Library, expected: &mut Option<Vec<u8>>) -> Result<(), String> {
    let parent = path.parent().ok_or("Library has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Serialize check-and-replace across Kova processes. Windows releases the
    // exclusive handle even if a process crashes; no stale lock-file deletion.
    use std::os::windows::fs::OpenOptionsExt;
    let lock_path = path.with_extension("lock");
    let mut lock = None;
    for _ in 0..40 {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(&lock_path)
        {
            Ok(file) => {
                lock = Some(file);
                break;
            }
            Err(error) if error.raw_os_error() == Some(32) => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let _lock =
        lock.ok_or("Another Kova window is saving the library. Retry after it finishes.")?;
    if read_bytes(path)? != *expected {
        return Err("The library changed in another Kova window. Its changes were preserved. Restart Kova before editing tags, collections or pins again.".into());
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(library).map_err(|e| e.to_string())?;
    if expected.as_ref() == Some(&bytes) {
        return Ok(());
    }
    use std::io::Write;
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    *expected = Some(bytes);
    Ok(())
}

pub struct LibraryRuntime {
    timer: Option<slint::Timer>,
    sender: Option<mpsc::SyncSender<Library>>,
    worker: Option<std::thread::JoinHandle<()>>,
    controller: std::sync::Arc<std::sync::Mutex<crate::app_state::AppController>>,
    writable: bool,
    initial_revision: u64,
}
impl Drop for LibraryRuntime {
    fn drop(&mut self) {
        // This runs after the event loop, flushing even a last-second edit.
        // The storage worker remains the only thread that touches the file.
        self.timer.take();
        if let Some(sender) = self.sender.take() {
            if self.writable {
                if let Ok(ctrl) = self.controller.lock() {
                    if ctrl.library.revision != self.initial_revision {
                        let _ = sender.send(ctrl.library.clone());
                    }
                }
            }
            drop(sender);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn connect(app: &MainWindow, dispatcher: CommandDispatcher) -> LibraryRuntime {
    let state = app.global::<AppState>();
    let path = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Kova/library.json"));
    let original = path
        .as_ref()
        .ok_or("LOCALAPPDATA is unavailable".into())
        .and_then(|path| read_bytes(path));
    let restored = original
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|bytes| match bytes {
            Some(bytes) => serde_json::from_slice::<Library>(bytes).map_err(|e| {
                format!("Could not read library: {e}. The original file has been preserved.")
            }),
            None => Ok(Library::default()),
        });
    let writable = restored.is_ok();
    match restored {
        Ok(library) => dispatcher.controller().lock().unwrap().library = library,
        Err(error) => state.set_library_error(error.into()),
    }
    state.set_library_writable(writable);
    let initial_revision = dispatcher.controller().lock().unwrap().library.revision;
    let (save_tx, save_rx) = mpsc::sync_channel::<Library>(1);
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("kova-library".into())
        .spawn(move || {
            let mut expected = original.unwrap_or_default();
            while let Ok(library) = save_rx.recv() {
                let result = path
                    .as_ref()
                    .ok_or("Library location unavailable".into())
                    .and_then(|path| save(path, &library, &mut expected));
                if let Err(error) = &result {
                    tracing::warn!("Library save failed: {error}");
                }
                let _ = result_tx.send(result);
            }
        });
    if worker.is_err() {
        state.set_library_writable(false);
        state.set_library_error("Library storage worker unavailable".into());
    }
    let weak = app.as_weak();
    let actions = dispatcher.clone();
    app.global::<AppState>()
        .on_library_action(move |action, key, value| {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            if action == 6 {
                state.invoke_request_navigate(key);
                state.set_library_visible(false);
                return;
            }
            if !state.get_library_writable() {
                return;
            }
            let controller = actions.controller();
            let mut ctrl = controller.lock().unwrap();
            let paths = ctrl.selected_paths();
            let result = match action {
                0 => {
                    let path = ctrl
                        .primary_selection()
                        .and_then(|i| ctrl.snapshot()?.entries.get(i))
                        .filter(|e| e.is_directory())
                        .map(|e| e.path.clone())
                        .or_else(|| ctrl.current_directory().map(|l| l.path.clone()));
                    if let Some(path) = path {
                        ctrl.library.pin(path);
                        Ok(())
                    } else {
                        Err("Select or open a folder to pin.".into())
                    }
                }
                1..=3 => {
                    if let Ok(index) = key.parse::<usize>() {
                        if action == 1 {
                            ctrl.library.unpin(index);
                        } else {
                            ctrl.library
                                .reorder_pin(index, if action == 2 { -1 } else { 1 });
                        }
                    }
                    Ok(())
                }
                4 | 5 => ctrl.library.add(action == 5, &value, paths),
                7 => {
                    if let Some((kind, name)) = key.split_once(':') {
                        ctrl.library.remove_group(kind == "tag", name);
                    }
                    Ok(())
                }
                8 => {
                    if let Some(key) = ctrl
                        .current_location()
                        .and_then(|l| l.virtual_key())
                        .map(str::to_owned)
                    {
                        ctrl.library.remove_references(&key, &paths);
                    }
                    Ok(())
                }
                _ => return,
            };
            state.set_library_error(result.err().unwrap_or_default().into());
            if action == 4 || action == 5 {
                state.set_library_name("".into());
            }
        });
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    let sender = save_tx.clone();
    let controller = dispatcher.controller();
    let mut shown_revision = None;
    let mut pending_save = None::<Library>;
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            for result in result_rx.try_iter() {
                if let Err(error) = result {
                    state.set_library_writable(false);
                    state.set_library_error(format!("Changes could not be saved: {error}").into());
                }
            }
            let controller = dispatcher.controller();
            let ctrl = controller.lock().unwrap();
            if shown_revision != Some(ctrl.library.revision) {
                let changed = shown_revision.is_some() || ctrl.library.revision != initial_revision;
                shown_revision = Some(ctrl.library.revision);
                let pins = ctrl
                    .library
                    .pins
                    .iter()
                    .map(|path| LibraryItem {
                        name: path
                            .file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                            .as_ref()
                            .into(),
                        key: path.to_string_lossy().as_ref().into(),
                        count: 0,
                        tag: false,
                    })
                    .collect::<Vec<_>>();
                let groups = ctrl
                    .library
                    .collections
                    .iter()
                    .map(|(name, paths)| ("collection", name, paths))
                    .chain(
                        ctrl.library
                            .tags
                            .iter()
                            .map(|(name, paths)| ("tag", name, paths)),
                    )
                    .map(|(kind, name, paths)| LibraryItem {
                        name: name.as_str().into(),
                        key: format!("{kind}:{name}").into(),
                        count: paths.len() as i32,
                        tag: kind == "tag",
                    })
                    .collect::<Vec<_>>();
                state.set_quick_access(slint::ModelRc::new(slint::VecModel::from(pins)));
                state.set_library_groups(slint::ModelRc::new(slint::VecModel::from(groups)));
                if changed && state.get_library_writable() {
                    pending_save = Some(ctrl.library.clone());
                }
                let virtual_tabs = if changed {
                    ctrl.tab_locations()
                        .into_iter()
                        .filter(|(_, l)| l.virtual_key().is_some())
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                drop(ctrl);
                for (tab, location) in virtual_tabs {
                    dispatcher.request_enumeration(tab, location);
                }
            } else {
                drop(ctrl);
            }
            if let Some(pending) = pending_save.take() {
                match save_tx.try_send(pending) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(pending)) => {
                        pending_save = Some(pending);
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        state.set_library_writable(false);
                        state.set_library_error("Library storage worker unavailable".into());
                    }
                }
            }
        },
    );
    LibraryRuntime {
        timer: Some(timer),
        sender: Some(sender),
        worker: worker.ok(),
        controller,
        writable,
        initial_revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_save_replaces_existing_library_and_roundtrips_references() {
        let root = std::env::temp_dir().join(format!(
            "kova-library-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("library.json");
        let mut library = Library::default();
        let mut expected = None;
        save(&path, &library, &mut expected).unwrap();
        let mut stale = expected.clone();
        library
            .add(false, "Assets", [root.join("file.png")])
            .unwrap();
        save(&path, &library, &mut expected).unwrap();
        assert!(save(&path, &Library::default(), &mut stale).is_err());
        let restored: Library = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(restored.collections, library.collections);
        std::fs::remove_dir_all(root).unwrap();
    }
}
