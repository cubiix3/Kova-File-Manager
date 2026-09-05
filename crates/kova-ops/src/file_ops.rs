use kova_core::domain::Location;
use kova_core::error::{OperationError, Result};
use kova_platform_windows::path_resolver::{child_path, map_io_error};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Create a new folder under `parent` named `name`.
///
/// This is a real filesystem operation and must only run in a sandbox in tests.
pub async fn new_folder(parent: &Location, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    let target = child_path(parent, name);
    fs::create_dir(&target)
        .await
        .map_err(|e| map_io_error(&target, e))?;
    Ok(target)
}

/// Allocate a default name atomically, without an exists/create race.
pub async fn new_folder_unique(parent: &Location, base: &str) -> Result<PathBuf> {
    for number in 1..=10_000 {
        let name = if number == 1 {
            base.to_owned()
        } else {
            format!("{base} ({number})")
        };
        match new_folder(parent, &name).await {
            Err(OperationError::AlreadyExists { .. }) => continue,
            result => return result,
        }
    }
    Err(OperationError::Shell(
        "No available default folder name".into(),
    ))
}

/// Rename an entry to `new_name` inside the same parent directory.
pub async fn rename(path: &Path, new_name: &str) -> Result<PathBuf> {
    validate_name(new_name)?;
    let Some(parent) = path.parent() else {
        return Err(OperationError::invalid_path(
            path.to_string_lossy().into_owned(),
            None,
        ));
    };
    let new_path = parent.join(new_name);
    let source = path.to_path_buf();
    let target = new_path.clone();
    tokio::task::spawn_blocking(move || {
        kova_platform_windows::path_resolver::rename_no_replace(&source, &target)
    })
    .await
    .map_err(|e| OperationError::Shell(e.to_string()))??;
    Ok(new_path)
}

/// A dialog accepts one Windows filename, never a path or alternate stream.
pub fn validate_name(name: &str) -> Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || reserved
        || name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
    {
        return Err(OperationError::invalid_path(name, None));
    }
    Ok(())
}

/// Open a file using the Windows default handler.
///
/// SAFETY: ShellExecuteExW is called with a null HWND and an operation string
/// whose backing wide characters stay alive for the duration of the call.
#[cfg(windows)]
pub fn open_with_default_handler(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::HWND;

    use windows::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::core::PCWSTR;

    let operation: Vec<u16> = "open\0".encode_utf16().collect();
    let file_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // Portable executables commonly resolve their assets relative to the working
    // directory. Shortcuts retain their own configured working directory.
    let directory: Option<Vec<u16>> = path
        .extension()
        .filter(|ext| ext.eq_ignore_ascii_case("exe"))
        .and_then(|_| path.parent())
        .map(|parent| {
            parent
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        });

    // SAFETY: We initialize COM on this thread in apartment-threaded mode so
    // ShellExecuteExW can talk to shell extensions. The operation, file and
    // optional directory wide strings are valid for the duration of the call.
    unsafe {
        kova_platform_windows::shell_menu::ensure_com_sta();

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            // This worker has no message loop: finish Shell launch dispatch
            // before returning, without waiting for the launched app to exit.
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            hwnd: HWND(std::ptr::null_mut()),
            lpVerb: PCWSTR(operation.as_ptr()),
            lpFile: PCWSTR(file_wide.as_ptr()),
            lpParameters: PCWSTR::null(),
            lpDirectory: directory
                .as_ref()
                .map_or(PCWSTR::null(), |wide| PCWSTR(wide.as_ptr())),
            nShow: 1, // SW_SHOWNORMAL
            ..Default::default()
        };

        let ok = ShellExecuteExW(&mut info);
        if let Err(error) = ok {
            return Err(OperationError::Shell(format!(
                "Could not open {}: {error}",
                path.display(),
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

    #[test]
    fn dialog_names_cannot_escape_parent_or_create_streams() {
        for name in [
            "",
            ".",
            "..",
            "../outside",
            "C:\\outside",
            "a/b",
            "a:b",
            "CON.txt",
            "LPT1",
            "bad.",
            "bad ",
            "a\0b",
        ] {
            assert!(validate_name(name).is_err(), "accepted {name:?}");
        }
        for name in ["Report.txt", "New Folder", "résumé.md", ".gitignore"] {
            assert!(validate_name(name).is_ok());
        }
    }

    #[tokio::test]
    async fn concurrent_default_folders_preserve_existing_items() {
        let root =
            std::env::temp_dir().join(format!("kova-default-folder-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("New folder"), b"existing file").unwrap();
        let parent = Location::new(root.clone());
        let (first, second) = tokio::join!(
            new_folder_unique(&parent, "New folder"),
            new_folder_unique(&parent, "New folder")
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_ne!(first, second);
        assert!(first.is_dir() && second.is_dir());
        assert_eq!(fs::read(root.join("New folder")).unwrap(), b"existing file");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rename_conflict_preserves_both_file_contents() {
        let root = std::env::temp_dir().join(format!("kova-conflict-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("source.txt"), b"source").unwrap();
        fs::write(root.join("existing.txt"), b"existing").unwrap();
        assert!(
            rename(&root.join("source.txt"), "existing.txt")
                .await
                .is_err()
        );
        assert_eq!(fs::read(root.join("source.txt")).unwrap(), b"source");
        assert_eq!(fs::read(root.join("existing.txt")).unwrap(), b"existing");
        fs::remove_dir_all(root).unwrap();
    }

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
