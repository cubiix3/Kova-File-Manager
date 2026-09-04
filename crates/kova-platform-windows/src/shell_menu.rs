//! Native Windows Explorer shell context menu for files and folders.
//!
//! Hosts the real `IContextMenu` of the shell namespace for one or more
//! paths, including all installed shell extensions (7-Zip, Git, "Open with",
//! Properties, ...). Owner-drawn menu items from extensions require message
//! forwarding (`IContextMenu2`/`IContextMenu3`); a dedicated hidden host
//! window with our own window procedure receives the popup menu messages, so
//! no subclassing of the Slint window and no hooks are needed.
//!
//! All functions here must be called from the UI thread.

use std::cell::RefCell;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, IContextMenu,
    IContextMenu2, IContextMenu3, ILFree, IShellFolder, SHBindToParent, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    GetForegroundWindow, HMENU, IsWindow, PostMessageW, RegisterClassW, SW_SHOWNORMAL,
    SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_DRAWITEM,
    WM_INITMENUPOPUP, WM_MEASUREITEM, WM_MENUCHAR, WM_NULL, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{Interface, PCSTR, PCWSTR};

/// First command id handed to the shell for the popup menu. The id range is
/// generous so all extension commands land inside it; the invoked verb is
/// encoded as `menu_id - CMD_FIRST` via MAKEINTRESOURCE semantics.
const CMD_FIRST: u32 = 1;
const CMD_LAST: u32 = 0x7FFF;

thread_local! {
    // Forwarding slots for the menu message pump. Only ever touched on the
    // UI thread while a menu is open.
    static MENU_FORWARD: RefCell<MenuForward> = RefCell::new(MenuForward::default());
    // Cached hidden host window for popup menus.
    static MENU_HOST: RefCell<Option<HWND>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct MenuForward {
    cm2: Option<IContextMenu2>,
    cm3: Option<IContextMenu3>,
}

/// Ensure COM is initialized on this thread in apartment-threaded mode.
///
/// winit calls OleInitialize on the event loop thread, so this usually
/// returns S_FALSE; RPC_E_CHANGED_MODE is also tolerated because in that case
/// COM is already usable for shell objects.
pub fn ensure_com_sta() {
    crate::com::ensure_sta();
}

/// Window procedure of the hidden menu host. It forwards the owner-draw
/// related popup messages to the active `IContextMenu2`/`3` and otherwise
/// behaves like the default window procedure.
unsafe extern "system" fn menu_host_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_DRAWITEM | WM_MEASUREITEM | WM_INITMENUPOPUP => {
                let forwarded = MENU_FORWARD.with(|f| {
                    let forward = f.borrow();
                    if let Some(cm) = &forward.cm3 {
                        let mut result = LRESULT(0);
                        if cm
                            .HandleMenuMsg2(msg, wparam, lparam, Some(&mut result))
                            .is_ok()
                        {
                            return true;
                        }
                    }
                    forward
                        .cm2
                        .as_ref()
                        .map(|cm| cm.HandleMenuMsg(msg, wparam, lparam).is_ok())
                        .unwrap_or(false)
                });
                if forwarded {
                    return LRESULT(0);
                }
            }
            WM_MENUCHAR => {
                let result = MENU_FORWARD.with(|f| -> Option<LRESULT> {
                    let forward = f.borrow();
                    let cm3 = forward.cm3.as_ref()?;
                    let mut out = LRESULT(0);
                    cm3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut out))
                        .ok()?;
                    Some(out)
                });
                if let Some(result) = result {
                    return result;
                }
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// Return the cached hidden host window, creating it on first use.
/// SAFETY: runs on the UI thread.
fn ensure_host() -> Option<HWND> {
    if let Some(hwnd) = MENU_HOST.with(|h| *h.borrow()) {
        // SAFETY: single well-ordered call.
        unsafe {
            if IsWindow(Some(hwnd)).as_bool() {
                return Some(hwnd);
            }
        }
    }

    unsafe {
        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("shell menu: GetModuleHandleW failed: {e}");
                return None;
            }
        };
        let class_name = PCWSTR::from_raw(windows::core::w!("KovaShellMenuHost").as_ptr());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(menu_host_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let atom = RegisterClassW(&wc);
        if atom == 0 {
            tracing::warn!("shell menu: RegisterClassW failed");
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name,
            PCWSTR::null(),
            WS_POPUP,
            -32000,
            -32000,
            1,
            1,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .ok()?;
        MENU_HOST.with(|h| *h.borrow_mut() = Some(hwnd));
        Some(hwnd)
    }
}

/// Show the native shell context menu for `paths` (usually the selection,
/// exactly like Explorer). Blocks until the menu is closed. Returns `true`
/// when the user invoked a command; the caller should then refresh the view.
pub fn show_shell_context_menu(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    ensure_com_sta();
    let Some(host) = ensure_host() else {
        return false;
    };

    // File-list selections share a parent. Never target a partial selection.
    if paths.iter().any(|p| p.parent() != paths[0].parent()) {
        return false;
    }

    // Keep absolute PIDLs alive while borrowing child IDs from them below.
    let pidls: Vec<*mut ITEMIDLIST> = paths
        .iter()
        .filter_map(|path| {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            // SAFETY: wide is NUL-terminated and alive for the call.
            unsafe {
                if SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None).is_ok()
                    && !pidl.is_null()
                {
                    Some(pidl)
                } else {
                    None
                }
            }
        })
        .collect();
    if pidls.len() != paths.len() {
        free_pidls(&pidls);
        return false;
    }

    // SAFETY: pidls are valid and freed on every exit path below.
    let context_menu: Result<IContextMenu, _> = unsafe {
        let bind = || -> windows::core::Result<IContextMenu> {
            let mut first_child = std::ptr::null_mut();
            let parent: IShellFolder = SHBindToParent(pidls[0], Some(&mut first_child))?;
            let mut children = vec![first_child as *const ITEMIDLIST];
            for pidl in pidls.iter().skip(1) {
                let mut child = std::ptr::null_mut();
                let _folder: IShellFolder = SHBindToParent(*pidl, Some(&mut child))?;
                children.push(child as *const ITEMIDLIST);
            }
            parent.GetUIObjectOf::<IContextMenu>(GetForegroundWindow(), &children, None)
        };
        bind()
    };
    let context_menu = match context_menu {
        Ok(cm) => cm,
        Err(e) => {
            tracing::warn!("shell menu: GetUIObjectOf failed: {e}");
            free_pidls(&pidls);
            return false;
        }
    };

    // SAFETY: plain shell call, no outstanding invariants.
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        free_pidls(&pidls);
        return false;
    };

    // SAFETY: menu is valid; the low word of the result carries the number of
    // commands the handler added.
    let hresult =
        unsafe { context_menu.QueryContextMenu(menu, 0, CMD_FIRST, CMD_LAST, CMF_NORMAL) };
    let added = (hresult.0 as u32) & 0xFFFF;
    if hresult.is_err() || added == 0 {
        // No commands offered (e.g. special namespace items): show nothing.
        unsafe {
            let _ = DestroyMenu(menu);
        }
        free_pidls(&pidls);
        return false;
    }

    // Install forwarding interfaces for owner-draw menu items.
    let cm2: Option<IContextMenu2> = context_menu.cast().ok();
    let cm3: Option<IContextMenu3> = context_menu.cast().ok();
    MENU_FORWARD.with(|f| {
        let mut f = f.borrow_mut();
        f.cm2 = cm2;
        f.cm3 = cm3;
    });

    let mut pt = POINT::default();
    // SAFETY: single well-ordered call.
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }

    // SAFETY: host and menu are valid; the call blocks until the menu closes.
    let invoked = unsafe { run_menu_and_invoke(host, &context_menu, menu, pt) };

    MENU_FORWARD.with(|f| {
        let mut f = f.borrow_mut();
        f.cm2 = None;
        f.cm3 = None;
    });
    // SAFETY: menu was created above and is no longer shown.
    unsafe {
        let _ = DestroyMenu(menu);
    }
    free_pidls(&pidls);
    invoked
}

fn free_pidls(pidls: &[*mut ITEMIDLIST]) {
    // SAFETY: each pidl was allocated by SHParseDisplayName.
    unsafe {
        for pidl in pidls {
            ILFree(Some(*pidl as *const _));
        }
    }
}

/// SAFETY: host and menu are valid windows/menus on the UI thread.
unsafe fn run_menu_and_invoke(
    host: HWND,
    context_menu: &IContextMenu,
    menu: HMENU,
    pt: POINT,
) -> bool {
    unsafe {
        let saved_fg = GetForegroundWindow();
        let _ = SetForegroundWindow(host);

        let choice = TrackPopupMenuEx(
            menu,
            (TPM_RIGHTBUTTON | TPM_RETURNCMD).0,
            pt.x,
            pt.y,
            host,
            None,
        );
        let command = choice.0 as u32 & 0xFFFF;

        // Restore activation to the Kova window; WM_NULL releases the menu
        // foreground quirk documented for TrackPopupMenu.
        let _ = PostMessageW(Some(host), WM_NULL, WPARAM(0), LPARAM(0));
        if !saved_fg.is_invalid() {
            let _ = SetForegroundWindow(saved_fg);
        }

        if command == 0 {
            return false;
        }

        // In-range commands use the classic offset encoding. Commands outside
        // the requested range come from namespace handlers that assigned their
        // own ids; those are invoked with the raw id.
        let verb = if (CMD_FIRST..=CMD_LAST).contains(&command) {
            command - CMD_FIRST
        } else {
            command
        };

        let info = CMINVOKECOMMANDINFOEX {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: CMIC_MASK_PTINVOKE,
            hwnd: if saved_fg.is_invalid() {
                host
            } else {
                saved_fg
            },
            lpVerb: PCSTR(verb as usize as *const u8),
            lpVerbW: windows::core::PCWSTR(verb as usize as *const u16),
            nShow: SW_SHOWNORMAL.0,
            ptInvoke: pt,
            ..Default::default()
        };

        match context_menu
            .InvokeCommand(&info as *const CMINVOKECOMMANDINFOEX as *const CMINVOKECOMMANDINFO)
        {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("shell menu: InvokeCommand failed: {e}");
                false
            }
        }
    }
}
