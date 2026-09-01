use kova_core::domain::Location;
use kova_core::error::{OperationError, Result};
use kova_platform_windows::path_resolver::{child_path, map_io_error};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Create a new folder under `parent` named `name`.
///
/// This is a real filesystem operation and must only run in a sandbox in tests.
pub async fn new_folder(parent: &Location, name: &str) -> Result<PathBuf> {
    let target = child_path(parent, name);
    fs::create_dir(&target)
        .await
        .map_err(|e| map_io_error(&target, e))?;
    Ok(target)
}

/// Rename an entry to `new_name` inside the same parent directory.
pub async fn rename(path: &Path, new_name: &str) -> Result<PathBuf> {
    let Some(parent) = path.parent() else {
        return Err(OperationError::invalid_path(
            path.to_string_lossy().into_owned(),
            None,
        ));
    };
    let new_path = parent.join(new_name);
    fs::rename(path, &new_path)
        .await
        .map_err(|e| map_io_error(path, e))?;
    Ok(new_path)
}

/// Open a file using the Windows default handler.
///
/// SAFETY: ShellExecuteExW is called with a null HWND and an operation string
/// whose backing wide characters stay alive for the duration of the call.
#[cfg(windows)]
pub fn open_with_default_handler(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
    };
    use windows::Win32::UI::Shell::{
        SEE_MASK_DEFAULT, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::core::PCWSTR;

    let operation: Vec<u16> = "open\0".encode_utf16().collect();
    let file_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: We initialize COM on this thread in apartment-threaded mode so
    // ShellExecuteExW can talk to shell extensions. The operation and file
    // wide strings are valid for the duration of the call.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_DEFAULT | SEE_MASK_INVOKEIDLIST,
            hwnd: HWND(std::ptr::null_mut()),
            lpVerb: PCWSTR(operation.as_ptr()),
            lpFile: PCWSTR(file_wide.as_ptr()),
            lpParameters: PCWSTR::null(),
            lpDirectory: PCWSTR::null(),
            nShow: 1, // SW_SHOWNORMAL
            ..Default::default()
        };

        let ok = ShellExecuteExW(&mut info);
        if ok.is_err() {
            return Err(OperationError::Shell(format!(
                "ShellExecuteExW failed for {} (hInstApp: {:?})",
                path.display(),
                info.hInstApp
            )));
        }
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn open_with_default_handler(_path: &Path) -> Result<()> {
    Err(OperationError::Unsupported {
        operation: "open_with_default_handler".into(),
        source: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::TestSandbox;
    use std::fs;

    #[tokio::test]
    async fn new_folder_and_rename_in_sandbox() {
        let root = std::env::temp_dir().join(format!("kova-ops-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sandbox = TestSandbox::new(root.clone());
        let test_dir = sandbox.create_unique_dir().unwrap();

        let parent = Location::new(test_dir.clone());
        let created = new_folder(&parent, "New Folder").await.unwrap();
        assert!(created.exists());

        let renamed = rename(&created, "Renamed Folder").await.unwrap();
        assert!(renamed.exists());
        assert!(!created.exists());

        fs::remove_dir_all(&root).ok();
    }
}
