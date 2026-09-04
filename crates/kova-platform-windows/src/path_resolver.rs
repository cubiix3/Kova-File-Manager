use kova_core::domain::{Location, LocationInput};
use kova_core::error::OperationError;
use std::path::{Path, PathBuf};

/// Resolve user input into a usable `Location`. Accepts absolute and relative
/// paths. Relative paths are resolved against `base`.
pub fn resolve_input(input: &LocationInput, base: &Location) -> Result<Location, OperationError> {
    let path = PathBuf::from(input.raw.trim().trim_matches('"'));
    let resolved = if path.is_absolute() {
        path
    } else {
        base.path.join(path)
    };

    canonicalize_location(&resolved)
}

/// Convert a path to a `Location`, failing with a categorized error if it is
/// not valid.
pub fn canonicalize_location(path: &Path) -> Result<Location, OperationError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(e) => return Err(OperationError::io("get current directory", e)),
        }
    };

    // We do NOT call fs::canonicalize here because that resolves symlinks and
    // may fail for paths with long-path prefixes or certain network roots. The
    // platform enumeration layer validates existence separately.
    Ok(Location::new(normalize_separators(abs)))
}

fn normalize_separators(path: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::Component;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|c| if c == b'/' as u16 { b'\\' as u16 } else { c })
        .collect();
    let path = PathBuf::from(std::ffi::OsString::from_wide(&wide));
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Validate that `target` is a directory and exists.
pub fn require_directory(path: &Path) -> Result<(), OperationError> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(OperationError::invalid_path(
            path.to_string_lossy().into_owned(),
            None,
        )),
        Err(e) => Err(map_io_error(path, e)),
    }
}

/// Categorize an `std::io::Error` into a Kova `OperationError` while
/// preserving the source.
pub fn map_io_error(path: &Path, err: std::io::Error) -> OperationError {
    use kova_core::error::IoResultKind;
    use std::io::ErrorKind;

    let path_str = path.to_string_lossy().into_owned();

    // Try the OS error code first for precise Windows classification.
    if let Some(raw) = err.raw_os_error() {
        match IoResultKind::classify_windows_error(raw) {
            IoResultKind::NotFound => return OperationError::not_found(path_str, Some(err)),
            IoResultKind::PermissionDenied => {
                return OperationError::permission_denied(path_str, Some(err));
            }
            IoResultKind::AlreadyExists => {
                return OperationError::already_exists(path_str, Some(err));
            }
            IoResultKind::DeviceNotReady => {
                return OperationError::device_unavailable(path_str, Some(err));
            }
            IoResultKind::NetworkPathNotFound => {
                return OperationError::network_unavailable(path_str, Some(err));
            }
            _ => {}
        }
    }

    match err.kind() {
        ErrorKind::NotFound => OperationError::not_found(path_str, Some(err)),
        ErrorKind::PermissionDenied => OperationError::permission_denied(path_str, Some(err)),
        ErrorKind::AlreadyExists => OperationError::already_exists(path_str, Some(err)),
        ErrorKind::InvalidInput => OperationError::invalid_path(path_str, Some(err)),
        _ => OperationError::io(path_str, err),
    }
}

/// Create a child path under `parent` for a new item named `name`.
pub fn child_path(parent: &Location, name: &str) -> PathBuf {
    parent.path.join(name)
}

/// Rename one entry without replacing an existing destination. Unlike
/// std::fs::rename on Windows this cannot silently overwrite another file.
pub fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), OperationError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::MoveFileW;
    use windows::core::PCWSTR;
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both paths are NUL-terminated and their backing buffers live
    // throughout the call. MoveFileW fails when the destination exists.
    unsafe {
        MoveFileW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
        )
    }
    .map_err(|e| OperationError::Shell(format!("Rename failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_quoted_paths_are_normalized_without_io() {
        let base = Location::new(PathBuf::from(r"G:\audit\folder"));
        assert_eq!(
            resolve_input(&LocationInput::new("../other"), &base)
                .unwrap()
                .path,
            PathBuf::from(r"G:\audit\other")
        );
        assert_eq!(
            resolve_input(&LocationInput::new(r#""G:\audit\other""#), &base)
                .unwrap()
                .path,
            PathBuf::from(r"G:\audit\other")
        );
        for path in [r"\\server\share\folder", r"\\?\G:\long\folder"] {
            assert_eq!(
                canonicalize_location(Path::new(path)).unwrap().path,
                PathBuf::from(path)
            );
        }
    }
}
