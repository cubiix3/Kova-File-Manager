mod app_state;
mod bridges;

use app_state::AppController;
use bridges::CommandDispatcher;
use kova_core::domain::{KovaEvent, LocationInput};
use kova_ops::worker::{WorkerCommand, spawn_worker};
use kova_platform_windows::known_folders::{KnownFolder, resolve_known_folder};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let initial = kova_platform_windows::known_folders::initial_location();
    tracing::info!("Kova starting at {}", initial.display());

    let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>(64);
    let (evt_tx, mut evt_rx) = mpsc::channel::<KovaEvent>(64);

    spawn_worker(cmd_rx, evt_tx);

    let app_controller = Arc::new(Mutex::new(AppController::new(initial.clone())));
    let dispatcher = CommandDispatcher::new(
        Arc::clone(&app_controller),
        cmd_tx.clone(),
        Default::default(),
    );

    let app = MainWindow::new().unwrap();
    let ui = app.as_weak();

    // Wire UI callbacks.
    wire_callbacks(ui.clone(), dispatcher.clone());

    // Populate known-folder sidebar targets.
    set_known_folder_paths(&ui);

    // Initial load.
    let start_tab = app_controller.lock().unwrap().active_tab_id();
    let start_loc = app_controller
        .lock()
        .unwrap()
        .current_location()
        .cloned()
        .unwrap_or(initial);
    dispatcher.request_enumeration(start_tab, start_loc);

    // Spawn event consumer that maps core events back into the UI.
    let ui_for_events = ui.clone();
    let controller_for_events = Arc::clone(&app_controller);
    let reload_dispatcher = dispatcher.clone();
    tokio::spawn(async move {
        while let Some(event) = evt_rx.recv().await {
            let Some(ui) = ui_for_events.upgrade() else {
                break;
            };
            match event {
                KovaEvent::DirectoryLoaded { tab_id, snapshot } => {
                    let mut ctrl = controller_for_events.lock().unwrap();
                    if ctrl.active_tab_id() == tab_id
                        && ctrl.is_current_request(tab_id, snapshot.request_id)
                    {
                        ctrl.apply_snapshot(tab_id, snapshot);
                        update_ui(&ui, &ctrl);
                    }
                }
                KovaEvent::DirectoryError {
                    tab_id, location, ..
                } => {
                    let mut ctrl = controller_for_events.lock().unwrap();
                    if ctrl.active_tab_id() == tab_id {
                        ctrl.set_status(format!("Error reading {}", location.display()));
                        update_ui(&ui, &ctrl);
                    }
                }
                KovaEvent::FolderCreated { parent: _, name: _ } => {
                    let ctrl = controller_for_events.lock().unwrap();
                    let tab_id = ctrl.active_tab_id();
                    if let Some(loc) = ctrl.current_location().cloned() {
                        drop(ctrl);
                        reload_dispatcher.request_enumeration(tab_id, loc);
                    }
                }
                KovaEvent::ItemRenamed {
                    old_path: _,
                    new_path: _,
                } => {
                    let ctrl = controller_for_events.lock().unwrap();
                    let tab_id = ctrl.active_tab_id();
                    if let Some(loc) = ctrl.current_location().cloned() {
                        drop(ctrl);
                        reload_dispatcher.request_enumeration(tab_id, loc);
                    }
                }
                KovaEvent::OperationError {
                    context,
                    error_message,
                } => {
                    tracing::error!("{}: {}", context, error_message);
                    let mut ctrl = controller_for_events.lock().unwrap();
                    ctrl.set_status(format!("{}: {}", context, error_message));
                    update_ui(&ui, &ctrl);
                }
                _ => {}
            }
        }
    });

    app.run().unwrap();
}

fn set_known_folder_paths(ui: &Weak<MainWindow>) {
    fn path(folder: KnownFolder) -> SharedString {
        resolve_known_folder(folder)
            .map(|l| l.display())
            .unwrap_or_default()
            .into()
    }
    if let Some(ui) = ui.upgrade() {
        let state = ui.global::<AppState>();
        state.set_home_path(path(KnownFolder::Home));
        state.set_desktop_path(path(KnownFolder::Desktop));
        state.set_documents_path(path(KnownFolder::Documents));
        state.set_downloads_path(path(KnownFolder::Downloads));
    }
}

fn wire_callbacks(ui: Weak<MainWindow>, dispatcher: CommandDispatcher) {
    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_navigate(move |path: SharedString| {
            let _ = d.dispatch_navigate(LocationInput::new(path.to_string()));
        });

    let d = dispatcher.clone();
    ui.unwrap().global::<AppState>().on_request_back(move || {
        let _ = d.dispatch_back();
    });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_forward(move || {
            let _ = d.dispatch_forward();
        });

    let d = dispatcher.clone();
    ui.unwrap().global::<AppState>().on_request_parent(move || {
        let _ = d.dispatch_parent();
    });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_refresh(move || {
            let _ = d.dispatch_refresh();
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_new_tab(move || {
            d.dispatch_new_tab();
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_close_tab(move |idx: i32| {
            let _ = d.dispatch_close_tab(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_switch_tab(move |idx: i32| {
            d.dispatch_switch_tab(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_select(move |idx: i32| {
            d.dispatch_select_single(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_toggle(move |idx: i32| {
            d.dispatch_select_toggle(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_range(move |idx: i32| {
            d.dispatch_select_range(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_select_all(move || {
            d.dispatch_select_all();
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_activate(move |idx: i32| {
            d.dispatch_activate(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_new_folder(move || {
            d.dispatch_new_folder();
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_rename(move |idx: i32| {
            d.dispatch_rename(idx as usize);
        });

    let d = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_sort(move |col: i32| {
            d.dispatch_sort(col as usize);
        });
}

fn update_ui(ui: &MainWindow, controller: &AppController) {
    ui.global::<AppState>()
        .set_address_path(controller.address_path().into());
    ui.global::<AppState>()
        .set_status_text(controller.status_text().into());
    ui.global::<AppState>()
        .set_can_go_back(controller.can_go_back());
    ui.global::<AppState>()
        .set_can_go_forward(controller.can_go_forward());
    ui.global::<AppState>()
        .set_can_go_parent(controller.can_go_parent());

    let items: Vec<FileListItem> = controller
        .file_list_items()
        .into_iter()
        .map(|item| FileListItem {
            name: item.name.into(),
            type_name: item.type_name.into(),
            size_text: item.size_text.into(),
            modified_text: item.modified_text.into(),
            icon_id: item.icon_id,
            selected: item.selected,
        })
        .collect();
    let model = VecModel::from(items);
    ui.global::<AppState>().set_files(ModelRc::new(model));

    let tabs: Vec<SharedString> = controller
        .tab_labels()
        .into_iter()
        .map(SharedString::from)
        .collect();
    let tab_model = VecModel::from(tabs);
    ui.global::<AppState>().set_tabs(ModelRc::new(tab_model));
    ui.global::<AppState>()
        .set_active_tab(controller.active_tab_index() as i32);
}
