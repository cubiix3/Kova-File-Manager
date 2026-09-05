use crate::app_state::AppController;
use kova_core::domain::{FileKind, Location, LocationInput, SortColumn, TabId};
use kova_ops::worker::{GenerationCounter, WorkerCommand};
use kova_platform_windows::known_folders::initial_location;
use kova_platform_windows::path_resolver::{canonicalize_location, resolve_input};
use kova_platform_windows::shell_ops::ShellOpCommand;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Bridges UI commands to core state and worker commands, handling generation
/// IDs so stale enumeration results are discarded.
#[derive(Clone)]
pub struct CommandDispatcher {
    controller: Arc<Mutex<AppController>>,
    tx: mpsc::UnboundedSender<WorkerCommand>,
    generations: Arc<Mutex<GenerationCounter>>,
    /// Explorer-grade file operations (copy/move/delete) that must run off the
    /// UI thread on the dedicated shell-ops thread.
    ops_tx: Sender<ShellOpCommand>,
}

impl CommandDispatcher {
    pub fn new(
        controller: Arc<Mutex<AppController>>,
        tx: mpsc::UnboundedSender<WorkerCommand>,
        generations: GenerationCounter,
        ops_tx: Sender<ShellOpCommand>,
    ) -> Self {
        Self {
            controller,
            tx,
            generations: Arc::new(Mutex::new(generations)),
            ops_tx,
        }
    }

    /// Access the shared controller for UI re-sync after view-model mutations.
    pub fn controller(&self) -> Arc<Mutex<AppController>> {
        Arc::clone(&self.controller)
    }

    /// Set a user-visible status message.
    pub fn set_status_message(&self, text: String) {
        self.controller.lock().unwrap().set_status(text);
    }

    fn send(&self, cmd: WorkerCommand) {
        if self.tx.send(cmd).is_err() {
            self.set_status_message("Filesystem worker unavailable".into());
        }
    }

    fn send_ops(&self, cmd: ShellOpCommand) {
        let _ = self.ops_tx.send(cmd);
    }

    fn next_request_id(&self, tab_id: TabId) -> u64 {
        self.generations.lock().unwrap().next(tab_id)
    }

    pub fn request_enumeration(&self, tab_id: TabId, location: Location) {
        let request_id = self.next_request_id(tab_id);
        {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.record_request(tab_id, request_id);
            if location.is_home() {
                ctrl.apply_snapshot(
                    tab_id,
                    kova_core::domain::DirectorySnapshot {
                        location,
                        request_id,
                        entries: Vec::new(),
                    },
                );
                return;
            }
            ctrl.set_status(format!("Loading {}...", location.display()));
        }
        self.send(WorkerCommand::Enumerate {
            tab_id,
            location,
            request_id,
        });
    }

    pub fn refresh_tabs(&self) {
        let locations = self.controller.lock().unwrap().tab_locations();
        for (id, location) in locations {
            self.request_enumeration(id, location);
        }
    }

    pub fn dispatch_navigate(&self, input: LocationInput) -> Result<(), String> {
        let ctrl = self.controller.lock().unwrap();
        let base = ctrl
            .current_directory()
            .cloned()
            .unwrap_or_else(initial_location);
        drop(ctrl);

        let location = if input.raw.trim().eq_ignore_ascii_case("home") {
            Location::home()
        } else {
            resolve_input(&input, &base).map_err(|e| e.to_string())?
        };
        {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.navigate(location.clone());
        }
        let tab_id = self.controller.lock().unwrap().active_tab_id();
        self.request_enumeration(tab_id, location);
        Ok(())
    }

    pub fn dispatch_back(&self) -> Result<(), String> {
        let tab_id = self.controller.lock().unwrap().active_tab_id();
        let location = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.back().ok_or("cannot go back")?
        };
        self.request_enumeration(tab_id, location);
        Ok(())
    }

    pub fn dispatch_forward(&self) -> Result<(), String> {
        let tab_id = self.controller.lock().unwrap().active_tab_id();
        let location = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.forward().ok_or("cannot go forward")?
        };
        self.request_enumeration(tab_id, location);
        Ok(())
    }

    pub fn dispatch_parent(&self) -> Result<(), String> {
        let tab_id = self.controller.lock().unwrap().active_tab_id();
        let location = {
            let ctrl = self.controller.lock().unwrap();
            ctrl.parent().ok_or("cannot go to parent")?
        };
        {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.navigate(location.clone());
        }
        self.request_enumeration(tab_id, location);
        Ok(())
    }

    pub fn dispatch_refresh(&self) -> Result<(), String> {
        let (tab_id, location) = {
            let ctrl = self.controller.lock().unwrap();
            let tab_id = ctrl.active_tab_id();
            let loc = ctrl.refresh_current().ok_or("no current location")?;
            (tab_id, loc)
        };
        self.request_enumeration(tab_id, location);
        Ok(())
    }

    pub fn dispatch_new_tab(&self) {
        let initial = Location::home();
        let id = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.new_tab(initial.clone())
        };
        self.request_enumeration(id, initial);
    }

    pub fn dispatch_duplicate_location(&self) -> Result<(), String> {
        let (id, location) = {
            let mut ctrl = self.controller.lock().unwrap();
            let location = ctrl
                .current_location()
                .cloned()
                .ok_or("no current location")?;
            (ctrl.new_tab(location.clone()), location)
        };
        self.request_enumeration(id, location);
        Ok(())
    }

    /// Open the entry at `index` in a brand-new tab. Only directories are
    /// valid targets. The new tab becomes active and starts with a fresh
    /// history rooted at the target location.
    pub fn dispatch_open_in_new_tab(&self, index: usize) -> Result<(), String> {
        let location = {
            let ctrl = self.controller.lock().unwrap();
            let entry = resolve_index(&ctrl, index).ok_or("no entry at index")?;
            if !entry.is_directory() {
                return Err("only folders can be opened in a new tab".into());
            }
            canonicalize_location(&entry.path).map_err(|e| e.to_string())?
        };
        let id = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.new_tab(location.clone())
        };
        self.request_enumeration(id, location);
        Ok(())
    }
    pub fn dispatch_close_tab(&self, index: usize) -> Result<(), String> {
        let new_active = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.close_tab(index).ok_or("cannot close tab")?
        };
        let location = {
            let ctrl = self.controller.lock().unwrap();
            ctrl.current_location()
                .cloned()
                .unwrap_or_else(initial_location)
        };
        if self.controller.lock().unwrap().needs_enumeration() {
            self.request_enumeration(new_active, location);
        }
        Ok(())
    }

    pub fn dispatch_switch_tab(&self, index: usize) {
        let switched = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.switch_tab(index)
        };
        if switched && self.controller.lock().unwrap().needs_enumeration() {
            let (tab_id, location) = {
                let ctrl = self.controller.lock().unwrap();
                let id = ctrl.active_tab_id();
                let loc = ctrl
                    .current_location()
                    .cloned()
                    .unwrap_or_else(initial_location);
                (id, loc)
            };
            self.request_enumeration(tab_id, location);
        }
    }

    pub fn dispatch_select_single(&self, index: usize) {
        let mut ctrl = self.controller.lock().unwrap();
        if let Some(sel) = ctrl.selection_mut() {
            sel.select_single(index);
        }
    }

    pub fn dispatch_select_toggle(&self, index: usize) {
        let mut ctrl = self.controller.lock().unwrap();
        if let Some(sel) = ctrl.selection_mut() {
            sel.toggle(index);
        }
    }

    pub fn dispatch_select_range(&self, index: usize) {
        let mut ctrl = self.controller.lock().unwrap();
        if let Some(sel) = ctrl.selection_mut() {
            sel.range_select(index);
        }
    }

    pub fn dispatch_select_all(&self) {
        let mut ctrl = self.controller.lock().unwrap();
        let len = ctrl.snapshot().map(|s| s.entries.len()).unwrap_or(0);
        if let Some(sel) = ctrl.selection_mut() {
            sel.select_all(len);
        }
    }

    pub fn dispatch_clear_selection(&self) {
        let mut ctrl = self.controller.lock().unwrap();
        if let Some(sel) = ctrl.selection_mut() {
            sel.clear();
        }
    }

    pub fn dispatch_activate(&self, index: usize) {
        let (tab_id, path, kind) = {
            let ctrl = self.controller.lock().unwrap();
            let tab_id = ctrl.active_tab_id();
            let entry = match resolve_index(&ctrl, index) {
                Some(e) => e.clone(),
                None => {
                    tracing::warn!("activate: no entry at index {}", index);
                    return;
                }
            };
            (tab_id, entry.path.clone(), entry.kind)
        };

        if matches!(kind, FileKind::Directory | FileKind::Junction) {
            let location = match canonicalize_location(&path) {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("{e}");
                    return;
                }
            };
            {
                let mut ctrl = self.controller.lock().unwrap();
                ctrl.navigate(location.clone());
            }
            self.request_enumeration(tab_id, location);
        } else {
            self.send(WorkerCommand::Open { path });
        }
    }

    pub fn dispatch_new_folder_named(&self, name: &str) {
        if name.is_empty() {
            return;
        }
        let parent = match self.controller.lock().unwrap().current_directory().cloned() {
            Some(l) => l,
            None => return,
        };
        self.send(WorkerCommand::NewFolder {
            parent,
            name: name.to_string(),
        });
    }

    pub fn dispatch_rename_path(&self, path: PathBuf, new_name: &str) {
        if new_name.is_empty() {
            tracing::warn!("rename: empty name");
            return;
        }
        self.send(WorkerCommand::Rename {
            path,
            new_name: new_name.to_string(),
        });
    }

    /// Put the current selection on the clipboard, Explorer-compatible.
    /// `cut` marks the selection for move (Ctrl+X), otherwise copy (Ctrl+C).
    pub fn dispatch_clipboard_selection(&self, cut: bool) -> Result<(), String> {
        let paths = {
            let ctrl = self.controller.lock().unwrap();
            ctrl.selected_paths()
        };
        if paths.is_empty() {
            return Err("nothing selected".into());
        }
        kova_platform_windows::clipboard::set_clipboard_files(&paths, cut)
            .map_err(|e| e.to_string())?;
        self.set_status_message(if cut {
            format!("Cut {} item(s)", paths.len())
        } else {
            format!("Copied {} item(s)", paths.len())
        });
        Ok(())
    }

    /// Paste clipboard files into the current directory through
    /// IFileOperation on the shell-ops thread. A cut selection moves.
    pub fn dispatch_paste(&self) -> Result<(), String> {
        let files = kova_platform_windows::clipboard::get_clipboard_files()
            .map_err(|e| e.to_string())?
            .ok_or("the clipboard does not contain any files")?;
        let dest = {
            let ctrl = self.controller.lock().unwrap();
            ctrl.current_directory()
                .cloned()
                .ok_or("no current location")?
                .path
        };
        let count = files.paths.len();
        let command = if files.cut {
            ShellOpCommand::Move {
                sources: files.paths,
                dest,
            }
        } else {
            ShellOpCommand::Copy {
                sources: files.paths,
                dest,
            }
        };
        self.send_ops(command);
        self.set_status_message(format!("Pasting {} item(s)...", count));
        Ok(())
    }

    /// Send the selection to the Recycle Bin via IFileOperation.
    pub fn dispatch_delete_selection(&self) -> Result<(), String> {
        let paths = {
            let ctrl = self.controller.lock().unwrap();
            ctrl.selected_paths()
        };
        if paths.is_empty() {
            return Err("nothing selected".into());
        }
        let count = paths.len();
        self.send_ops(ShellOpCommand::Delete { sources: paths });
        self.set_status_message(format!("Deleting {} item(s)...", count));
        Ok(())
    }

    /// Open the native Windows Explorer shell context menu for the rows the
    /// user right-clicked. If the clicked row is part of the current
    /// selection, the menu targets the whole selection (Explorer behavior).
    /// The call blocks while the menu is open, so the controller lock must be
    /// released before the menu shows. Returns Ok(true) when a command was
    /// invoked and the view should refresh.
    pub fn dispatch_shell_menu(&self, index: usize) -> Result<bool, String> {
        let paths: Vec<PathBuf> = {
            let ctrl = self.controller.lock().unwrap();
            let clicked = ctrl.path_at(index).ok_or("no entry at index")?;
            let selected = ctrl.selected_paths();
            if selected.contains(&clicked) {
                selected
            } else {
                vec![clicked]
            }
        };

        let invoked = kova_platform_windows::shell_menu::show_shell_context_menu(&paths);
        if invoked {
            // The shell command may have modified the filesystem: refresh.
            let (tab_id, location) = {
                let ctrl = self.controller.lock().unwrap();
                let tab_id = ctrl.active_tab_id();
                let loc = ctrl
                    .current_location()
                    .cloned()
                    .ok_or("no current location")?;
                (tab_id, loc)
            };
            self.request_enumeration(tab_id, location);
        }
        Ok(invoked)
    }

    pub fn dispatch_sort(&self, column_index: usize) {
        let column = match column_index {
            0 => SortColumn::Name,
            1 => SortColumn::Type,
            2 => SortColumn::Size,
            3 => SortColumn::Modified,
            _ => return,
        };
        {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.set_sort(column);
        }
        // Sorting is a pure view-model operation; no filesystem re-read needed.
    }

    pub fn item_name(&self, index: usize) -> String {
        let ctrl = self.controller.lock().unwrap();
        resolve_index(&ctrl, index)
            .map(|e| e.name.clone())
            .unwrap_or_default()
    }

    pub fn item_path(&self, index: usize) -> Option<PathBuf> {
        let ctrl = self.controller.lock().unwrap();
        resolve_index(&ctrl, index).map(|entry| entry.path.clone())
    }
}

fn resolve_index(ctrl: &AppController, index: usize) -> Option<&kova_core::domain::FileEntry> {
    let snapshot = ctrl.snapshot()?;
    let idx = if index == usize::MAX {
        ctrl.primary_selection()?
    } else {
        index
    };
    snapshot.entries.get(idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn home_does_not_enqueue_filesystem_reads_or_folder_creation() {
        let controller = Arc::new(Mutex::new(AppController::new(Location::home())));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (ops_tx, _ops_rx) = std::sync::mpsc::channel();
        let dispatcher = CommandDispatcher::new(controller.clone(), tx, Default::default(), ops_tx);
        let id = controller.lock().unwrap().active_tab_id();
        dispatcher.request_enumeration(id, Location::home());
        dispatcher.dispatch_new_folder_named("must-not-be-created");
        assert!(rx.try_recv().is_err());
        assert!(!controller.lock().unwrap().is_loading());
        assert_eq!(controller.lock().unwrap().item_count(), 0);
    }
}
