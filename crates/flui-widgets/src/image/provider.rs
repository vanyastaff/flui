//! [`ImageProvider`] trait and built-in implementations for the
//! [`Image`](crate::Image) widget.
//!
//! # Sync-first design
//!
//! [`ImageProvider::resolve`] is synchronous because the FLUI view layer has
//! no async rebuild path today. When async loading lands, a companion
//! supertrait will be added:
//!
//! ```text
//! pub trait AsyncImageProvider: ImageProvider {
//!     async fn resolve_async(&self) -> Result<PixelImage, ImageProviderError>;
//! }
//! ```
//!
//! Existing sync implementations satisfy the async version trivially via
//! `future::ready(self.resolve())`, keeping backward compatibility. The sync
//! [`resolve`](ImageProvider::resolve) always remains the required baseline.
//!
//! # Deferred functionality
//!
//! - **Network loading** (`NetworkImage`, behind the `network-images` feature) — returns
//!   [`ImageProviderError::AsyncNotWired`] until the async path lands.
//! - **Image cache** — will be integrated when the scheduler gains async
//!   rebuild support.

use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use flui_types::painting::Image as PixelImage;

/// Describes how to obtain a decoded image synchronously.
///
/// The [`Image`](crate::Image) widget calls [`resolve`](Self::resolve) when
/// creating or updating its render object. On error the widget renders an
/// empty box — no panic.
///
/// # Extension point
///
/// An `AsyncImageProvider: ImageProvider` supertrait will be added when the
/// FLUI view layer gains an async rebuild path. All existing sync providers
/// will remain compatible; the async version defaults to
/// `future::ready(self.resolve())`.
///
/// # Object safety
///
/// This trait is object-safe and intended to be stored as
/// `Arc<dyn ImageProvider + Send + Sync>` inside [`Image`](crate::Image).
pub trait ImageProvider: std::fmt::Debug + Send + Sync {
    /// Synchronously decode and return the image.
    ///
    /// Called on every widget rebuild. For expensive providers (file I/O,
    /// decoding), pre-decode once and supply the result via
    /// [`DirectImageProvider`] or [`Image::from_image`](crate::Image::from_image).
    ///
    /// # Errors
    ///
    /// Returns [`ImageProviderError`] on I/O or decode failure.
    /// [`Image`](crate::Image) renders an empty box on error rather than
    /// propagating or panicking.
    fn resolve(&self) -> Result<PixelImage, ImageProviderError>;
}

/// Errors returned by [`ImageProvider::resolve`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ImageProviderError {
    /// Image decoding requires the `flui-widgets/images` feature.
    ///
    /// Enable `features = ["images"]` on the `flui-widgets` dependency to
    /// activate the `image` decode library.
    #[error("image decoding unavailable: enable the `flui-widgets/images` feature")]
    DecoderUnavailable,

    /// The encoded bytes could not be decoded.
    #[error("image decode failed: {reason}")]
    DecodeFailed {
        /// Human-readable description of the decode failure.
        reason: String,
    },

    /// The image file was not found at the given path.
    #[error("image file not found: {path}")]
    FileNotFound {
        /// Path that does not exist.
        path: PathBuf,
    },

    /// The image file could not be read.
    #[error("failed to read image file `{path}`: {reason}")]
    ReadFailed {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error description.
        reason: String,
    },

    /// The provider requires async I/O, which is not yet integrated with the
    /// FLUI view-layer rebuild path.
    ///
    /// Pre-decode the image externally and supply the result via
    /// [`Image::from_image`](crate::Image::from_image) as a workaround.
    #[error("`{provider_name}` requires async I/O, not yet wired to the FLUI view layer")]
    AsyncNotWired {
        /// Name of the provider requiring async I/O.
        provider_name: &'static str,
    },
}

/// An [`ImageProvider`] backed by an already-decoded [`PixelImage`].
///
/// [`resolve`](ImageProvider::resolve) is O(1): the pixel buffer is
/// `Arc`-backed, so cloning the handle is a reference-count bump. Use this
/// for images decoded at startup, loaded ahead of time, or constructed
/// procedurally.
///
/// Prefer [`Image::from_image`](crate::Image::from_image) as the ergonomic
/// constructor.
#[derive(Debug, Clone)]
pub struct DirectImageProvider {
    decoded: PixelImage,
}

impl DirectImageProvider {
    /// Creates a provider that returns `decoded` on every [`resolve`](ImageProvider::resolve) call.
    pub fn new(decoded: PixelImage) -> Self {
        Self { decoded }
    }
}

impl ImageProvider for DirectImageProvider {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        Ok(self.decoded.clone())
    }
}

/// An [`ImageProvider`] that decodes an encoded image (PNG, JPEG, GIF, …)
/// from owned bytes.
///
/// Decoding is synchronous and occurs on **every** call to
/// [`resolve`](ImageProvider::resolve). For static images in
/// frequently-rebuilt trees, pre-decode once with
/// [`Image::from_image`](crate::Image::from_image) to avoid per-build cost.
///
/// Requires the `flui-widgets/images` feature. Without it,
/// [`resolve`](ImageProvider::resolve) returns
/// [`ImageProviderError::DecoderUnavailable`].
///
/// Prefer [`Image::memory`](crate::Image::memory) as the ergonomic
/// constructor.
#[derive(Debug, Clone)]
pub struct MemoryImage {
    /// Encoded image bytes. Stored in `Arc` so cloning the provider is O(1).
    bytes: Arc<Vec<u8>>,
}

impl MemoryImage {
    /// Creates a provider that decodes `bytes` on each resolution.
    ///
    /// The bytes are moved into an `Arc` so subsequent clones of the provider
    /// share the buffer.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: Arc::new(bytes.into()),
        }
    }

    /// Returns the raw encoded bytes this provider will decode.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl ImageProvider for MemoryImage {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        decode_bytes(&self.bytes)
    }
}

/// An [`ImageProvider`] that reads and decodes a local image file
/// synchronously.
///
/// Both I/O and decoding are synchronous and block the calling thread on
/// every [`resolve`](ImageProvider::resolve) call. For static file images in
/// frequently-rebuilt trees, pre-decode once with
/// [`Image::from_image`](crate::Image::from_image).
///
/// Requires the `flui-widgets/images` feature. Without it,
/// [`resolve`](ImageProvider::resolve) returns
/// [`ImageProviderError::DecoderUnavailable`].
///
/// Prefer [`Image::file`](crate::Image::file) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct FileImage {
    path: PathBuf,
}

impl FileImage {
    /// Creates a provider that reads and decodes the image at `path` on each
    /// resolution.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the path this provider will read.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl ImageProvider for FileImage {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        let bytes = std::fs::read(&self.path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ImageProviderError::FileNotFound {
                    path: self.path.clone(),
                }
            } else {
                ImageProviderError::ReadFailed {
                    path: self.path.clone(),
                    reason: e.to_string(),
                }
            }
        })?;
        decode_bytes(&bytes)
    }
}

/// A typed placeholder for HTTP/HTTPS image loading.
///
/// [`resolve`](ImageProvider::resolve) always returns
/// [`ImageProviderError::AsyncNotWired`] because network I/O requires async
/// scheduling that is not yet integrated with the FLUI view-layer rebuild
/// path.
///
/// The type is stable so widget trees can reference network images today;
/// they will load automatically when the async path lands.
///
/// **Workaround**: fetch the image bytes externally, decode them, and supply
/// the result via [`Image::from_image`](crate::Image::from_image).
///
/// Prefer [`Image::network`](crate::Image::network) as the ergonomic
/// constructor.
#[cfg(feature = "network-images")]
#[derive(Debug, Clone)]
pub struct NetworkImage {
    url: String,
}

#[cfg(feature = "network-images")]
impl NetworkImage {
    /// Creates a stub provider for `url`.
    ///
    /// [`resolve`](ImageProvider::resolve) always fails with
    /// [`ImageProviderError::AsyncNotWired`] until network loading is
    /// integrated with the view layer.
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    /// Returns the URL this provider will load when async loading is
    /// available.
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[cfg(feature = "network-images")]
impl ImageProvider for NetworkImage {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        Err(ImageProviderError::AsyncNotWired {
            provider_name: "NetworkImage",
        })
    }
}

// ---------------------------------------------------------------------------
// Internal decode helper
// ---------------------------------------------------------------------------

/// Decodes encoded bytes (PNG, JPEG, GIF, …) into a [`PixelImage`].
///
/// Gated on the `images` feature. Returns
/// [`ImageProviderError::DecoderUnavailable`] when the feature is absent so
/// the widget renders an empty box without a compile error.
fn decode_bytes(bytes: &[u8]) -> Result<PixelImage, ImageProviderError> {
    #[cfg(feature = "images")]
    {
        let dynamic =
            image::load_from_memory(bytes).map_err(|e| ImageProviderError::DecodeFailed {
                reason: e.to_string(),
            })?;
        let rgba = dynamic.to_rgba8();
        let (width, height) = rgba.dimensions();
        Ok(PixelImage::from_rgba8(width, height, rgba.into_raw()))
    }

    #[cfg(not(feature = "images"))]
    {
        let _ = bytes;
        Err(ImageProviderError::DecoderUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_image() -> PixelImage {
        // 1x1 RGBA pixel -- content is irrelevant, only identity/equality matter.
        PixelImage::from_rgba8(1, 1, vec![10, 20, 30, 255])
    }

    #[test]
    fn direct_image_provider_resolves_to_a_clone_of_the_given_image() {
        let image = test_image();
        let provider = DirectImageProvider::new(image.clone());

        let resolved = provider.resolve().expect("DirectImageProvider never fails");
        assert_eq!(resolved, image);
    }

    #[test]
    fn memory_image_bytes_returns_the_original_encoded_bytes() {
        let provider = MemoryImage::new(vec![1, 2, 3, 4]);
        assert_eq!(provider.bytes(), &[1, 2, 3, 4]);
    }

    #[cfg(not(feature = "images"))]
    #[test]
    fn memory_image_reports_decoder_unavailable_without_the_images_feature() {
        // Without the `images` feature, decode_bytes short-circuits to
        // DecoderUnavailable regardless of the input bytes -- this is the
        // graceful-degradation contract the Image widget relies on to render
        // an empty box instead of panicking.
        let result = MemoryImage::new(vec![0xFFu8; 16]).resolve();
        assert!(
            matches!(result, Err(ImageProviderError::DecoderUnavailable)),
            "expected DecoderUnavailable without the images feature, got {result:?}",
        );
    }

    #[test]
    fn file_image_path_returns_the_configured_path() {
        let provider = FileImage::new("/some/configured/path.png");
        assert_eq!(
            provider.path(),
            std::path::Path::new("/some/configured/path.png")
        );
    }

    #[test]
    fn file_image_reports_file_not_found_for_a_missing_path() {
        let missing = std::env::temp_dir().join("flui-provider-test-does-not-exist.png");
        // Guard against a stale leftover from a previous interrupted run.
        let _ = std::fs::remove_file(&missing);

        let result = FileImage::new(&missing).resolve();
        match result {
            Err(ImageProviderError::FileNotFound { path }) => assert_eq!(path, missing),
            other => panic!("expected FileNotFound for a missing path, got {other:?}"),
        }
    }

    #[cfg(not(feature = "images"))]
    #[test]
    fn file_image_reports_decoder_unavailable_once_the_read_succeeds() {
        // The I/O read is real regardless of the `images` feature; only the
        // subsequent decode_bytes call is feature-gated. A file that exists
        // but isn't a real image must still surface DecoderUnavailable (not
        // silently succeed, and not report a spurious FileNotFound/ReadFailed).
        let path =
            std::env::temp_dir().join(format!("flui-provider-test-{}.bin", std::process::id()));
        std::fs::write(&path, [1u8, 2, 3, 4]).expect("writing the scratch file must succeed");

        let result = FileImage::new(&path).resolve();

        let _ = std::fs::remove_file(&path);

        assert!(
            matches!(result, Err(ImageProviderError::DecoderUnavailable)),
            "expected DecoderUnavailable once the read succeeds, got {result:?}",
        );
    }
}

#[cfg(all(test, feature = "images"))]
mod decode_tests {
    use super::*;

    /// Round-trip a known-size image through PNG encoding and
    /// [`MemoryImage`]: the decoder must recover the exact pixel dimensions.
    /// Exercises the real `images`-feature decode path (`decode_bytes`), which
    /// the default `from_image` tests never reach.
    #[test]
    fn memory_image_decodes_png_to_its_source_dimensions() {
        let source = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encoding a 3×2 RGBA image to PNG cannot fail");

        let decoded = MemoryImage::new(png)
            .resolve()
            .expect("a valid PNG decodes to a PixelImage");
        let size = decoded.size();
        assert_eq!(
            (size.width.get(), size.height.get()),
            (3.0, 2.0),
            "the decoded image keeps its 3×2 source dimensions",
        );
    }

    /// A non-image byte blob surfaces a typed [`ImageProviderError::DecodeFailed`]
    /// rather than panicking.
    #[test]
    fn memory_image_reports_decode_failure_on_garbage_bytes() {
        let result = MemoryImage::new(vec![0u8, 1, 2, 3, 4]).resolve();
        assert!(
            matches!(result, Err(ImageProviderError::DecodeFailed { .. })),
            "garbage bytes must yield a typed DecodeFailed error, got {result:?}",
        );
    }
}
