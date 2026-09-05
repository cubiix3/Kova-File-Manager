use crate::{AppState, MainWindow};
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

pub fn connect(app: &MainWindow) -> slint::Timer {
    let (tx, rx) = mpsc::sync_channel::<(u64, String, i32)>(1);
    let generation = Arc::new(AtomicU64::new(0));
    let latest = generation.clone();
    let weak = app.as_weak();
    let worker = std::thread::Builder::new()
        .name("kova-preview".into())
        .spawn(move || {
            while let Ok((id, path, page)) = rx.recv() {
                if latest.load(Ordering::Relaxed) != id {
                    continue;
                }
                let result =
                    kova_platform_windows::preview::load(std::path::Path::new(&path), page as u32);
                let latest = latest.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let state = ui.global::<AppState>();
                    if latest.load(Ordering::Relaxed) != id
                        || !state.get_preview_visible()
                        || state.get_preview_path() != path
                        || state.get_preview_page() != page
                    {
                        return;
                    }
                    match result {
                        Ok(preview) => {
                            state.set_preview_text(preview.text.into());
                            state.set_preview_pages(preview.pages as i32);
                            if let Some((w, h, bytes)) = preview.pixels {
                                let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
                                pixels.make_mut_bytes().copy_from_slice(&bytes);
                                state.set_preview_image(slint::Image::from_rgba8(pixels));
                                state.set_preview_has_image(true);
                            }
                        }
                        Err(message) => state.set_preview_text(message.into()),
                    }
                });
            }
        });
    let timer = slint::Timer::default();
    if worker.is_err() {
        app.global::<AppState>()
            .set_preview_text("Preview worker unavailable".into());
        return timer;
    }
    let weak = app.as_weak();
    let mut last_key = None;
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(120),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            let path = state.get_preview_path().to_string();
            let key = (
                state.get_preview_visible(),
                path.clone(),
                state.get_preview_page(),
                state.get_preview_revision(),
            );
            if last_key.as_ref() == Some(&key) {
                return;
            }
            let id = generation.fetch_add(1, Ordering::Relaxed) + 1;
            state.set_preview_has_image(false);
            state.set_preview_image(slint::Image::default());
            state.set_preview_pages(0);
            if !key.0 || path.is_empty() {
                state.set_preview_title("Preview".into());
                state.set_preview_text("Select one file to preview".into());
                last_key = Some(key);
                return;
            }
            state.set_preview_title(
                std::path::Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref()
                    .into(),
            );
            state.set_preview_text("Loading preview…".into());
            if tx.try_send((id, path, key.2)).is_ok() {
                last_key = Some(key);
            }
        },
    );
    timer
}
