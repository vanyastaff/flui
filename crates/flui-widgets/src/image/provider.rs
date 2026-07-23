//! [`ImageProvider`] trait and built-in implementations for the
//! [`Image`](crate::Image) widget.
//!
//! # Sync-first, async-capable design
//!
//! [`ImageProvider::resolve`] is synchronous and remains the required
//! baseline — the FLUI view layer's most common image sources (an
//! already-decoded [`PixelImage`], in-memory bytes, a local file) have no
//! reason to leave the thread. [`resolve_async`](ImageProvider::resolve_async)
//! and [`cache_key`](ImageProvider::cache_key) are defaulted extension
//! points: a provider that never overrides them behaves exactly as before
//! (`resolve_async`'s default is `Box::pin(future::ready(self.resolve()))` —
//! sync-in-async-clothing, evaluated blocking at the poll site, same thread
//! as today; `cache_key`'s default `None` keeps the provider on the
//! sync-only path). `AssetImage` (behind the `asset-images` feature) and
//! `NetworkImage` (behind `network-images`) override both to genuinely load
//! off-thread.
//!
//! FLUI's `Image` widget is a one-shot resolver, not a port of Flutter's
//! `ImageStream`: it has no chunk/progress events and no multi-frame
//! (animated-image) support, because FLUI's `Image` view is single-frame.
//! This is a documented divergence from `widgets/image.dart`, to revisit when
//! animated images land.
//!
//! # Deferred functionality
//!
//! Not yet built (tracked, not silently missing): `frameBuilder`,
//! `loadingBuilder`, `errorBuilder` (an error currently renders the same
//! empty box as no data, with a `tracing::warn!`), `gaplessPlayback`,
//! `ImageConfiguration`/`devicePixelRatio`-based cache-key scaling, an
//! `evict`/`clearLiveImages` cache-management API, and font unification.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;

use flui_types::painting::Image as PixelImage;

use super::cache_key::ImageCacheKey;

/// Describes how to obtain a decoded image, synchronously or asynchronously.
///
/// The [`Image`](crate::Image) widget calls [`cache_key`](Self::cache_key) to
/// decide which path to take: `None` means synchronous-only, and it calls
/// [`resolve`](Self::resolve) when creating or updating its render object;
/// `Some(key)` means it probes the decode cache for `key` and, on a miss,
/// awaits [`resolve_async`](Self::resolve_async) through a
/// [`FutureBuilder`](crate::FutureBuilder). On error the widget renders an
/// empty box — no panic, but a `tracing::warn!` so the failure is observable.
///
/// # Object safety
///
/// This trait is object-safe and intended to be stored as
/// `Arc<dyn ImageProvider + Send + Sync>` inside [`Image`](crate::Image).
pub trait ImageProvider: std::fmt::Debug + Send + Sync {
    /// Synchronously decode and return the image.
    ///
    /// Called on every widget rebuild for a provider whose
    /// [`cache_key`](Self::cache_key) is `None`. For expensive providers
    /// (file I/O, decoding), pre-decode once and supply the result via
    /// [`DirectImageProvider`] or [`Image::from_image`](crate::Image::from_image).
    ///
    /// # Errors
    ///
    /// Returns [`ImageProviderError`] on I/O or decode failure.
    /// [`Image`](crate::Image) renders an empty box on error rather than
    /// propagating or panicking.
    fn resolve(&self) -> Result<PixelImage, ImageProviderError>;

    /// Asynchronously decode and return the image.
    ///
    /// The default implementation is sync-in-async-clothing: it evaluates
    /// [`resolve`](Self::resolve) — blocking — at the poll site, on whatever
    /// thread polls the returned future (the same thread [`resolve`](Self::resolve)
    /// would have run on today). Override this to genuinely move work off
    /// thread; a provider that does so should also override
    /// [`cache_key`](Self::cache_key) to return `Some`, or [`Image`](crate::Image)
    /// never calls this method at all.
    fn resolve_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        Box::pin(std::future::ready(self.resolve()))
    }

    /// The cache/coalescing/subscription identity for asynchronous
    /// resolution, or `None` to stay on the synchronous-only path.
    ///
    /// The default `None` preserves today's behavior exactly: [`Image`](crate::Image)
    /// never probes the decode cache, never spawns a load, and always calls
    /// [`resolve`](Self::resolve) directly. A provider backed by genuinely
    /// asynchronous I/O (disk via a background runtime, network) overrides
    /// this to opt into the cached/coalesced/[`FutureBuilder`](crate::FutureBuilder)-driven
    /// path.
    ///
    /// Note: [`Image`](crate::Image) only *acts* on a `Some` key — probing
    /// the decode cache and wrapping in a `FutureBuilder` — when the
    /// `asset-images` feature is enabled (the cache/coalescing engine's
    /// dependencies, `lru`/`futures-util`, are pulled in only by that
    /// feature). A custom provider overriding this method without
    /// `asset-images` enabled falls back to [`resolve`](Self::resolve).
    fn cache_key(&self) -> Option<ImageCacheKey> {
        None
    }
}

/// Errors returned by [`ImageProvider::resolve`] and
/// [`ImageProvider::resolve_async`].
#[derive(Debug, Clone, Error)]
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

    /// The provider only supports asynchronous resolution
    /// ([`ImageProvider::resolve_async`]); a synchronous
    /// [`resolve`](ImageProvider::resolve) call found no cached result to
    /// fall back on.
    ///
    /// This is not a transient failure — it is `resolve`'s honest answer for
    /// a provider whose source (an unloaded asset, an unfetched URL) cannot
    /// be read without leaving the calling thread. [`Image`](crate::Image)
    /// never hits this path: it dispatches on
    /// [`cache_key`](ImageProvider::cache_key) to call `resolve_async`
    /// instead. A caller invoking `resolve` directly on such a provider (e.g.
    /// a custom async provider used without the `asset-images` feature,
    /// where `Image` cannot act on `cache_key`) should pre-decode externally
    /// and supply the result via [`Image::from_image`](crate::Image::from_image).
    #[error("`{provider_name}` has no synchronously available result; use resolve_async")]
    RequiresAsyncResolve {
        /// Name of the async-only provider.
        provider_name: &'static str,
    },

    /// The image source (an asset path or a URL) could not be found.
    ///
    /// Distinct from [`FileNotFound`](Self::FileNotFound): this variant
    /// covers `AssetImage`/`NetworkImage`'s asynchronous sources, backed by
    /// a `flui-assets::AssetError::NotFound`. `path` may be a URL, not
    /// necessarily a filesystem path, so a `PathBuf` would misrepresent it.
    #[error("image source not found: {path}")]
    SourceNotFound {
        /// The asset path or URL that could not be found.
        path: String,
    },

    /// The underlying `flui-assets` load failed for a reason other than "not
    /// found" — a read error, a failed HTTP request, an unsupported format,
    /// or any other asset-loading failure surfaced by `flui-assets`.
    ///
    /// Distinct from [`DecodeFailed`](Self::DecodeFailed): this means the
    /// load itself failed, so bytes may never have reached a decoder at all
    /// — reporting it as a decode failure would misdiagnose the cause (e.g.
    /// a refused network connection is not a decode problem).
    #[error("failed to load image source: {reason}")]
    AssetLoadFailed {
        /// The underlying `flui-assets::AssetError`'s message.
        reason: String,
    },
}

/// Maps a `flui-assets::AssetError` onto the closest honest
/// [`ImageProviderError`] — never [`DecodeFailed`](ImageProviderError::DecodeFailed),
/// since a `flui-assets` load failure (missing file, refused connection,
/// unsupported format, …) happens before any bytes reach a decoder. Shared by
/// `AssetImage` and `NetworkImage`.
///
/// As of `flui-assets` 0.2, `ImageAsset::load`'s file-read branch wraps I/O
/// errors — including "not found" — as `AssetError::LoadFailed`, not
/// `AssetError::NotFound`, so [`SourceNotFound`](ImageProviderError::SourceNotFound)
/// is not reachable through a missing local file today; a missing file
/// currently surfaces as [`AssetLoadFailed`](ImageProviderError::AssetLoadFailed)
/// with the underlying I/O message preserved. The match here still handles
/// `NotFound` correctly so this stays honest if that upstream distinction is
/// added later — the important, load-bearing guarantee this function makes
/// today is only "never mislabel a load failure as `DecodeFailed`".
#[cfg(feature = "asset-images")]
impl ImageProviderError {
    pub(crate) fn from_asset_error(source: String, error: flui_assets::AssetError) -> Self {
        if matches!(error, flui_assets::AssetError::NotFound { .. }) {
            return Self::SourceNotFound { path: source };
        }
        Self::AssetLoadFailed {
            reason: error.to_string(),
        }
    }
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

// ---------------------------------------------------------------------------
// Internal decode helper
// ---------------------------------------------------------------------------

/// Decodes encoded bytes (PNG, JPEG, GIF, …) into a [`PixelImage`].
///
/// Gated on the `images` feature. Returns
/// [`ImageProviderError::DecoderUnavailable`] when the feature is absent so
/// the widget renders an empty box without a compile error.
///
/// Used by [`MemoryImage`] and [`FileImage`] only — `AssetImage`/`NetworkImage`
/// (`asset-images`/`network-images` features) decode through `flui-assets`'
/// own `image`-crate usage instead, not this helper.
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
