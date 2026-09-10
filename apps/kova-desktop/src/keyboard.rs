//! Window-level navigation shortcuts also work while a search field has focus.
use crate::{AppState, MainWindow};
use slint::winit_030::winit::{
    event::{ElementState, WindowEvent},
    keyboard::{Key, ModifiersState, NamedKey},
};
use slint::{ComponentHandle, Model};
#[derive(Default)]
pub struct Shortcuts {
    modifiers: ModifiersState,
}
impl Shortcuts {
    pub fn handle(&mut self, app: &MainWindow, event: &WindowEvent) -> bool {
        if matches!(event, WindowEvent::Focused(false)) {
            self.modifiers = ModifiersState::default();
            app.invoke_dismiss_file_menu();
        }
        if let WindowEvent::ModifiersChanged(modifiers) = event {
            self.modifiers = modifiers.state();
        }
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return false;
        };
        if event.state != ElementState::Pressed {
            return false;
        }
        let state = app.global::<AppState>();
        if state.get_file_menu_visible()
            || state.get_inline_visible()
            || state.get_dialog_visible()
            || state.get_conflict_visible()
            || state.get_undo_visible()
            || state.get_library_visible()
            || state.get_storage_visible()
            || state.get_transfers_visible()
        {
            return false;
        }
        if self.modifiers.control_key() && !self.modifiers.alt_key() {
            match &event.logical_key {
                Key::Character(key) if key == "1" => state.set_gallery(false),
                Key::Character(key) if key == "2" => state.set_gallery(true),
                Key::Character(key) if key.eq_ignore_ascii_case("l") => app.invoke_focus_command(0),
                Key::Character(key) if key.eq_ignore_ascii_case("f") => app.invoke_focus_command(1),
                Key::Character(key) if key.eq_ignore_ascii_case("t") => {
                    state.invoke_request_new_tab()
                }
                Key::Character(key) if key.eq_ignore_ascii_case("w") => {
                    state.invoke_request_close_tab(state.get_active_tab())
                }
                Key::Named(NamedKey::Tab) => {
                    let count = state.get_tabs().row_count() as i32;
                    if count > 0 {
                        state.invoke_request_switch_tab(
                            (state.get_active_tab()
                                + if self.modifiers.shift_key() {
                                    count - 1
                                } else {
                                    1
                                })
                                % count,
                        );
                    }
                }
                _ => return false,
            }
            return true;
        }
        if event.logical_key == Key::Named(NamedKey::F5) {
            state.invoke_request_refresh();
            return true;
        }
        false
    }
}
