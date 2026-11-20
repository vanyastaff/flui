//! A screenshot Chrome answered (`browsingContext.captureScreenshot`: base64 of a PNG) as RGBA
//! rows.
//!
//! What the browser sends is untrusted, so every size is checked before the work it would cost:
//! the base64's length before it is decoded, and the PNG's header before its pixels are, against
//! [`MAX_PIXELS`]; the decoder's own allocation is capped too. A capture is never resampled: its
//! pixels are the canvas's device pixels.

use std::io::Cursor;

use base64::Engine as _;

use crate::error::{Error, Result};
use crate::lane::OsCapture;

/// The most pixels a capture may hold: an 8K canvas.
pub const MAX_PIXELS: u64 = 7680 * 4320;

/// The most base64 a screenshot may be: a PNG is at most a little larger than its raw pixels.
pub const MAX_BASE64: usize = 48 << 20;

/// `data`, a base64 PNG, as a capture.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for base64 longer than [`MAX_BASE64`], for an image of more than
/// [`MAX_PIXELS`] pixels, and for data that is not base64 or not a PNG.
pub fn decode(data: &str) -> Result<OsCapture> {
    if data.len() > MAX_BASE64 {
        return Err(Error::invalid_argument(format!(
            "the browser's screenshot is {} bytes of base64, past the {MAX_BASE64} a capture takes",
            data.len()
        )));
    }
    let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| refused(format!("its base64 does not decode: {error}")))?;
    // The decoder may allocate a row buffer beside the image itself.
    let limit = usize::try_from(MAX_PIXELS * 4 * 2).unwrap_or(usize::MAX);
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(png), png::Limits { bytes: limit });
    let (width, height) = decoder
        .read_header_info()
        .map_err(|error| refused(format!("it is not a PNG: {error}")))?
        .size();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::invalid_argument(format!(
            "the browser's screenshot is {width} x {height}, past the {MAX_PIXELS} pixels a \
             capture takes"
        )));
    }
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|error| refused(format!("its PNG does not decode: {error}")))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| refused("its PNG's size overflows".to_owned()))?;
    let mut pixels = vec![0_u8; size];
    let frame = reader
        .next_frame(&mut pixels)
        .map_err(|error| refused(format!("its PNG does not decode: {error}")))?;
    pixels.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
            .collect(),
        other => {
            return Err(refused(format!("its PNG decodes to {other:?}, not RGBA")));
        }
    };
    OsCapture::try_new(width, height, rgba)
}

fn refused(why: String) -> Error {
    Error::invalid_argument(format!("the browser's screenshot is refused: {why}"))
}
