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
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Select a file to preview".into());
    }
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let image = matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tif" | "tiff" | "ico" | "webp"
    );
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
    // SAFETY: load runs on a dedicated worker, never Slint's UI/STA thread.
    // The successful initialization is balanced by the scoped guard.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
    let _apartment = Apartment;
    render(path, page_index, extension == "pdf").map_err(|e| format!("Preview unavailable: {e}"))
}

fn render(path: &Path, page_index: u32, pdf: bool) -> windows::core::Result<Preview> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
    let (stream, pages): (IRandomAccessStream, u32) = if pdf {
        let document = PdfDocument::LoadFromFileAsync(&file)?.get()?;
        let pages = document.PageCount()?;
        let page = document.GetPage(page_index.min(pages.saturating_sub(1)))?;
        let stream = InMemoryRandomAccessStream::new()?;
        let options = PdfPageRenderOptions::new()?;
        let size = page.Size()?;
        let scale = 1024.0 / size.Width.max(size.Height).max(1.0);
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
    let scale = (1024.0 / f64::from(width.max(height).max(1))).min(1.0);
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
}
