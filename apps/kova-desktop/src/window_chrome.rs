//! Custom caption controls backed by the existing winit window. Slint's
//! explicit edge/corner input surfaces start native resizing; no Win32 hooks.
use crate::MainWindow;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use slint::winit_030::winit::platform::windows::{IconExtWindows, WindowExtWindows};
use slint::winit_030::winit::window::{Icon, ResizeDirection};
use slint::winit_030::{EventResult, WinitWindowAccessor};

pub fn connect(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_resize_window(move |direction| {
        let Some(app) = weak.upgrade() else { return };
        let Some(direction) = resize_direction(direction) else {
            return;
        };
        app.window().with_winit_window(|window| {
            if !window.is_maximized() {
                if let Err(error) = window.drag_resize_window(direction) {
                    tracing::warn!(%error, "window resize unavailable");
                }
            }
        });
    });
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
    let styled = std::cell::Cell::new(false);
    app.window().on_winit_window_event(move |window, _event| {
        if let Some(app) = weak.upgrade() {
            window.with_winit_window(|native| {
                app.set_window_maximized(native.is_maximized());
                if !styled.get() {
                    if let Ok(handle) = native.window_handle() {
                        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                            kova_platform_windows::window_theme::style_window(handle.hwnd.get());
                            styled.set(true);
                            // Winit's normal window icon sets ICON_SMALL only on
                            // Windows. Set the separate taskbar/Alt+Tab icon too.
                            match Icon::from_resource(1, None) {
                                Ok(icon) => native.set_taskbar_icon(Some(icon)),
                                Err(error) => tracing::warn!(%error, "load taskbar icon failed"),
                            }
                        }
                    }
                }
            });
        }
        EventResult::Propagate
    });
}

fn resize_direction(direction: i32) -> Option<ResizeDirection> {
    Some(match direction {
        0 => ResizeDirection::North,
        1 => ResizeDirection::NorthEast,
        2 => ResizeDirection::East,
        3 => ResizeDirection::SouthEast,
        4 => ResizeDirection::South,
        5 => ResizeDirection::SouthWest,
        6 => ResizeDirection::West,
        7 => ResizeDirection::NorthWest,
        _ => return None,
    })
}
