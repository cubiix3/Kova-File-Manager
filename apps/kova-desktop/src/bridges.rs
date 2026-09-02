use crate::app_state::AppController;
use kova_core::domain::{FileKind, Location, LocationInput, SortColumn, TabId};
use kova_ops::worker::{GenerationCounter, WorkerCommand};
use kova_platform_windows::known_folders::initial_location;
use kova_platform_windows::path_resolver::{canonicalize_location, resolve_input};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Bridges UI commands to core state and worker commands, handling generation
/// IDs so stale enumeration results are discarded.
#[derive(Clone)]
pub struct CommandDispatcher {
    controller: Arc<Mutex<AppController>>,
    tx: mpsc::Sender<WorkerCommand>,
    generations: Arc<Mutex<GenerationCounter>>,
}

impl CommandDispatcher {
    pub fn new(
        controller: Arc<Mutex<AppController>>,
        tx: mpsc::Sender<WorkerCommand>,
        generations: GenerationCounter,
    ) -> Self {
        Self {
            controller,
            tx,
            generations: Arc::new(Mutex::new(generations)),
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
        let _ = self.tx.try_send(cmd);
    }

    fn next_request_id(&self, tab_id: TabId) -> u64 {
        self.generations.lock().unwrap().next(tab_id)
    }

    pub fn request_enumeration(&self, tab_id: TabId, location: Location) {
        let request_id = self.next_request_id(tab_id);
        {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.record_request(tab_id, request_id);
            ctrl.set_status(format!("Loading {}...", location.display()));
        }
        self.send(WorkerCommand::Enumerate {
            tab_id,
            location,
            request_id,
        });
    }

    pub fn dispatch_navigate(&self, input: LocationInput) -> Result<(), String> {
        let ctrl = self.controller.lock().unwrap();
        let base = ctrl
            .current_location()
            .cloned()
            .unwrap_or_else(initial_location);
        drop(ctrl);

        let location = resolve_input(&input, &base).map_err(|e| e.to_string())?;
        if !location.path.exists() {
            return Err(format!("path does not exist: {}", location.display()));
        }
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
        let initial = initial_location();
        let id = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.new_tab(initial.clone())
        };
        self.request_enumeration(id, initial);
    }

    /// Open the entry at `index` in a brand-new tab. Only directories are
    /// valid targets. The new tab becomes active and starts with a fresh
    /// history rooted at the target location.
    pub fn dispatch_open_in_new_tab(&self, index: usize) -> Result<(), String> {
        let location = {
            let ctrl = self.controller.lock().unwrap();
            let entry = resolve_index(&ctrl, index).ok_or("no entry at index")?;
            if entry.kind != FileKind::Directory {
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
        self.request_enumeration(new_active, location);
        Ok(())
    }

    pub fn dispatch_switch_tab(&self, index: usize) {
        let switched = {
            let mut ctrl = self.controller.lock().unwrap();
            ctrl.switch_tab(index)
        };
        if switched {
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

        if kind == FileKind::Directory {
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
        let parent = match self.controller.lock().unwrap().current_location().cloned() {
            Some(l) => l,
            None => return,
        };
        self.send(WorkerCommand::NewFolder {
            parent,
            name: name.to_string(),
        });
    }

    pub fn dispatch_rename_to(&self, index: usize, new_name: &str) {
        if new_name.is_empty() {
            tracing::warn!("rename: empty name");
            return;
        }
        let path = {
            let ctrl = self.controller.lock().unwrap();
            match resolve_index(&ctrl, index).map(|e| e.path.clone()) {
                Some(p) => p,
                None => {
                    tracing::warn!("rename: no entry at index {}", index);
                    return;
                }
            }
        };
        self.send(WorkerCommand::Rename {
            path,
            new_name: new_name.to_string(),
        });
    }

    /// Copy the full Windows path of the entry at `index` to the clipboard.
    pub fn dispatch_copy_path(&self, index: usize) -> Result<(), String> {
        let path = {
            let ctrl = self.controller.lock().unwrap();
            resolve_index(&ctrl, index)
                .map(|e| e.path.clone())
                .ok_or("no entry at index")?
        };
        kova_platform_windows::clipboard::set_clipboard_text(&path.display().to_string())
            .map_err(|e| e.to_string())?;
        Ok(())
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
}

fn resolve_index(ctrl: &AppController, index: usize) -> Option<&kova_core::domain::FileEntry> {
    let snapshot = ctrl.snapshot()?;
    let idx = if index == usize::MAX {
        ctrl.primary_selection().unwrap_or(0)
    } else {
        index
    };
    snapshot.entries.get(idx)
}
