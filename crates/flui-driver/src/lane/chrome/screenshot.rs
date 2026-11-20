//! A screenshot Chrome answered (`browsingContext.captureScreenshot`: base64 of a PNG) as an
//! image.
//!
//! What the browser sends is untrusted, so every size is checked before the work it would cost:
//! the base64's length before it is decoded, and then the PNG goes through the driver's one PNG
//! decoder (`pixels::codec`), which reads the declared size before any pixel and caps its
//! allocations. A capture is never resampled: its pixels are the canvas's device pixels.

use base64::Engine as _;

use crate::error::{Error, Result};
use crate::pixels::Image;

/// The most base64 a screenshot may be: a PNG is at most a little larger than its raw pixels.
pub const MAX_BASE64: usize = 48 << 20;

/// `data`, a base64 PNG, as an image.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for base64 longer than [`MAX_BASE64`], for an image past the
/// decoder's bound, and for data that is not base64 or not a PNG.
pub fn decode(data: &str) -> Result<Image> {
    if data.len() > MAX_BASE64 {
        return Err(Error::invalid_argument(format!(
            "the browser's screenshot is {} bytes of base64, past the {MAX_BASE64} a capture takes",
            data.len()
        )));
    }
    let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| refused(&format!("its base64 does not decode: {error}")))?;
    crate::pixels::codec::decode(&png).map_err(|why| refused(&why))
}

fn refused(why: &str) -> Error {
    Error::invalid_argument(format!("the browser's screenshot is refused: {why}"))
}
