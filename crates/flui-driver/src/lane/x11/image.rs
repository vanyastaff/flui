//! A window's pixels as `GetImage` answers them in `ZPixmap` format, turned into the RGBA rows a
//! capture carries.
//!
//! The server sends each row padded to its scanline pad, each pixel in its bits per pixel and
//! byte order, the colour channels where the window's visual masks put them. A true-colour visual
//! of depth 24 or 32 at 32 bits per pixel is what every server in use answers with; 16 and 24
//! bits per pixel are read too. A capture carries no alpha: every pixel is opaque.

use crate::error::{Error, Result};

/// How a `ZPixmap` image is laid out, from the server's setup and the window's visual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixmapFormat {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits a pixel takes in the data: 16, 24 or 32.
    pub bits_per_pixel: u8,
    /// The multiple of bits each row is padded to.
    pub scanline_pad: u8,
    /// Whether the server sends a pixel's least significant byte first.
    pub lsb_first: bool,
    /// The bits of a pixel that are red.
    pub red_mask: u32,
    /// The bits of a pixel that are green.
    pub green_mask: u32,
    /// The bits of a pixel that are blue.
    pub blue_mask: u32,
}

/// The bytes one row of `format` takes.
fn row_bytes(format: &PixmapFormat) -> Option<usize> {
    let bits = u64::from(format.width) * u64::from(format.bits_per_pixel);
    let pad = u64::from(format.scanline_pad.max(8));
    usize::try_from(bits.div_ceil(pad) * pad / 8).ok()
}

/// One channel of `pixel` under `mask`, scaled to eight bits.
fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    let value = (pixel & mask) >> shift;
    // Scaled with rounding; `max` is at least one and the result at most 255.
    let scaled = (u64::from(value) * 255 + u64::from(max) / 2) / u64::from(max);
    u8::try_from(scaled).unwrap_or(u8::MAX)
}

/// `data`, laid out as `format` says, as tightly packed RGBA rows, top row first.
///
/// # Errors
///
/// [`Error::Platform`] for bits per pixel other than 16, 24 or 32, or data shorter than its rows.
pub fn rgba(format: &PixmapFormat, data: &[u8]) -> Result<Vec<u8>> {
    let bytes_per_pixel = match format.bits_per_pixel {
        16 => 2,
        24 => 3,
        32 => 4,
        other => {
            return Err(Error::Platform {
                effect: None,
                message: format!("the X server sent an image of {other} bits a pixel"),
                retry: flui_protocol::error::Retry::Never,
            });
        }
    };
    let row = row_bytes(format).ok_or_else(|| too_short(format, data.len()))?;
    let (width, height) = (
        usize::try_from(format.width).map_err(|_| too_short(format, data.len()))?,
        usize::try_from(format.height).map_err(|_| too_short(format, data.len()))?,
    );
    let needed = row.checked_mul(height);
    if needed.is_none_or(|needed| data.len() < needed) {
        return Err(too_short(format, data.len()));
    }
    let mut out = Vec::with_capacity(width.saturating_mul(height).saturating_mul(4));
    for y in 0..height {
        let line = &data[y * row..y * row + row];
        for x in 0..width {
            let bytes = &line[x * bytes_per_pixel..(x + 1) * bytes_per_pixel];
            let pixel = bytes.iter().enumerate().fold(0_u32, |pixel, (i, byte)| {
                let at = if format.lsb_first {
                    i
                } else {
                    bytes_per_pixel - 1 - i
                };
                pixel | (u32::from(*byte) << (8 * at))
            });
            out.extend([
                channel(pixel, format.red_mask),
                channel(pixel, format.green_mask),
                channel(pixel, format.blue_mask),
                u8::MAX,
            ]);
        }
    }
    Ok(out)
}

fn too_short(format: &PixmapFormat, len: usize) -> Error {
    Error::Platform {
        effect: None,
        message: format!(
            "the X server sent {len} bytes for a {} x {} image of {} bits a pixel",
            format.width, format.height, format.bits_per_pixel
        ),
        retry: flui_protocol::error::Retry::Never,
    }
}
