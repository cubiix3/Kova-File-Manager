//! OLE file drag/drop; all COM objects stay on the owning window's STA thread.
use std::{
    cell::RefCell,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::ScreenToClient,
        System::{
            Com::*,
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            Ole::*,
            SystemServices::MODIFIERKEYS_FLAGS,
        },
        UI::Shell::{
            BHID_DataObject, Common::ITEMIDLIST, ILFree, SHCreateShellItemArrayFromIDLists,
            SHParseDisplayName,
        },
    },
    core::{BOOL, HRESULT, PCWSTR, Ref, implement},
};

pub enum Event<'a> {
    Hover {
        x: i32,
        y: i32,
        paths: &'a [PathBuf],
        moving: bool,
    },
    Leave,
    Drop {
        target: PathBuf,
        paths: Vec<PathBuf>,
        moving: bool,
    },
}
pub type Handler = Rc<dyn Fn(Event<'_>) -> Option<PathBuf>>;
#[implement(IDropTarget)]
struct Target {
    feedback: Option<crate::drop_feedback::Feedback>,
    hwnd: HWND,
    handler: Handler,
    paths: RefCell<Vec<PathBuf>>,
}
impl Target {
    fn choose(
        &self,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        allowed: DROPEFFECT,
    ) -> (Option<PathBuf>, DROPEFFECT) {
        let mut position = POINT {
            x: point.x,
            y: point.y,
        };
        // SAFETY: the registered target owns a live window, and position is writable.
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut position);
        }
        let paths = self.paths.borrow();
        if paths.is_empty() || keys.0 & 0x20 != 0 || keys.0 & 12 == 12 {
            (self.handler)(Event::Leave);
            return (None, DROPEFFECT_NONE);
        }
        let requested_move = keys.0 & 4 != 0;
        let target = (self.handler)(Event::Hover {
            x: position.x,
            y: position.y,
            paths: &paths,
            moving: requested_move,
        });
        let Some(target) = target else {
            return (None, DROPEFFECT_NONE);
        };
        if paths.iter().any(|p| target == *p || target.starts_with(p)) {
            (self.handler)(Event::Leave);
            return (None, DROPEFFECT_NONE);
        }
        let moving = if keys.0 & 8 != 0 {
            false
        } else if requested_move {
            true
        } else {
            paths.iter().all(|p| same_root(p, &target))
        };
        let effect = if moving {
            DROPEFFECT_MOVE
        } else {
            DROPEFFECT_COPY
        };
        let effect = if allowed.0 & effect.0 != 0 {
            effect
        } else {
            DROPEFFECT_NONE
        };
        if effect == DROPEFFECT_NONE {
            (self.handler)(Event::Leave);
            return (None, effect);
        }
        (self.handler)(Event::Hover {
            x: position.x,
            y: position.y,
            paths: &paths,
            moving: effect == DROPEFFECT_MOVE,
        });
        (Some(target), effect)
    }
}
fn same_root(a: &Path, b: &Path) -> bool {
    match (a.components().next(), b.components().next()) {
        (Some(a), Some(b)) => a
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy()),
        _ => false,
    }
}
#[allow(non_snake_case)]
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        *self.paths.borrow_mut() = data
            .as_ref()
            .and_then(|data| read_files(data).ok())
            .unwrap_or_default();
        self.DragOver(keys, point, effect)
    }
    fn DragOver(
        &self,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        if effect.is_null() {
            return Err(windows::core::Error::from_hresult(E_POINTER));
        }
        // SAFETY: OLE supplies a checked writable DROPEFFECT for this synchronous callback.
        unsafe {
            let (target, chosen) = self.choose(keys, point, *effect);
            if let Some(feedback) = &self.feedback {
                if let Some(target) = target.filter(|_| chosen != DROPEFFECT_NONE) {
                    feedback.show(point, &target, chosen == DROPEFFECT_MOVE);
                } else {
                    feedback.hide();
                }
            }
            *effect = chosen;
        }
        Ok(())
    }
    fn DragLeave(&self) -> windows::core::Result<()> {
        if let Some(feedback) = &self.feedback {
            feedback.hide();
        }
        self.paths.borrow_mut().clear();
        (self.handler)(Event::Leave);
        Ok(())
    }
    fn Drop(
        &self,
        _: Ref<'_, IDataObject>,
        keys: MODIFIERKEYS_FLAGS,
        point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        if effect.is_null() {
            return Err(windows::core::Error::from_hresult(E_POINTER));
        }
        // SAFETY: OLE supplies a checked writable DROPEFFECT for this callback.
        let (target, chosen) = self.choose(keys, point, unsafe { *effect });
        let mut accepted = false;
        if let Some(target) = target {
            if chosen != DROPEFFECT_NONE {
                accepted = (self.handler)(Event::Drop {
                    target,
                    paths: self.paths.take(),
                    moving: chosen == DROPEFFECT_MOVE,
                })
                .is_some();
            }
        }
        // SAFETY: same OLE-owned effect pointer, checked above.
        unsafe {
            *effect = if accepted { chosen } else { DROPEFFECT_NONE };
        }
        self.DragLeave()
    }
}
pub struct Registration {
    hwnd: HWND,
    _target: IDropTarget,
}
impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: revoke our own registration on its owning STA thread before the window is destroyed.
        unsafe {
            let _ = RevokeDragDrop(self.hwnd);
        }
    }
}
pub fn register(hwnd: isize, handler: Handler) -> windows::core::Result<Registration> {
    let hwnd = HWND(hwnd as *mut _);
    let target: IDropTarget = Target {
        feedback: crate::drop_feedback::Feedback::new(hwnd).ok(),
        hwnd,
        handler,
        paths: RefCell::new(Vec::new()),
    }
    .into();
    // SAFETY: window is owned by this UI thread. Replace winit's default target so
    // one OLE target handles a whole selection and reports the negotiated effect.
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        RegisterDragDrop(hwnd, &target)?;
    }
    Ok(Registration {
        hwnd,
        _target: target,
    })
}
fn read_files(data: &IDataObject) -> windows::core::Result<Vec<PathBuf>> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
        ..Default::default()
    };
    // SAFETY: format describes a standard OLE HGLOBAL payload. The returned medium
    // is released on every path; parsing is bounded by its actual allocation size.
    unsafe {
        let mut medium = data.GetData(&format)?;
        let result = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            let memory = medium.u.hGlobal;
            let size = GlobalSize(memory);
            if size > 16 * 1024 * 1024 {
                Vec::new()
            } else {
                let pointer = GlobalLock(memory);
                let paths = crate::clipboard::parse_hdrop(pointer.cast(), size);
                if !pointer.is_null() {
                    let _ = GlobalUnlock(memory);
                }
                paths
            }
        } else {
            Vec::new()
        };
        ReleaseStgMedium(&mut medium);
        Ok(result)
    }
}
#[implement(IDropSource)]
struct Source;
#[allow(non_snake_case)]
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & 1 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }
    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}
pub fn start(paths: &[PathBuf]) -> windows::core::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    struct Lists(Vec<*mut ITEMIDLIST>);
    impl Drop for Lists {
        fn drop(&mut self) {
            for &p in &self.0 {
                // SAFETY: every PIDL was allocated by SHParseDisplayName and is freed exactly once.
                unsafe {
                    ILFree(Some(p));
                }
            }
        }
    }
    let mut lists = Lists(Vec::new());
    // SAFETY: parsing strings remain valid during each call; PIDLs and COM objects
    // remain owned in this apartment throughout OLE's nested drag loop.
    unsafe {
        for path in paths {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut pidl = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None)?;
            lists.0.push(pidl);
        }
        let pointers: Vec<_> = lists.0.iter().map(|p| *p as *const ITEMIDLIST).collect();
        let array = SHCreateShellItemArrayFromIDLists(&pointers)?;
        let data: IDataObject = array.BindToHandler(None, &BHID_DataObject)?;
        let source: IDropSource = Source.into();
        let mut effect = DROPEFFECT_NONE;
        DoDragDrop(
            &data,
            &source,
            DROPEFFECT_COPY | DROPEFFECT_MOVE,
            &mut effect,
        )
        .ok()
    }
}
