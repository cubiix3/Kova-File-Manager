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
    let (results, output) = mpsc::sync_channel::<(u64, PathBuf, (Option<u64>, String))>(64);
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
                        (None, "Local only".into())
                    } else {
                        match kova_platform_windows::folder_size::calculate(&path, &latest, id) {
                            Ok(size) => (
                                Some(size.bytes),
                                format!(
                                    "{}{}",
                                    if size.complete { "" } else { "≥ " },
                                    crate::format_bytes(size.bytes)
                                ),
                            ),
                            Err(_) => (None, "Unavailable".into()),
                        }
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
    let mut last_key = None;
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(150),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            let mut ctrl = controller.lock().unwrap();
            let key = (
                state.get_folder_sizes(),
                ctrl.active_tab_id(),
                ctrl.snapshot().map(|s| s.request_id),
                state.get_show_hidden(),
                state.get_show_system(),
                ctrl.is_loading(),
            );
            let mut dirty = false;
            if last_key.as_ref() != Some(&key) {
                let id = generation.fetch_add(1, Ordering::Relaxed) + 1;
                ctrl.folder_sizes.clear();
                dirty = true;
                if key.0 && !key.5 {
                    let paths: Vec<_> = ctrl
                        .snapshot()
                        .map(|s| {
                            s.entries
                                .iter()
                                .filter(|e| e.is_directory())
                                .map(|e| e.path.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    if requests.try_send((id, paths)).is_ok() {
                        last_key = Some(key);
                    }
                } else {
                    last_key = Some(key);
                }
            }
            let current = generation.load(Ordering::Relaxed);
            while let Ok((id, path, label)) = output.try_recv() {
                if id == current {
                    ctrl.folder_sizes.insert(path, label);
                    dirty = true;
                }
            }
            if dirty {
                update_ui(&ui, &ctrl, &last_address, &models);
            }
        },
    );
    timer
}
