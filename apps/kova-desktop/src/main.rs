#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_state;
mod bridges;

use app_state::AppController;
use bridges::CommandDispatcher;
use kova_core::domain::{IconHandle, KovaEvent, LocationInput, SortDirection};
use kova_ops::worker::{WorkerCommand, spawn_worker};
use kova_platform_windows::known_folders::{KnownFolder, resolve_known_folder};
use kova_platform_windows::shell_icons::{IconBitmap, IconCache, IconKey, icon_key_for};
use kova_platform_windows::shell_menu;
use kova_platform_windows::shell_ops::{ShellOpCommand, ShellOpOutcome, spawn_shell_ops_thread};
use slint::{
    ComponentHandle, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel, Weak,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[derive(Debug, Clone)]
enum PendingDialog {
    NewFolder,
    Rename { path: std::path::PathBuf },
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
    model: Rc<VecModel<slint::Image>>,
    ids: HashMap<IconKey, u32>,
    pending: HashSet<IconKey>,
}

impl IconStore {
    fn new(model: Rc<VecModel<slint::Image>>) -> Self {
        Self {
            model,
            ids: HashMap::new(),
            pending: HashSet::new(),
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
        // The id doubles as the index into the Slint icon model, so
        // derive it from the model itself: pre-seeded slots occupy
        // 0..N and every intern appends exactly one row.
        let id = self.model.row_count() as u32;
        self.model.push(image_from_bitmap(bitmap));
        self.ids.insert(key.clone(), id);
        Some(id)
    }

    /// Register a pre-seeded id for a generic key (no bitmap push).
    fn register_preseeded(&mut self, key: IconKey, id: u32) {
        self.ids.insert(key, id);
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
    requests: &std::sync::mpsc::Sender<IconRequest>,
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
    files: Rc<VecModel<FileListItem>>,
    tabs: Rc<VecModel<SharedString>>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let initial = kova_platform_windows::known_folders::initial_location();
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
    let (icon_req_tx, icon_req_rx) = std::sync::mpsc::channel::<IconRequest>();
    let (icon_res_tx, icon_res_rx) = std::sync::mpsc::channel::<IconResolved>();

    spawn_worker(cmd_rx, evt_tx);

    // Dedicated icon worker thread. Shell icon resolution must not run
    // concurrently (see shell_icons::SHELL_ICON_LOCK) and must not block
    // directory enumeration, so it lives outside the Tokio runtime.
    {
        let res_tx = icon_res_tx.clone();
        std::thread::Builder::new()
            .name("kova-icons".into())
            .spawn(move || {
                let cache = IconCache::new();
                while let Ok(request) = icon_req_rx.recv() {
                    let bitmap = cache.get_or_resolve(&request.key);
                    let _ = res_tx.send(IconResolved {
                        key: request.key,
                        bitmap,
                    });
                }
            })
            .expect("icon worker thread");
    }

    // Dedicated shell operations thread (IFileOperation): copy/move/delete
    // run off the UI thread with native progress and conflict dialogs.
    let (ops_tx, ops_rx) = std::sync::mpsc::channel::<ShellOpCommand>();
    let (ops_out_tx, ops_out_rx) = std::sync::mpsc::channel::<ShellOpOutcome>();
    let _ops_thread = spawn_shell_ops_thread(ops_rx, ops_out_tx);

    let app_controller = Arc::new(Mutex::new(AppController::new(initial.clone())));
    let dispatcher = CommandDispatcher::new(
        Arc::clone(&app_controller),
        cmd_tx.clone(),
        Default::default(),
        ops_tx,
    );

    let app = MainWindow::new().unwrap();

    let files_model = Rc::new(VecModel::from(Vec::new()));
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
    });

    let icon_store = Rc::new(RefCell::new(IconStore::new(Rc::clone(&icons_model))));
    {
        let mut store = icon_store.borrow_mut();
        preseed_icon_store(&mut store, &icons_model, &icon_req_tx);
    }

    let ui = app.as_weak();

    let pending_dialog = Arc::new(Mutex::new(None::<PendingDialog>));
    let last_address: LastAddress = Arc::new(Mutex::new(String::new()));

    // Wire UI callbacks.
    wire_callbacks(
        ui.clone(),
        dispatcher.clone(),
        Arc::clone(&pending_dialog),
        Arc::clone(&last_address),
        Rc::clone(&ui_models),
    );

    // Populate sidebar targets and icons.
    let (sidebar_tx, sidebar_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sidebar_tx.send(load_sidebar());
    });

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
            let mut ui_dirty = false;
            if let Ok(data) = sidebar_rx.try_recv() {
                if let Some(ui) = ui_ref.upgrade() {
                    apply_sidebar(&ui, data);
                }
            }

            while let Ok(event) = ui_evt_rx.try_recv() {
                let Some(ui) = ui_ref.upgrade() else { return };
                let mut ctrl = ctrl_ref.lock().unwrap();
                match event {
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
                    KovaEvent::FolderCreated { .. } | KovaEvent::ItemRenamed { .. } => {
                        drop(ctrl);
                        reload_ref.refresh_tabs();
                        return;
                    }
                    KovaEvent::OperationError {
                        context,
                        error_message,
                    } => {
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

            // Icon resolutions: intern the bitmap, then stamp the new icon id
            // onto every snapshot entry that shares the key.
            let mut resolved_icons = HashMap::new();
            for res in icon_res_rx.try_iter().take(64) {
                let mut store = store_ref.borrow_mut();
                let was_pending = store.take_pending(&res.key);
                if !was_pending {
                    continue;
                }
                let id = store.intern(&res.key, res.bitmap.as_ref());
                drop(store);
                if let Some(id) = id {
                    resolved_icons.insert(res.key, IconHandle(id));
                    ui_dirty = true;
                }
            }
            if !resolved_icons.is_empty() {
                let mut ctrl = ctrl_ref.lock().unwrap();
                for snapshot in ctrl.snapshots_mut() {
                    for entry in &mut snapshot.entries {
                        if entry.icon_handle.is_none() {
                            entry.icon_handle = resolved_icons
                                .get(&icon_key_for(&entry.path, entry.is_directory()))
                                .copied();
                        }
                    }
                }
            }

            if let Some(ui) = ui_ref.upgrade() {
                let ctrl = ctrl_ref.lock().unwrap();
                let state = ui.global::<AppState>();
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
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        format!("{} GB", (bytes + GB / 2) / GB)
    } else if bytes >= MB {
        format!("{} MB", (bytes + MB / 2) / MB)
    } else {
        format!("{bytes} B")
    }
}

/// Send icon requests for snapshot entries that do not have an icon yet.
/// Cached ids are applied synchronously; misses are queued to the worker.
/// Must be called while the controller is already locked.
fn queue_icon_requests(
    store: &Rc<RefCell<IconStore>>,
    icon_req_tx: &std::sync::mpsc::Sender<IconRequest>,
    ctrl: &mut AppController,
) {
    let keys: HashSet<IconKey> = ctrl
        .snapshots_mut()
        .flat_map(|snap| {
            snap.entries
                .iter()
                .filter(|e| e.icon_handle.is_none())
                .map(|e| icon_key_for(&e.path, e.is_directory()))
        })
        .collect();
    if keys.is_empty() {
        return;
    }

    let mut store = store.borrow_mut();
    let mut hits: HashMap<IconKey, u32> = HashMap::new();
    for key in keys {
        if let Some(id) = store.id_for(&key) {
            hits.insert(key, id);
            continue;
        }
        if store.pending.contains(&key) {
            continue;
        }
        store.mark_pending(key.clone());
        let _ = icon_req_tx.send(IconRequest { key: key.clone() });
    }

    // Cache hits resolve synchronously: stamp the known id onto the rows.
    if !hits.is_empty() {
        for snapshot in ctrl.snapshots_mut() {
            for entry in snapshot.entries.iter_mut() {
                if entry.icon_handle.is_some() {
                    continue;
                }
                let key = icon_key_for(&entry.path, entry.is_directory());
                if let Some(id) = hits.get(&key) {
                    entry.icon_handle = Some(IconHandle(*id));
                }
            }
        }
    }
}

struct SidebarData {
    folders: Vec<(KnownFolder, kova_core::domain::Location, Option<IconBitmap>)>,
    drives: Vec<(
        kova_platform_windows::volumes::DriveInfo,
        Option<IconBitmap>,
    )>,
}

fn load_sidebar() -> SidebarData {
    let cache = IconCache::new();
    let folders = [
        KnownFolder::Home,
        KnownFolder::Desktop,
        KnownFolder::Documents,
        KnownFolder::Downloads,
    ]
    .into_iter()
    .filter_map(|folder| {
        let location = resolve_known_folder(folder)?;
        let bitmap = cache.get_or_resolve(&IconKey::Path(location.path.clone()));
        Some((folder, location, bitmap))
    })
    .collect();
    let drives = kova_platform_windows::volumes::list_local_drives()
        .into_iter()
        .map(|drive| {
            let bitmap = cache.get_or_resolve(&IconKey::Drive(drive.path.clone()));
            (drive, bitmap)
        })
        .collect();
    SidebarData { folders, drives }
}

fn apply_sidebar(ui: &MainWindow, data: SidebarData) {
    let state = ui.global::<AppState>();
    for (folder, location, bitmap) in data.folders {
        let path = location.display().into();
        let icon = bitmap.as_ref().map(image_from_bitmap).unwrap_or_default();
        match folder {
            KnownFolder::Home => {
                state.set_home_path(path);
                state.set_home_icon(icon);
            }
            KnownFolder::Desktop => {
                state.set_desktop_path(path);
                state.set_desktop_icon(icon);
            }
            KnownFolder::Documents => {
                state.set_documents_path(path);
                state.set_documents_icon(icon);
            }
            KnownFolder::Downloads => {
                state.set_downloads_path(path);
                state.set_downloads_icon(icon);
            }
        }
    }
    let drives: Vec<DriveItem> = data
        .drives
        .into_iter()
        .map(|(drive, bitmap)| {
            let usage = if drive.total_bytes == 0 {
                0.0
            } else {
                drive.total_bytes.saturating_sub(drive.free_bytes) as f32 / drive.total_bytes as f32
            };
            let detail = if drive.total_bytes == 0 {
                String::new()
            } else {
                format!(
                    "{} free of {}",
                    format_bytes(drive.free_bytes),
                    format_bytes(drive.total_bytes)
                )
            };
            DriveItem {
                name: drive.name.into(),
                path: drive.path.display().to_string().into(),
                icon: bitmap.as_ref().map(image_from_bitmap).unwrap_or_default(),
                usage,
                detail: detail.into(),
            }
        })
        .collect();
    state.set_drives(ModelRc::new(VecModel::from(drives)));
}
fn show_dialog(
    ui: &MainWindow,
    title: &str,
    value: &str,
    mode: PendingDialog,
    pending: &Arc<Mutex<Option<PendingDialog>>>,
) {
    let state = ui.global::<AppState>();
    state.set_dialog_title(title.into());
    state.set_dialog_value(value.into());
    state.set_dialog_visible(true);
    *pending.lock().unwrap() = Some(mode);
}

fn close_dialog(ui: &MainWindow, pending: &Arc<Mutex<Option<PendingDialog>>>) {
    let state = ui.global::<AppState>();
    state.set_dialog_visible(false);
    state.set_dialog_value("".into());
    *pending.lock().unwrap() = None;
}

fn show_error_dialog(ui: &MainWindow, message: &str) {
    let state = ui.global::<AppState>();
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
    let controller = dispatcher.controller();
    let ctrl = controller.lock().unwrap();
    let selected = ctrl.selected_indices();
    ui.global::<AppState>()
        .set_selected_count(selected.len() as i32);
    for i in 0..models.files.row_count() {
        if let Some(mut row) = models.files.row_data(i) {
            let is_selected = selected.contains(&i);
            if row.selected != is_selected {
                row.selected = is_selected;
                models.files.set_row_data(i, row);
            }
        }
    }
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

fn wire_callbacks(
    ui: Weak<MainWindow>,
    dispatcher: CommandDispatcher,
    pending_dialog: Arc<Mutex<Option<PendingDialog>>>,
    last_address: LastAddress,
    models: Rc<UiModels>,
) {
    let d = dispatcher.clone();
    let ui_nav = ui.clone();
    let last_nav = Arc::clone(&last_address);
    let models_nav = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_navigate(move |path: SharedString| {
            if let Err(e) = d.dispatch_navigate(LocationInput::new(path.to_string())) {
                show_action_error(&ui_nav, &d, &last_nav, &models_nav, &e);
            }
        });

    let d = dispatcher.clone();
    let ui_back = ui.clone();
    let last_back = Arc::clone(&last_address);
    let models_back = Rc::clone(&models);
    ui.unwrap().global::<AppState>().on_request_back(move || {
        if let Err(e) = d.dispatch_back() {
            show_action_error(&ui_back, &d, &last_back, &models_back, &e);
        }
    });

    let d = dispatcher.clone();
    let ui_fwd = ui.clone();
    let last_fwd = Arc::clone(&last_address);
    let models_fwd = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_forward(move || {
            if let Err(e) = d.dispatch_forward() {
                show_action_error(&ui_fwd, &d, &last_fwd, &models_fwd, &e);
            }
        });

    let d = dispatcher.clone();
    let ui_parent = ui.clone();
    let last_parent = Arc::clone(&last_address);
    let models_parent = Rc::clone(&models);
    ui.unwrap().global::<AppState>().on_request_parent(move || {
        if let Err(e) = d.dispatch_parent() {
            show_action_error(&ui_parent, &d, &last_parent, &models_parent, &e);
        }
    });

    let d = dispatcher.clone();
    let ui_refresh = ui.clone();
    let last_refresh = Arc::clone(&last_address);
    let models_refresh = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_refresh(move || {
            if let Err(e) = d.dispatch_refresh() {
                show_action_error(&ui_refresh, &d, &last_refresh, &models_refresh, &e);
            }
        });

    let d = dispatcher.clone();
    let ui_new = ui.clone();
    let last_new = Arc::clone(&last_address);
    let models_new = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_new_tab(move || {
            d.dispatch_new_tab();
            if let Some(ui) = ui_new.upgrade() {
                sync_ui(&ui, &d, &last_new, &models_new);
            }
        });

    let d = dispatcher.clone();
    let ui_close = ui.clone();
    let last_close = Arc::clone(&last_address);
    let models_close = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_close_tab(move |idx: i32| {
            if let Err(e) = d.dispatch_close_tab(idx as usize) {
                show_action_error(&ui_close, &d, &last_close, &models_close, &e);
            }
            if let Some(ui) = ui_close.upgrade() {
                sync_ui(&ui, &d, &last_close, &models_close);
            }
        });

    let d = dispatcher.clone();
    let ui_switch = ui.clone();
    let last_switch = Arc::clone(&last_address);
    let models_switch = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_switch_tab(move |idx: i32| {
            d.dispatch_switch_tab(idx as usize);
            if let Some(ui) = ui_switch.upgrade() {
                sync_ui(&ui, &d, &last_switch, &models_switch);
            }
        });

    // Selection and sorting are pure view-model operations: they change
    // controller state that must be pushed back into the UI model.
    let d = dispatcher.clone();
    let ui_sel = ui.clone();

    let models_sel = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_select(move |idx: i32| {
            d.dispatch_select_single(idx as usize);
            if let Some(u) = ui_sel.upgrade() {
                sync_selection(&u, &d, &models_sel);
            }
        });

    let d = dispatcher.clone();
    let ui_toggle = ui.clone();

    let models_toggle = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_toggle(move |idx: i32| {
            d.dispatch_select_toggle(idx as usize);
            if let Some(u) = ui_toggle.upgrade() {
                sync_selection(&u, &d, &models_toggle);
            }
        });

    let d = dispatcher.clone();
    let ui_range = ui.clone();

    let models_range = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_range(move |idx: i32| {
            d.dispatch_select_range(idx as usize);
            if let Some(u) = ui_range.upgrade() {
                sync_selection(&u, &d, &models_range);
            }
        });

    let d = dispatcher.clone();
    let ui_all = ui.clone();

    let models_all = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_select_all(move || {
            d.dispatch_select_all();
            if let Some(u) = ui_all.upgrade() {
                sync_selection(&u, &d, &models_all);
            }
        });

    let d = dispatcher.clone();
    let ui_clear = ui.clone();

    let models_clear = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_clear_selection(move || {
            d.dispatch_clear_selection();
            if let Some(u) = ui_clear.upgrade() {
                sync_selection(&u, &d, &models_clear);
            }
        });

    let d = dispatcher.clone();
    let ui_sort = ui.clone();
    let last_sort = Arc::clone(&last_address);
    let models_sort = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_sort(move |col: i32| {
            d.dispatch_sort(col as usize);
            if let Some(u) = ui_sort.upgrade() {
                sync_ui(&u, &d, &last_sort, &models_sort);
            }
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_activate(move |idx: i32| {
            d.dispatch_activate(idx as usize);
        });

    // Keyboard shortcut: open the primary selection's folder in a new tab.
    let d = dispatcher.clone();
    let ui_new_tab_open = ui.clone();
    let last_nto = Arc::clone(&last_address);
    let models_nto = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_context_open_in_new_tab(move |idx: i32| {
            if let Err(e) = d.dispatch_open_in_new_tab(idx as usize) {
                show_action_error(&ui_new_tab_open, &d, &last_nto, &models_nto, &e);
            }
        });

    // Ctrl+C / Ctrl+X: Explorer-compatible clipboard selection.
    let d = dispatcher.clone();
    let ui_copy = ui.clone();
    let last_copy = Arc::clone(&last_address);
    let models_copy = Rc::clone(&models);
    ui.unwrap().global::<AppState>().on_request_copy(move || {
        if let Err(e) = d.dispatch_clipboard_selection(false) {
            show_action_error(&ui_copy, &d, &last_copy, &models_copy, &e);
        }
    });

    let d = dispatcher.clone();
    let ui_cut = ui.clone();
    let last_cut = Arc::clone(&last_address);
    let models_cut = Rc::clone(&models);
    ui.unwrap().global::<AppState>().on_request_cut(move || {
        if let Err(e) = d.dispatch_clipboard_selection(true) {
            show_action_error(&ui_cut, &d, &last_cut, &models_cut, &e);
        }
    });

    let d = dispatcher.clone();
    let ui_paste = ui.clone();
    let last_paste = Arc::clone(&last_address);
    let models_paste = Rc::clone(&models);
    ui.unwrap().global::<AppState>().on_request_paste(move || {
        if let Err(e) = d.dispatch_paste() {
            show_action_error(&ui_paste, &d, &last_paste, &models_paste, &e);
        }
    });

    let d = dispatcher.clone();
    let ui_delete = ui.clone();
    let last_delete = Arc::clone(&last_address);
    let models_delete = Rc::clone(&models);
    ui.unwrap().global::<AppState>().on_request_delete(move || {
        if let Err(e) = d.dispatch_delete_selection() {
            show_action_error(&ui_delete, &d, &last_delete, &models_delete, &e);
        }
    });

    // Right click on a row: native Explorer shell context menu.
    let d = dispatcher.clone();
    let ui_menu = ui.clone();
    let last_menu = Arc::clone(&last_address);
    let models_menu = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_shell_menu(move |idx: i32| {
            if let Err(e) = d.dispatch_shell_menu(idx as usize) {
                show_action_error(&ui_menu, &d, &last_menu, &models_menu, &e);
            }
        });

    let pending = Arc::clone(&pending_dialog);
    let dialog_ui = ui.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_new_folder(move || {
            if let Some(ui) = dialog_ui.upgrade() {
                show_dialog(
                    &ui,
                    "New Folder",
                    "New folder",
                    PendingDialog::NewFolder,
                    &pending,
                );
            }
        });

    let d = dispatcher.clone();
    let pending = Arc::clone(&pending_dialog);
    let dialog_ui = ui.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_rename(move |idx: i32| {
            let name = d.item_name(idx as usize);
            if name.is_empty() {
                return;
            }
            let Some(path) = d.item_path(idx as usize) else {
                return;
            };
            if let Some(ui) = dialog_ui.upgrade() {
                show_dialog(
                    &ui,
                    "Rename",
                    &name,
                    PendingDialog::Rename { path },
                    &pending,
                );
            }
        });

    let d = dispatcher.clone();
    let pending = Arc::clone(&pending_dialog);
    let dialog_ui = ui.clone();
    let ui_status = ui.clone();
    let status_dispatcher = dispatcher.clone();
    let last_status = Arc::clone(&last_address);
    let models_status = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_dialog_confirm(move |value: SharedString| {
            if let Some(ui) = dialog_ui.upgrade() {
                if ui.global::<AppState>().get_dialog_title() == "Error" {
                    close_dialog(&ui, &pending);
                    return;
                }
            }
            let name = value.to_string();
            if name.is_empty() {
                status_dispatcher.set_status_message("Name must not be empty".into());
                if let Some(u) = ui_status.upgrade() {
                    sync_ui(&u, &status_dispatcher, &last_status, &models_status);
                }
                return;
            }
            let mode = pending.lock().unwrap().take();
            if let Some(mode) = mode {
                match mode {
                    PendingDialog::NewFolder => {
                        d.dispatch_new_folder_named(&name);
                    }
                    PendingDialog::Rename { path } => {
                        d.dispatch_rename_path(path, &name);
                    }
                }
            }
            if let Some(ui) = dialog_ui.upgrade() {
                close_dialog(&ui, &pending);
            }
        });

    let pending = Arc::clone(&pending_dialog);
    let dialog_ui = ui.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_dialog_cancel(move || {
            if let Some(ui) = dialog_ui.upgrade() {
                close_dialog(&ui, &pending);
            }
        });
}

fn update_ui(
    ui: &MainWindow,
    controller: &AppController,
    last_address: &LastAddress,
    models: &UiModels,
) {
    // Only touch the address bar when the navigation state actually changed;
    // otherwise a background refresh would clobber text being typed.
    let address = controller.address_path();
    {
        let mut last = last_address.lock().unwrap();
        if *last != address {
            *last = address.clone();
            ui.global::<AppState>().set_address_path(address.into());
        }
    }

    let state = ui.global::<AppState>();
    state.set_current_path(controller.address_path().into());
    state.set_status_text(controller.status_text().into());
    state.set_item_count(controller.item_count() as i32);
    state.set_selected_count(controller.selected_count() as i32);
    state.set_loading(controller.is_loading());
    state.set_directory_error(controller.directory_error().into());
    state.set_can_go_back(controller.can_go_back());
    state.set_can_go_forward(controller.can_go_forward());
    state.set_can_go_parent(controller.can_go_parent());

    let sort = controller.sort_descriptor();
    state.set_sort_column(sort.column.as_index() as i32);
    state.set_sort_ascending(sort.direction == SortDirection::Ascending);

    let items: Vec<FileListItem> = controller
        .file_list_items()
        .into_iter()
        .map(|item| FileListItem {
            name: item.name.into(),
            type_name: item.type_name.into(),
            size_text: item.size_text.into(),
            modified_text: item.modified_text.into(),
            icon_id: item.icon_id,
            is_dir: item.is_dir,
            selected: item.selected,
        })
        .collect();
    if models.files.row_count() != items.len() {
        models.files.set_vec(items);
    } else {
        for (i, item) in items.into_iter().enumerate() {
            if models.files.row_data(i) != Some(item.clone()) {
                models.files.set_row_data(i, item);
            }
        }
    }

    let tabs: Vec<SharedString> = controller
        .tab_labels()
        .into_iter()
        .map(SharedString::from)
        .collect();
    if models.tabs.row_count() != tabs.len() {
        models.tabs.set_vec(tabs);
    } else {
        for (i, label) in tabs.into_iter().enumerate() {
            if models.tabs.row_data(i) != Some(label.clone()) {
                models.tabs.set_row_data(i, label);
            }
        }
    }
    state.set_active_tab(controller.active_tab_index() as i32);
}
