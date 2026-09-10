//! UI command routing; filesystem mutations remain in worker modules.
use crate::*;

pub(crate) fn wire_callbacks(
    ui: Weak<MainWindow>,
    dispatcher: CommandDispatcher,
    pending_dialog: Arc<Mutex<Option<PendingDialog>>>,
    last_address: LastAddress,
    models: Rc<UiModels>,
    icon_store: Rc<RefCell<IconStore>>,
    icon_requests: std::sync::mpsc::SyncSender<IconRequest>,
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
    let actions_copy = dispatcher.clone();
    ui.unwrap().global::<AppState>().on_copy_text(move |text| {
        if let Err(error) = kova_platform_windows::clipboard::set_clipboard_text(&text) {
            actions_copy.set_status_message(format!("Copy failed: {error}"));
        } else {
            actions_copy.set_status_message("Copied to clipboard".into());
        }
    });
    let paths_copy = dispatcher.clone();
    let paths_ui = ui.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_copy_selected_paths(move || {
            let text = paths_copy
                .controller()
                .lock()
                .unwrap()
                .selected_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\r\n");
            if !text.is_empty() {
                if let Some(ui) = paths_ui.upgrade() {
                    ui.global::<AppState>().invoke_copy_text(text.into());
                }
            }
        });
    let actions = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_search_scope(move |recursive| {
            let controller = actions.controller();
            let mut ctrl = controller.lock().unwrap();
            let id = ctrl.active_tab_id();
            if recursive {
                ctrl.recursive_tabs.insert(id);
            } else {
                ctrl.recursive_tabs.remove(&id);
            }
            let location = ctrl.current_directory().cloned();
            drop(ctrl);
            if let Some(location) = location {
                actions.request_enumeration(id, location);
            }
        });
    let actions = dispatcher.clone();
    ui.unwrap()
        .global::<AppState>()
        .on_request_filter(move |kind, value| {
            let controller = actions.controller();
            let mut ctrl = controller.lock().unwrap();
            let id = ctrl.active_tab_id();
            if let Some(filter) = ctrl
                .filters
                .entry(id)
                .or_default()
                .get_mut(kind.max(0) as usize)
            {
                *filter = value.max(0) as usize;
            }
            ctrl.refilter(Some(id));
            ctrl.set_status("Filters updated");
        });
    let search_timer = Rc::new(slint::Timer::default());
    let ui_search = ui.clone();
    let last_search = Arc::clone(&last_address);
    let models_search = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_search(move |text| {
            let Some(ui) = ui_search.upgrade() else {
                return;
            };
            if ui.global::<AppState>().get_inline_visible() {
                return;
            }
            let expected_tab = d.controller().lock().unwrap().active_tab_id();
            crate::diagnostics::begin(expected_tab, "search");
            let expected_location = d.controller().lock().unwrap().current_location().cloned();
            ui.global::<AppState>().set_search_text(text.clone());
            ui.global::<AppState>().set_search_pending(true);
            let weak = ui.as_weak();
            let actions = d.clone();
            let address = last_search.clone();
            let models = models_search.clone();
            search_timer.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(150),
                move || {
                    let Some(ui) = weak.upgrade() else { return };
                    ui.global::<AppState>().set_search_pending(false);
                    let controller = actions.controller();
                    let mut ctrl = controller.lock().unwrap();
                    if ctrl.active_tab_id() != expected_tab
                        || ctrl.current_location() != expected_location.as_ref()
                    {
                        return;
                    }
                    ctrl.set_search(text.to_string());
                    update_ui(&ui, &ctrl, &address, &models);
                },
            );
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
    ui.unwrap().global::<AppState>().on_request_grid_marquee(
        move |phase, first, end, left, right, columns, additive| {
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
            let columns = columns.max(1) as usize;
            let range = (
                first.max(0) as usize,
                end.max(0) as usize,
                left.clamp(0, columns as i32) as usize,
                right.clamp(0, columns as i32) as usize,
                columns,
                additive,
            );
            if phase == 1 && *last_range == Some(range) {
                return;
            }
            let len = ctrl.item_count();
            if let Some(selection) = ctrl.selection_mut() {
                if phase == 1 {
                    let rows =
                        range.0.min(len.div_ceil(columns))..range.1.min(len.div_ceil(columns));
                    let indices =
                        rows.flat_map(|row| (range.2..range.3).map(move |col| row * columns + col));
                    selection.marquee_indices(baseline, indices, additive, len);
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
        },
    );
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

    // More Windows options / Shift+F10: retain the native Explorer extension menu.
    let d = dispatcher.clone();
    let ui_menu = ui.clone();
    let last_menu = Arc::clone(&last_address);
    let models_menu = Rc::clone(&models);
    ui.unwrap()
        .global::<AppState>()
        .on_request_shell_menu(move |idx: i32| {
            let context = {
                let controller = d.controller();
                let ctrl = controller.lock().unwrap();
                (
                    ctrl.active_tab_id(),
                    ctrl.path_at(idx as usize),
                    ctrl.selected_paths(),
                )
            };
            let d = d.clone();
            let ui_menu = ui_menu.clone();
            let last_menu = Arc::clone(&last_menu);
            let models_menu = Rc::clone(&models_menu);
            // TrackPopupMenu runs a native modal loop. Give Slint a frame to
            // paint the dismissed themed menu before entering that loop.
            if let Some(ui) = ui_menu.upgrade() {
                ui.window().request_redraw();
            }
            slint::Timer::single_shot(std::time::Duration::from_millis(32), move || {
                if ui_menu.upgrade().is_none() {
                    return;
                }
                let current = {
                    let controller = d.controller();
                    let ctrl = controller.lock().unwrap();
                    (
                        ctrl.active_tab_id(),
                        ctrl.path_at(idx as usize),
                        ctrl.selected_paths(),
                    )
                };
                if context != current {
                    return;
                }
                if let Err(e) = d.dispatch_shell_menu(idx as usize) {
                    show_action_error(&ui_menu, &d, &last_menu, &models_menu, &e);
                }
            });
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
