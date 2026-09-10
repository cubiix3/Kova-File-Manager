//! Read-only metadata collection on the preview worker, never on the UI thread.
use std::{
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
};
use windows::{
    Win32::{
        Foundation::PROPERTYKEY,
        System::Com::StructuredStorage::PropVariantToUInt64,
        UI::Shell::PropertiesSystem::{
            GPS_DEFAULT, IPropertyStore, PSGetPropertyKeyFromName,
            SHGetPropertyStoreFromParsingName,
        },
    },
    core::PCWSTR,
};

pub fn read(path: &Path) -> Vec<(String, String)> {
    let mut rows = vec![
        (
            "Name".into(),
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
        ),
        ("Path".into(), path.to_string_lossy().into_owned()),
    ];
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            rows.push(("Unavailable".into(), error.to_string()));
            return rows;
        }
    };
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    rows.push((
        "Type".into(),
        if metadata.is_dir() {
            "Folder".into()
        } else if ext.is_empty() {
            "File".into()
        } else {
            format!("{} file", ext.to_uppercase())
        },
    ));
    if !metadata.is_dir() {
        rows.push((
            "Size".into(),
            format!("{} bytes", crate::formatting::integer(metadata.len())),
        ));
    }
    if !ext.is_empty() && !metadata.is_dir() {
        rows.push(("Extension".into(), format!(".{ext}")));
    }
    for (label, date) in [
        ("Created", metadata.created()),
        ("Modified", metadata.modified()),
    ] {
        if let Ok(date) = date {
            let date: chrono::DateTime<chrono::Local> = date.into();
            rows.push((label.into(), crate::formatting::date(date, true)));
        }
    }
    // Do not hydrate cloud placeholders or follow reparse points merely to
    // obtain optional media properties. Remote handlers stay out of this path.
    if metadata.is_dir()
        || metadata.file_attributes() & 0x0044_1400 != 0
        || !crate::folder_size::is_local_fixed(path)
    {
        return rows;
    }
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
    // SAFETY: initialize this worker only; balance success after store disposal.
    if unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_err() {
        return rows;
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    let _apartment = Apartment;
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: terminated path remains alive for this call; the store and all
    // variants are owned by this apartment and dropped before it is released.
    let store: Result<IPropertyStore, _> =
        unsafe { SHGetPropertyStoreFromParsingName(PCWSTR(wide.as_ptr()), None, GPS_DEFAULT) };
    if let Ok(store) = store {
        let number = |name: &str| -> Option<u64> {
            let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
            let mut key = PROPERTYKEY::default();
            // SAFETY: live output key, terminated name, and live property store.
            // PROPVARIANT has windows-rs RAII cleanup, including failure paths.
            unsafe {
                PSGetPropertyKeyFromName(PCWSTR(name.as_ptr()), &mut key).ok()?;
                let value = store.GetValue(&key).ok()?;
                PropVariantToUInt64(&value).ok().filter(|n| *n > 0)
            }
        };
        let dimensions = number("System.Image.HorizontalSize")
            .zip(number("System.Image.VerticalSize"))
            .or_else(|| number("System.Video.FrameWidth").zip(number("System.Video.FrameHeight")));
        if let Some((width, height)) = dimensions {
            rows.push(("Dimensions".into(), format!("{width} × {height}")));
        }
        if let Some(ticks) = number("System.Media.Duration") {
            let seconds = ticks / 10_000_000;
            rows.push((
                "Duration".into(),
                format!(
                    "{}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                ),
            ));
        }
    }
    rows
}
