//! Windows shell icon resolution.
//!
//! Icons are resolved through `SHGetFileInfoW` and rendered into a 32-bit
//! RGBA pixel buffer with `DrawIconEx`, which composites color bitmap and
//! mask correctly (including alpha-channel icons). Results are opaque
//! premultiplied RGBA pixels suitable for Slint's
//! `Image::from_rgba8_premultiplied`.
//!
//! The heavy work happens on a dedicated worker thread; the UI only touches
//! the produced buffers.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
};
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON};
use windows::core::PCWSTR;

/// Side length of the requested shell icons (SHGFI_LARGEICON system metric).
pub const ICON_SIZE: u32 = 32;
/// Shell icon resolution is serialized process-wide. The shell's icon cache
/// initialization is not reliably concurrent-safe during the first calls, and
/// Kova resolves all shell icons from a single icon worker anyway.
static SHELL_ICON_LOCK: Mutex<()> = Mutex::new(());

/// Cache key describing which icon to load. Extension and directory keys are
/// shared across all entries with the same type; executable and shortcut
/// entries resolve per path because their icons are file specific.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IconKey {
    Folder,
    File,
    Symlink,
    UnknownType,
    Drive(PathBuf),
    Extension(String),
    Path(PathBuf),
}

/// Derive the cache key for a directory entry.
pub fn icon_key_for(path: &Path, is_dir: bool) -> IconKey {
    if is_dir {
        return IconKey::Folder;
    }
    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "" => IconKey::File,
        "exe" | "lnk" | "msi" | "bat" | "cmd" => IconKey::Path(path.to_path_buf()),
        other => IconKey::Extension(other.to_string()),
    }
}

/// A decoded icon: premultiplied RGBA, top-down rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl IconBitmap {
    fn from_bgra(bgra: &[u8], width: u32, height: u32) -> Self {
        let mut rgba = bgra.to_vec();
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        Self {
            width,
            height,
            rgba,
        }
    }
}

/// Thread-safe icon cache. Hit/miss counters double as performance
/// instrumentation for the icon pipeline.
#[derive(Default)]
pub struct IconCache {
    entries: Mutex<HashMap<IconKey, Option<IconBitmap>>>,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached bitmap for `key`, resolving it via the shell on the
    /// first miss. Negative results are cached as well so failed lookups are
    /// not retried for every repaint request.
    pub fn get_or_resolve(&self, key: &IconKey) -> Option<IconBitmap> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(cached) = entries.get(key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return cached.clone();
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let resolved = resolve_icon(key);
        entries.insert(key.clone(), resolved.clone());
        resolved
    }

    /// (cache hits, cache misses) since creation.
    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }
}
/// Resolve an icon without caching.
pub fn resolve_icon(key: &IconKey) -> Option<IconBitmap> {
    init_com_for_shell();
    let (source, attributes, use_file_attributes) = icon_source(key)?;
    let _serialized = SHELL_ICON_LOCK.lock().unwrap();
    let hicon = get_shell_icon(&source, attributes, use_file_attributes)?;
    // SAFETY: `hicon` is owned by this function from here on; the guard
    // destroys it exactly once on drop.
    let guard = IconGuard(hicon);
    render_hicon(guard.0)
}

/// The shell association code behind SHGFI_USEFILEATTRIBUTES requires a COM
/// apartment on the calling thread. Initialize once per thread; a changed
/// mode (RPC_E_CHANGED_MODE) is fine because COM is then already set up.
fn init_com_for_shell() {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::core::HRESULT;
    // SAFETY: CoInitializeEx has no pointer arguments; the result only tells
    // us whether this call performed the initialization.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let ok = hr.is_ok() || hr == HRESULT(1) || hr.0 as u32 == 0x8001_0106;
        if !ok {
            tracing::debug!("CoInitializeEx failed for icon thread: {hr:?}");
        }
    }
}

/// Which filesystem-ish name to hand to SHGetFileInfoW for a key.
fn icon_source(key: &IconKey) -> Option<(std::ffi::OsString, FILE_FLAGS_AND_ATTRIBUTES, bool)> {
    match key {
        IconKey::Folder => Some((
            OsStr::new("folder").to_os_string(),
            FILE_ATTRIBUTE_DIRECTORY,
            true,
        )),
        IconKey::File => Some((
            OsStr::new("file").to_os_string(),
            FILE_ATTRIBUTE_NORMAL,
            true,
        )),
        IconKey::Symlink => Some((
            OsStr::new("shortcut.lnk").to_os_string(),
            FILE_ATTRIBUTE_NORMAL,
            true,
        )),
        IconKey::UnknownType => Some((
            OsStr::new("file.kovaunknown").to_os_string(),
            FILE_ATTRIBUTE_NORMAL,
            true,
        )),
        IconKey::Extension(ext) => {
            if ext.is_empty()
                || ext.len() > 16
                || ext.chars().any(|c| c == '.' || c == '\\' || c == '/')
            {
                return None;
            }
            Some((
                std::ffi::OsString::from(format!("file.{ext}")),
                FILE_ATTRIBUTE_NORMAL,
                true,
            ))
        }
        IconKey::Drive(root) => {
            let s = root.to_string_lossy();
            // Drive roots must look like "C:\" for the shell.
            if s.len() >= 2 && s.ends_with('\\') {
                Some((
                    root.clone().into_os_string(),
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    false,
                ))
            } else {
                None
            }
        }
        IconKey::Path(path) => Some((
            path.as_os_str().to_os_string(),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            false,
        )),
    }
}

/// Query the shell for the large icon of `source`.
///
/// With `use_file_attributes` the path is not touched on disk; the shell maps
/// the attributes plus extension to an icon. Otherwise the real path is used
/// (drives, executables, special folders).
fn get_shell_icon(
    source: &OsStr,
    attributes: FILE_FLAGS_AND_ATTRIBUTES,
    use_file_attributes: bool,
) -> Option<HICON> {
    let wide: Vec<u16> = source.encode_wide().chain(std::iter::once(0)).collect();
    let mut flags = SHGFI_ICON | SHGFI_LARGEICON;
    if use_file_attributes {
        flags |= SHGFI_USEFILEATTRIBUTES;
    }
    let mut info = SHFILEINFOW::default();
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer alive for the call,
    // `info` is a valid out parameter of exactly the passed size.
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            attributes,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if result == 0 || info.hIcon.is_invalid() {
        return None;
    }
    Some(info.hIcon)
}

/// Render an HICON into a premultiplied RGBA buffer via DrawIconEx.
///
/// DrawIconEx composites the icon's color bitmap and mask onto the 32bpp DIB,
/// which handles both alpha-channel icons and legacy mask-only icons.
fn render_hicon(hicon: HICON) -> Option<IconBitmap> {
    let side = ICON_SIZE as i32;
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: side,
        biHeight: -side, // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: DIB_RGB_COLORS with 32 bpp does not require a screen DC. The
    // out pointer `bits` receives the DIB's pixel memory, owned by `hbmp`.
    let hbmp = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    // SAFETY: Handles are valid GDI objects; the old object is restored and
    // the DIB/DC deleted below.
    let dc = unsafe { CreateCompatibleDC(None) };
    let old = unsafe { SelectObject(dc, HGDIOBJ(hbmp.0)) };

    // SAFETY: `hicon` is a valid icon handle and `dc` holds our DIB.
    let drawn = unsafe { DrawIconEx(dc, 0, 0, hicon, side, side, 0, None, DI_NORMAL) };

    let mut bitmap = None;
    if drawn.is_ok() && !bits.is_null() {
        let len = (ICON_SIZE * ICON_SIZE * 4) as usize;
        // SAFETY: `bits` points at len bytes owned by the selected DIB.
        let bgra = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), len) };
        bitmap = Some(IconBitmap::from_bgra(bgra, ICON_SIZE, ICON_SIZE));
    }

    // SAFETY: Restore the previous GDI selection and release the objects we
    // created in this function.
    unsafe {
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        let _ = DeleteDC(dc);
    }
    bitmap
}

/// RAII wrapper ensuring `DestroyIcon` runs for every HICON we obtain.
struct IconGuard(HICON);

impl Drop for IconGuard {
    fn drop(&mut self) {
        // SAFETY: The guard owns the icon and drops it exactly once.
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_key_for_maps_directories_and_types() {
        assert_eq!(icon_key_for(Path::new("C:\\data"), true), IconKey::Folder);
        assert_eq!(
            icon_key_for(Path::new("C:\\notes.txt"), false),
            IconKey::Extension("txt".into())
        );
        assert_eq!(
            icon_key_for(Path::new("C:\\app.exe"), false),
            IconKey::Path(PathBuf::from("C:\\app.exe"))
        );
        assert_eq!(
            icon_key_for(Path::new("C:\\Makefile"), false),
            IconKey::File
        );
    }

    #[test]
    fn folder_icon_resolves_to_opaque_bitmap() {
        let bitmap = resolve_icon(&IconKey::Folder).expect("folder icon should resolve");
        assert_eq!(bitmap.width, ICON_SIZE);
        assert_eq!(bitmap.height, ICON_SIZE);
        assert_eq!(bitmap.rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(
            bitmap.rgba.chunks_exact(4).any(|px| px[3] != 0),
            "folder icon must contain visible pixels"
        );
    }

    #[test]
    fn txt_extension_icon_resolves() {
        let bitmap =
            resolve_icon(&IconKey::Extension("txt".into())).expect("txt icon should resolve");
        assert_eq!(bitmap.width, ICON_SIZE);
        assert!(bitmap.rgba.chunks_exact(4).any(|px| px[3] != 0));
    }

    #[test]
    fn drive_icon_resolves_for_existing_root() {
        let root =
            PathBuf::from(std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\");
        let bitmap = resolve_icon(&IconKey::Drive(root)).expect("drive icon should resolve");
        assert_eq!(bitmap.width, ICON_SIZE);
    }

    #[test]
    fn invalid_extension_key_is_rejected() {
        assert!(icon_source(&IconKey::Extension("..".into())).is_none());
        assert!(icon_source(&IconKey::Extension("a\\b".into())).is_none());
    }

    #[test]
    fn cache_counts_hits_and_misses() {
        let cache = IconCache::new();
        assert!(cache.get_or_resolve(&IconKey::Folder).is_some());
        let first = cache.stats();
        assert_eq!(first.1, 1, "first lookups must count as misses");
        assert!(cache.get_or_resolve(&IconKey::Folder).is_some());
        let second = cache.stats();
        assert_eq!(second.0, 1, "second lookup must be a cache hit");
        assert_eq!(first.1, 1);
    }
}
