use crate::{AppState, MainWindow, bridges::CommandDispatcher};
use kova_core::domain::TabId;
use kova_platform_windows::directory_watch::DirectoryWatcher;
use slint::ComponentHandle;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

pub fn connect(app: &MainWindow, dispatcher: CommandDispatcher) -> slint::Timer {
    let timer = slint::Timer::default();
    let watcher = match DirectoryWatcher::new() {
        Ok(watcher) => watcher,
        Err(error) => {
            dispatcher.set_status_message(format!("Automatic refresh unavailable: {error}"));
            return timer;
        }
    };
    let weak = app.as_weak();
    let mut previous = None;
    let mut roots: HashMap<TabId, HashSet<PathBuf>> = HashMap::new();
    let mut pending = HashSet::new();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let controller = dispatcher.controller();
            let ctrl = controller.lock().unwrap();
            let locations = ctrl.tab_locations();
            let key = (
                locations.clone(),
                ctrl.library.revision,
                ctrl.folder_sizes_enabled,
                ctrl.recursive_tabs.clone(),
            );
            if previous.as_ref() != Some(&key) {
                roots.clear();
                for (tab, location) in &locations {
                    let paths = if let Some(key) = location.virtual_key() {
                        ctrl.library
                            .entries(key)
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|path| path.parent().map(PathBuf::from))
                            .collect()
                    } else if !location.is_home() {
                        HashSet::from([location.path.clone()])
                    } else {
                        HashSet::new()
                    };
                    roots.insert(*tab, paths);
                }
                let mut watched = HashMap::new();
                for (tab, paths) in &roots {
                    let recursive = ctrl.folder_sizes_enabled || ctrl.recursive_tabs.contains(tab);
                    for path in paths {
                        *watched.entry(path.clone()).or_insert(false) |= recursive;
                    }
                }
                watcher.set_paths(watched);
                previous = Some(key);
                pending.retain(|tab| roots.contains_key(tab));
            }
            let changes = watcher.take_changes();
            for (tab, paths) in &roots {
                if !paths.is_disjoint(&changes) {
                    pending.insert(*tab);
                }
            }
            let editing = ui.global::<AppState>().get_inline_visible()
                || ui.global::<AppState>().get_creating_folder()
                || ui.global::<AppState>().get_file_menu_visible();
            let active = ctrl.active_tab_id();
            drop(ctrl);
            for (tab, location) in locations {
                if pending.contains(&tab)
                    && !(editing && tab == active)
                    && dispatcher.refresh_background(tab, location)
                {
                    pending.remove(&tab);
                }
            }
        },
    );
    timer
}
