//! Versioned workspace persistence with debounced, atomic background writes.
use crate::{AppState, MainWindow, app_state::AppController};
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Session {
    version: u32,
    tabs: Vec<SavedTab>,
    active: usize,
    recent: Vec<PathBuf>,
    width: f32,
    height: f32,
    maximized: bool,
    inspector_width: f32,
    columns: [f32; 3],
    gallery_size: i32,
    gallery: bool,
    hidden: bool,
    system: bool,
    extensions: bool,
    preview: bool,
    compact: bool,
    alternating: bool,
    animations: bool,
    folder_sizes: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct SavedTab {
    filters: [usize; 3],
    recursive: bool,
    path: PathBuf,
    virtual_key: Option<String>,
    search: String,
    sort: usize,
    descending: bool,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            version: 1,
            tabs: Vec::new(),
            active: 0,
            recent: Vec::new(),
            width: 1280.,
            height: 820.,
            maximized: false,
            inspector_width: 340.,
            columns: [140., 148., 200.],
            gallery_size: 1,
            gallery: false,
            hidden: false,
            system: false,
            extensions: true,
            preview: false,
            compact: true,
            alternating: false,
            animations: true,
            folder_sizes: false,
        }
    }
}

fn session_path() -> Option<PathBuf> {
    Some(path()?.with_file_name("session.json"))
}

pub fn restore(app: &MainWindow, ctrl: &mut AppController, restore_tabs: bool) -> bool {
    let Some(path) = session_path() else {
        return false;
    };
    let saved = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Session>(&bytes) {
            Ok(session) if session.version == 1 => session,
            _ => {
                tracing::warn!(
                    "Session unreadable or newer than this application; original preserved"
                );
                return false;
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            restore_legacy(app, ctrl);
            return true;
        }
        Err(error) => {
            tracing::warn!(%error, "Cannot read session; original preserved");
            return false;
        }
    };
    let state = app.global::<AppState>();
    state.set_gallery(saved.gallery);
    state.set_gallery_size(saved.gallery_size.clamp(0, 2));
    state.set_show_hidden(saved.hidden);
    state.set_show_system(saved.system);
    state.set_show_extensions(saved.extensions);
    state.set_preview_visible(saved.preview);
    state.set_compact_rows(saved.compact);
    state.set_alternating_rows(saved.alternating);
    state.set_animations(saved.animations);
    state.set_folder_sizes(saved.folder_sizes);
    ctrl.set_visibility(saved.hidden, saved.system);
    ctrl.show_extensions = saved.extensions;
    ctrl.folder_sizes_enabled = saved.folder_sizes;
    ctrl.recent_folders = saved.recent.into_iter().take(12).collect();
    app.set_inspector_width(saved.inspector_width.clamp(240., 800.));
    app.set_saved_type_width(saved.columns[0].clamp(60., 600.));
    app.set_saved_size_width(saved.columns[1].clamp(60., 600.));
    app.set_saved_date_width(saved.columns[2].clamp(80., 600.));
    app.set_normal_width(saved.width.clamp(760., 3840.));
    app.set_normal_height(saved.height.clamp(500., 2160.));
    app.window().set_size(slint::LogicalSize::new(
        app.get_normal_width(),
        app.get_normal_height(),
    ));
    app.window()
        .with_winit_window(|window| window.set_maximized(saved.maximized));
    app.set_restore_maximized(saved.maximized);
    app.set_window_maximized(saved.maximized);
    if restore_tabs && !saved.tabs.is_empty() {
        for (index, tab) in saved.tabs.into_iter().take(50).enumerate() {
            let location = if let Some(key) = tab.virtual_key {
                kova_core::domain::Location::virtual_folder(key)
            } else if tab.path.as_os_str().is_empty() {
                kova_core::domain::Location::home()
            } else {
                kova_core::domain::Location::new(tab.path)
            };
            if index == 0 {
                ctrl.tabs = kova_core::domain::TabCollection::new(location);
            } else {
                ctrl.new_tab(location);
            }
            let active = ctrl.tabs.active_mut().unwrap();
            active.sort = kova_core::domain::SortDescriptor::new(
                match tab.sort {
                    1 => kova_core::domain::SortColumn::Type,
                    2 => kova_core::domain::SortColumn::Size,
                    3 => kova_core::domain::SortColumn::Modified,
                    _ => kova_core::domain::SortColumn::Name,
                },
                if tab.descending {
                    kova_core::domain::SortDirection::Descending
                } else {
                    kova_core::domain::SortDirection::Ascending
                },
            );
            ctrl.filters.insert(
                active.id,
                [
                    tab.filters[0].min(6),
                    tab.filters[1].min(3),
                    tab.filters[2].min(3),
                ],
            );
            if tab.recursive {
                ctrl.recursive_tabs.insert(active.id);
            }
            ctrl.searches.insert(active.id, tab.search);
        }
        ctrl.switch_tab(saved.active.min(ctrl.tabs.tabs().len() - 1));
    }
    true
}

fn capture(app: &MainWindow, ctrl: &AppController, normal_size: &mut (f32, f32)) -> Session {
    let state = app.global::<AppState>();
    let mut maximized = app.get_window_maximized();
    app.window().with_winit_window(|window| {
        maximized = window.is_maximized();
        if !maximized && !window.is_minimized().unwrap_or(false) {
            let size = window.inner_size().to_logical::<f32>(window.scale_factor());
            *normal_size = (size.width, size.height);
        }
    });
    Session {
        tabs: ctrl
            .tabs
            .tabs()
            .iter()
            .filter_map(|t| {
                t.current_location().map(|loc| SavedTab {
                    filters: ctrl.filters.get(&t.id).copied().unwrap_or_default(),
                    recursive: ctrl.recursive_tabs.contains(&t.id),
                    path: loc.path.clone(),
                    virtual_key: loc.virtual_key().map(str::to_owned),
                    search: ctrl.searches.get(&t.id).cloned().unwrap_or_default(),
                    sort: t.sort.column.as_index(),
                    descending: t.sort.direction == kova_core::domain::SortDirection::Descending,
                })
            })
            .collect(),
        active: ctrl.active_tab_index(),
        recent: ctrl.recent_folders.clone(),
        width: normal_size.0,
        height: normal_size.1,
        maximized,
        inspector_width: app.get_inspector_width(),
        columns: [
            app.get_saved_type_width(),
            app.get_saved_size_width(),
            app.get_saved_date_width(),
        ],
        gallery_size: state.get_gallery_size(),
        gallery: state.get_gallery(),
        hidden: state.get_show_hidden(),
        system: state.get_show_system(),
        extensions: state.get_show_extensions(),
        preview: state.get_preview_visible(),
        compact: state.get_compact_rows(),
        alternating: state.get_alternating_rows(),
        animations: state.get_animations(),
        folder_sizes: state.get_folder_sizes(),
        ..Session::default()
    }
}

fn atomic_save(path: &Path, session: &Session) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(session).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().ok_or("No settings directory")?)
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

pub struct Persistence {
    timer: slint::Timer,
    sender: Option<mpsc::Sender<Session>>,
    worker: Option<std::thread::JoinHandle<()>>,
    app: slint::Weak<MainWindow>,
    ctrl: Arc<Mutex<AppController>>,
    normal: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
}
impl Drop for Persistence {
    fn drop(&mut self) {
        // Drop the callback's sender clone before joining the storage worker.
        self.timer = slint::Timer::default();
        if let Some(sender) = self.sender.take() {
            if let Some(app) = self.app.upgrade() {
                let _ = sender.send(capture(
                    &app,
                    &self.ctrl.lock().unwrap(),
                    &mut self.normal.get(),
                ));
            }
            drop(sender);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
pub fn connect(app: &MainWindow, ctrl: Arc<Mutex<AppController>>, writable: bool) -> Persistence {
    let (sender, input) = mpsc::channel::<Session>();
    let (results, output) = mpsc::channel::<Result<(), String>>();
    let path = session_path();
    let worker = if writable {
        std::thread::Builder::new()
            .name("kova-session".into())
            .spawn(move || {
                while let Ok(mut session) = input.recv() {
                    while let Ok(newer) = input.try_recv() {
                        session = newer;
                    }
                    let result = path
                        .as_ref()
                        .ok_or("Settings location unavailable".into())
                        .and_then(|p| atomic_save(p, &session));
                    let _ = results.send(result);
                }
            })
            .ok()
    } else {
        None
    };
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    let controller = ctrl.clone();
    let saves = sender.clone();
    let normal = std::rc::Rc::new(std::cell::Cell::new((
        app.get_normal_width(),
        app.get_normal_height(),
    )));
    let remembered = normal.clone();
    let mut last = capture(app, &ctrl.lock().unwrap(), &mut normal.get());
    let mut changed = Some(Instant::now());
    if worker.is_some() {
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(200),
            move || {
                let Some(app) = weak.upgrade() else { return };
                let mut dimensions = remembered.get();
                let current = capture(&app, &controller.lock().unwrap(), &mut dimensions);
                remembered.set(dimensions);
                if current != last {
                    last = current;
                    changed = Some(Instant::now());
                }
                if changed.is_some_and(|when: Instant| when.elapsed() >= Duration::from_millis(600))
                {
                    let _ = saves.send(last.clone());
                    changed = None;
                }
                for result in output.try_iter() {
                    if let Err(error) = result {
                        tracing::warn!(%error, "Session save failed");
                        app.global::<AppState>()
                            .set_status_text(format!("Could not save workspace: {error}").into());
                    }
                }
            },
        );
    }
    Persistence {
        timer,
        sender: worker.as_ref().map(|_| sender),
        worker,
        app: app.as_weak(),
        ctrl,
        normal,
    }
}
fn path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Kova/view-options.txt"))
}
fn restore_legacy(app: &MainWindow, controller: &mut AppController) {
    let Some(path) = path() else { return };
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let state = app.global::<AppState>();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key == "gallery_size" {
            if let Ok(size) = value.parse::<i32>() {
                state.set_gallery_size(size.clamp(0, 2));
            }
            continue;
        }
        let value = match value {
            "true" => true,
            "false" => false,
            _ => continue,
        };
        match key {
            "gallery" => state.set_gallery(value),
            "hidden" => state.set_show_hidden(value),
            "system" => state.set_show_system(value),
            "extensions" => state.set_show_extensions(value),
            "preview" => state.set_preview_visible(value),
            "compact" => state.set_compact_rows(value),
            "alternating" => state.set_alternating_rows(value),
            "animations" => state.set_animations(value),
            "folder_sizes" => state.set_folder_sizes(value),
            _ => {}
        }
    }
    controller.set_visibility(state.get_show_hidden(), state.get_show_system());
    controller.show_extensions = state.get_show_extensions();
    controller.folder_sizes_enabled = state.get_folder_sizes();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_session_replacement_roundtrips_tabs_and_geometry() {
        let root = std::env::temp_dir().join(format!("kova-session-test-{}", std::process::id()));
        let path = root.join("session.json");
        let mut session = Session::default();
        atomic_save(&path, &session).unwrap();
        session.tabs.push(SavedTab {
            path: PathBuf::from(r"Z:\temporarily-unavailable"),
            search: "report".into(),
            recursive: true,
            filters: [2, 0, 1],
            ..SavedTab::default()
        });
        session.width = 1440.;
        session.columns = [180., 140., 220.];
        atomic_save(&path, &session).unwrap();
        assert_eq!(
            serde_json::from_slice::<Session>(&std::fs::read(&path).unwrap()).unwrap(),
            session
        );
        assert!(
            !path
                .with_extension(format!("{}.tmp", std::process::id()))
                .exists()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
