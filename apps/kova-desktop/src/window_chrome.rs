//! Custom caption controls backed by the existing winit window. Slint's
//! explicit edge/corner input surfaces start native resizing; no Win32 hooks.
use crate::{AppState, MainWindow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use slint::winit_030::winit::platform::windows::{IconExtWindows, WindowExtWindows};
use slint::winit_030::winit::window::{Icon, ResizeDirection};
use slint::winit_030::{EventResult, WinitWindowAccessor};

pub fn connect(
    app: &MainWindow,
    transfers: std::sync::Arc<kova_platform_windows::transfers::TransferQueue>,
) {
    let weak = app.as_weak();
    let close_queue = transfers.clone();
    app.window().on_close_requested(move || {
        if let Some(app) = weak.upgrade() {
            if defer_close(&app, &close_queue) {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
        }
        slint::CloseRequestResponse::HideWindow
    });
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
            if defer_close(&app, &transfers) {
                return;
            }
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
    let performance = std::env::var_os("KOVA_PERF").is_some();
    let input = std::rc::Rc::new(std::cell::Cell::new(None::<std::time::Instant>));
    tracing::info!(performance, "Window diagnostics");
    if performance {
        let pending = input.clone();
        if let Err(error) = app.window().set_rendering_notifier(move |state, _| {
            if matches!(state, slint::RenderingState::AfterRendering) {
                crate::diagnostics::rendered();
                if let Some(start) = pending.take() {
                    tracing::info!(
                        elapsed_ms = start.elapsed().as_secs_f64() * 1000.,
                        "input to rendered frame"
                    );
                }
            }
        }) {
            tracing::warn!(%error, "Frame timing unavailable");
        }
    }
    let mut shortcuts = crate::keyboard::Shortcuts::default();
    app.window().on_winit_window_event(move |window, event| {
        if let Some(app) = weak.upgrade() {
            if shortcuts.handle(&app, event) {
                return EventResult::PreventDefault;
            }
        }
        if performance
            && matches!(
                event,
                slint::winit_030::winit::event::WindowEvent::RedrawRequested
            )
        {
            if let Some(start) = input.take() {
                tracing::info!(
                    elapsed_ms = start.elapsed().as_secs_f64() * 1000.,
                    "input to redraw request"
                );
            }
        }
        if performance
            && matches!(
                event,
                slint::winit_030::winit::event::WindowEvent::KeyboardInput { .. }
                    | slint::winit_030::winit::event::WindowEvent::MouseInput { .. }
                    | slint::winit_030::winit::event::WindowEvent::MouseWheel { .. }
            )
            && input.get().is_none()
        {
            input.set(Some(std::time::Instant::now()));
        }
        if let Some(app) = weak.upgrade() {
            window.with_winit_window(|native| {
                app.set_window_maximized(native.is_maximized());
                if !styled.get() {
                    if app.get_restore_maximized() {
                        native.set_maximized(true);
                        app.set_restore_maximized(false);
                    }
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

fn defer_close(
    app: &MainWindow,
    transfers: &kova_platform_windows::transfers::TransferQueue,
) -> bool {
    if transfers
        .snapshots()
        .iter()
        .any(|transfer| !transfer.finished)
    {
        app.global::<AppState>().set_transfers_visible(true);
        app.global::<AppState>().set_status_text(
            "Transfers are running. Let them finish or cancel them before closing.".into(),
        );
        true
    } else {
        false
    }
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
