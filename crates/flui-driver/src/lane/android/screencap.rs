//! The device's own image of its display (`screencap`), read as untrusted input and cut to the
//! app's window.
//!
//! `screencap` writes the display raw unless asked for a PNG: a header of little-endian `u32`s
//! (width, height, pixel format, and from Android 9 the colour space) and the pixels, four bytes
//! each. The lane reads the raw form first, which needs no encoding on the device and no decoding
//! here, and takes it only in a format it knows as four bytes red first (`RGBA_8888`,
//! `RGBX_8888`); any other format is captured again as a PNG (`screencap -p`), decoded within the
//! same bounds. The header is checked against [`MAX_SIDE`] and the bytes that came before any
//! pixel is read.

use super::uiautomator::Bounds;
use crate::lane::OsCapture;

/// The longest side of a display the lane reads, in pixels.
pub const MAX_SIDE: u32 = 8192;

/// The most bytes a capture may hold: a display of [`MAX_SIDE`] squared, four bytes a pixel, and
/// its header.
pub const MAX_CAPTURE: usize = (MAX_SIDE as usize) * (MAX_SIDE as usize) * 4 + 16;

/// The line that captures the display raw.
pub const RAW: &str = "screencap";

/// The line that captures the display as a PNG.
pub const PNG: &str = "screencap -p";

/// `PixelFormat.RGBA_8888`.
const RGBA_8888: u32 = 1;
/// `PixelFormat.RGBX_8888`: its fourth byte is no alpha.
const RGBX_8888: u32 = 2;

/// What a raw capture is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Raw {
    /// Its pixels, four bytes each, red first, opaque.
    Rgba {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// The pixels, top row first.
        rgba: Vec<u8>,
    },
    /// A pixel format the lane does not read raw: capture it as a PNG.
    OtherFormat(u32),
}

/// The raw capture `bytes` holds.
///
/// # Errors
///
/// Why it is none: a header too short, a side of zero or past [`MAX_SIDE`], or a length that is
/// neither header plus pixels for a 12- nor a 16-byte header.
pub fn parse_raw(bytes: &[u8]) -> Result<Raw, String> {
    let word = |at: usize| -> Option<u32> {
        let slice = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    };
    let (Some(width), Some(height), Some(format)) = (word(0), word(4), word(8)) else {
        return Err(format!(
            "a raw capture of {} bytes has no header",
            bytes.len()
        ));
    };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!(
            "a raw capture of {width} x {height} pixels is outside 1 to {MAX_SIDE}"
        ));
    }
    if format != RGBA_8888 && format != RGBX_8888 {
        return Ok(Raw::OtherFormat(format));
    }
    // Both sides are at most `MAX_SIDE`, so the product fits.
    let pixels = (width as usize) * (height as usize) * 4;
    let Some(header @ (12 | 16)) = bytes.len().checked_sub(pixels) else {
        return Err(format!(
            "a raw capture of {width} x {height} pixels holds {} bytes, not its {pixels} and a \
             header",
            bytes.len()
        ));
    };
    let mut rgba = bytes[header..].to_vec();
    if format == RGBX_8888 {
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = u8::MAX;
        }
    }
    Ok(Raw::Rgba {
        width,
        height,
        rgba,
    })
}

/// The PNG capture `bytes` holds, as four bytes a pixel, red first.
///
/// # Errors
///
/// Why it is none: no PNG, a side past [`MAX_SIDE`], a decoding failure.
pub fn parse_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_CAPTURE as u64 * 2);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|error| format!("the PNG capture does not decode: {error}"))?
        .into_rgba8();
    let (width, height) = decoded.dimensions();
    Ok((width, height, decoded.into_raw()))
}

/// The part of a `width` by `height` capture of `rgba` that `window` covers.
///
/// # Errors
///
/// When the window is empty or reaches past the capture.
pub fn crop(width: u32, height: u32, rgba: &[u8], window: Bounds) -> Result<OsCapture, String> {
    let inside = window.left >= 0
        && window.top >= 0
        && window.width() > 0
        && window.height() > 0
        && u32::try_from(window.right).is_ok_and(|right| right <= width)
        && u32::try_from(window.bottom).is_ok_and(|bottom| bottom <= height);
    if !inside {
        return Err(format!(
            "the window {window:?} is not inside the {width} x {height} display capture"
        ));
    }
    // Every value below is checked inside the capture, whose sides are at most `MAX_SIDE`.
    let to_usize = |value: i32| usize::try_from(value).unwrap_or_default();
    let (left, top) = (to_usize(window.left), to_usize(window.top));
    let (cut_width, cut_height) = (to_usize(window.width()), to_usize(window.height()));
    let stride = width as usize * 4;
    let mut cut = Vec::with_capacity(cut_width * cut_height * 4);
    for row in top..top + cut_height {
        let start = row * stride + left * 4;
        let line = rgba
            .get(start..start + cut_width * 4)
            .ok_or_else(|| "the capture holds fewer rows than its header says".to_owned())?;
        cut.extend_from_slice(line);
    }
    OsCapture::try_new(
        u32::try_from(cut_width).unwrap_or_default(),
        u32::try_from(cut_height).unwrap_or_default(),
        cut,
    )
    .map_err(|error| error.to_string())
}
