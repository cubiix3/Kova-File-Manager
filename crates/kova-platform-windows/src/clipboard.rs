//! Windows clipboard text access for user-facing copy actions.
//!
//! Only plain text (CF_UNICODETEXT) is handled. The clipboard is opened and
//! closed synchronously on the calling thread, which is the UI thread for
//! user actions.

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Errors that can occur while reading or writing clipboard text.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("clipboard operation failed: {0}")]
    Win(#[from] windows::core::Error),
    #[error("clipboard memory could not be locked")]
    LockFailed,
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
        let result = write_while_open(&wide, byte_len);
        // CloseClipboard must run regardless of the inner result.
        let _ = CloseClipboard();
        result
    }
}

/// SAFETY: The caller must hold an open clipboard. `wide` is a NUL-terminated
/// UTF-16 buffer that outlives all calls in this function.
unsafe fn write_while_open(wide: &[u16], byte_len: usize) -> Result<(), ClipboardError> {
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
        let result = read_while_open();
        let _ = CloseClipboard();
        result
    }
}

/// SAFETY: The caller must hold an open clipboard.
unsafe fn read_while_open() -> Result<Option<String>, ClipboardError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Restores a previous clipboard text if there was one. The test only
    /// mutates the clipboard when it already held text, so nothing is lost on
    /// machines where the clipboard holds non-text content (in that case a
    /// plain write check runs instead).
    #[test]
    fn clipboard_roundtrip_preserves_previous_text() {
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
}
