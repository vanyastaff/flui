//! [`ImageProvider`] trait and built-in implementations for the
//! [`Image`](crate::Image) widget.
//!
//! # Sync baseline, three ways off the owner thread
//!
//! [`ImageProvider::resolve`] is synchronous and remains the required
//! baseline. A provider that also returns a [`cache_key`](ImageProvider::cache_key)
//! is resolved by a subscription instead, and leaves the owner thread in one
//! of two ways:
//!
//! - [`decode_job`](ImageProvider::decode_job): a [`DecodeJob`] the widget
//!   runs through the UI runtime's background capability, reading on the IO
//!   lane and decoding on the compute lane. [`MemoryImage`] and
//!   [`FileImage`] take this way, so neither decodes on the owner thread.
//! - [`resolve_async`](ImageProvider::resolve_async): a future of the
//!   provider's own, for a source with its own loader, handed the same
//!   background lanes. `AssetImage` (behind the `asset-images` feature) and
//!   `NetworkImage` (behind `network-images`) take this way: their registry
//!   reads and decodes on those lanes.
//!
//! A provider with no key, such as [`DirectImageProvider`], resolves inline
//! on every build.
//!
//! FLUI's `Image` widget is a one-shot resolver, not an image stream: it has
//! no chunk/progress events and no multi-frame (animated-image) support,
//! because the `Image` view is single-frame. To revisit when
//! animated images land.
//!
//! # Deferred functionality
//!
//! Not yet built (tracked, not silently missing): frame, loading and error
//! builders (an error currently renders the same empty box as no data, with a
//! `tracing::warn!`), device-pixel-ratio-based cache-key scaling, font
//! unification, and a decoded-image cache owned by the UI runtime that
//! [`MemoryImage`] and [`FileImage`] widgets with equal keys would share.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use thiserror::Error;

use flui_painting::paint::Image as PixelImage;
use flui_view::background::Background;

use super::cache_key::ImageCacheKey;

/// Describes how to obtain a decoded image, synchronously or asynchronously.
///
/// The [`Image`](crate::Image) widget calls [`cache_key`](Self::cache_key) to
/// decide which path to take: `None` means synchronous-only, and it calls
/// [`resolve`](Self::resolve) when creating or updating its render object;
/// `Some(key)` means it probes [`cached`](Self::cached) and, on a miss,
/// awaits [`resolve_async`](Self::resolve_async) for as long as it stays
/// mounted with that key. On error the widget renders an empty box — no
/// panic, but a `tracing::warn!` so the failure is observable.
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
    ///
    /// `background` is the UI runtime's background lanes, for a provider
    /// whose loader runs its own work on them; [`Image`](crate::Image) passes
    /// `None` on a host without them, and a caller outside a widget passes
    /// what it has.
    fn resolve_async(
        &self,
        background: Option<&Background>,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let _ = background;
        Box::pin(std::future::ready(self.resolve()))
    }

    /// The decoded image available without leaving the calling thread, if
    /// any: the probe [`Image`](crate::Image) makes before starting an async
    /// load, so a hit renders in the same frame with no placeholder.
    ///
    /// Must not block. The default `None` always starts
    /// [`resolve_async`](Self::resolve_async).
    fn cached(&self) -> Option<PixelImage> {
        None
    }

    /// The subscription identity for asynchronous resolution, or `None` to
    /// stay on the synchronous-only path.
    ///
    /// With the default `None`, [`Image`](crate::Image) never probes
    /// [`cached`](Self::cached), never starts a load, and calls
    /// [`resolve`](Self::resolve) on every build. A provider whose source
    /// costs a read or a decode returns a key, and `Image` resolves it once
    /// per key, through [`decode_job`](Self::decode_job) or else
    /// [`resolve_async`](Self::resolve_async).
    fn cache_key(&self) -> Option<ImageCacheKey> {
        None
    }

    /// The work that resolves this provider off the owner thread, run through
    /// the UI runtime's background lanes.
    ///
    /// Asked once per subscription, before [`resolve_async`](Self::resolve_async),
    /// and only when [`cache_key`](Self::cache_key) is `Some`. The default
    /// `None` leaves resolution to `resolve_async`.
    ///
    /// On a host without background lanes `Image` runs the job on the owner
    /// thread instead, so the image still shows (at the cost of a frame).
    fn decode_job(&self) -> Option<DecodeJob> {
        None
    }
}

/// The result of one step of a [`DecodeJob`].
type StepResult<T> = Result<T, ImageProviderError>;

/// Work that turns a provider's source into a decoded image, split by the
/// lane each step belongs on: blocking or awaiting reads on the IO lane,
/// decoding on the compute lane.
///
/// Returned from [`ImageProvider::decode_job`]. Its steps are chosen through
/// its constructors and stay opaque, so a new kind of step (a progressive
/// decode, a GPU upload) is a new constructor rather than a change to every
/// provider.
pub struct DecodeJob {
    steps: DecodeSteps,
}

enum DecodeSteps {
    Decode(Box<dyn FnOnce() -> StepResult<PixelImage> + Send>),
    ReadThenDecode {
        read: Pin<Box<dyn Future<Output = StepResult<Vec<u8>>> + Send>>,
        decode: Box<dyn FnOnce(Vec<u8>) -> StepResult<PixelImage> + Send>,
    },
}

impl DecodeJob {
    /// Decodes a source the provider already holds, on the compute lane.
    pub fn decode(decode: impl FnOnce() -> StepResult<PixelImage> + Send + 'static) -> Self {
        Self {
            steps: DecodeSteps::Decode(Box::new(decode)),
        }
    }

    /// Reads the source on the IO lane, then decodes what it read on the
    /// compute lane.
    ///
    /// `read` may block for as long as an IO read does (a file) or await (a
    /// socket); it never runs on the compute lane, where a blocking read
    /// would hold a decode worker.
    pub fn read_then_decode(
        read: impl Future<Output = StepResult<Vec<u8>>> + Send + 'static,
        decode: impl FnOnce(Vec<u8>) -> StepResult<PixelImage> + Send + 'static,
    ) -> Self {
        Self {
            steps: DecodeSteps::ReadThenDecode {
                read: Box::pin(read),
                decode: Box::new(decode),
            },
        }
    }

    /// The job as a future polled on the owner thread: through `background`'s
    /// lanes when there is one, else step by step on the owner thread.
    /// Dropping the future cancels the step in flight.
    pub(super) fn run(
        self,
        background: Option<Background>,
    ) -> Pin<Box<dyn Future<Output = StepResult<PixelImage>>>> {
        let Some(background) = background else {
            return Box::pin(async move {
                match self.steps {
                    DecodeSteps::Decode(decode) => decode(),
                    DecodeSteps::ReadThenDecode { read, decode } => decode(read.await?),
                }
            });
        };
        Box::pin(async move {
            match self.steps {
                DecodeSteps::Decode(decode) => background
                    .compute(decode)
                    .into_future()
                    .await
                    .map_err(ImageProviderError::Background)?,
                DecodeSteps::ReadThenDecode { read, decode } => {
                    let bytes = background
                        .io(read)
                        .into_future()
                        .await
                        .map_err(ImageProviderError::Background)??;
                    background
                        .compute(move || decode(bytes))
                        .into_future()
                        .await
                        .map_err(ImageProviderError::Background)?
                }
            }
        })
    }
}

impl std::fmt::Debug for DecodeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let steps = match self.steps {
            DecodeSteps::Decode(_) => "decode",
            DecodeSteps::ReadThenDecode { .. } => "read_then_decode",
        };
        f.debug_struct("DecodeJob").field("steps", &steps).finish()
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

    /// A [`DecodeJob`] step ended without an answer: its lane refused it, it
    /// panicked, or the UI runtime ended first.
    #[error("image decode did not finish: {0}")]
    Background(flui_view::background::BackgroundError),
}

impl ImageProviderError {
    /// The variant name, with no interpolated path or reason.
    ///
    /// `Display` deliberately names the file that failed, because that is what
    /// a developer reading an error needs. That makes it unsuitable for a
    /// `tracing` field: an image path is usually a document the user chose, and
    /// a field is world-readable in Apple's unified log and in logcat. Log this
    /// instead, and let the path reach the developer through the error value.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::DecoderUnavailable => "decoder-unavailable",
            Self::DecodeFailed { .. } => "decode-failed",
            Self::FileNotFound { .. } => "file-not-found",
            Self::ReadFailed { .. } => "read-failed",
            Self::RequiresAsyncResolve { .. } => "requires-async-resolve",
            Self::SourceNotFound { .. } => "source-not-found",
            Self::AssetLoadFailed { .. } => "asset-load-failed",
            Self::Background(_) => "background",
        }
    }
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
/// [`Image`](crate::Image) decodes it once per key on the compute lane: two
/// providers made from equal bytes share a key, so a rebuild with the same
/// bytes decodes nothing again. [`resolve`](ImageProvider::resolve) still
/// decodes synchronously, for a caller outside the widget.
///
/// Requires the `flui-widgets/images` feature. Without it, decoding answers
/// [`ImageProviderError::DecoderUnavailable`].
///
/// Prefer [`Image::memory`](crate::Image::memory) as the ergonomic
/// constructor.
#[derive(Debug, Clone)]
pub struct MemoryImage {
    /// Encoded image bytes. Stored in `Arc` so cloning the provider is O(1).
    bytes: Arc<[u8]>,
    /// The bytes' hash, taken once so a key compares in O(1) until two
    /// distinct buffers hash alike.
    hash: u64,
}

impl MemoryImage {
    /// Creates a provider that decodes `bytes`.
    ///
    /// Hashes the bytes once, so a rebuild that makes an equal provider is
    /// recognised without decoding.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        use std::hash::{Hash, Hasher};

        let bytes: Arc<[u8]> = Arc::from(bytes.into());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self {
            bytes,
            hash: hasher.finish(),
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

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::memory(Arc::clone(&self.bytes), self.hash))
    }

    fn decode_job(&self) -> Option<DecodeJob> {
        let bytes = Arc::clone(&self.bytes);
        Some(DecodeJob::decode(move || decode_bytes(&bytes)))
    }
}

/// An [`ImageProvider`] that reads and decodes a local image file.
///
/// [`Image`](crate::Image) reads the file on the IO lane and decodes it on
/// the compute lane, once per path. A file that changes on disk under the
/// same path shows its new content only once the widget's key changes.
/// [`resolve`](ImageProvider::resolve) still reads and decodes synchronously,
/// for a caller outside the widget.
///
/// Requires the `flui-widgets/images` feature. Without it, decoding answers
/// [`ImageProviderError::DecoderUnavailable`].
///
/// Prefer [`Image::file`](crate::Image::file) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct FileImage {
    path: PathBuf,
}

impl FileImage {
    /// Creates a provider that reads and decodes the image at `path`.
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
        decode_bytes(&read_file(&self.path)?)
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::file(self.path.clone()))
    }

    fn decode_job(&self) -> Option<DecodeJob> {
        let path = self.path.clone();
        Some(DecodeJob::read_then_decode(
            async move { read_file(&path) },
            |bytes| decode_bytes(&bytes),
        ))
    }
}

/// Reads `path` whole, telling a missing file from one that cannot be read.
fn read_file(path: &std::path::Path) -> Result<Vec<u8>, ImageProviderError> {
    std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ImageProviderError::FileNotFound {
                path: path.to_path_buf(),
            }
        } else {
            ImageProviderError::ReadFailed {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }
        }
    })
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
        PixelImage::try_from_rgba8(width, height, rgba.into_raw()).map_err(|e| {
            ImageProviderError::DecodeFailed {
                reason: e.to_string(),
            }
        })
    }

    #[cfg(not(feature = "images"))]
    {
        let _ = bytes;
        Err(ImageProviderError::DecoderUnavailable)
    }
}
