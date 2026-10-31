//! [`NetworkImage`] — an [`ImageProvider`] fetched with the host's HTTP
//! capability and decoded by `flui-assets`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::{AssetRegistry, ImageSource};
use flui_painting::paint::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::provider::{ImageProvider, ImageProviderError, ImageServices};

/// An [`ImageProvider`] that fetches an image over HTTP/HTTPS with the
/// host's HTTP capability and decodes it through a `flui-assets`
/// [`AssetRegistry`], off the calling thread.
///
/// Like [`AssetImage`](super::AssetImage), the registry is an explicit
/// constructor argument, so the decoded image is kept by the registry the
/// application already owns. The request is sent from the UI runtime's IO
/// lane with the HTTP client `Image` acquired (`HttpCapability`).
/// Cached responses and pending loads are shared only by providers using the
/// same registry and URL, because client headers and policies can change the
/// response at an otherwise identical URL.
///
/// Prefer [`Image::network`](crate::Image::network) as the ergonomic
/// constructor.
#[derive(Debug, Clone)]
pub struct NetworkImage {
    registry: Arc<AssetRegistry>,
    url: Arc<str>,
}

impl NetworkImage {
    /// Creates a provider that fetches `url` through `registry` on a decoded
    /// cache miss during asynchronous resolution.
    pub fn new(registry: Arc<AssetRegistry>, url: impl Into<String>) -> Self {
        Self {
            registry,
            url: Arc::from(url.into()),
        }
    }

    /// The URL this provider fetches.
    pub fn url(&self) -> &str {
        &self.url
    }

    fn source(&self) -> ImageSource {
        ImageSource::Network(Arc::clone(&self.url))
    }
}

impl ImageProvider for NetworkImage {
    /// Returns the registry's decoded image for this URL, if any.
    ///
    /// `NetworkImage` never performs a blocking network request here — see
    /// [`AssetImage::resolve`](super::AssetImage) for the same contract on
    /// the asset-file provider.
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        self.cached()
            .ok_or(ImageProviderError::RequiresAsyncResolve {
                provider_name: "NetworkImage",
            })
    }

    /// Reuses the registry's decoded image, or shares an in-flight load for
    /// the same URL. A hit refreshes recency without HTTP traffic. The
    /// request is sent with `services`' HTTP client from their IO lane,
    /// and the decode runs on their compute lane. Without a client the
    /// load fails with `AssetError::HttpUnavailable`. Failed loads are not
    /// cached and a later request can retry. Dropping the future drops the
    /// request once no other request waits for it.
    fn resolve_async(
        &self,
        services: &ImageServices,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let load = self
            .registry
            .load_network_image(Arc::clone(&self.url), services.load_services());
        let url = Arc::clone(&self.url);
        Box::pin(async move {
            load.await
                .map_err(|error| ImageProviderError::from_asset_error(url.to_string(), error))
        })
    }

    fn cached(&self) -> Option<PixelImage> {
        self.registry.cached_image(&self.source())
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::registry(&self.registry, self.source()))
    }
}
