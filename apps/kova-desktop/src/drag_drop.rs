use crate::{AppState, MainWindow, bridges::CommandDispatcher};
use kova_platform_windows::{
    drag_drop::{self, Event, Registration},
    shell_ops::ShellOpCommand,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{ComponentHandle, winit_030::WinitWindowAccessor};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

#[derive(Clone)]
struct Zone {
    target: PathBuf,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    priority: i32,
}
#[derive(Default)]
struct Zones {
    epoch: i32,
    items: Vec<Zone>,
}
pub struct Runtime {
    _timer: slint::Timer,
    _registration: Rc<RefCell<Option<Registration>>>,
}
pub fn connect(app: &MainWindow, dispatcher: CommandDispatcher) -> Runtime {
    let zones = Rc::new(RefCell::new(Zones::default()));
    let collected = zones.clone();
    app.global::<AppState>().on_register_drop_zone(
        move |epoch, target, x, y, width, height, priority| {
            let path = PathBuf::from(target.as_str());
            if !path.is_absolute() || width <= 0. || height <= 0. {
                return;
            }
            let mut zones = collected.borrow_mut();
            if zones.epoch != epoch {
                zones.epoch = epoch;
                zones.items.clear();
            }
            zones.items.push(Zone {
                target: path,
                x,
                y,
                width,
                height,
                priority,
            });
        },
    );
    let weak = app.as_weak();
    let actions = dispatcher.clone();
    app.global::<AppState>().on_start_drag(move || {
        let paths = {
            let controller = actions.controller();
            let ctrl = controller.lock().unwrap();
            if ctrl.is_loading() {
                return;
            }
            ctrl.selected_paths()
        };
        if let Err(error) = drag_drop::start(&paths) {
            if let Some(ui) = weak.upgrade() {
                crate::show_error_dialog(&ui, &format!("Drag failed: {error}"));
            }
        }
        actions.refresh_tabs();
    });
    let registered = Rc::new(RefCell::new(None));
    let registration = registered.clone();
    let timer = slint::Timer::default();
    let weak = app.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AppState>();
            state.set_drop_epoch(state.get_drop_epoch().wrapping_add(1));
            if registration.borrow().is_some() {
                return;
            }
            let weak = app.as_weak();
            let actions = dispatcher.clone();
            let geometry = zones.clone();
            let handler: drag_drop::Handler = Rc::new(move |event| {
                let ui = weak.upgrade()?;
                let state = ui.global::<AppState>();
                match event {
                    Event::Leave => {
                        state.set_drop_feedback("".into());
                        state.set_drop_path("".into());
                        None
                    }
                    Event::Hover {
                        x,
                        y,
                        paths: _,
                        moving,
                    } => {
                        state.set_drop_feedback("".into());
                        state.set_drop_path("".into());
                        if state.get_undo_visible()
                            || state.get_dialog_visible()
                            || state.get_conflict_visible()
                            || state.get_library_visible()
                            || state.get_storage_visible()
                            || state.get_transfers_visible()
                        {
                            return None;
                        }
                        let scale = ui.window().scale_factor();
                        let x = x as f32 / scale;
                        let y = y as f32 / scale;
                        let zones = geometry.borrow();
                        if zones.epoch != state.get_drop_epoch() {
                            return None;
                        }
                        let zone = zones
                            .items
                            .iter()
                            .filter(|z| {
                                x >= z.x && x < z.x + z.width && y >= z.y && y < z.y + z.height
                            })
                            .max_by_key(|z| z.priority)?;
                        state.set_drop_path(zone.target.to_string_lossy().as_ref().into());
                        state.set_drop_feedback(
                            format!(
                                "{} to {}",
                                if moving { "Move" } else { "Copy" },
                                zone.target.display()
                            )
                            .into(),
                        );
                        Some(zone.target.clone())
                    }
                    Event::Drop {
                        target,
                        paths,
                        moving,
                    } => {
                        let command = if moving {
                            ShellOpCommand::Move {
                                sources: paths,
                                dest: target.clone(),
                            }
                        } else {
                            ShellOpCommand::Copy {
                                sources: paths,
                                dest: target.clone(),
                            }
                        };
                        match actions.send_ops(command) {
                            Ok(()) => Some(target),
                            Err(error) => {
                                crate::show_error_dialog(&ui, &error);
                                None
                            }
                        }
                    }
                }
            });
            app.window().with_winit_window(|native| {
                if let Ok(handle) = native.window_handle() {
                    if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                        match drag_drop::register(handle.hwnd.get(), handler) {
                            Ok(target) => *registration.borrow_mut() = Some(target),
                            Err(error) => {
                                tracing::warn!(%error,"Native drag/drop registration failed")
                            }
                        }
                    }
                }
            });
        },
    );
    Runtime {
        _timer: timer,
        _registration: registered,
    }
}
