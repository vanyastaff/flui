//! The Simulator's own image of the app's window, read as untrusted input.
//!
//! `simctl io screenshot` writes the screen as a PNG. Its file length is held to
//! [`MAX_SCREENSHOT_BYTES`] before it is read ([`super::xcrun`]), and the size its header declares
//! ([`png_size`]) to the window's before any pixel is decoded: a UIKit window fills its screen, so
//! the screen is the window, and a screen of another size (a turned device, a window that does not
//! fill it) is refused rather than published as the window.

use flui_protocol::session::Placement;

use super::xcrun::lane_error;
use crate::error::{Error, Result};
use crate::pixels::Image;

/// The most bytes a screenshot's file may have.
pub const MAX_SCREENSHOT_BYTES: u64 = crate::pixels::codec::MAX_PNG_FILE_BYTES;

/// The longest side of a screen the lane reads, in pixels.
pub const MAX_SIDE: u32 = 8192;

/// How far the screen may be from the window's logical size times its scale, in pixels on each
/// side: a surface is laid out in whole pixels.
pub const SIZE_TOLERANCE: f64 = 2.0;

/// The PNG signature.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The width and height a PNG's header declares, read from its first chunk, which the format
/// makes `IHDR`; `None` for bytes that are no PNG.
#[must_use]
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let word = |at: usize| -> Option<u32> {
        let slice = bytes.get(at..at + 4)?;
        Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
    };
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" || word(8)? != 13 {
        return None;
    }
    Some((word(16)?, word(20)?))
}

/// The pixels the window `placement` describes covers on a screen of `width` by `height`: the
/// whole screen when the window fills it.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a placement whose size or scale is not finite and positive;
/// [`Error::NotSupported`] naming `window_position` when the window does not fill the screen,
/// since the lane does not know where else it sits.
pub fn check_fills(placement: &Placement, width: u32, height: u32) -> Result<()> {
    let device = |logical: f64| logical * placement.scale;
    let (window_width, window_height) =
        (device(placement.size.width), device(placement.size.height));
    let finite = [window_width, window_height, placement.scale]
        .iter()
        .all(|value| value.is_finite() && *value > 0.0);
    if !finite {
        return Err(Error::invalid_argument(format!(
            "window {} is {} x {} at scale {}, which is no surface",
            placement.window, placement.size.width, placement.size.height, placement.scale
        )));
    }
    let fills = (f64::from(width) - window_width).abs() <= SIZE_TOLERANCE
        && (f64::from(height) - window_height).abs() <= SIZE_TOLERANCE;
    if fills {
        return Ok(());
    }
    Err(Error::not_supported(
        "window_position",
        format!(
            "window {} is {window_width} x {window_height} pixels and the simulator's screen \
             {width} x {height}: the window does not fill the screen, or the screen is turned, and \
             the lane does not know where the window sits on it",
            placement.window
        ),
    ))
}

/// The window `placement` describes, from the screenshot `png`: its declared size checked against
/// the window's ([`check_fills`]) and against [`MAX_SIDE`] before its pixels are decoded.
///
/// # Errors
///
/// As [`check_fills`]; [`Error::Lane`] for bytes that are no PNG, a side past [`MAX_SIDE`] or a
/// decoding failure.
pub fn window_image(png: &[u8], placement: &Placement) -> Result<Image> {
    let (width, height) =
        png_size(png).ok_or_else(|| lane_error("the Simulator's screenshot is no PNG"))?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(lane_error(format!(
            "the Simulator's screenshot of {width} x {height} pixels is outside 1 to {MAX_SIDE} on \
             a side"
        )));
    }
    check_fills(placement, width, height)?;
    let image = crate::pixels::codec::decode(png)
        .map_err(|why| lane_error(format!("the Simulator's screenshot does not decode: {why}")))?;
    if (image.width(), image.height()) != (width, height) {
        return Err(lane_error(format!(
            "the Simulator's screenshot declared {width} x {height} pixels and decoded to {} x {}",
            image.width(),
            image.height()
        )));
    }
    Ok(image)
}
