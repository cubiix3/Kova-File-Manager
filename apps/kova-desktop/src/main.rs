#![cfg_attr(windows, windows_subsystem = "windows")]

mod app_state;
mod bridges;
mod default_manager;
mod folder_sizes;
mod preferences;
mod preview;
mod thumbnails;
mod window_chrome;

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
    thumbnails: RefCell<thumbnails::Cache>,
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

    let _menu_theme = kova_platform_windows::window_theme::initialize_dark_menus();
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .select()
        .expect("initialize desktop window backend");
    let app = MainWindow::new().unwrap();
    preferences::restore(&app, &mut app_controller.lock().unwrap());
    window_chrome::connect(&app);
    default_manager::connect(&app, dispatcher.clone());
    let _preview_timer = preview::connect(&app);

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
        thumbnails: RefCell::new(thumbnails::Cache::default()),
    });
    let _thumbnail_timer = thumbnails::connect(&app, app_controller.clone(), ui_models.clone());

    let icon_store = Rc::new(RefCell::new(IconStore::new(Rc::clone(&icons_model))));
    {
        let mut store = icon_store.borrow_mut();
        preseed_icon_store(&mut store, &icons_model, &icon_req_tx);
    }

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
                        let mut edit_path = None;
                        if reveal.as_ref().is_some_and(|(tab, _, _)| *tab == tab_id) {
                            if let Some((tab, path, edit)) = reveal.take() {
                                if tab == ctrl.active_tab_id() {
                                    if let Some(index) = ctrl
                                        .snapshot()
                                        .and_then(|s| s.entries.iter().position(|e| e.path == path))
                                    {
                                        if let Some(selection) = ctrl.selection_mut() {
                                            selection.select_single(index);
                                        }
                                        if edit {
                                            edit_path = Some(path);
                                        }
                                    }
                                }
                            }
                        }
                        queue_icon_requests(&store_ref, &icon_req_ref, &mut ctrl);
                        update_ui(&ui, &ctrl, &last_address_ref, &models_ref);
                        if let Some(path) = edit_path {
                            begin_inline_rename(&ui, &ctrl, path, &pending_for_pump);
                        }
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
                    KovaEvent::ItemRenamed { new_path, .. } => {
                        if ctrl
                            .current_directory()
                            .is_some_and(|loc| Some(loc.path.as_path()) == new_path.parent())
                        {
                            reveal = Some((ctrl.active_tab_id(), new_path, false));
                        }
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
    preferences::save(&app);
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
    if bytes >= 1024 * GB {
        format!("{:.1} TB", bytes as f64 / (1024 * GB) as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
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
    state.set_drives_loading(false);
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
                file_system: drive.file_system.into(),
                total_text: if drive.total_bytes == 0 {
                    "Unavailable".into()
                } else {
                    format_bytes(drive.total_bytes).into()
                },
                free_text: if drive.total_bytes == 0 {
                    "—".into()
                } else {
                    format_bytes(drive.free_bytes).into()
                },
                used_text: if drive.total_bytes == 0 {
                    "—".into()
                } else {
                    format!("{:.1}%", usage * 100.0).into()
                },
                capacity_known: drive.total_bytes > 0,
            }
        })
        .collect();
    state.set_drives(ModelRc::new(VecModel::from(drives)));
}
fn refresh_drive_info(ui: &MainWindow) {
    if ui.global::<AppState>().get_drives_loading() {
        return;
    }
    ui.global::<AppState>().set_drives_loading(true);
    let weak = ui.as_weak();
    if std::thread::Builder::new()
        .name("kova-drive-refresh".into())
        .spawn(move || {
            let data = load_sidebar();
            let _ = weak.upgrade_in_event_loop(move |ui| apply_sidebar(&ui, data));
        })
        .is_err()
    {
        ui.global::<AppState>().set_drives_loading(false);
        show_error_dialog(ui, "Could not start drive refresh");
    }
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
    let controller = dispatcher.controller();
    let ctrl = controller.lock().unwrap();
    let selected = ctrl.selected_indices();
    sync_preview_path(ui, &ctrl);
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
                if entry.is_directory() {
                    ctrl.folder_sizes
                        .get(&entry.path)
                        .and_then(|(bytes, label)| {
                            bytes.map(|bytes| (bytes, label.starts_with('≥')))
                        })
                } else {
                    entry.metadata.size.map(|bytes| (bytes, false))
                }
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
            .filter(|e| !e.is_directory())
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

fn wire_callbacks(
    ui: Weak<MainWindow>,
    dispatcher: CommandDispatcher,
    pending_dialog: Arc<Mutex<Option<PendingDialog>>>,
    last_address: LastAddress,
    models: Rc<UiModels>,
    icon_store: Rc<RefCell<IconStore>>,
    icon_requests: std::sync::mpsc::Sender<IconRequest>,
) {
    let d = dispatcher.clone();
    let ui_view = ui.clone();
    let last_view = Arc::clone(&last_address);
    let models_view = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_view_option(move |option| {
            let Some(ui) = ui_view.upgrade() else { return };
            let state = ui.global::<AppState>();
            match option {
                0 => state.set_show_hidden(!state.get_show_hidden()),
                1 => state.set_show_system(!state.get_show_system()),
                2 => state.set_show_extensions(!state.get_show_extensions()),
                3 => state.set_preview_visible(!state.get_preview_visible()),
                4 => state.set_compact_rows(!state.get_compact_rows()),
                5 => state.set_alternating_rows(!state.get_alternating_rows()),
                6 => state.set_animations(!state.get_animations()),
                7 => state.set_folder_sizes(!state.get_folder_sizes()),
                _ => return,
            }
            let controller = d.controller();
            let mut ctrl = controller.lock().unwrap();
            if option <= 1 {
                ctrl.set_visibility(state.get_show_hidden(), state.get_show_system());
                queue_icon_requests(&icon_store, &icon_requests, &mut ctrl);
            }
            ctrl.show_extensions = state.get_show_extensions();
            ctrl.folder_sizes_enabled = state.get_folder_sizes();
            update_ui(&ui, &ctrl, &last_view, &models_view);
        });
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
            if let Some(ui) = ui_refresh.upgrade() {
                if ui.global::<AppState>().get_drive_overview() {
                    refresh_drive_info(&ui);
                }
            }
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
    let ui_duplicate = ui.clone();
    let last_duplicate = Arc::clone(&last_address);
    let models_duplicate = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_duplicate_location(move || {
            if let Err(e) = d.dispatch_duplicate_location() {
                show_action_error(&ui_duplicate, &d, &last_duplicate, &models_duplicate, &e);
            }
            if let Some(ui) = ui_duplicate.upgrade() {
                sync_ui(&ui, &d, &last_duplicate, &models_duplicate);
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
    let ui_marquee = ui.clone();
    let models_marquee = Rc::clone(&models);
    let gesture = RefCell::new(None);
    ui.unwrap()
        .global::<AppState>()
        .on_request_marquee(move |phase, first, end, additive| {
            let controller = d.controller();
            let mut ctrl = controller.lock().unwrap();
            let key = (ctrl.active_tab_id(), ctrl.snapshot().map(|s| s.request_id));
            let mut gesture = gesture.borrow_mut();
            if phase == 0 {
                *gesture = ctrl.selection_mut().map(|s| (key, s.clone(), None));
                return;
            }
            let Some((saved_key, baseline, last_range)) = gesture.as_mut() else {
                return;
            };
            if *saved_key != key || ctrl.is_loading() {
                *gesture = None;
                return;
            }
            let range = (first.max(0) as usize, end.max(0) as usize, additive);
            if phase == 1 && *last_range == Some(range) {
                return;
            }
            let len = ctrl.item_count();
            if let Some(selection) = ctrl.selection_mut() {
                if phase == 1 {
                    selection.marquee(baseline, range.0..range.1, additive, len);
                    *last_range = Some(range);
                } else if phase == 3 {
                    *selection = baseline.clone();
                }
            }
            if phase != 1 {
                *gesture = None;
            }
            drop(gesture);
            drop(ctrl);
            if let Some(u) = ui_marquee.upgrade() {
                sync_selection(&u, &d, &models_marquee);
            }
        });
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

    let d = dispatcher.clone();
    let pending = Arc::clone(&pending_dialog);
    let dialog_ui = ui.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_new_folder(move || {
            let Some(ui) = dialog_ui.upgrade() else {
                return;
            };
            let state = ui.global::<AppState>();
            if state.get_creating_folder() {
                return;
            }
            let controller = d.controller();
            let ctrl = controller.lock().unwrap();
            let Some(parent) = ctrl.current_directory().cloned() else {
                return;
            };
            if ctrl.is_loading() {
                return;
            }
            *pending.lock().unwrap() = Some(PendingDialog::Creating {
                tab: ctrl.active_tab_id(),
                parent,
            });
            drop(ctrl);
            state.set_inline_visible(false);
            state.set_creating_folder(true);
            d.dispatch_new_folder_named("New folder");
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
                let controller = d.controller();
                let ctrl = controller.lock().unwrap();
                begin_inline_rename(&ui, &ctrl, path, &pending);
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
                if ui.global::<AppState>().get_dialog_visible()
                    && ui.global::<AppState>().get_dialog_title() == "Error"
                {
                    close_dialog(&ui, &pending);
                    return;
                }
            }
            let name = value.to_string();
            if let Err(error) = kova_ops::file_ops::validate_name(&name) {
                status_dispatcher.set_status_message(error.to_string());
                if let Some(u) = ui_status.upgrade() {
                    sync_ui(&u, &status_dispatcher, &last_status, &models_status);
                }
                return;
            }
            let mode = pending.lock().unwrap().take();
            if let Some(mode) = mode {
                match mode {
                    PendingDialog::Creating { .. } => {}
                    PendingDialog::Rename { path, suffix } => {
                        let name = format!("{name}{suffix}");
                        if path
                            .file_name()
                            .is_none_or(|old| old != std::ffi::OsStr::new(&name))
                        {
                            d.dispatch_rename_path(path, &name);
                        }
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

// Keep only the closest ancestors visible; Ctrl+L exposes the full editable path.
// Path traversal here is lexical and never queries a drive or network share.
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

fn update_ui(
    ui: &MainWindow,
    controller: &AppController,
    last_address: &LastAddress,
    models: &UiModels,
) {
    sync_preview_path(ui, controller);
    // Only touch the address bar when the navigation state actually changed;
    // otherwise a background refresh would clobber text being typed.
    let address = controller.address_path();
    let mut crumbs = breadcrumb_items(&address);
    for crumb in &mut crumbs {
        crumb.label = location_label(ui, crumb.path.as_str(), crumb.label.as_str()).into();
    }
    if crumbs.len() == 1 && address != "Home" {
        crumbs.insert(
            0,
            Breadcrumb {
                label: "Dieser PC".into(),
                path: "Home".into(),
            },
        );
    }
    let current = ui.global::<AppState>().get_breadcrumbs();
    if current.row_count() != crumbs.len()
        || current.iter().zip(&crumbs).any(|(old, new)| old != *new)
    {
        ui.global::<AppState>()
            .set_breadcrumbs(ModelRc::new(VecModel::from(crumbs)));
    }
    {
        let mut last = last_address.lock().unwrap();
        if *last != address {
            *last = address.clone();
            ui.global::<AppState>().set_address_path(address.into());
        }
    }

    let state = ui.global::<AppState>();
    state.set_current_path(controller.address_path().into());
    state.set_drive_overview(
        controller
            .current_location()
            .is_some_and(|location| location.is_home()),
    );
    state.set_status_text(controller.status_text().into());
    state.set_item_count(controller.item_count() as i32);
    state.set_filtered_count(controller.filtered_count() as i32);
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
        .enumerate()
        .map(|(index, item)| {
            let thumbnail = thumbnails::image_for_row(&state, controller, models, index);
            FileListItem {
                has_thumbnail: thumbnail.is_some(),
                thumbnail: thumbnail.unwrap_or_default(),
                name: item.name.into(),
                type_name: item.type_name.into(),
                size_text: item.size_text.into(),
                modified_text: item.modified_text.into(),
                icon_id: item.icon_id,
                is_dir: item.is_dir,
                selected: item.selected,
            }
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
        .map(|label| SharedString::from(location_label(ui, &label, &label)))
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

fn location_label(ui: &MainWindow, path: &str, fallback: &str) -> String {
    ui.global::<AppState>()
        .get_drives()
        .iter()
        .find(|drive| drive.path.as_str().eq_ignore_ascii_case(path))
        .map(|drive| drive.name.to_string())
        .unwrap_or_else(|| fallback.to_owned())
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
