use crate::{AppState, MainWindow, StorageItem, bridges::CommandDispatcher};
use slint::ComponentHandle;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

pub fn connect(app: &MainWindow, dispatcher: CommandDispatcher) -> slint::Timer {
    let generation = Arc::new(AtomicU64::new(0));
    let latest = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::sync_channel::<(u64, std::path::PathBuf)>(1);
    let worker_generation = generation.clone();
    let worker_latest = latest.clone();
    let worker = std::thread::Builder::new()
        .name("kova-storage".into())
        .spawn(move || {
            while let Ok((id, path)) = rx.recv() {
                kova_platform_windows::storage_analysis::scan(
                    &path,
                    &worker_generation,
                    id,
                    |result| {
                        if let Ok(mut output) = worker_latest.lock() {
                            *output = Some((id, result));
                        }
                    },
                );
            }
        });
    let weak = app.as_weak();
    let request_generation = generation.clone();
    app.global::<AppState>().on_request_analyze(move |path| {
        let Some(ui) = weak.upgrade() else { return };
        let state = ui.global::<AppState>();
        if worker.is_err() {
            state.set_status_text("Storage worker unavailable".into());
            return;
        }
        let path = if path.is_empty() {
            let controller = dispatcher.controller();
            let ctrl = controller.lock().unwrap();
            ctrl.primary_selection()
                .and_then(|i| ctrl.snapshot()?.entries.get(i))
                .filter(|e| e.is_directory())
                .map(|e| e.path.clone())
                .or_else(|| {
                    ctrl.current_directory()
                        .map(|location| location.path.clone())
                })
        } else {
            Some(std::path::PathBuf::from(path.as_str()))
        };
        let Some(path) = path else { return };
        let id = request_generation.fetch_add(1, Ordering::Relaxed) + 1;
        match tx.try_send((id, path.clone())) {
            Ok(()) => {
                state.set_storage_visible(true);
                state.set_storage_busy(true);
                state.set_storage_path(path.to_string_lossy().as_ref().into());
                state.set_storage_status("Scanning files…".into());
                state.set_storage_total("0 B".into());
                state.set_storage_items(slint::ModelRc::default());
            }
            Err(_) => {
                state.set_storage_busy(false);
                state.set_storage_status("Previous scan is stopping. Try Analyze again.".into());
            }
        }
    });
    let cancel_generation = generation.clone();
    let weak = app.as_weak();
    app.global::<AppState>().on_cancel_analysis(move || {
        cancel_generation.fetch_add(1, Ordering::Relaxed);
        if let Some(ui) = weak.upgrade() {
            ui.global::<AppState>().set_storage_busy(false);
            ui.global::<AppState>()
                .set_storage_status("Cancelled · partial results".into());
        }
    });
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some((id, result)) = latest.lock().ok().and_then(|mut value| value.take()) else {
                return;
            };
            if id != generation.load(Ordering::Relaxed) {
                return;
            }
            let state = ui.global::<AppState>();
            state.set_storage_busy(!result.finished);
            state.set_storage_total(crate::format_bytes(result.bytes).into());
            state.set_storage_status(
                result
                    .error
                    .unwrap_or_else(|| {
                        format!(
                            "{} · {} files · {} skipped{}",
                            if result.finished {
                                "Complete"
                            } else {
                                "Scanning"
                            },
                            result.files,
                            result.skipped,
                            if result.skipped > 0 {
                                " (links, offline or inaccessible)"
                            } else {
                                ""
                            }
                        )
                    })
                    .into(),
            );
            let items = result
                .folders
                .into_iter()
                .map(|item| ("Folder", item))
                .chain(result.largest_files.into_iter().map(|item| ("File", item)))
                .map(|(kind, (bytes, path))| StorageItem {
                    name: path
                        .file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .as_ref()
                        .into(),
                    path: path.to_string_lossy().as_ref().into(),
                    kind: kind.into(),
                    size: crate::format_bytes(bytes).into(),
                    fraction: if result.bytes == 0 {
                        0.0
                    } else {
                        bytes as f32 / result.bytes as f32
                    },
                })
                .collect::<Vec<_>>();
            state.set_storage_items(slint::ModelRc::new(slint::VecModel::from(items)));
        },
    );
    timer
}
