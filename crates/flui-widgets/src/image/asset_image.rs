//! [`AssetImage`] — an [`ImageProvider`] backed by `flui-assets`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::ImageSource;
use flui_painting::paint::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::provider::{ImageProvider, ImageProviderError, ImageServices};

/// An [`ImageProvider`] that loads and decodes an asset file through the UI
/// runtime's `flui-assets` image cache, off the calling thread.
///
/// The cache is the one [`Image`](crate::Image) acquired
/// (`flui_assets::ImageCacheCapability`, ADR-0387): every `AssetImage` for
/// one path in one UI runtime shares its decode and its decoded pixels, and
/// the runtime gives them back under memory pressure.
///
/// Prefer [`Image::asset`](crate::Image::asset) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct AssetImage {
    path: Arc<str>,
}

impl AssetImage {
    /// Creates a provider that loads `path` through the UI runtime's image
    /// cache on a decoded cache miss during asynchronous resolution.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: Arc::from(path.into()),
        }
    }

    /// The asset path this provider loads.
    pub fn path(&self) -> &str {
        &self.path
    }

    fn source(&self) -> ImageSource {
        ImageSource::asset(Arc::clone(&self.path))
    }
}

impl ImageProvider for AssetImage {
    /// Always [`ImageProviderError::RequiresAsyncResolve`]: the decoded image
    /// is kept by the cache [`Image`](crate::Image) acquired, and `resolve`
    /// has none. [`cached`](ImageProvider::cached) probes with it.
    ///
    /// `AssetImage` never performs blocking I/O here — the whole point of the
    /// provider is to load off-thread.
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        Err(ImageProviderError::RequiresAsyncResolve {
            provider_name: "AssetImage",
        })
    }

    /// Reuses the cache's decoded image, or shares an in-flight load for
    /// the same path. A hit refreshes recency without reading the source.
    /// The cache reads and decodes on `services`' lanes, or where the
    /// future is polled without them. Without a cache it fails with
    /// [`ImageProviderError::ImageCacheUnavailable`] and reads nothing.
    /// Failed loads are not cached and a later request can retry. Dropping
    /// the future cancels the load once no other request waits for it.
    fn resolve_async(
        &self,
        services: &ImageServices,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let Some(cache) = services.image_cache() else {
            let image = self.path.to_string();
            return Box::pin(
                async move { Err(ImageProviderError::ImageCacheUnavailable { image }) },
            );
        };
        let load = cache.load(&self.source(), &services.load_services());
        Box::pin(async move {
            load.await
                .map_err(|error| ImageProviderError::from_load_error(&error))
        })
    }

    /// The cache's decoded image for this path; `None` without a cache.
    fn cached(&self, services: &ImageServices) -> Option<PixelImage> {
        services.image_cache()?.get(&self.source())
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::asset(Arc::clone(&self.path)))
    }
}
