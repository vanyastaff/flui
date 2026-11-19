//! [`ImageCache`]: the decoded images one cache keeps, and the loads that
//! fill it.

use std::num::NonZeroUsize;
use std::sync::Arc;

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};
use flui_painting::paint::Image;

mod loads;
mod ordered_slots;

pub use loads::{ImageLoad, ImageSource, LoadServices, NetworkSource};

/// How an [`ImageCache`] keeps and decodes images.
///
/// ```
/// use std::num::NonZeroUsize;
/// use flui_assets::{ImageCache, ImageCacheConfig};
///
/// let cache = ImageCache::new(
///     ImageCacheConfig::default()
///         .budget(64 * 1024 * 1024)
///         .decode_threads(NonZeroUsize::MIN),
/// );
/// # drop(cache);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCacheConfig {
    budget: usize,
    decode_threads: NonZeroUsize,
}

impl Default for ImageCacheConfig {
    fn default() -> Self {
        Self {
            budget: loads::DEFAULT_DECODED_IMAGE_BYTES,
            decode_threads: loads::default_decode_threads(),
        }
    }
}

impl ImageCacheConfig {
    /// Keep at most `bytes` of decoded pixels (`width * height * 4` per
    /// image).
    ///
    /// The least recently used images go first once the total passes it; an
    /// image larger than it is handed to its waiters and not kept, and `0`
    /// keeps none. Loads in flight are still shared. The default is 256 MiB,
    /// and 64 MiB on Android, iOS and the web, where the system ends a process
    /// for its footprint long before it runs out of address space.
    #[must_use]
    pub const fn budget(mut self, bytes: usize) -> Self {
        self.budget = bytes;
        self
    }

    /// Run at most `threads` decodes at once.
    ///
    /// Decodes run on the compute lane of the lanes each load is given, never
    /// on its IO lane. The default is half the available cores, at most four,
    /// so decoding never takes the cores the owner and raster threads need.
    #[must_use]
    pub const fn decode_threads(mut self, threads: NonZeroUsize) -> Self {
        self.decode_threads = threads;
        self
    }
}

/// The decoded images an application shows, and the loads that fill them.
///
/// A completed image is served from a byte-budgeted LRU
/// ([`ImageCacheConfig::budget`]); a request for an image already loading
/// joins that load; a cold request reads its source on the IO lane of the
/// caller's lanes and decodes it on their compute lane. The cache starts no
/// runtime or thread of its own. Two sources are the same image only within
/// one cache: each cache keeps its own decoded images.
pub struct ImageCache {
    images: Arc<loads::DecodedImages>,
}

impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("images", &self.images)
            .finish()
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new(ImageCacheConfig::default())
    }
}

impl ImageCache {
    /// An empty cache that keeps and decodes images as `config` says.
    #[must_use]
    pub fn new(config: ImageCacheConfig) -> Self {
        Self {
            images: Arc::new(loads::DecodedImages::new(
                config.budget,
                config.decode_threads,
            )),
        }
    }

    /// Loads and decodes the image file at `path` on `services`.
    ///
    /// A completed image for `path` is returned at once and made most recently
    /// used; a request for a path already loading joins that load; otherwise
    /// the file is read on the IO lane of the services' lanes and decoded on
    /// their compute lane, at most [`ImageCacheConfig::decode_threads`]
    /// decodes at a time. The decoded image is kept within
    /// [`ImageCacheConfig::budget`]; failures are not kept, so a later request
    /// retries.
    ///
    /// With no lanes, or lanes too full or unable to take the load, it runs
    /// inside the returned [`ImageLoad`], on the thread that polls it.
    ///
    /// The load is the cache's, shared by every request for `path`: dropping
    /// one [`ImageLoad`] leaves it running for the rest, and dropping the last
    /// cancels it (the file is read in chunks and stops at the next one, and a
    /// decode that has not started never runs). A load dropped unfinished by
    /// the lanes or the request it ran in is taken over by the next request
    /// still waiting, on that request's services. Loads start reading in the
    /// order they were requested, up to four per decode thread; a decode slot
    /// goes to the earliest-requested load whose bytes are ready, never held
    /// for one still fetching.
    ///
    /// # Errors
    ///
    /// The load resolves to [`AssetError::LoadFailed`](crate::AssetError::LoadFailed)
    /// if the file cannot be read, or if the load panicked or kept being
    /// dropped unfinished; to [`AssetError::DecodeFailed`](crate::AssetError::DecodeFailed)
    /// if it cannot be decoded; and to
    /// [`AssetError::LanesRefused`](crate::AssetError::LanesRefused) if the
    /// lanes are shutting down.
    pub fn load_image(&self, path: impl Into<Arc<str>>, services: LoadServices) -> ImageLoad {
        self.images.load(ImageSource::Asset(path.into()), services)
    }

    /// Fetches and decodes an image over HTTP or HTTPS with the services'
    /// HTTP client, with the same cache, lanes, sharing and cancellation as
    /// [`load_image`](Self::load_image).
    ///
    /// The request is sent from the IO lane; the client drives it, so the
    /// lane needs no async runtime of its own. Dropping the last waiter drops
    /// the request. A body over 256 MiB fails without being read whole.
    ///
    /// # Errors
    ///
    /// The load resolves to [`AssetError::HttpUnavailable`](crate::AssetError::HttpUnavailable)
    /// when the services have no HTTP client;
    /// [`AssetError::Http`](crate::AssetError::Http) when the request fails
    /// and [`AssetError::HttpStatus`](crate::AssetError::HttpStatus) when it
    /// answers a status other than 2xx; and otherwise to the errors of
    /// [`load_image`](Self::load_image).
    pub fn load_network_image(
        &self,
        url: impl Into<Arc<str>>,
        services: LoadServices,
    ) -> ImageLoad {
        self.images
            .load(services.network_source(url.into()), services)
    }

    /// The completed image this cache keeps for `source`, made most recently
    /// used: the synchronous probe a frame makes before starting a load.
    #[must_use]
    pub fn cached_image(&self, source: &ImageSource) -> Option<Image> {
        self.images.cached(source)
    }
}

/// Under memory pressure the cache gives back completed images by recency:
/// moderate pressure trims them to half of [`ImageCacheConfig::budget`],
/// critical pressure drops them all. Images already given out keep their
/// pixels, and loads in flight finish into the cache.
impl ReclaimMemory for ImageCache {
    fn reclaim(&self, level: MemoryPressureLevel) {
        self.images.reclaim(level);
    }
}
