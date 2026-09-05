//! Best-effort native appearance. DWM corners are documented Windows 11 APIs.
//! Classic popup dark mode uses optional UXTheme exports (also used by Microsoft's
//! PowerToys/ZoomIt Utility.cpp). Ordinal 135 changed ABI before Windows 10 1903,
//! so older systems deliberately keep their system menu appearance.
use windows::Win32::Foundation::{FreeLibrary, HMODULE, HWND};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, w};

/// Owns the theme-library reference for the lifetime of the desktop event loop.
pub struct ThemeModule(HMODULE);
impl ThemeModule {
    fn load() -> Option<Self> {
        let version = windows_version::OsVersion::current();
        if version.major != 10 || version.build < 18362 {
            return None;
        }
        // SAFETY: a static DLL name is loaded exclusively from System32. The
        // owned module reference remains valid until all resolved calls finish.
        unsafe {
            LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                .ok()
                .map(Self)
        }
    }
}
impl Drop for ThemeModule {
    fn drop(&mut self) {
        // SAFETY: this reference was acquired by LoadLibraryExW, and no function
        // pointer escapes its owning ThemeModule's lifetime.
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

/// Call on the UI thread before creating windows. This affects only Kova's
/// process; it does not change Windows personalization or paint extension items.
pub fn initialize_dark_menus() -> Option<ThemeModule> {
    let module = ThemeModule::load()?;
    // SAFETY: these optional system exports have the stated ABIs on 1903+;
    // ordinals use MAKEINTRESOURCE semantics. The module is live for both calls.
    unsafe {
        if let Some(proc) = GetProcAddress(module.0, PCSTR(135usize as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(proc);
            set_mode(2); // PreferredAppMode::ForceDark, matching Kova's dark UI.
        }
        if let Some(proc) = GetProcAddress(module.0, PCSTR(136usize as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(proc);
            flush();
        }
    }
    Some(module)
}

/// Opt the existing native menu host into dark menus, preserving its wndproc and
/// all IContextMenu2/3 message forwarding.
pub fn allow_dark_menu_host(hwnd: HWND) {
    let Some(module) = ThemeModule::load() else {
        return;
    };
    // SAFETY: hwnd belongs to the UI thread; the optional export's HWND/bool ABI
    // is version-gated and the owning module remains loaded throughout the call.
    unsafe {
        if let Some(proc) = GetProcAddress(module.0, PCSTR(133usize as *const u8)) {
            let allow: unsafe extern "system" fn(HWND, bool) -> bool = std::mem::transmute(proc);
            allow(hwnd, true);
        }
    }
}

/// Apply compositor-owned rounding. Unsupported DWM attributes on Windows 10
/// fail harmlessly; maximized/snapped corner treatment remains the OS's choice.
pub fn style_window(handle: isize) {
    let hwnd = HWND(handle as *mut core::ffi::c_void);
    let dark = 1i32;
    // SAFETY: the caller obtains this live window handle from winit on the UI
    // thread. DWM reads stack values of the exact attribute size synchronously.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const i32).cast(),
            std::mem::size_of_val(&dark) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &DWMWCP_ROUND as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&DWMWCP_ROUND) as u32,
        );
    }
}
