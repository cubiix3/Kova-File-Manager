use crate::{AppState, MainWindow};
use slint::ComponentHandle;
use std::os::windows::process::CommandExt;

pub fn connect(app: &MainWindow) {
    let weak = app.as_weak();
    app.global::<AppState>()
        .on_request_default_manager(move |enable| {
            let Some(ui) = weak.upgrade() else { return };
            if ui.global::<AppState>().get_integration_busy() {
                return;
            }
            ui.global::<AppState>().set_integration_busy(true);
            let weak = ui.as_weak();
            let result = std::thread::Builder::new()
                .name("kova-associations".into())
                .spawn(move || {
                    let result = configure(enable);
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.global::<AppState>().set_integration_busy(false);
                        ui.global::<AppState>().set_status_text(
                            result
                                .unwrap_or_else(|e| format!("Folder integration: {e}"))
                                .into(),
                        );
                    });
                });
            if let Err(error) = result {
                ui.global::<AppState>().set_integration_busy(false);
                ui.global::<AppState>()
                    .set_status_text(format!("Folder integration: {error}").into());
            }
        });
}

fn configure(enable: bool) -> Result<String, String> {
    if enable {
        let install = std::env::var_os("LOCALAPPDATA").ok_or("Local app data is unavailable")?;
        let install = std::path::PathBuf::from(install).join("Kova");
        std::fs::create_dir_all(&install).map_err(|e| e.to_string())?;
        std::fs::write(
            install.join("FILES-ICONS-LICENSE.txt"),
            include_str!("../ui/third-party/FILES-ICONS-LICENSE.txt"),
        )
        .map_err(|e| e.to_string())?;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let script = include_str!("../../../scripts/default-file-manager.ps1");
    let mode = if enable { "Enable" } else { "Restore" };
    // PowerShell single-quoted literals escape apostrophes by doubling them.
    // Paths never become executable PowerShell expressions.
    let literal = exe.to_string_lossy().replace('\'', "''");
    let command = format!("& {{ {script} }} -Mode {mode} -Executable '{literal}'");
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW; work runs off the UI thread.
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}
