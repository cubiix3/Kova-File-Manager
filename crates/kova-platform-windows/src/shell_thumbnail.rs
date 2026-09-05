//! Windows thumbnail-provider fallback. Caller owns an initialized COM apartment.
use crate::preview::Preview;
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS,
            DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
        },
        UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY},
    },
    core::PCWSTR,
};

struct BitmapGuard(HBITMAP);
impl Drop for BitmapGuard {
    fn drop(&mut self) {
        // SAFETY: GetImage transfers ownership of this bitmap to the caller.
        // It is never selected into a DC and is deleted once by this guard.
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

pub fn load(path: &Path) -> Option<Preview> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: wide is terminated and remains alive for the call. COM has been
    // initialized on this worker; the returned interface never leaves it.
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    // SAFETY: factory is a live Shell interface and the size is bounded.
    // THUMBNAILONLY fails when unavailable, preserving the existing type icon.
    let bitmap = BitmapGuard(
        unsafe { factory.GetImage(SIZE { cx: 64, cy: 64 }, SIIGBF_THUMBNAILONLY) }.ok()?,
    );
    let mut info = BITMAP::default();
    // SAFETY: bitmap is owned by the guard; info is a correctly sized output.
    if unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut info as *mut BITMAP).cast()),
        )
    } == 0
    {
        return None;
    }
    let (w, h) = (info.bmWidth, info.bmHeight);
    if !(1..=64).contains(&w) || !(1..=64).contains(&h) {
        return None;
    }
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    // SAFETY: creates a private memory DC, released below on both outcomes.
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    // SAFETY: the bitmap is not selected into a DC. The 32-bit top-down output
    // buffer has exactly width * height * 4 bytes and all pointers are live.
    let rows = unsafe {
        GetDIBits(
            dc,
            bitmap.0,
            0,
            h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    // SAFETY: dc was created above and is no longer used.
    unsafe {
        let _ = DeleteDC(dc);
    }
    if rows != h {
        return None;
    }
    // Shell bitmaps use premultiplied BGRA. Legacy opaque bitmaps have no alpha.
    let opaque = pixels.chunks_exact(4).all(|pixel| pixel[3] == 0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        if opaque {
            pixel[3] = 255;
        } else if pixel[3] > 0 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Some(Preview {
        text: String::new(),
        pixels: Some((w as u32, h as u32, pixels)),
        pages: 0,
    })
}
