//! Background drive and known-folder discovery and sidebar models.
use crate::*;

pub(crate) struct SidebarData {
    folders: Vec<(KnownFolder, kova_core::domain::Location, Option<IconBitmap>)>,
    drives: Vec<(
        kova_platform_windows::volumes::DriveInfo,
        Option<IconBitmap>,
    )>,
}

pub(crate) fn load_sidebar() -> SidebarData {
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
            let bitmap = cache.get_or_resolve(&if drive.drive_type == "Network" {
                IconKey::Folder
            } else {
                IconKey::Drive(drive.path.clone())
            });
            (drive, bitmap)
        })
        .collect();
    SidebarData { folders, drives }
}

pub(crate) fn apply_sidebar(ui: &MainWindow, data: SidebarData) {
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
                drive_type: drive.drive_type.as_str().into(),
                total_text: if drive.total_bytes == 0 {
                    if drive.drive_type == "Network" {
                        "Not measured".into()
                    } else {
                        "Unavailable".into()
                    }
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
pub(crate) fn refresh_drive_info(ui: &MainWindow) {
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
