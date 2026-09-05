//! Small view preferences, read before the event loop and saved after it exits.
use crate::{AppState, MainWindow, app_state::AppController};
use slint::ComponentHandle;
fn path() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Kova/view-options.txt"))
}
pub fn restore(app: &MainWindow, controller: &mut AppController) {
    let Some(path) = path() else { return };
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let state = app.global::<AppState>();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = match value {
            "true" => true,
            "false" => false,
            _ => continue,
        };
        match key {
            "hidden" => state.set_show_hidden(value),
            "system" => state.set_show_system(value),
            "extensions" => state.set_show_extensions(value),
            "preview" => state.set_preview_visible(value),
            "compact" => state.set_compact_rows(value),
            "alternating" => state.set_alternating_rows(value),
            "animations" => state.set_animations(value),
            "folder_sizes" => state.set_folder_sizes(value),
            _ => {}
        }
    }
    controller.set_visibility(state.get_show_hidden(), state.get_show_system());
    controller.show_extensions = state.get_show_extensions();
    controller.folder_sizes_enabled = state.get_folder_sizes();
}
pub fn save(app: &MainWindow) {
    let Some(path) = path() else { return };
    let state = app.global::<AppState>();
    let values = [
        ("hidden", state.get_show_hidden()),
        ("system", state.get_show_system()),
        ("extensions", state.get_show_extensions()),
        ("preview", state.get_preview_visible()),
        ("compact", state.get_compact_rows()),
        ("alternating", state.get_alternating_rows()),
        ("animations", state.get_animations()),
        ("folder_sizes", state.get_folder_sizes()),
    ];
    let text: String = values
        .into_iter()
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect();
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    })();
    if let Err(error) = result {
        tracing::warn!("Could not save view options: {error}");
    }
}
