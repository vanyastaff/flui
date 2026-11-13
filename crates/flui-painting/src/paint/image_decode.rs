//! Encoded image bytes (PNG, JPEG, GIF) decoded into an RGBA8 [`Image`], with
//! the decoded size bounded before any pixel is decoded.

use super::image::Image;

/// The largest image a decode produces, in RGBA8 bytes: 256 MiB (a 64 MP
/// image), and 128 MiB (32 MP) on Android, iOS and the web.
///
/// Checked against the header's dimensions before any pixel is decoded, and
/// given to the decoder as its allocation limit, so an image that compresses
/// small but decodes huge is refused instead of allocating.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
pub const MAX_DECODED_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
/// The largest image a decode produces, in RGBA8 bytes: 256 MiB (a 64 MP
/// image), and 128 MiB (32 MP) on Android, iOS and the web.
///
/// Checked against the header's dimensions before any pixel is decoded, and
/// given to the decoder as its allocation limit, so an image that compresses
/// small but decodes huge is refused instead of allocating.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub const MAX_DECODED_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

/// Why encoded bytes did not decode into an [`Image`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ImageDecodeError {
    /// The image would decode past [`MAX_DECODED_IMAGE_BYTES`]; nothing was
    /// decoded.
    #[error(
        "the decoded image would take {requested} bytes, larger than the {limit} a decode takes"
    )]
    TooLarge {
        /// The RGBA8 bytes the header's dimensions ask for, saturated at
        /// `u64::MAX`; `u64::MAX` too when the decoder refused an allocation
        /// past the limit before the dimensions were read.
        requested: u64,
        /// [`MAX_DECODED_IMAGE_BYTES`].
        limit: u64,
    },
    /// The bytes are not an image in a format this build decodes, or are
    /// broken.
    #[error("the image does not decode: {0}")]
    Malformed(String),
}

impl Image {
    /// Decode encoded image bytes (PNG, JPEG or GIF, told apart by their
    /// content) into RGBA8 pixels.
    ///
    /// CPU-bound and synchronous: call it off the frame path.
    ///
    /// # Errors
    ///
    /// [`ImageDecodeError::TooLarge`] when the header's dimensions ask for more
    /// than [`MAX_DECODED_IMAGE_BYTES`], before any pixel is decoded;
    /// [`ImageDecodeError::Malformed`] for bytes that are not such an image.
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageDecodeError> {
        use image::ImageDecoder as _;

        let limit = MAX_DECODED_IMAGE_BYTES;
        let malformed = |error: image::ImageError| match error {
            image::ImageError::Limits(_) => ImageDecodeError::TooLarge {
                requested: u64::MAX,
                limit,
            },
            error => ImageDecodeError::Malformed(error.to_string()),
        };
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| ImageDecodeError::Malformed(error.to_string()))?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(limit);
        reader.limits(limits);
        let decoder = reader.into_decoder().map_err(malformed)?;
        let (width, height) = decoder.dimensions();
        // `u32::MAX² × 4` does not fit in a u64: it saturates.
        let requested = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .unwrap_or(u64::MAX);
        if requested > limit {
            return Err(ImageDecodeError::TooLarge { requested, limit });
        }
        let rgba = image::DynamicImage::from_decoder(decoder)
            .map_err(malformed)?
            .into_rgba8();
        let (width, height) = rgba.dimensions();
        Self::try_from_rgba8(width, height, rgba.into_raw())
            .map_err(|error| ImageDecodeError::Malformed(error.to_string()))
    }
}
