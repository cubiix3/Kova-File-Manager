//! Visible-row thumbnails. All decoding happens outside the UI thread.
use crate::{AppState, MainWindow, UiModels, app_state::AppController};
use kova_core::domain::TabId;
use slint::{ComponentHandle, Model, Rgba8Pixel, SharedPixelBuffer};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

type Context = Option<(TabId, u64)>;
const CACHE_LIMIT: usize = 256;

#[derive(Default)]
pub struct Cache {
    context: Context,
    entries: HashMap<PathBuf, Option<slint::Image>>,
    order: VecDeque<PathBuf>,
}

fn context(ctrl: &AppController) -> Context {
    if ctrl.is_loading() {
        return None;
    }
    ctrl.snapshot()
        .map(|s| (ctrl.active_tab_id(), s.request_id))
}

fn range(state: &AppState) -> std::ops::Range<usize> {
    let first = state.get_first_visible_row().max(0) as usize;
    first..first.saturating_add(state.get_visible_row_count().clamp(1, 128) as usize)
}

pub fn image_for_row(
    state: &AppState,
    ctrl: &AppController,
    models: &UiModels,
    index: usize,
) -> Option<slint::Image> {
    if !range(state).contains(&index) {
        return None;
    }
    let cache = models.thumbnails.borrow();
    if cache.context != context(ctrl) {
        return None;
    }
    let path = &ctrl.snapshot()?.entries.get(index)?.path;
    cache.entries.get(path)?.clone()
}

pub fn connect(
    app: &MainWindow,
    controller: Arc<Mutex<AppController>>,
    models: Rc<UiModels>,
) -> slint::Timer {
    let (requests, input) = mpsc::sync_channel::<(u64, Vec<PathBuf>)>(1);
    let (results, output) = mpsc::sync_channel(8);
    let generation = Arc::new(AtomicU64::new(0));
    let latest = generation.clone();
    let worker = std::thread::Builder::new()
        .name("kova-thumbnails".into())
        .spawn(move || {
            while let Ok((id, paths)) = input.recv() {
                for path in paths {
                    if latest.load(Ordering::Relaxed) != id {
                        break;
                    }
                    let bitmap = kova_platform_windows::preview::load_thumbnail(&path)
                        .ok()
                        .and_then(|p| p.pixels);
                    if results.send((id, path, bitmap)).is_err() {
                        return;
                    }
                }
            }
        });
    let timer = slint::Timer::default();
    if worker.is_err() {
        return timer;
    }
    let weak = app.as_weak();
    let mut pending = HashSet::new();
    let mut displayed = Vec::new();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            let ctrl = controller.lock().unwrap();
            let current = context(&ctrl);
            let mut cache = models.thumbnails.borrow_mut();
            if cache.context != current {
                generation.fetch_add(1, Ordering::Relaxed);
                *cache = Cache {
                    context: current,
                    ..Cache::default()
                };
                pending.clear();
            }
            let id = generation.load(Ordering::Relaxed);
            for (result_id, path, bitmap) in output.try_iter().take(64) {
                if result_id != id {
                    continue;
                }
                pending.remove(&path);
                let image = bitmap.map(|(w, h, bytes)| {
                    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
                    buffer.make_mut_bytes().copy_from_slice(&bytes);
                    slint::Image::from_rgba8(buffer)
                });
                while cache.entries.len() >= CACHE_LIMIT {
                    if let Some(old) = cache.order.pop_front() {
                        cache.entries.remove(&old);
                    }
                }
                cache.order.push_back(path.clone());
                cache.entries.insert(path, image);
            }
            let visible = range(&state);
            for index in displayed.drain(..) {
                if !visible.contains(&index) || current.is_none() {
                    if let Some(mut row) = models.files.row_data(index) {
                        if row.has_thumbnail {
                            row.has_thumbnail = false;
                            row.thumbnail = slint::Image::default();
                            models.files.set_row_data(index, row);
                        }
                    }
                }
            }
            let mut needed = Vec::new();
            if current.is_some() {
                if let Some(snapshot) = ctrl.snapshot() {
                    for index in visible {
                        let Some(entry) = snapshot.entries.get(index) else {
                            break;
                        };
                        let image = cache.entries.get(&entry.path).and_then(Clone::clone);
                        if let Some(mut row) = models.files.row_data(index) {
                            let thumbnail = image.clone().unwrap_or_default();
                            if row.has_thumbnail != image.is_some() || row.thumbnail != thumbnail {
                                row.has_thumbnail = image.is_some();
                                row.thumbnail = thumbnail;
                                models.files.set_row_data(index, row);
                            }
                        }
                        displayed.push(index);
                        if entry.is_file()
                            && !cache.entries.contains_key(&entry.path)
                            && !pending.contains(&entry.path)
                        {
                            needed.push(entry.path.clone());
                        }
                    }
                }
            }
            if !needed.is_empty() && requests.try_send((id, needed.clone())).is_ok() {
                pending.extend(needed);
            }
        },
    );
    timer
}
