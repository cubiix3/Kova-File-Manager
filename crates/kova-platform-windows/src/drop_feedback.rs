//! Native feedback keeps painting inside OLE's nested drag loop, where Slint's
//! event loop may be suspended until DoDragDrop returns.
use windows::{
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
    core::{PCWSTR, w},
};
pub(crate) struct Feedback(HWND);
impl Feedback {
    pub fn new(owner: HWND) -> windows::core::Result<Self> {
        // SAFETY: built-in STATIC class; the popup is owned and destroyed on this STA.
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT,
                w!("STATIC"),
                w!(""),
                WS_POPUP | WS_BORDER | WINDOW_STYLE(0x201),
                0,
                0,
                320,
                32,
                Some(owner),
                None,
                None,
                None,
            )?;
            let font = GetStockObject(DEFAULT_GUI_FONT);
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
            Ok(Self(hwnd))
        }
    }
    pub fn show(&self, point: &POINTL, path: &std::path::Path, moving: bool) {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let name: String = name.chars().take(60).collect();
        let text: Vec<u16> = format!("{} to {name}", if moving { "Move" } else { "Copy" })
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: this live popup remains on its creating UI thread. No activation
        // or keyboard capture; only its caption and position change during dragging.
        unsafe {
            let _ = SetWindowTextW(self.0, PCWSTR(text.as_ptr()));
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let handle = MonitorFromPoint(
                POINT {
                    x: point.x,
                    y: point.y,
                },
                MONITOR_DEFAULTTONEAREST,
            );
            let (mut x, mut y) = (point.x + 16, point.y + 20);
            if GetMonitorInfoW(handle, &mut monitor).as_bool() {
                x = x.clamp(
                    monitor.rcWork.left,
                    (monitor.rcWork.right - 420).max(monitor.rcWork.left),
                );
                y = y.clamp(
                    monitor.rcWork.top,
                    (monitor.rcWork.bottom - 32).max(monitor.rcWork.top),
                );
            }
            let _ = SetWindowPos(
                self.0,
                Some(HWND_TOPMOST),
                x,
                y,
                420,
                32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = UpdateWindow(self.0);
        }
    }
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.0, SW_HIDE);
        }
    }
}
impl Drop for Feedback {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
