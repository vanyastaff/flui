//! [`AssetImage`] — an [`ImageProvider`] backed by `flui-assets`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::{ImageCache, ImageSource};
use flui_painting::paint::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::provider::{ImageProvider, ImageProviderError, ImageServices};

/// An [`ImageProvider`] that loads and decodes an asset file through a
/// `flui-assets` [`ImageCache`], off the calling thread.
///
/// # The registry is an argument
///
/// There is no process-wide registry: the registry is an explicit
/// constructor argument, so the same registry (and therefore
/// the same decoded-image cache and lifetime) an
/// application already manages is the one `AssetImage` loads through. The
/// registry keeps the decoded images: two `AssetImage`s for one path share a
/// decode only when they share a registry, and the registry gives them back
/// under memory pressure once the application registers it
/// (`AppConfig::memory_reclaim` in `flui-app`).
///
/// Prefer [`Image::asset`](crate::Image::asset) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct AssetImage {
    registry: Arc<ImageCache>,
    path: Arc<str>,
}

impl AssetImage {
    /// Creates a provider that loads `path` through `registry` on a decoded
    /// cache miss during asynchronous resolution.
    pub fn new(registry: Arc<ImageCache>, path: impl Into<String>) -> Self {
        Self {
            registry,
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
    /// Returns the registry's decoded image for this path, if any.
    ///
    /// `AssetImage` never performs blocking I/O here — the whole point of the
    /// provider is to load off-thread. A cache miss is
    /// [`ImageProviderError::RequiresAsyncResolve`]: the caller must go
    /// through [`resolve_async`](ImageProvider::resolve_async) (which
    /// [`Image`](crate::Image) does automatically via
    /// [`cache_key`](ImageProvider::cache_key)).
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        self.registry
            .get(&self.source())
            .ok_or(ImageProviderError::RequiresAsyncResolve {
                provider_name: "AssetImage",
            })
    }

    /// Reuses the registry's decoded image, or shares an in-flight load for
    /// the same path. A hit refreshes recency without reading the source.
    /// The registry reads and decodes on `services`' lanes, or where the
    /// future is polled without them. Failed loads are not cached and a later
    /// request can retry. Dropping the future cancels the load once no other
    /// request waits for it.
    fn resolve_async(
        &self,
        services: &ImageServices,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let load = self
            .registry
            .load(&self.source(), &services.load_services());
        Box::pin(async move {
            load.await
                .map_err(|error| ImageProviderError::from_load_error(&error))
        })
    }

    fn cached(&self, _services: &ImageServices) -> Option<PixelImage> {
        self.registry.get(&self.source())
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::asset(&self.registry, Arc::clone(&self.path)))
    }
}
