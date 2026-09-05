//! Streaming animation decoding on the preview worker, never on the UI thread.
use std::{fs::File, io::BufReader, path::Path, time::Duration};

use image::{AnimationDecoder, ImageDecoder};

use crate::preview::Preview;

const MAX_PIXELS: u64 = 4_000_000;

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits
}

fn check_dimensions(decoder: &impl ImageDecoder) -> Result<(), String> {
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("Animation exceeds the 4 megapixel limit".into());
    }
    Ok(())
}

fn open(path: &Path) -> Result<Option<image::Frames<'static>>, String> {
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "gif" | "webp" | "png" | "apng") {
        return Ok(None);
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err("Animation preview is limited to files below 128 MiB".into());
    }
    let reader = BufReader::new(file);
    let result = match extension.as_str() {
        "gif" => {
            let mut decoder =
                image::codecs::gif::GifDecoder::new(reader).map_err(|e| e.to_string())?;
            check_dimensions(&decoder)?;
            decoder.set_limits(limits()).map_err(|e| e.to_string())?;
            decoder.into_frames()
        }
        "webp" => {
            let decoder =
                image::codecs::webp::WebPDecoder::new(reader).map_err(|e| e.to_string())?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            check_dimensions(&decoder)?;
            decoder.into_frames()
        }
        _ => {
            let decoder = image::codecs::png::PngDecoder::with_limits(reader, limits())
                .map_err(|e| e.to_string())?;
            if !decoder.is_apng().map_err(|e| e.to_string())? {
                return Ok(None);
            }
            check_dimensions(&decoder)?;
            decoder.apng().map_err(|e| e.to_string())?.into_frames()
        }
    };
    Ok(Some(result))
}

fn convert(frame: image::Frame) -> (Preview, Duration) {
    // Broken/zero delays must not cause a hot playback loop. Otherwise keep the
    // authored timing (including long holds), rounded only by the UI timer.
    let delay = Duration::from(frame.delay()).max(Duration::from_millis(20));
    let buffer = frame.into_buffer();
    let (width, height) = buffer.dimensions();
    let small = image::DynamicImage::ImageRgba8(buffer)
        .thumbnail(640, 640)
        .into_rgba8();
    (
        Preview {
            text: format!("{width} × {height}"),
            pixels: Some((small.width(), small.height(), small.into_raw())),
            pages: 0,
        },
        delay,
    )
}

/// Returns false for still formats. `emit` supplies bounded backpressure and
/// checks cancellation while waiting; returning false stops decoding promptly.
/// The inspector loops animations; list thumbnails remain still images.
pub fn stream(
    path: &Path,
    current: impl Fn() -> bool,
    mut emit: impl FnMut(Preview, Duration, bool) -> bool,
) -> Result<bool, String> {
    while current() {
        let Some(mut frames) = open(path)? else {
            return Ok(false);
        };
        let Some(first) = frames.next() else {
            return Err("Animation contains no frames".into());
        };
        let first = convert(first.map_err(|e| e.to_string())?);
        if !current() {
            return Ok(true);
        }
        let second = frames
            .next()
            .transpose()
            .map_err(|e| e.to_string())?
            .map(convert);
        let animated = second.is_some();
        if !emit(first.0, first.1, animated) {
            return Ok(true);
        }
        if let Some(second) = second {
            if !emit(second.0, second.1, true) {
                return Ok(true);
            }
        } else {
            return Ok(true);
        }
        // Keep only decoder state and one frame, regardless of clip duration.
        while current() {
            let Some(frame) = frames.next() else {
                break;
            };
            let (preview, delay) = convert(frame.map_err(|e| e.to_string())?);
            if !emit(preview, delay, true) {
                return Ok(true);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_stream_keeps_timing_loops_and_cancels() {
        let path = std::env::temp_dir().join(format!(
            "kova-animation-{}-{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut encoded = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut encoded);
            for (color, delay) in [([255, 0, 0, 255], 80), ([0, 0, 255, 255], 240)] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(8, 8, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(delay, 1),
                    ))
                    .unwrap();
            }
        }
        std::fs::write(&path, encoded).unwrap();
        let mut seen = Vec::new();
        let result = stream(
            &path,
            || true,
            |preview, delay, animated| {
                seen.push((
                    preview.pixels.unwrap().2[..4].to_vec(),
                    delay.as_millis(),
                    animated,
                ));
                seen.len() < 3
            },
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(result, Ok(true));
        assert_eq!(
            seen,
            vec![
                (vec![255, 0, 0, 255], 80, true),
                (vec![0, 0, 255, 255], 240, true),
                (vec![255, 0, 0, 255], 80, true)
            ]
        );
    }

    #[test]
    fn zero_delay_is_bounded_and_large_canvases_are_rejected() {
        let frame = image::Frame::new(image::RgbaImage::new(1, 1));
        assert_eq!(convert(frame).1, Duration::from_millis(20));
        let mut encoded = Vec::new();
        image::codecs::gif::GifEncoder::new(&mut encoded)
            .encode(
                &vec![0; 2100 * 2000 * 4],
                2100,
                2000,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(encoded)).unwrap();
        assert!(check_dimensions(&decoder).is_err());
    }
}
