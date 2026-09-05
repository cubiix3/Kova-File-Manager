//! Custom caption controls backed by the existing winit window. Slint's
//! frameless resize border handles edge/corner resizing; no Win32 hooks.
use crate::MainWindow;
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor};

pub fn connect(app: &MainWindow) {
    let weak = app.as_weak();

    app.on_window_action(move |action| {
        let Some(app) = weak.upgrade() else { return };
        if action == 3 {
            if let Err(error) = app.hide() {
                tracing::warn!(%error, "close window failed");
            }
            return;
        }
        app.window().with_winit_window(|window| match action {
            0 => {
                if let Err(error) = window.drag_window() {
                    tracing::debug!(%error, "window drag unavailable");
                }
            }
            1 => window.set_minimized(true),
            2 => window.set_maximized(!window.is_maximized()),
            _ => {}
        });
    });
    let weak = app.as_weak();
    app.window().on_winit_window_event(move |window, _event| {
        if let Some(app) = weak.upgrade() {
            window.with_winit_window(|native| app.set_window_maximized(native.is_maximized()));
        }
        EventResult::Propagate
    });
}
