//! Application-owned decoded-image registries.

#[cfg(feature = "images")]
use std::sync::Arc;

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};

#[cfg(feature = "images")]
mod decode;
#[cfg(feature = "images")]
mod images;
#[cfg(feature = "images")]
mod ordered_slots;
#[cfg(feature = "images")]
pub use images::{ImageLoad, ImageSource, LoadServices, NetworkSource};

/// How a registry keeps and decodes images.
#[cfg(feature = "images")]
#[derive(Debug, Clone, Copy)]
struct ImageConfig {
    /// The most decoded pixel bytes kept.
    budget: usize,
    /// The most decodes running at once.
    decode_threads: std::num::NonZeroUsize,
}

#[cfg(feature = "images")]
impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            budget: images::DEFAULT_DECODED_IMAGE_BYTES,
            decode_threads: images::default_decode_threads(),
        }
    }
}

/// An application-owned cache of decoded images and their loads in flight.
///
/// The application shares one registry through an `Arc` and passes it to image
/// widgets or calls [`load_image`](Self::load_image) directly. The registry
/// starts no runtime or thread: each load uses the [`LoadServices`] supplied by
/// its caller, or runs where its returned [`ImageLoad`] is polled.
///
/// Completed images share one byte-budgeted LRU. Requests for one source join
/// one load; failures are not retained, so a later request can recover.
#[derive(Debug)]
pub struct AssetRegistry {
    /// Decoded images and the loads in flight that fill them.
    #[cfg(feature = "images")]
    decoded_images: Arc<images::DecodedImages>,
}

impl AssetRegistry {
    /// Loads and decodes the image at `path` through this registry.
    ///
    /// A completed image is returned immediately and made most recently used;
    /// a request for a path already loading joins that load. Otherwise the
    /// file is read on the services' IO lane and decoded on their compute
    /// lane, at most [`with_decode_threads`](AssetRegistryBuilder::with_decode_threads)
    /// decodes at a time. With no lanes the work runs where the returned
    /// [`ImageLoad`] is polled.
    ///
    /// The load belongs to the registry, not one waiter. Dropping one waiter
    /// leaves it running for the rest; dropping the last cancels it. A load
    /// dropped unfinished by its lanes or request is taken over by the next
    /// waiter on that waiter's services.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::AssetError::LoadFailed`] for file or decode failures,
    /// [`crate::error::AssetError::DecodeFailed`] when a decoder panics, and
    /// [`crate::error::AssetError::LanesRefused`] when the supplied lanes are shutting down.
    #[cfg(feature = "images")]
    pub fn load_image(&self, path: impl Into<Arc<str>>, services: LoadServices) -> ImageLoad {
        self.decoded_images
            .load(ImageSource::Asset(path.into()), services)
    }

    /// Fetches and decodes an HTTP or HTTPS image through this registry.
    ///
    /// Fetching uses the caller's HTTP capability and the same cache, lanes,
    /// sharing, cancellation and handover as [`load_image`](Self::load_image).
    /// A response body over 256 MiB fails without being buffered in full.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::AssetError::HttpUnavailable`] when `services` has no HTTP
    /// client, [`crate::error::AssetError::Http`] for a failed request,
    /// [`crate::error::AssetError::HttpStatus`] for a non-2xx response, and otherwise the
    /// errors of [`load_image`](Self::load_image).
    #[cfg(feature = "images")]
    pub fn load_network_image(
        &self,
        url: impl Into<Arc<str>>,
        services: LoadServices,
    ) -> ImageLoad {
        self.decoded_images
            .load(services.network_source(url.into()), services)
    }

    /// Returns the completed image for `source`, making it most recently used.
    ///
    /// This synchronous probe lets a frame use an already decoded image before
    /// starting a load. It never performs file, network or decode work.
    #[cfg(feature = "images")]
    #[must_use]
    pub fn cached_image(&self, source: &ImageSource) -> Option<crate::Image> {
        self.decoded_images.cached(source)
    }
}

/// Gives decoded images back according to memory pressure.
///
/// Moderate pressure trims the cache to half its configured byte budget;
/// critical pressure clears it. Images already returned to callers stay alive,
/// and loads in flight retain their sharing and cancellation behavior.
impl ReclaimMemory for AssetRegistry {
    fn reclaim(&self, level: MemoryPressureLevel) {
        #[cfg(feature = "images")]
        self.decoded_images.reclaim(level);
        #[cfg(not(feature = "images"))]
        let _ = level;
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        AssetRegistryBuilder::new().build()
    }
}

/// Configures an [`AssetRegistry`].
///
/// No cache-capacity selection is required: decoded-image retention has a
/// platform-appropriate byte default, and `build` is available immediately.
///
/// ```
/// use flui_assets::AssetRegistryBuilder;
///
/// let registry = AssetRegistryBuilder::new().build();
/// ```
#[derive(Debug)]
pub struct AssetRegistryBuilder {
    #[cfg(feature = "images")]
    images: ImageConfig,
}

impl AssetRegistryBuilder {
    /// Creates a builder with the default decoded-image byte budget and decode
    /// concurrency.
    #[must_use]
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "images")]
            images: ImageConfig::default(),
        }
    }

    /// Bounds retained decoded images in RGBA8 pixel bytes
    /// (`width * height * 4` each).
    ///
    /// Least recently used images are evicted once the total exceeds `bytes`.
    /// An image larger than `bytes` is returned to its waiters but not kept;
    /// zero disables retention without disabling load sharing. The default is
    /// 256 MiB, and 64 MiB on Android, iOS and the web.
    #[cfg(feature = "images")]
    #[must_use]
    pub fn with_decoded_image_budget(mut self, bytes: usize) -> Self {
        self.images.budget = bytes;
        self
    }

    /// Bounds how many images decode at once.
    ///
    /// Decodes run on each load's compute lane, never its IO lane. The default
    /// is half the available cores, at most four.
    #[cfg(feature = "images")]
    #[must_use]
    pub fn with_decode_threads(mut self, threads: std::num::NonZeroUsize) -> Self {
        self.images.decode_threads = threads;
        self
    }

    /// Builds the registry.
    #[must_use]
    pub fn build(self) -> AssetRegistry {
        AssetRegistry {
            #[cfg(feature = "images")]
            decoded_images: Arc::new(images::DecodedImages::new(
                self.images.budget,
                self.images.decode_threads,
            )),
        }
    }
}

impl Default for AssetRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}
