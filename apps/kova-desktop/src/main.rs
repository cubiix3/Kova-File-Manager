#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_state;
mod callbacks;
mod diagnostics;
mod file_model;
mod sidebar;
mod ui_sync;
use callbacks::wire_callbacks;
use sidebar::{apply_sidebar, load_sidebar, refresh_drive_info};
use ui_sync::update_ui;
mod bridges;
mod default_manager;
mod directory_watch;
mod drag_drop;
mod folder_sizes;
mod keyboard;
mod library;
mod operations;
mod preferences;
mod preview;
mod search;
mod storage;
mod thumbnails;
mod window_chrome;

use app_state::AppController;
use bridges::CommandDispatcher;
use kova_core::domain::{KovaEvent, LocationInput, SortDirection};
use kova_ops::worker::{WorkerCommand, spawn_worker};
use kova_platform_windows::known_folders::{KnownFolder, resolve_known_folder};
use kova_platform_windows::shell_icons::{IconBitmap, IconCache, IconKey, icon_key_for};
use kova_platform_windows::shell_menu;
use kova_platform_windows::shell_ops::{ShellOpOutcome, spawn_shell_ops_thread};
use slint::{
    ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel, Weak,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[derive(Debug, Clone)]
enum PendingDialog {
    Creating {
        tab: kova_core::domain::TabId,
        parent: kova_core::domain::Location,
    },
    Rename {
        path: std::path::PathBuf,
        suffix: String,
    },
}

/// One resolved icon batch coming back from the icon worker thread.
#[derive(Debug)]
struct IconResolved {
    key: IconKey,
    bitmap: Option<IconBitmap>,
}

/// A request for the icon worker thread.
#[derive(Debug)]
struct IconRequest {
    key: IconKey,
}

/// Mirrors the last path the UI address bar was programmatically set to, so
/// update_ui does not clobber text the user is currently typing.
type LastAddress = Arc<Mutex<String>>;

/// UI-side icon registry. Maps icon cache keys to ids in the Slint icon
/// model, dedupes in-flight requests, and hands resolved images to the UI.
/// Lives on the UI thread only.
struct IconStore {
    viewport: std::ops::Range<usize>,
    model: Rc<VecModel<slint::Image>>,
    ids: HashMap<IconKey, u32>,
    pending: HashSet<IconKey>,
    queued: VecDeque<IconKey>,
    free: Vec<u32>,
    wanted: Arc<Mutex<HashSet<IconKey>>>,
}

impl IconStore {
    fn new(model: Rc<VecModel<slint::Image>>, wanted: Arc<Mutex<HashSet<IconKey>>>) -> Self {
        Self {
            model,
            viewport: 0..128,
            ids: HashMap::new(),
            pending: HashSet::new(),
            queued: VecDeque::new(),
            free: Vec::new(),
            wanted,
        }
    }

    /// Insert a resolved bitmap as a new icon id, or return the existing id
    /// when the key was resolved before.
    fn intern(&mut self, key: &IconKey, bitmap: Option<&IconBitmap>) -> Option<u32> {
        if let Some(id) = self.ids.get(key) {
            if let Some(bitmap) = bitmap {
                self.model
                    .set_row_data(*id as usize, image_from_bitmap(bitmap));
            }
            return Some(*id);
        }
        let Some(bitmap) = bitmap else {
            self.ids.insert(key.clone(), 1);
            return Some(1);
        };
        let image = image_from_bitmap(bitmap);
        let id = if let Some(id) = self.free.pop() {
            self.model.set_row_data(id as usize, image);
            id
        } else {
            let id = self.model.row_count() as u32;
            self.model.push(image);
            id
        };
        self.ids.insert(key.clone(), id);
        Some(id)
    }

    /// Register a pre-seeded id for a generic key (no bitmap push).
    fn register_preseeded(&mut self, key: IconKey, id: u32) {
        self.wanted.lock().unwrap().insert(key.clone());
        self.ids.insert(key, id);
    }

    fn retain_live(&mut self, mut live: HashSet<IconKey>) {
        // Generic slots remain stable; negative results also use slot 1 but
        // their arbitrary keys must still be reclaimed.
        live.extend([
            IconKey::Folder,
            IconKey::File,
            IconKey::Symlink,
            IconKey::Drive(system_drive_root()),
            IconKey::UnknownType,
        ]);
        self.ids.retain(|key, id| {
            if live.contains(key) {
                return true;
            }
            if *id >= 5 {
                self.model
                    .set_row_data(*id as usize, slint::Image::default());
                self.free.push(*id);
            }
            false
        });
        self.pending.retain(|key| live.contains(key));
        self.queued.retain(|key| live.contains(key));
        *self.wanted.lock().unwrap() = live;
    }

    fn flush(&mut self, requests: &std::sync::mpsc::SyncSender<IconRequest>) {
        while let Some(key) = self.queued.pop_front() {
            match requests.try_send(IconRequest { key }) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(request)) => {
                    self.queued.push_front(request.key);
                    break;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.queued.clear();
                    self.pending.clear();
                    break;
                }
            }
        }
    }

    fn mark_pending(&mut self, key: IconKey) {
        self.pending.insert(key);
    }

    fn take_pending(&mut self, key: &IconKey) -> bool {
        self.pending.remove(key)
    }

    fn id_for(&self, key: &IconKey) -> Option<u32> {
        self.ids.get(key).copied()
    }
}

fn image_from_bitmap(bitmap: &IconBitmap) -> slint::Image {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(bitmap.width, bitmap.height);
    buffer.make_mut_bytes().copy_from_slice(&bitmap.rgba);
    slint::Image::from_rgba8_premultiplied(buffer)
}

/// Reserve stable generic icon ids (0..=4) and resolve them on the icon worker.
fn preseed_icon_store(
    store: &mut IconStore,
    icons_model: &Rc<VecModel<slint::Image>>,
    requests: &std::sync::mpsc::SyncSender<IconRequest>,
) {
    let generics: [(IconKey, u32); 5] = [
        (IconKey::Folder, 0),
        (IconKey::File, 1),
        (IconKey::Symlink, 2),
        (IconKey::Drive(system_drive_root()), 3),
        (IconKey::UnknownType, 4),
    ];
    for (key, id) in generics {
        let slot = id as usize;
        while icons_model.row_count() <= slot {
            icons_model.push(slint::Image::default());
        }
        store.register_preseeded(key.clone(), id);
        store.mark_pending(key.clone());
        let _ = requests.send(IconRequest { key });
    }
}

fn system_drive_root() -> std::path::PathBuf {
    std::env::var("SystemDrive")
        .map(|d| std::path::PathBuf::from(format!("{d}\\").to_uppercase()))
        .unwrap_or_else(|_| std::path::PathBuf::from("C:\\"))
}

/// Long-lived model handles. The same model instances stay installed in the
/// Slint globals for the whole app lifetime and are updated in place, so row
/// delegates (and their TouchAreas) keep their identity across updates.
/// Recreating a model on every selection click would break Slint
/// double-click detection, because the second click would land on a fresh
/// row element.
struct UiModels {
    files: Rc<file_model::FileModel>,
    tabs: Rc<VecModel<SharedString>>,
    thumbnails: RefCell<thumbnails::Cache>,
    render_key: std::cell::Cell<(u64, u64, bool, bool)>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let mut launch_args = std::env::args_os().skip(1);
    let first = launch_args.next();
    let requested = if first.as_deref() == Some(std::ffi::OsStr::new("--open")) {
        launch_args.next()
    } else {
        first
    };
    let restore_tabs = requested.is_none();
    let initial = requested
        .and_then(|path| {
            kova_platform_windows::path_resolver::canonicalize_location(std::path::Path::new(&path))
                .ok()
        })
        .unwrap_or_else(kova_core::domain::Location::home);
    tracing::info!("Kova starting at {}", initial.display());

    // The UI thread hosts shell COM objects (native context menus, drag
    // formats); make sure an apartment-threaded COM is present before any
    // window is created.
    shell_menu::ensure_com_sta();

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WorkerCommand>();
    let (evt_tx, mut evt_rx) = mpsc::channel::<KovaEvent>(64);
    // Worker events are forwarded into a plain channel and drained by a Slint
    // timer, because Slint properties must only be touched from the UI thread.
    let (ui_evt_tx, ui_evt_rx) = std::sync::mpsc::channel::<KovaEvent>();
    let (icon_req_tx, icon_req_rx) = std::sync::mpsc::sync_channel::<IconRequest>(256);
    let (icon_res_tx, icon_res_rx) = std::sync::mpsc::sync_channel::<IconResolved>(64);
    let wanted_icons = Arc::new(Mutex::new(HashSet::<IconKey>::new()));

    // Dedicated icon worker thread. Shell icon resolution must not run
    // concurrently (see shell_icons::SHELL_ICON_LOCK) and must not block
    // directory enumeration, so it lives outside the Tokio runtime.
    {
        let res_tx = icon_res_tx.clone();
        let wanted = wanted_icons.clone();
        std::thread::Builder::new()
            .name("kova-icons".into())
            .spawn(move || {
                let cache = IconCache::new();
                while let Ok(request) = icon_req_rx.recv() {
                    if !wanted.lock().unwrap().contains(&request.key) {
                        continue;
                    }
                    let bitmap = cache.get_or_resolve(&request.key);
                    if !wanted.lock().unwrap().contains(&request.key) {
                        continue;
                    }
                    if res_tx
                        .send(IconResolved {
                            key: request.key,
                            bitmap,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("icon worker thread");
    }

    // Dedicated shell operations thread (IFileOperation): copy/move/delete
    // run off the UI thread with native progress and conflict dialogs.
    let (ops_tx, ops_rx) =
        std::sync::mpsc::channel::<kova_platform_windows::transfers::ShellRequest>();
    let (ops_out_tx, ops_out_rx) = std::sync::mpsc::channel::<ShellOpOutcome>();
    let _ops_thread = spawn_shell_ops_thread(ops_rx, ops_out_tx);

    let app_controller = Arc::new(Mutex::new(AppController::new(initial.clone())));
    let dispatcher = CommandDispatcher::new(
        Arc::clone(&app_controller),
        cmd_tx.clone(),
        Default::default(),
        ops_tx,
    );

    spawn_worker(cmd_rx, evt_tx, dispatcher.transfers.undo.clone());

    let _menu_theme = kova_platform_windows::window_theme::initialize_dark_menus();
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .select()
        .expect("initialize desktop window backend");
    let app = MainWindow::new().unwrap();
    let session_writable =
        preferences::restore(&app, &mut app_controller.lock().unwrap(), restore_tabs);
    let _persistence = preferences::connect(&app, app_controller.clone(), session_writable);
    window_chrome::connect(&app, dispatcher.transfers.clone());
    default_manager::connect(&app, dispatcher.clone());
    let _preview_timer = preview::connect(&app);
    let _storage_timer = storage::connect(&app, dispatcher.clone());
    let _library_timer = library::connect(&app, dispatcher.clone());
    let _drag_drop = drag_drop::connect(&app, dispatcher.clone());
    let _operations_timer = operations::connect(&app, dispatcher.clone());
    let _directory_watch_timer = directory_watch::connect(&app, dispatcher.clone());

    let files_model = Rc::new(file_model::FileModel::default());
    let tabs_model = Rc::new(VecModel::from(Vec::new()));
    let icons_model = Rc::new(VecModel::from(Vec::new()));
    app.global::<AppState>()
        .set_files(ModelRc::from(Rc::clone(&files_model)));
    app.global::<AppState>()
        .set_tabs(ModelRc::from(Rc::clone(&tabs_model)));
    app.global::<AppState>()
        .set_icons(ModelRc::from(Rc::clone(&icons_model)));
    let ui_models = Rc::new(UiModels {
        files: files_model.clone(),
        tabs: tabs_model,
        thumbnails: RefCell::new(thumbnails::Cache::default()),
        render_key: std::cell::Cell::new((0, 0, false, false)),
    });
    let _thumbnail_timer = thumbnails::connect(&app, app_controller.clone(), ui_models.clone());

    let icon_store = Rc::new(RefCell::new(IconStore::new(
        Rc::clone(&icons_model),
        wanted_icons,
    )));
    {
        let mut store = icon_store.borrow_mut();
        preseed_icon_store(&mut store, &icons_model, &icon_req_tx);
    }

    let row_icons = icon_store.clone();
    ui_models
        .files
        .set_icon_lookup(Rc::new(move |path, directory| {
            row_icons
                .borrow()
                .id_for(&icon_key_for(path, directory))
                .unwrap_or(if directory { 0 } else { 1 }) as i32
        }));

    let ui = app.as_weak();

    let pending_dialog = Arc::new(Mutex::new(None::<PendingDialog>));
    let last_address: LastAddress = Arc::new(Mutex::new(String::new()));
    let _folder_size_timer = folder_sizes::connect(
        &app,
        Arc::clone(&app_controller),
        Arc::clone(&last_address),
        Rc::clone(&ui_models),
    );

    // Wire UI callbacks.
    wire_callbacks(
        ui.clone(),
        dispatcher.clone(),
        Arc::clone(&pending_dialog),
        Arc::clone(&last_address),
        Rc::clone(&ui_models),
        Rc::clone(&icon_store),
        icon_req_tx.clone(),
    );

    // Populate sidebar targets and icons.
    let (sidebar_tx, sidebar_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sidebar_tx.send(load_sidebar());
    });

    let drive_timer = slint::Timer::default();
    let weak_drives = app.as_weak();
    let drive_dispatcher = dispatcher.clone();
    let mut drive_mask = kova_platform_windows::volumes::logical_drive_mask();
    let mut last_drive_refresh = std::time::Instant::now();
    drive_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(1),
        move || {
            let current = kova_platform_windows::volumes::logical_drive_mask();
            if current != drive_mask
                || last_drive_refresh.elapsed() >= std::time::Duration::from_secs(30)
            {
                let changed = drive_mask ^ current;
                if changed != 0 {
                    let locations = drive_dispatcher
                        .controller()
                        .lock()
                        .unwrap()
                        .tab_locations();
                    for (id, location) in locations {
                        let drive = location
                            .path
                            .to_string_lossy()
                            .as_bytes()
                            .first()
                            .copied()
                            .map(|c| c.to_ascii_uppercase());
                        if drive.is_some_and(|letter| {
                            letter.is_ascii_uppercase() && changed & (1u32 << (letter - b'A')) != 0
                        }) {
                            drive_dispatcher.request_enumeration(id, location);
                        }
                    }
                }
                drive_mask = current;
                last_drive_refresh = std::time::Instant::now();
                if let Some(ui) = weak_drives.upgrade() {
                    refresh_drive_info(&ui);
                }
            }
        },
    );

    // Initial load.
    let start_tab = app_controller.lock().unwrap().active_tab_id();
    let start_loc = app_controller
        .lock()
        .unwrap()
        .current_location()
        .cloned()
        .unwrap_or(initial);
    dispatcher.request_enumeration(start_tab, start_loc.clone());
    {
        let mut ctrl = app_controller.lock().unwrap();
        queue_icon_requests(&icon_store, &icon_req_tx, &mut ctrl);
    }

    // Forward worker events into a std channel consumed by a Slint UI timer.
    tokio::spawn(async move {
        while let Some(event) = evt_rx.recv().await {
            let _ = ui_evt_tx.send(event);
        }
    });

    // UI-thread event pump: process core events forwarded by the worker,
    // shell-operation outcomes and icon results from the icon worker thread.
    let ui_for_pump = ui.clone();
    let controller_for_pump = Arc::clone(&app_controller);
    let reload_dispatcher_pump = dispatcher.clone();
    let last_address_pump = Arc::clone(&last_address);
    let models_for_pump = Rc::clone(&ui_models);
    let store_for_pump = Rc::clone(&icon_store);
    let icon_req_for_pump = icon_req_tx.clone();
    let pending_for_pump = Arc::clone(&pending_dialog);
    let mut reveal: Option<(kova_core::domain::TabId, std::path::PathBuf, bool)> = None;
    let mut deferred_events = std::collections::VecDeque::new();
    let pump_timer = slint::Timer::default();
    pump_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let ui_ref = ui_for_pump.clone();
            let ctrl_ref = Arc::clone(&controller_for_pump);
            let reload_ref = reload_dispatcher_pump.clone();
            let last_address_ref = Arc::clone(&last_address_pump);
            let models_ref = Rc::clone(&models_for_pump);
            let store_ref = Rc::clone(&store_for_pump);
            let icon_req_ref = icon_req_for_pump.clone();
            let editing = ui_ref.upgrade().is_some_and(|ui| {
                ui.global::<AppState>().get_inline_visible()
                    || ui.global::<AppState>().get_file_menu_visible()
            });
            let mut ui_dirty = !editing && ctrl_ref.lock().unwrap().poll_views();
            if ui_dirty {
                queue_icon_requests(&store_ref, &icon_req_ref, &mut ctrl_ref.lock().unwrap());
            }
            if let Ok(data) = sidebar_rx.try_recv() {
                if let Some(ui) = ui_ref.upgrade() {
                    apply_sidebar(&ui, data);
                }
            }

            deferred_events.extend(ui_evt_rx.try_iter());
            for _ in 0..deferred_events.len() {
                let Some(event) = deferred_events.pop_front() else {
                    break;
                };
                let Some(ui) = ui_ref.upgrade() else { return };
                let mut ctrl = ctrl_ref.lock().unwrap();
                let background_tab = match &event {
                    KovaEvent::DirectoryLoaded { tab_id, snapshot }
                        if ctrl.is_current_request(*tab_id, snapshot.request_id)
                            && ctrl.background_in_flight(*tab_id) =>
                    {
                        Some(*tab_id)
                    }
                    KovaEvent::DirectoryError {
                        tab_id, request_id, ..
                    } if ctrl.is_current_request(*tab_id, *request_id)
                        && ctrl.background_in_flight(*tab_id) =>
                    {
                        Some(*tab_id)
                    }
                    _ => None,
                };
                let state = ui.global::<AppState>();
                if background_tab.is_some_and(|tab| {
                    state.get_file_menu_visible()
                        || (tab == ctrl.active_tab_id() && state.get_inline_visible())
                }) {
                    deferred_events.push_back(event);
                    continue;
                }
                match event {
                    KovaEvent::DirectoryProgress {
                        tab_id,
                        request_id,
                        folders,
                        entries,
                        skipped,
                    } if ctrl.is_current_request(tab_id, request_id)
                        && ctrl.active_tab_id() == tab_id =>
                    {
                        ui.global::<AppState>().set_search_progress(
                            format!("{folders} folders · {entries} entries · {skipped} skipped")
                                .into(),
                        );
                    }
                    KovaEvent::DirectoryLoaded { tab_id, snapshot }
                        if ctrl.is_current_request(tab_id, snapshot.request_id) =>
                    {
                        ctrl.apply_snapshot(tab_id, snapshot);
                        queue_icon_requests(&store_ref, &icon_req_ref, &mut ctrl);
                        update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                    }
                    KovaEvent::DirectoryError {
                        tab_id,
                        request_id,
                        error_message,
                        ..
                    } if ctrl.is_current_request(tab_id, request_id) => {
                        ctrl.apply_error(tab_id, request_id, error_message);
                        update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                    }
                    KovaEvent::FolderCreated { parent, name } => {
                        ui.global::<AppState>().set_creating_folder(false);
                        let creating = {
                            let mut pending = pending_for_pump.lock().unwrap();
                            if matches!(*pending, Some(PendingDialog::Creating { .. })) {
                                pending.take()
                            } else {
                                None
                            }
                        };
                        if let Some(PendingDialog::Creating {
                            tab,
                            parent: expected,
                        }) = creating
                        {
                            if parent == expected
                                && tab == ctrl.active_tab_id()
                                && ctrl.current_directory() == Some(&parent)
                            {
                                reveal = Some((tab, parent.path.join(name), true));
                            }
                        }
                        drop(ctrl);
                        reload_ref.refresh_tabs();
                        return;
                    }
                    KovaEvent::ItemRenamed { old_path, new_path } => {
                        ctrl.library.relocate(&old_path, &new_path);
                        if ctrl
                            .current_directory()
                            .is_some_and(|loc| Some(loc.path.as_path()) == new_path.parent())
                            || ctrl.snapshot().is_some_and(|snapshot| {
                                snapshot.entries.iter().any(|entry| entry.path == old_path)
                            })
                        {
                            reveal = Some((ctrl.active_tab_id(), new_path, false));
                        }
                        drop(ctrl);
                        reload_ref.refresh_tabs();
                        return;
                    }
                    KovaEvent::ItemsRestored { paths } => {
                        if let Some(path) = paths.iter().find(|path| {
                            ctrl.current_directory()
                                .is_some_and(|loc| Some(loc.path.as_path()) == path.parent())
                        }) {
                            reveal = Some((ctrl.active_tab_id(), path.clone(), false));
                        }
                        ctrl.set_status(format!(
                            "Restored {} item(s) from Recycle Bin",
                            paths.len()
                        ));
                        drop(ctrl);
                        reload_ref.refresh_tabs();
                        return;
                    }
                    KovaEvent::OperationError {
                        context,
                        error_message,
                    } => {
                        ui.global::<AppState>().set_creating_folder(false);
                        tracing::error!("{}: {}", context, error_message);
                        ctrl.set_status(format!("{}: {}", context, error_message));
                        update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                        show_error_dialog(&ui, &error_message);
                    }
                    _ => {}
                }
            }

            // Shell file-operation outcomes: refresh the directory because
            // copy/move/delete may have changed it, and surface the result.
            for (old, new) in reload_ref.transfers.take_moves() {
                ctrl_ref.lock().unwrap().library.relocate(&old, &new);
            }
            if let Ok(outcome) = ops_out_rx.try_recv() {
                let Some(ui) = ui_ref.upgrade() else { return };
                let mut ctrl = ctrl_ref.lock().unwrap();
                match outcome {
                    ShellOpOutcome::Completed { summary } => {
                        ctrl.set_status(format!("{summary} finished"));
                        drop(ctrl);
                        reload_ref.refresh_tabs();
                        return;
                    }
                    ShellOpOutcome::Failed {
                        summary,
                        message,
                        code,
                    } => {
                        if op_was_cancelled(code) {
                            ctrl.set_status(format!("{summary} cancelled"));
                            update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                        } else {
                            tracing::error!("shell op failed ({code:#010x}): {message}");
                            ctrl.set_status(format!("{summary} failed"));
                            update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                            show_error_dialog(&ui, &message);
                        }
                        // A failed or cancelled batch can already have changed
                        // some files. Reconcile every open tab with disk.
                        drop(ctrl);
                        reload_ref.refresh_tabs();
                        return;
                    }
                }
            }

            if let Some(ui) = ui_ref.upgrade() {
                let mut ctrl = ctrl_ref.lock().unwrap();
                if !ctrl.view_in_flight()
                    && !ctrl.request_in_flight(ctrl.active_tab_id())
                    && reveal
                        .as_ref()
                        .is_some_and(|(tab, _, _)| *tab == ctrl.active_tab_id())
                {
                    if let Some((_, path, edit)) = reveal.take() {
                        if let Some(index) = ctrl
                            .snapshot()
                            .and_then(|s| s.entries.iter().position(|e| e.path == path))
                        {
                            if let Some(selection) = ctrl.selection_mut() {
                                selection.select_single(index);
                            }
                            drop(ctrl);
                            sync_selection(&ui, &reload_ref, &models_ref);
                            let ctrl = ctrl_ref.lock().unwrap();
                            if edit {
                                begin_inline_rename(&ui, &ctrl, path, &pending_for_pump);
                            }
                        }
                    }
                }
            }

            // Icon resolution is restricted to the viewport. The shared snapshot
            // stays immutable; rows resolve their icon against the bounded registry.
            let mut icons_changed = false;
            for res in icon_res_rx.try_iter().take(64) {
                let mut store = store_ref.borrow_mut();
                if store.take_pending(&res.key) {
                    icons_changed |= store.intern(&res.key, res.bitmap.as_ref()).is_some();
                }
            }
            if let Some(ui) = ui_ref.upgrade() {
                let state = ui.global::<AppState>();
                let first = state.get_first_visible_row().max(0) as usize;
                let viewport = first..first + state.get_visible_row_count().clamp(1, 128) as usize;
                if store_ref.borrow().viewport != viewport {
                    store_ref.borrow_mut().viewport = viewport;
                    queue_icon_requests(&store_ref, &icon_req_ref, &mut ctrl_ref.lock().unwrap());
                    icons_changed = true;
                }
            }
            if icons_changed {
                models_ref.files.refresh_icons();
            }

            if let Some(ui) = ui_ref.upgrade() {
                let ctrl = ctrl_ref.lock().unwrap();
                let state = ui.global::<AppState>();
                ui_dirty |= models_ref.render_key.get().1 != ctrl.revision;
                ui_dirty |= state.get_active_tab() != ctrl.active_tab_index() as i32
                    || state.get_loading() != ctrl.is_loading()
                    || state.get_status_text().as_str() != ctrl.status_text()
                    || *last_address_ref.lock().unwrap() != ctrl.address_path()
                    || state.get_tabs().row_count() != ctrl.tab_labels().len();
            }
            if ui_dirty {
                if let Some(ui) = ui_ref.upgrade() {
                    let mut ctrl = ctrl_ref.lock().unwrap();
                    queue_icon_requests(&store_ref, &icon_req_ref, &mut ctrl);
                    update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                }
            }
            store_ref.borrow_mut().flush(&icon_req_ref);
        },
    );

    // Render the initial controller state (tab, address bar, empty list) so
    // the window is never blank while the first enumeration is in flight.
    {
        let ctrl = app_controller.lock().unwrap();
        if let Some(ui) = ui.upgrade() {
            update_ui(&ui, &ctrl, &last_address, &ui_models);
        }
    }

    app.run().unwrap();
}

/// HRESULT codes the shell reports when the user aborted a file operation in
/// the native progress/conflict dialog. These are not failures worth a modal
/// error dialog.
fn op_was_cancelled(code: i32) -> bool {
    matches!(
        code as u32,
        0x8007_04C7 // HRESULT_FROM_WIN32(ERROR_CANCELLED)
            | 0x8007_03E3 // HRESULT_FROM_WIN32(ERROR_OPERATION_ABORTED)
            | 0xC004_0004 // COPYENGINE_E_USER_CANCELLED
    )
}

/// Human readable byte count for drive details.
fn format_bytes(bytes: u64) -> String {
    kova_platform_windows::formatting::bytes(bytes)
}

fn queue_icon_requests(
    store: &Rc<RefCell<IconStore>>,
    icon_req_tx: &std::sync::mpsc::SyncSender<IconRequest>,
    ctrl: &mut AppController,
) {
    let mut store = store.borrow_mut();
    let keys: HashSet<_> = ctrl
        .snapshot()
        .into_iter()
        .flat_map(|snapshot| {
            snapshot
                .entries
                .iter()
                .skip(store.viewport.start)
                .take(store.viewport.len())
        })
        .map(|e| icon_key_for(&e.path, e.is_directory()))
        .collect();
    store.retain_live(keys.clone());
    for key in keys {
        if store.id_for(&key).is_none() && !store.pending.contains(&key) {
            store.mark_pending(key.clone());
            store.queued.push_back(key);
        }
    }
    store.flush(icon_req_tx);
}

fn begin_inline_rename(
    ui: &MainWindow,
    ctrl: &AppController,
    path: std::path::PathBuf,
    pending: &Arc<Mutex<Option<PendingDialog>>>,
) {
    let Some((index, entry)) = ctrl
        .snapshot()
        .and_then(|s| s.entries.iter().enumerate().find(|(_, e)| e.path == path))
    else {
        return;
    };
    let state = ui.global::<AppState>();
    let stem = if entry.is_directory() {
        entry.name.as_str()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&entry.name)
    };
    let hide_extension = !entry.is_directory() && !state.get_show_extensions();
    let suffix = if hide_extension {
        entry.name[stem.len()..].to_owned()
    } else {
        String::new()
    };
    state.set_dialog_value(if hide_extension { stem } else { &entry.name }.into());
    state.set_inline_selection_end(stem.len() as i32);
    state.set_inline_path(path.to_string_lossy().as_ref().into());
    state.set_inline_row(index as i32);
    state.set_inline_visible(true);
    *pending.lock().unwrap() = Some(PendingDialog::Rename { path, suffix });
}

fn close_dialog(ui: &MainWindow, pending: &Arc<Mutex<Option<PendingDialog>>>) {
    let state = ui.global::<AppState>();
    state.set_dialog_visible(false);
    state.set_inline_visible(false);
    state.set_dialog_value("".into());
    *pending.lock().unwrap() = None;
}

fn show_error_dialog(ui: &MainWindow, message: &str) {
    ui.invoke_dismiss_file_menu();
    let state = ui.global::<AppState>();
    state.set_inline_visible(false);
    state.set_dialog_title("Error".into());
    state.set_dialog_value(
        message
            .strip_prefix("shell error: ")
            .unwrap_or(message)
            .into(),
    );
    state.set_dialog_visible(true);
}

/// Re-sync the UI from the controller after a view-model-only mutation.
fn sync_ui(
    ui: &MainWindow,
    dispatcher: &CommandDispatcher,
    last_address: &LastAddress,
    models: &UiModels,
) {
    let controller_arc = dispatcher.controller();
    let ctrl = controller_arc.lock().unwrap();
    update_ui(ui, &ctrl, last_address, models);
}

fn sync_selection(ui: &MainWindow, dispatcher: &CommandDispatcher, models: &UiModels) {
    ui.invoke_dismiss_file_menu();
    let controller = dispatcher.controller();
    let ctrl = controller.lock().unwrap();
    let selected = ctrl.selected_indices();
    sync_preview_path(ui, &ctrl);
    ui.global::<AppState>()
        .set_selected_count(selected.len() as i32);
    models.files.set_selection(selected);
}

fn sync_preview_path(ui: &MainWindow, ctrl: &AppController) {
    let state = ui.global::<AppState>();
    state.set_primary_row(ctrl.primary_selection().map(|i| i as i32).unwrap_or(-1));
    if state.get_inline_visible() {
        let edit_path = state.get_inline_path();
        if ctrl.is_loading()
            || !ctrl
                .selected_paths()
                .iter()
                .any(|p| p.to_string_lossy() == edit_path.as_str())
        {
            state.set_inline_visible(false);
        } else if let Some(index) = ctrl.snapshot().and_then(|s| {
            s.entries
                .iter()
                .position(|e| e.path.to_string_lossy() == edit_path.as_str())
        }) {
            state.set_inline_row(index as i32);
        }
    }
    let info = if ctrl.selected_count() == 1 {
        ctrl.primary_selection()
            .and_then(|i| ctrl.snapshot()?.entries.get(i))
            .filter(|e| !e.is_directory())
            .map(|entry| {
                let extension = entry.extension_lower();
                let kind = if extension.is_empty() {
                    "File".into()
                } else {
                    extension.to_uppercase()
                };
                match entry.metadata.size {
                    Some(bytes) => format!("{kind} · {}", format_bytes(bytes)),
                    None => kind,
                }
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    state.set_preview_info(info.into());
    let selection_info = if ctrl.selected_count() == 1 {
        ctrl.primary_selection()
            .and_then(|i| ctrl.snapshot()?.entries.get(i))
            .and_then(|entry| {
                kova_core::domain::effective_size(
                    entry,
                    &ctrl.folder_sizes,
                    ctrl.folder_sizes_enabled,
                )
                .map(|size| (size.bytes, !size.complete))
            })
            .map(|(bytes, partial)| {
                format!(
                    "{}{} ({bytes} Bytes)",
                    if partial { "At least " } else { "" },
                    format_bytes(bytes)
                )
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    state.set_selection_info(selection_info.into());
    let path = if !ctrl.is_loading() && ctrl.selected_count() == 1 {
        ctrl.primary_selection()
            .and_then(|i| ctrl.snapshot()?.entries.get(i))
            .map(|e| e.path.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };
    if state.get_preview_path() != path {
        state.set_preview_page(0);
        state.set_preview_path(path.into());
        state.set_preview_has_image(false);
        state.set_preview_text("Select one file to preview".into());
    }
    state.set_preview_revision(ctrl.snapshot().map(|s| s.request_id as i32).unwrap_or(0));
}

/// Show a user-visible error for a failed user action and re-sync the UI.
fn show_action_error(
    ui: &Weak<MainWindow>,
    dispatcher: &CommandDispatcher,
    last_address: &LastAddress,
    models: &UiModels,
    message: &str,
) {
    dispatcher.set_status_message(format!("Error: {message}"));
    if let Some(ui) = ui.upgrade() {
        sync_ui(&ui, dispatcher, last_address, models);
    }
}

fn breadcrumb_items(address: &str) -> Vec<Breadcrumb> {
    let mut items: Vec<_> = std::path::Path::new(address)
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
        .take(3)
        .map(|path| Breadcrumb {
            label: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .as_ref()
                .into(),
            path: path.to_string_lossy().as_ref().into(),
        })
        .collect();
    items.reverse();
    items
}

fn location_label(ui: &MainWindow, path: &str, fallback: &str) -> String {
    ui.global::<AppState>()
        .get_drives()
        .iter()
        .find(|drive| drive.path.as_str().eq_ignore_ascii_case(path))
        .map(|drive| drive.name.to_string())
        .unwrap_or_else(|| fallback.to_owned())
}

#[cfg(test)]
mod icon_lifecycle_tests {
    use super::*;
    #[test]
    fn navigation_reclaims_images_and_reuses_slots() {
        let model = Rc::new(VecModel::from(vec![slint::Image::default(); 5]));
        let mut store = IconStore::new(model.clone(), Arc::default());
        let bitmap = IconBitmap {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        for cycle in 0..200 {
            let keys: HashSet<_> = (0..20)
                .map(|i| IconKey::Path(format!("{cycle}-{i}.exe").into()))
                .collect();
            store.retain_live(keys.clone());
            for key in &keys {
                store.intern(key, Some(&bitmap));
            }
            assert_eq!(model.row_count(), 25);
            assert_eq!(store.ids.len(), 20);
        }
        store.retain_live(HashSet::new());
        assert!(store.ids.is_empty());
        assert!((5..25).all(|i| model.row_data(i).unwrap().size().width == 0));
    }
    #[test]
    fn bounded_icon_delivery_retries_and_discards_obsolete_work() {
        let mut store = IconStore::new(Rc::new(VecModel::default()), Arc::default());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let a = IconKey::Extension("a".into());
        let b = IconKey::Extension("b".into());
        store.pending.extend([a.clone(), b.clone()]);
        store.queued.extend([a.clone(), b.clone()]);
        store.flush(&tx);
        assert_eq!(store.queued.len(), 1);
        assert_eq!(rx.try_recv().unwrap().key, a);
        store.flush(&tx);
        assert_eq!(rx.try_recv().unwrap().key, b);
        store.retain_live(HashSet::new());
        assert!(!store.take_pending(&b));
        assert!(store.queued.is_empty());
    }
}

#[cfg(test)]
mod breadcrumb_tests {
    use super::breadcrumb_items;

    #[test]
    fn home_has_no_empty_parent_breadcrumb() {
        let items = breadcrumb_items("Home");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Home");
        assert_eq!(items[0].path, "Home");
    }

    #[test]
    fn breadcrumbs_keep_full_navigation_targets_when_deep_paths_are_shortened() {
        let items = breadcrumb_items(r"G:\one\two\three\four");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "two");
        assert_eq!(items[0].path, r"G:\one\two");
        assert_eq!(items[2].path, r"G:\one\two\three\four");
    }

    #[test]
    fn share_root_is_one_breadcrumb_not_a_server_navigation_target() {
        let items = breadcrumb_items(r"\\server\share\folder");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].path, r"\\server\share\");
        assert_eq!(items[1].label, "folder");
    }
}
