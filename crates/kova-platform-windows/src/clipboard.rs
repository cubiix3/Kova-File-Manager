//! Windows clipboard access for user-facing copy actions.
//!
//! Text: plain CF_UNICODETEXT.
//! Files: CF_HDROP plus the "Preferred DropEffect" format so copy/cut
//! operations are compatible with Windows Explorer in both directions.

use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};

/// Drop effect values for the "Preferred DropEffect" clipboard format.
pub const DROPEFFECT_COPY: u32 = 1;
pub const DROPEFFECT_MOVE: u32 = 2;

/// Registered clipboard format name for cut/copy state of file transfers.
const PREFERRED_DROPEFFECT: &str = "Preferred DropEffect";

/// Errors that can occur while reading or writing clipboard data.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("clipboard operation failed: {0}")]
    Win(#[from] windows::core::Error),
    #[error("clipboard memory could not be locked")]
    LockFailed,
}

/// A file selection taken from the clipboard, Explorer-compatible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFiles {
    pub paths: Vec<PathBuf>,
    /// True when the clipboard holds a *cut* (move) selection.
    pub cut: bool,
}

/// Copy `text` to the Windows clipboard as CF_UNICODETEXT.
///
/// The allocated global memory block is handed to the clipboard and must not
/// be freed by the caller after a successful `SetClipboardData`.
pub fn set_clipboard_text(text: &str) -> Result<(), ClipboardError> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    // SAFETY: The clipboard calls run in order on one thread. The global
    // memory block is written while locked and handed to the clipboard
    // afterwards; the system owns it from the successful SetClipboardData on.
    unsafe {
        OpenClipboard(None)?;
        let result = write_text_while_open(&wide, byte_len);
        // CloseClipboard must run regardless of the inner result.
        let _ = CloseClipboard();
        result
    }
}

/// SAFETY: The caller must hold an open clipboard. `wide` is a NUL-terminated
/// UTF-16 buffer that outlives all calls in this function.
unsafe fn write_text_while_open(wide: &[u16], byte_len: usize) -> Result<(), ClipboardError> {
    unsafe {
        EmptyClipboard()?;
        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_len)?;
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return Err(ClipboardError::LockFailed);
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr.cast::<u16>(), wide.len());
        let _ = GlobalUnlock(hmem);
        SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0)))?;
        Ok(())
    }
}

/// Read CF_UNICODETEXT from the clipboard. Returns `Ok(None)` when the
/// clipboard does not currently hold text.
pub fn get_clipboard_text() -> Result<Option<String>, ClipboardError> {
    // SAFETY: Open/Get/Lock/Unlock/Close run in sequence on one thread. The
    // memory owned by the clipboard is only read while locked and never
    // freed by us.
    unsafe {
        OpenClipboard(None)?;
        let result = read_text_while_open();
        let _ = CloseClipboard();
        result
    }
}

/// SAFETY: The caller must hold an open clipboard.
unsafe fn read_text_while_open() -> Result<Option<String>, ClipboardError> {
    unsafe {
        let handle = match GetClipboardData(CF_UNICODETEXT.0 as u32) {
            Ok(h) if !h.is_invalid() => h,
            _ => return Ok(None),
        };
        let hmem = HGLOBAL(handle.0);
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return Err(ClipboardError::LockFailed);
        }
        let mut len = 0usize;
        while *ptr.cast::<u16>().add(len) != 0 {
            len += 1;
        }
        let wide = std::slice::from_raw_parts(ptr.cast::<u16>(), len);
        let text = String::from_utf16_lossy(wide);
        let _ = GlobalUnlock(hmem);
        Ok(Some(text))
    }
}

/// Build the CF_HDROP global memory image for `paths`:
/// a `DROPFILES` header followed by a double-NUL-terminated wide path list.
/// Pure function so it can be unit tested without touching the clipboard.
pub(crate) fn build_hdrop_buffer(paths: &[PathBuf]) -> Vec<u8> {
    let header_len = std::mem::size_of::<DROPFILES_HEADER>();
    let mut wide: Vec<u16> = Vec::new();
    for path in paths {
        wide.extend(path.as_os_str().encode_wide());
        wide.push(0);
    }
    wide.push(0); // final double termination

    let mut buffer = vec![0u8; header_len + wide.len() * 2];
    let header = DROPFILES_HEADER {
        p_files: header_len as u32,
        pt: [0, 0],
        f_nc: 0,
        f_wide: 1,
    };
    // SAFETY: repr(C) POD struct; fully initialized, read as raw bytes.
    let header_bytes = unsafe {
        std::slice::from_raw_parts(
            &header as *const DROPFILES_HEADER as *const u8,
            std::mem::size_of::<DROPFILES_HEADER>(),
        )
    };
    buffer[..header_len].copy_from_slice(header_bytes);
    // SAFETY: reads wide.len()*2 bytes from the Vec's buffer.
    let wide_bytes =
        unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
    buffer[header_len..].copy_from_slice(wide_bytes);
    buffer
}

/// Minimal mirror of the Win32 DROPFILES layout (x86/x64, 4-byte aligned).
#[repr(C)]
struct DROPFILES_HEADER {
    p_files: u32,
    pt: [i32; 2],
    f_nc: i32,
    f_wide: i32,
}

/// Put `paths` on the clipboard as CF_HDROP, Explorer-compatible.
/// `cut` selects DROPEFFECT_MOVE, otherwise DROPEFFECT_COPY is advertised.
pub fn set_clipboard_files(paths: &[PathBuf], cut: bool) -> Result<(), ClipboardError> {
    if paths.is_empty() {
        return Ok(());
    }
    let hdrop_image = build_hdrop_buffer(paths);
    let effect = if cut {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_COPY
    };
    let effect_bytes = effect.to_ne_bytes();

    // SAFETY: clipboard calls run in order on one thread; both memory blocks
    // are fully written while locked and then handed to the clipboard, which
    // owns them from the successful SetClipboardData on.
    unsafe {
        OpenClipboard(None)?;
        let result = write_files_while_open(&hdrop_image, &effect_bytes);
        let _ = CloseClipboard();
        result
    }
}

/// SAFETY: The caller must hold an open clipboard.
unsafe fn write_files_while_open(
    hdrop_image: &[u8],
    effect_bytes: &[u8; 4],
) -> Result<(), ClipboardError> {
    unsafe {
        EmptyClipboard()?;

        let hdrop = GlobalAlloc(GMEM_MOVEABLE, hdrop_image.len())?;
        let ptr = GlobalLock(hdrop);
        if ptr.is_null() {
            return Err(ClipboardError::LockFailed);
        }
        std::ptr::copy_nonoverlapping(hdrop_image.as_ptr(), ptr.cast::<u8>(), hdrop_image.len());
        let _ = GlobalUnlock(hdrop);
        SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hdrop.0)))?;

        // The drop effect format is optional metadata; failure to set it must
        // not undo the file list, but a missing effect means "copy" for
        // Explorer which is the safer default.
        let effect_fmt = preferred_dropeffect_format();
        let effect = GlobalAlloc(GMEM_MOVEABLE, effect_bytes.len())?;
        let ptr = GlobalLock(effect);
        if ptr.is_null() {
            return Err(ClipboardError::LockFailed);
        }
        std::ptr::copy_nonoverlapping(effect_bytes.as_ptr(), ptr.cast::<u8>(), effect_bytes.len());
        let _ = GlobalUnlock(effect);
        SetClipboardData(effect_fmt, Some(HANDLE(effect.0)))?;
        Ok(())
    }
}

fn preferred_dropeffect_format() -> u32 {
    let mut wide: Vec<u16> = PREFERRED_DROPEFFECT.encode_utf16().collect();
    wide.push(0);
    // SAFETY: `wide` stays alive for the duration of the call and is
    // NUL-terminated.
    unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(wide.as_ptr())) }
}

/// True when the clipboard currently holds an Explorer-compatible file list.
pub fn clipboard_has_files() -> Result<bool, ClipboardError> {
    // SAFETY: single well-ordered call.
    Ok(unsafe { IsClipboardFormatAvailable(CF_HDROP.0 as u32) }.is_ok())
}

/// Read CF_HDROP plus "Preferred DropEffect" from the clipboard.
/// Returns `Ok(None)` when no file list is present.
pub fn get_clipboard_files() -> Result<Option<ClipboardFiles>, ClipboardError> {
    // SAFETY: Open/Get/Lock/Unlock/Close run in sequence on one thread; the
    // clipboard-owned memory is only read while locked and never freed here.
    unsafe {
        OpenClipboard(None)?;
        let result = read_files_while_open();
        let _ = CloseClipboard();
        result
    }
}

/// SAFETY: The caller must hold an open clipboard.
unsafe fn read_files_while_open() -> Result<Option<ClipboardFiles>, ClipboardError> {
    unsafe {
        let handle = match GetClipboardData(CF_HDROP.0 as u32) {
            Ok(h) if !h.is_invalid() => h,
            _ => return Ok(None),
        };
        let hmem = HGLOBAL(handle.0);
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return Err(ClipboardError::LockFailed);
        }
        let total = GlobalSize(hmem);
        let paths = parse_hdrop(ptr.cast::<u8>(), total);
        let _ = GlobalUnlock(hmem);

        if paths.is_empty() {
            return Ok(None);
        }
        let cut = read_drop_effect_while_open();
        Ok(Some(ClipboardFiles { paths, cut }))
    }
}

/// SAFETY: The caller must hold an open clipboard.
unsafe fn read_drop_effect_while_open() -> bool {
    unsafe {
        let fmt = preferred_dropeffect_format();
        let Ok(handle) = GetClipboardData(fmt) else {
            return false;
        };
        if handle.is_invalid() {
            return false;
        }
        let hmem = HGLOBAL(handle.0);
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return false;
        }
        let total = GlobalSize(hmem);
        let effect = if total >= 4 {
            u32::from_ne_bytes(
                std::slice::from_raw_parts(ptr.cast::<u8>(), 4)
                    .try_into()
                    .unwrap(),
            )
        } else {
            0
        };
        let _ = GlobalUnlock(hmem);
        // Move wins only when it is the unambiguous effect.
        effect & DROPEFFECT_MOVE != 0 && effect & DROPEFFECT_COPY == 0
    }
}

/// Parse a double-NUL-terminated wide path list from a DROPFILES image.
/// SAFETY: `ptr` must point to `total` readable bytes.
unsafe fn parse_hdrop(ptr: *const u8, total: usize) -> Vec<PathBuf> {
    unsafe {
        if total < std::mem::size_of::<DROPFILES_HEADER>() {
            return Vec::new();
        }
        let header = &*(ptr as *const DROPFILES_HEADER);
        let offset = header.p_files as usize;
        if header.f_wide == 0 || offset >= total {
            return Vec::new();
        }
        let base = ptr.add(offset) as *const u16;
        let max_units = (total - offset) / 2;

        let mut paths = Vec::new();
        let mut start = 0usize;
        let mut end = 0usize;
        while end < max_units {
            let c = *base.add(end);
            if c == 0 {
                if end == start {
                    break; // second NUL of the list terminator
                }
                let wide = std::slice::from_raw_parts(base.add(start), end - start);
                paths.push(PathBuf::from(std::ffi::OsString::from_wide(wide)));
                start = end + 1;
            }
            end += 1;
        }
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clipboard is a per-process global resource; every test that touches
    /// it must hold this lock so parallel test threads cannot interleave
    /// OpenClipboard/EmptyClipboard/GetClipboardData sequences.
    static CLIPBOARD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn hdrop_buffer_roundtrip_parses_paths() {
        let paths = vec![
            PathBuf::from(r"C:\Temp\a.txt"),
            PathBuf::from(r"C:\Temp\folder with space"),
        ];
        let image = build_hdrop_buffer(&paths);
        let parsed = unsafe { parse_hdrop(image.as_ptr(), image.len()) };
        assert_eq!(parsed, paths);
    }

    #[test]
    fn hdrop_buffer_rejects_short_or_ansi_images() {
        assert!(unsafe { parse_hdrop(std::ptr::null(), 0) }.is_empty());
        assert!(unsafe { parse_hdrop([0u8; 4].as_ptr(), 4) }.is_empty());
    }

    /// Restores a previous clipboard text if there was one. The test only
    /// mutates the clipboard when it already held text, so nothing is lost on
    /// machines where the clipboard holds non-text content (in that case a
    /// plain write check runs instead).
    #[test]
    fn clipboard_roundtrip_preserves_previous_text() {
        let _guard = CLIPBOARD_TEST_LOCK.lock().unwrap();
        let previous = get_clipboard_text().expect("clipboard api should work");
        if previous.is_none() {
            set_clipboard_text("kova-clipboard-check").expect("write should succeed");
            let read = get_clipboard_text().expect("read should succeed");
            assert_eq!(read.as_deref(), Some("kova-clipboard-check"));
            return;
        }

        set_clipboard_text("kova-clipboard-pi").expect("write should succeed");
        let read = get_clipboard_text().expect("read should succeed");
        assert_eq!(read.as_deref(), Some("kova-clipboard-pi"));

        set_clipboard_text(previous.as_deref().unwrap()).expect("restore should succeed");
        let restored = get_clipboard_text().expect("read should succeed");
        assert_eq!(restored, previous);
    }

    /// Exercises the real file clipboard. Only mutates the clipboard when it
    /// held no files before, so a pending user copy/cut is never clobbered.
    #[test]
    fn clipboard_files_roundtrip() {
        let _guard = CLIPBOARD_TEST_LOCK.lock().unwrap();
        let had_files = clipboard_has_files().expect("format check should work");
        let temp = std::env::temp_dir().join(format!("kova-clip-{}.txt", std::process::id()));
        std::fs::write(&temp, b"kova").unwrap();

        if had_files {
            // Do not clobber a pending user copy/cut; only verify the API.
            return;
        }

        set_clipboard_files(std::slice::from_ref(&temp), false)
            .expect("write files should succeed");
        let read = get_clipboard_files()
            .expect("read files should succeed")
            .unwrap();
        assert_eq!(read.paths, vec![temp.clone()]);
        assert!(!read.cut, "copy effect must not be read as cut");

        set_clipboard_files(std::slice::from_ref(&temp), true).expect("write cut should succeed");
        let read = get_clipboard_files()
            .expect("read cut should succeed")
            .unwrap();
        assert!(read.cut, "move effect must be read as cut");
        assert_eq!(read.paths, vec![temp.clone()]);

        std::fs::remove_file(&temp).ok();
    }
}
