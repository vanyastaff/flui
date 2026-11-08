//! Bounded RGBA8 image decoding for the registry's compute lane.

use crate::error::AssetError;

/// The largest image a decode produces, in RGBA8 bytes: 256 MiB (a 64 MP
/// image), and 128 MiB (32 MP) on Android, iOS and the web.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
pub(crate) const MAX_DECODED_IMAGE_BYTES: u64 = 128 * 1024 * 1024;

/// The largest image a decode produces, in RGBA8 bytes.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub(crate) const MAX_DECODED_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

/// Decodes encoded image bytes into RGBA8 pixels, refusing an image whose
/// header or decoder allocation exceeds [`MAX_DECODED_IMAGE_BYTES`].
pub(crate) fn decode_rgba8(
    path: &str,
    bytes: &[u8],
) -> Result<flui_painting::paint::Image, AssetError> {
    use image::ImageDecoder;

    let too_large = || AssetError::LoadFailed {
        path: path.to_owned(),
        reason: format!("the decoded image would be larger than {MAX_DECODED_IMAGE_BYTES} bytes"),
    };
    let failed = |error: image::ImageError| match error {
        image::ImageError::Limits(_) => too_large(),
        error => AssetError::LoadFailed {
            path: path.to_owned(),
            reason: format!("Failed to decode image: {error}"),
        },
    };
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| failed(error.into()))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(failed)?;
    let (width, height) = decoder.dimensions();
    // `u32::MAX² × 4` does not fit in a u64.
    let rgba_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4));
    if rgba_bytes.is_none_or(|bytes| bytes > MAX_DECODED_IMAGE_BYTES) {
        return Err(too_large());
    }
    let image = image::DynamicImage::from_decoder(decoder).map_err(failed)?;
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    let data = rgba.into_raw();

    flui_painting::paint::Image::try_from_rgba8(width, height, data).map_err(|error| {
        AssetError::LoadFailed {
            path: path.to_owned(),
            reason: format!("Decoded image is malformed: {error}"),
        }
    })
}
