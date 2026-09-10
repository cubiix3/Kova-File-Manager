//! Incremental Slint model synchronization and view properties.
use crate::*;

pub(crate) fn update_ui(
    ui: &MainWindow,
    controller: &AppController,
    last_address: &LastAddress,
    models: &UiModels,
) {
    // Dismiss before selection/count properties can hide a focused menu item.
    let render_key = (
        controller.active_tab_id().0,
        controller.revision,
        controller.show_extensions,
        controller.folder_sizes_enabled,
    );
    if models.render_key.get() != render_key {
        ui.invoke_dismiss_file_menu();
    }
    sync_preview_path(ui, controller);
    // Only touch the address bar when the navigation state actually changed;
    // otherwise a background refresh would clobber text being typed.
    let address = controller.address_path();
    let mut crumbs = breadcrumb_items(&address);
    for crumb in &mut crumbs {
        crumb.label = location_label(ui, crumb.path.as_str(), crumb.label.as_str()).into();
    }
    if crumbs.len() == 1 && controller.current_directory().is_some() {
        crumbs.insert(
            0,
            Breadcrumb {
                label: "This PC".into(),
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
    if !state.get_search_pending() {
        state.set_search_text(controller.search_text().into());
    }
    let recent: Vec<Breadcrumb> = controller
        .recent_folders
        .iter()
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
    if state.get_recent_folders().iter().collect::<Vec<_>>() != recent {
        state.set_recent_folders(ModelRc::new(VecModel::from(recent)));
    }
    state.set_current_path(controller.address_path().into());
    state.set_search_recursive(
        controller
            .recursive_tabs
            .contains(&controller.active_tab_id()),
    );
    let filters = controller
        .filters
        .get(&controller.active_tab_id())
        .copied()
        .unwrap_or_default();
    state.set_filter_type(filters[0] as i32);
    state.set_filter_size(filters[1] as i32);
    state.set_filter_date(filters[2] as i32);
    state.set_filesystem_location(controller.current_directory().is_some());
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

    let model_started = std::time::Instant::now();
    if models.render_key.get() != render_key {
        models.files.replace(
            controller.snapshot_shared(),
            controller.show_extensions,
            controller.folder_sizes_enabled,
            controller.folder_sizes.clone(),
            controller.selected_indices(),
        );

        models.render_key.set(render_key);
        if controller.item_count() >= 2000 {
            tracing::info!(
                entries = controller.item_count(),
                elapsed_ms = model_started.elapsed().as_secs_f64() * 1000.,
                "UI model update"
            );
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
    let tab_paths: Vec<SharedString> = controller
        .tab_locations()
        .iter()
        .map(|(_, location)| {
            if location.is_virtual() {
                SharedString::default()
            } else {
                location.display().into()
            }
        })
        .collect();
    if state.get_tab_paths().iter().collect::<Vec<_>>() != tab_paths {
        state.set_tab_paths(ModelRc::new(VecModel::from(tab_paths)));
    }
    state.set_active_tab(controller.active_tab_index() as i32);
    if !state.get_search_pending()
        && !controller.view_in_flight()
        && !controller.request_in_flight(controller.active_tab_id())
    {
        crate::diagnostics::ready(controller.active_tab_id());
    }
}
