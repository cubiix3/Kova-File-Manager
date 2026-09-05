//! Bounded, read-only previews. Call only from the dedicated preview worker.
use std::{io::Read, path::Path};
use windows::{
    Data::Pdf::{PdfDocument, PdfPageRenderOptions},
    Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode,
        ExifOrientationMode,
    },
    Storage::{
        StorageFile,
        Streams::{Buffer, DataReader, IRandomAccessStream, InMemoryRandomAccessStream},
    },
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{HSTRING, Interface},
};

pub struct Preview {
    pub text: String,
    pub pixels: Option<(u32, u32, Vec<u8>)>,
    pub pages: u32,
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: constructed only after successful RoInitialize on this thread;
        // all preview WinRT objects are dropped before this guard.
        unsafe { RoUninitialize() };
    }
}

pub fn load(path: &Path, page_index: u32) -> Result<Preview, String> {
    load_scaled(path, page_index, 1024)
}

pub fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_str(),
        "png" | "apng" | "jpg" | "jpeg" | "bmp" | "gif" | "tif" | "tiff" | "ico" | "webp"
    )
}

/// Automatic list thumbnails stay on local disks and skip observed placeholders.
pub fn load_thumbnail(path: &Path) -> Result<Preview, String> {
    use std::os::windows::fs::MetadataExt;
    if !crate::folder_size::is_local_fixed(path) {
        return Err("No automatic thumbnail for this location or type".into());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_attributes() & 0x0044_1400 != 0 {
        return Err("Automatic thumbnail skipped for link or offline file".into());
    }
    if is_image(path)
        || path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        if let Ok(preview) = load_scaled(path, 0, 64) {
            return Ok(preview);
        }
    }
    // SAFETY: this function runs only on the dedicated thumbnail worker. The
    // guard balances initialization after all Shell COM objects are released.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
    let _apartment = Apartment;
    crate::shell_thumbnail::load(path).ok_or_else(|| "No Windows thumbnail available".into())
}

fn load_scaled(path: &Path, page_index: u32, max_edge: u32) -> Result<Preview, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Select a file to preview".into());
    }
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let image = is_image(path);
    if !image && extension != "pdf" {
        let text = matches!(
            extension.as_str(),
            "txt"
                | "md"
                | "rs"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
                | "xml"
                | "html"
                | "css"
                | "js"
                | "ts"
                | "csv"
                | "log"
                | "ini"
                | "cfg"
                | "ps1"
                | "bat"
                | "c"
                | "cpp"
                | "h"
                | "py"
                | "slint"
                | "svg"
                | "gitignore"
                | ""
        );
        if !text {
            return Err("No preview for this file type".into());
        }
        let mut data = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(65_536)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        let mut text = decode_text(&data)?;
        if text.is_empty() {
            text = "Empty text file".into();
        }
        if metadata.len() > data.len() as u64 {
            text.push_str("\n\n[Preview limited to the first 64 KiB]");
        }
        return Ok(Preview {
            text,
            pixels: None,
            pages: 0,
        });
    }
    if metadata.len() > 128 * 1024 * 1024 {
        return Err("Preview is limited to files below 128 MiB".into());
    }
    if extension == "webp" {
        return render_webp(path, max_edge);
    }
    // SAFETY: load runs on a dedicated worker, never Slint's UI/STA thread.
    // The successful initialization is balanced by the scoped guard.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
    let _apartment = Apartment;
    render(path, page_index, extension == "pdf", max_edge)
        .map_err(|e| format!("Preview unavailable: {e}"))
}

fn render_webp(path: &Path, max_edge: u32) -> Result<Preview, String> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?;
    reader.set_format(image::ImageFormat::WebP);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    // Check before allocating the decoded image; decoder allocation limits alone
    // are best-effort and do not guard DynamicImage's output allocation.
    if decoder.total_bytes() > 64 * 1024 * 1024 {
        return Err("WebP decoded image exceeds the 64 MiB preview limit".into());
    }
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    let small = image.thumbnail(max_edge, max_edge).into_rgba8();
    Ok(Preview {
        text: format!("{width} × {height}"),
        pixels: Some((small.width(), small.height(), small.into_raw())),
        pages: 0,
    })
}

fn render(
    path: &Path,
    page_index: u32,
    pdf: bool,
    max_edge: u32,
) -> windows::core::Result<Preview> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
    let (stream, pages): (IRandomAccessStream, u32) = if pdf {
        let document = PdfDocument::LoadFromFileAsync(&file)?.get()?;
        let pages = document.PageCount()?;
        let page = document.GetPage(page_index.min(pages.saturating_sub(1)))?;
        let stream = InMemoryRandomAccessStream::new()?;
        let options = PdfPageRenderOptions::new()?;
        let size = page.Size()?;
        let scale = max_edge as f32 / size.Width.max(size.Height).max(1.0);
        options.SetDestinationWidth((size.Width * scale).max(1.0) as u32)?;
        options.SetDestinationHeight((size.Height * scale).max(1.0) as u32)?;
        let result = page
            .RenderWithOptionsToStreamAsync(&stream, &options)?
            .get();
        page.Close()?;
        result?;
        stream.Seek(0)?;
        (stream.into(), pages)
    } else {
        (file.OpenReadAsync()?.get()?.cast()?, 0)
    };
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let (width, height) = (decoder.PixelWidth()?, decoder.PixelHeight()?);
    if u64::from(width) * u64::from(height) > 80_000_000 {
        return Err(windows::core::Error::new(
            windows::core::HRESULT(0x80070057u32 as i32),
            "Image exceeds the preview pixel limit",
        ));
    }
    let scale = (f64::from(max_edge) / f64::from(width.max(height).max(1))).min(1.0);
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth((f64::from(width) * scale).max(1.0) as u32)?;
    transform.SetScaledHeight((f64::from(height) * scale).max(1.0) as u32)?;
    let bitmap = decoder
        .GetSoftwareBitmapTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()?;
    let (w, h) = (bitmap.PixelWidth()? as u32, bitmap.PixelHeight()? as u32);
    let buffer = Buffer::Create(w * h * 4)?;
    bitmap.CopyToBuffer(&buffer)?;
    let mut pixels = vec![0; (w * h * 4) as usize];
    DataReader::FromBuffer(&buffer)?.ReadBytes(&mut pixels)?;
    bitmap.Close()?;
    Ok(Preview {
        text: if pdf {
            format!(
                "Page {} of {pages}",
                page_index.min(pages.saturating_sub(1)) + 1
            )
        } else {
            format!("{width} × {height}")
        },
        pixels: Some((w, h, pixels)),
        pages,
    })
}

fn decode_text(data: &[u8]) -> Result<String, String> {
    if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
        let little = data[0] == 0xff;
        let units: Vec<_> = data[2..]
            .chunks_exact(2)
            .map(|c| {
                if little {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    if data.contains(&0) {
        return Err("Binary content has no text preview".into());
    }
    Ok(
        String::from_utf8_lossy(data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data))
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::decode_text;
    #[test]
    fn text_encodings_and_binary_detection() {
        assert_eq!(decode_text(&[0xff, 0xfe, 0x41, 0, 0xe4, 0]).unwrap(), "Aä");
        assert_eq!(decode_text(&[0xfe, 0xff, 0, 0x41]).unwrap(), "A");
        assert_eq!(decode_text(b"\xef\xbb\xbfhello").unwrap(), "hello");
        assert!(decode_text(b"binary\0data").is_err());
    }

    #[test]
    fn webp_thumbnail_preserves_alpha_and_bounds_without_a_windows_codec() {
        let path = std::env::temp_dir().join(format!(
            "kova-webp-{}-{}.webp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut encoded = Vec::new();
        let source = [24, 120, 200, 128].repeat(120 * 80);
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .encode(&source, 120, 80, image::ExtendedColorType::Rgba8)
            .unwrap();
        std::fs::write(&path, encoded).unwrap();
        let result = super::render_webp(&path, 64);
        std::fs::remove_file(&path).unwrap();
        let (width, height, pixels) = result.unwrap().pixels.unwrap();
        assert_eq!(width, 64);
        assert!((42..=43).contains(&height));
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        assert!(
            pixels
                .chunks_exact(4)
                .all(|pixel| pixel == [24, 120, 200, 128])
        );
    }
}
