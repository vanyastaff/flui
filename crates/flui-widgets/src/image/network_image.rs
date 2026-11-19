//! [`NetworkImage`] — an [`ImageProvider`] fetched with the host's HTTP
//! capability and decoded by `flui-assets`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::{ImageCache, ImageSource};
use flui_painting::paint::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::provider::{ImageProvider, ImageProviderError, ImageServices};

/// An [`ImageProvider`] that fetches an image over HTTP/HTTPS with the
/// host's HTTP capability and decodes it through a `flui-assets`
/// [`ImageCache`], off the calling thread.
///
/// Like [`AssetImage`](super::AssetImage), the registry is an explicit
/// constructor argument, so the decoded image is kept by the registry the
/// application already owns. The request is sent from the UI runtime's IO
/// lane with the HTTP client `Image` acquired (`HttpCapability`).
/// Cached responses and pending loads are shared only by loads using the
/// same registry, URL and client authority, because two clients can be
/// answered different images at an otherwise identical URL.
///
/// Prefer [`Image::network`](crate::Image::network) as the ergonomic
/// constructor.
#[derive(Debug, Clone)]
pub struct NetworkImage {
    registry: Arc<ImageCache>,
    url: Arc<str>,
}

impl NetworkImage {
    /// Creates a provider that fetches `url` through `registry` on a decoded
    /// cache miss during asynchronous resolution.
    pub fn new(registry: Arc<ImageCache>, url: impl Into<String>) -> Self {
        Self {
            registry,
            url: Arc::from(url.into()),
        }
    }

    /// The URL this provider fetches.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl ImageProvider for NetworkImage {
    /// Always [`ImageProviderError::RequiresAsyncResolve`]: which decoded
    /// image is this provider's depends on the client that fetches it, and
    /// `resolve` has none. [`cached`](ImageProvider::cached) probes with
    /// one.
    ///
    /// `NetworkImage` never performs a blocking network request here — see
    /// [`AssetImage::resolve`](super::AssetImage) for the same contract on
    /// the asset-file provider.
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        Err(ImageProviderError::RequiresAsyncResolve {
            provider_name: "NetworkImage",
        })
    }

    /// Reuses the cache's decoded image, or shares an in-flight load for
    /// the same URL and client authority. A hit refreshes recency without
    /// HTTP traffic. The request is sent with `services`' HTTP client from
    /// their IO lane, and the decode runs on their compute lane. Without a
    /// client it fails with [`ImageProviderError::HttpUnavailable`] and
    /// sends nothing. Failed loads are not cached and a later request can
    /// retry. Dropping the future drops the request once no other request
    /// waits for it.
    fn resolve_async(
        &self,
        services: &ImageServices,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let Some(http) = services.http() else {
            let url = self.url.to_string();
            return Box::pin(async move { Err(ImageProviderError::HttpUnavailable { url }) });
        };
        let load = self.registry.load(
            &ImageSource::network(Arc::clone(&self.url), http),
            &services.load_services(),
        );
        Box::pin(async move {
            load.await
                .map_err(|error| ImageProviderError::from_load_error(&error))
        })
    }

    /// The cache's decoded image for this URL as `services`' client
    /// fetched it; `None` without a client.
    fn cached(&self, services: &ImageServices) -> Option<PixelImage> {
        let http = services.http()?;
        self.registry
            .get(&ImageSource::network(Arc::clone(&self.url), http))
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::network(
            &self.registry,
            Arc::clone(&self.url),
        ))
    }
}
