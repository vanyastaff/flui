//! PNG files: baselines, their `.new` and `.diff` images, and a trace's captured evidence.
//!
//! A PNG the driver reads is untrusted (a baseline anyone can commit, a trace from another
//! machine): its file length is checked before it is read, its declared size before its pixels
//! are decoded, and the decoder's allocations are capped. What it writes is 8-bit RGBA.

use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use png::{BitDepth, ColorType, Decoder, Encoder, Limits, Transformations};

use super::Image;
use crate::error::{Error, Result};

/// The longest PNG file read: a 4096 by 4096 capture compresses far below it.
pub const MAX_PNG_FILE_BYTES: u64 = 64 << 20;

/// The most pixel bytes a PNG may decode to: a 8192 by 8192 RGBA surface.
pub const MAX_DECODED_BYTES: usize = 256 << 20;

/// `image` as PNG bytes.
///
/// # Errors
///
/// [`Error::InvalidArgument`] when the encoder refuses the image.
pub fn encode(image: &Image) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, image.width(), image.height());
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let refused = |error: png::EncodingError| {
        Error::invalid_argument(format!("a PNG did not encode: {error}"))
    };
    let mut writer = encoder.write_header().map_err(refused)?;
    writer.write_image_data(image.rgba()).map_err(refused)?;
    writer.finish().map_err(refused)?;
    Ok(bytes)
}

/// The image the PNG file at `path` holds, read within [`MAX_PNG_FILE_BYTES`] and decoded within
/// [`MAX_DECODED_BYTES`].
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read; [`Error::Unreadable`] naming the file when it is
/// past a bound or is not a PNG.
pub fn read(path: &Path) -> Result<Image> {
    let file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > MAX_PNG_FILE_BYTES {
        return Err(Error::unreadable(
            path,
            format!("{length} bytes, past the {MAX_PNG_FILE_BYTES} a PNG file may have"),
        ));
    }
    let mut bytes = Vec::new();
    // One byte past the bound tells a file that grew since its length was read.
    file.take(MAX_PNG_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_or(true, |read| read > MAX_PNG_FILE_BYTES) {
        return Err(Error::unreadable(
            path,
            "the file grew past the bound as it was read",
        ));
    }
    decode(&bytes).map_err(|message| Error::unreadable(path, message))
}

/// The image `bytes` hold, as 8-bit RGBA; why not, for people.
pub(crate) fn decode(bytes: &[u8]) -> std::result::Result<Image, String> {
    let mut decoder = Decoder::new_with_limits(
        Cursor::new(bytes),
        Limits {
            bytes: MAX_DECODED_BYTES,
        },
    );
    decoder.set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("not a PNG: {error}"))?;
    let (width, height) = (reader.info().width, reader.info().height);
    let declared = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4));
    let fits = declared
        .and_then(|declared| usize::try_from(declared).ok())
        .is_some_and(|declared| declared <= MAX_DECODED_BYTES);
    if !fits {
        return Err(format!(
            "a {width} x {height} image, past the {MAX_DECODED_BYTES} pixel bytes a PNG may decode to"
        ));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("a {width} x {height} image does not fit in memory"))?;
    if size > MAX_DECODED_BYTES {
        return Err(format!(
            "it decodes to {size} bytes, past the {MAX_DECODED_BYTES} a PNG may"
        ));
    }
    let mut pixels = vec![0_u8; size];
    let frame = reader
        .next_frame(&mut pixels)
        .map_err(|error| format!("its pixels do not decode: {error}"))?;
    pixels.truncate(frame.buffer_size());
    let rgba = match (frame.color_type, frame.bit_depth) {
        (ColorType::Rgba, BitDepth::Eight) => pixels,
        (ColorType::GrayscaleAlpha, BitDepth::Eight) => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
            .collect(),
        (color, depth) => {
            return Err(format!(
                "it decodes to {color:?} at {depth:?}, which the driver does not read"
            ));
        }
    };
    Image::try_new(width, height, rgba).map_err(|error| error.to_string())
}
