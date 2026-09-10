//! IFileOperation notifications remain inside the worker's COM apartment.
use crate::transfers::TransferHandle;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Mutex,
};
use windows::{
    Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            COPYENGINE_E_USER_CANCELLED, COPYENGINE_S_USER_IGNORED, IFileOperationProgressSink,
            IFileOperationProgressSink_Impl, IShellItem, SIGDN_FILESYSPATH,
        },
    },
    core::{HRESULT, PCWSTR, Ref, implement},
};

#[implement(IFileOperationProgressSink)]
pub struct ProgressSink {
    record_undo: bool,
    handle: TransferHandle,
    sizes: Mutex<HashMap<PathBuf, u64>>,
    roots: Mutex<HashSet<PathBuf>>,
    base: f32,
    weight: f32,
}
impl ProgressSink {
    pub fn new(
        handle: TransferHandle,
        sources: &[PathBuf],
        base: f32,
        weight: f32,
        record_undo: bool,
    ) -> Self {
        Self {
            record_undo,
            handle,
            sizes: Mutex::new(HashMap::new()),
            roots: Mutex::new(sources.iter().cloned().collect()),
            base,
            weight,
        }
    }
    fn check(&self) -> windows::core::Result<()> {
        if self.handle.is_cancelled() {
            Err(windows::core::Error::from_hresult(
                COPYENGINE_E_USER_CANCELLED,
            ))
        } else {
            Ok(())
        }
    }
    fn before(&self, item: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        self.check()?;
        if let Some(path) = item_path(item) {
            let bytes = std::fs::symlink_metadata(&path)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len());
            if let Some(bytes) = bytes {
                if let Ok(mut sizes) = self.sizes.lock() {
                    sizes.insert(path.clone(), bytes);
                }
            }
            self.handle.update(|state| {
                state.current = path.display().to_string();
            });
        }
        Ok(())
    }
    fn after(
        &self,
        item: Ref<'_, IShellItem>,
        result: HRESULT,
        moved: Ref<'_, IShellItem>,
        is_move: bool,
    ) -> windows::core::Result<()> {
        if let Some(path) = item_path(item) {
            let bytes = self
                .sizes
                .lock()
                .ok()
                .and_then(|mut sizes| sizes.remove(&path));
            let root_done = self.roots.lock().is_ok_and(|mut roots| roots.remove(&path));
            self.handle.update(|state| {
                if root_done && state.progress.is_none() {
                    state.remaining = state.remaining.saturating_sub(1);
                }
                if result.is_err() {
                    state.error = format!(
                        "{}: {}",
                        path.display(),
                        windows::core::Error::from_hresult(result)
                    );
                } else if result != COPYENGINE_S_USER_IGNORED {
                    if let Some(bytes) = bytes {
                        state.bytes = state.bytes.saturating_add(bytes);
                        state.files += 1;
                    }
                }
            });
            if is_move && result.is_ok() && result != COPYENGINE_S_USER_IGNORED {
                if let Some(destination) = item_path(moved) {
                    if root_done && self.record_undo {
                        self.handle.undo.record(&path, &destination, false);
                    }
                    if let Ok(mut moves) = self.handle.moved.lock() {
                        moves.push((path, destination));
                    }
                }
            }
        }
        self.check()
    }
}

fn item_path(item: Ref<'_, IShellItem>) -> Option<PathBuf> {
    let item = item.as_ref()?;
    // SAFETY: live apartment-local Shell item. GetDisplayName returns an owned
    // terminated string allocated by COM; free it after copying, on all paths.
    unsafe {
        let value = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        if value.0.is_null() {
            return None;
        }
        let path = value.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(value.0.cast()));
        path
    }
}

#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for ProgressSink_Impl {
    fn StartOperations(&self) -> windows::core::Result<()> {
        self.check()
    }
    fn FinishOperations(&self, _: HRESULT) -> windows::core::Result<()> {
        Ok(())
    }
    fn PreRenameItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.before(item)
    }
    fn PostRenameItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        _: &PCWSTR,
        result: HRESULT,
        new: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.after(item, result, new, true)
    }
    fn PreMoveItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.before(item)
    }
    fn PostMoveItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        result: HRESULT,
        new: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.after(item, result, new, true)
    }
    fn PreCopyItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> windows::core::Result<()> {
        self.before(item)
    }
    fn PostCopyItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        result: HRESULT,
        new: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.after(item, result, new, false)
    }
    fn PreDeleteItem(&self, _: u32, item: Ref<'_, IShellItem>) -> windows::core::Result<()> {
        self.before(item)
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        item: Ref<'_, IShellItem>,
        result: HRESULT,
        new: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.after(item, result, new, false)
    }
    fn PreNewItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> windows::core::Result<()> {
        self.check()
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> windows::core::Result<()> {
        self.check()
    }
    fn UpdateProgress(&self, total: u32, done: u32) -> windows::core::Result<()> {
        self.check()?;
        if total > 0 {
            self.handle.update(|state| {
                state.progress =
                    Some(self.base + self.weight * (done as f32 / total as f32).clamp(0.0, 1.0));
                state.remaining = total.saturating_sub(done) as usize;
            });
        }
        Ok(())
    }
    fn ResetTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> windows::core::Result<()> {
        Ok(())
    }
}
