//! [`AssetImage`] — an [`ImageProvider`] backed by `flui-assets`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::{AssetRegistry, ImageSource};
use flui_painting::paint::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::provider::{ImageProvider, ImageProviderError, ImageServices};

/// An [`ImageProvider`] that loads and decodes an asset file through a
/// `flui-assets` [`AssetRegistry`], off the calling thread.
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
/// (`AppConfig::with_memory_reclaim` in `flui-app`).
///
/// Prefer [`Image::asset`](crate::Image::asset) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct AssetImage {
    registry: Arc<AssetRegistry>,
    path: Arc<str>,
}

impl AssetImage {
    /// Creates a provider that loads `path` through `registry` on a decoded
    /// cache miss during asynchronous resolution.
    pub fn new(registry: Arc<AssetRegistry>, path: impl Into<String>) -> Self {
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
        ImageSource::Asset(Arc::clone(&self.path))
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
            .cached_image(&self.source())
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
            .load_image(Arc::clone(&self.path), services.load_services());
        let path = Arc::clone(&self.path);
        Box::pin(async move {
            load.await
                .map_err(|error| ImageProviderError::from_asset_error(path.to_string(), error))
        })
    }

    fn cached(&self, _services: &ImageServices) -> Option<PixelImage> {
        self.registry.cached_image(&self.source())
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::asset(&self.registry, Arc::clone(&self.path)))
    }
}
