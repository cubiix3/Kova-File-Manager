use crate::{AppState, MainWindow};
use kova_platform_windows::preview::Preview;
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct Output {
    id: u64,
    result: Result<Preview, String>,
    delay: Duration,
    animated: bool,
}

// Bounded backpressure, with cancellation even while playback is paused.
fn send(tx: &mpsc::SyncSender<Output>, latest: &AtomicU64, mut output: Output) -> bool {
    while latest.load(Ordering::Relaxed) == output.id {
        match tx.try_send(output) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Full(value)) => {
                output = value;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
        }
    }
    false
}

#[derive(Default)]
struct PlaybackClock {
    remaining: Duration,
}
impl PlaybackClock {
    fn tick(&mut self, elapsed: Duration, paused: bool) -> bool {
        if paused {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(elapsed);
        self.remaining.is_zero()
    }
}

pub fn connect(app: &MainWindow) -> slint::Timer {
    let (tx, rx) = mpsc::sync_channel::<(u64, String, i32)>(1);
    let (output_tx, output_rx) = mpsc::sync_channel::<Output>(2);
    let generation = Arc::new(AtomicU64::new(0));
    let latest = generation.clone();
    let worker = std::thread::Builder::new()
        .name("kova-preview".into())
        .spawn(move || {
            while let Ok((id, path, page)) = rx.recv() {
                if latest.load(Ordering::Relaxed) != id {
                    continue;
                }
                let path = std::path::Path::new(&path);
                let animation = kova_platform_windows::preview_animation::stream(
                    path,
                    || latest.load(Ordering::Relaxed) == id,
                    |preview, delay, animated| {
                        send(
                            &output_tx,
                            &latest,
                            Output {
                                id,
                                result: Ok(preview),
                                delay,
                                animated,
                            },
                        )
                    },
                );
                if animation == Ok(true) || latest.load(Ordering::Relaxed) != id {
                    continue;
                }
                let mut result = kova_platform_windows::preview::load(path, page as u32);
                if let (Err(error), Ok(preview)) = (animation, &mut result) {
                    preview
                        .text
                        .push_str(&format!(" · Still preview ({error})"));
                }
                send(
                    &output_tx,
                    &latest,
                    Output {
                        id,
                        result,
                        delay: Duration::ZERO,
                        animated: false,
                    },
                );
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
    let mut clock = PlaybackClock::default();
    let mut last_tick = Instant::now();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let state = ui.global::<AppState>();
            let now = Instant::now();
            let elapsed = now.duration_since(last_tick);
            last_tick = now;
            let path = state.get_preview_path().to_string();
            let key = (
                state.get_preview_visible() && !state.get_drive_overview(),
                path.clone(),
                state.get_preview_page(),
                state.get_preview_revision(),
            );
            if last_key.as_ref() != Some(&key) {
                let id = generation.fetch_add(1, Ordering::Relaxed) + 1;
                state.set_preview_has_image(false);
                state.set_preview_image(slint::Image::default());
                state.set_preview_pages(0);
                state.set_preview_animated(false);
                state.set_preview_paused(false);
                clock.remaining = Duration::ZERO;
                while output_rx.try_recv().is_ok() {}
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
                return;
            }
            if !clock.tick(elapsed, state.get_preview_paused()) {
                return;
            }
            while let Ok(output) = output_rx.try_recv() {
                if generation.load(Ordering::Relaxed) != output.id {
                    continue;
                }
                state.set_preview_animated(output.animated);
                clock.remaining = output.delay;
                match output.result {
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
                break;
            }
        },
    );
    timer
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pause_preserves_remaining_frame_time() {
        let mut clock = PlaybackClock {
            remaining: Duration::from_millis(240),
        };
        assert!(!clock.tick(Duration::from_millis(80), false));
        assert!(!clock.tick(Duration::from_secs(10), true));
        assert_eq!(clock.remaining, Duration::from_millis(160));
        assert!(!clock.tick(Duration::from_millis(159), false));
        assert!(clock.tick(Duration::from_millis(1), false));
    }

    #[test]
    fn full_frame_queue_cancels_without_waiting_for_playback() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let latest = AtomicU64::new(1);
        let output = || Output {
            id: 1,
            result: Err("test".into()),
            delay: Duration::ZERO,
            animated: true,
        };
        assert!(send(&tx, &latest, output()));
        latest.store(2, Ordering::Relaxed);
        assert!(!send(&tx, &latest, output()));
    }
}
