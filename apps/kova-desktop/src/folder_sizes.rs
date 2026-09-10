use crate::{AppState, LastAddress, MainWindow, UiModels, app_state::AppController, update_ui};
use slint::ComponentHandle;
use std::{
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

pub fn connect(
    app: &MainWindow,
    controller: Arc<Mutex<AppController>>,
    last_address: LastAddress,
    models: Rc<UiModels>,
) -> slint::Timer {
    let (requests, input) = mpsc::sync_channel::<(u64, Vec<PathBuf>)>(1);
    let (results, output) =
        mpsc::sync_channel::<(u64, PathBuf, Option<kova_core::domain::EffectiveSize>)>(64);
    let generation = Arc::new(AtomicU64::new(0));
    let latest = generation.clone();
    let worker = std::thread::Builder::new()
        .name("kova-folder-sizes".into())
        .spawn(move || {
            while let Ok((id, paths)) = input.recv() {
                for path in paths {
                    if latest.load(Ordering::Relaxed) != id {
                        break;
                    }
                    let label = if !kova_platform_windows::folder_size::is_local_fixed(&path) {
                        None
                    } else {
                        kova_platform_windows::folder_size::calculate(&path, &latest, id)
                            .ok()
                            .map(|size| kova_core::domain::EffectiveSize {
                                bytes: size.bytes,
                                complete: size.complete,
                            })
                    };
                    if results.send((id, path, label)).is_err() {
                        return;
                    }
                }
            }
        });
    let timer = slint::Timer::default();
    if worker.is_err() {
        return timer;
    }
    let weak = app.as_weak();
    type ScanKey = (
        bool,
        kova_core::domain::TabId,
        Option<u64>,
        bool,
        bool,
        bool,
    );
    let mut last_key: Option<ScanKey> = None;
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(150),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            if state.get_file_menu_visible() || state.get_inline_visible() {
                return;
            }
            let mut ctrl = controller.lock().unwrap();
            let key = (
                state.get_folder_sizes(),
                ctrl.active_tab_id(),
                ctrl.folder_scan_generation(),
                state.get_show_hidden(),
                state.get_show_system(),
                ctrl.is_loading(),
            );
            let mut dirty = false;
            if last_key.as_ref() != Some(&key) {
                let id = generation.fetch_add(1, Ordering::Relaxed) + 1;
                // Retain measured values during a same-folder rescan. Clearing
                // them on every notification made every folder visibly flicker.
                let reset = !key.0 || last_key.as_ref().is_none_or(|previous| previous.1 != key.1);
                if reset {
                    dirty = !ctrl.folder_sizes.is_empty();
                    ctrl.folder_sizes.clear();
                }
                if key.0 && !key.5 {
                    let paths = ctrl.folder_size_paths();
                    let live: std::collections::HashSet<_> = paths.iter().cloned().collect();
                    let old_count = ctrl.folder_sizes.len();
                    ctrl.folder_sizes.retain(|path, _| live.contains(path));
                    dirty |= old_count != ctrl.folder_sizes.len();
                    if requests.try_send((id, paths)).is_ok() {
                        last_key = Some(key);
                    }
                } else {
                    last_key = Some(key);
                }
            }
            let current = generation.load(Ordering::Relaxed);
            while let Ok((id, path, label)) = output.try_recv() {
                if id == current && ctrl.folder_sizes.get(&path) != Some(&label) {
                    ctrl.folder_sizes.insert(path, label);
                    dirty = true;
                }
            }
            if dirty {
                let tab = ctrl.active_tab_id();
                ctrl.refilter(Some(tab));
                update_ui(&ui, &ctrl, &last_address, &models);
            }
        },
    );
    timer
}
