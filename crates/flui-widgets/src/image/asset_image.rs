//! [`AssetImage`] — an [`ImageProvider`] for one of the application's assets.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::ImageSource;
use flui_painting::paint::Image as PixelImage;
use flui_platform::assets::{AssetKey, AssetKeyError};

use super::cache_key::ImageCacheKey;
use super::provider::{ImageProvider, ImageProviderError, ImageServices};

/// An [`ImageProvider`] that reads one of the application's assets by its
/// key through the host's asset bundle (`flui_platform::assets::AssetBundleCapability`,
/// ADR-0384) and decodes it through the UI runtime's image cache, off the
/// calling thread.
///
/// The key is the asset's path from the project root as `flui.toml`'s
/// `[assets] directories` declares it (`assets/logo.png`), the same on every
/// platform. The bundle and the cache are the ones [`Image`](crate::Image)
/// acquired (ADR-0387): every `AssetImage` for one key in one UI runtime
/// shares its decode and its decoded pixels, and the runtime gives them back
/// under memory pressure.
///
/// Prefer [`Image::asset`](crate::Image::asset) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct AssetImage {
    /// The key as written, which the widget's subscription is keyed by.
    written: Arc<str>,
    /// The key the bundle reads, or why the written one names no asset.
    key: Result<AssetKey, AssetKeyError>,
}

impl AssetImage {
    /// Creates a provider that loads the asset `key` on a decoded cache miss
    /// during asynchronous resolution.
    ///
    /// A key [`AssetKey::try_new`] refuses (`../secret.png`, `/etc/passwd`)
    /// reads nothing: its load fails with
    /// [`ImageProviderError::InvalidAssetKey`], as a missing asset fails
    /// with [`SourceNotFound`](ImageProviderError::SourceNotFound).
    pub fn new(key: impl Into<String>) -> Self {
        let written: Arc<str> = Arc::from(key.into());
        Self {
            key: AssetKey::try_new(&written),
            written,
        }
    }

    /// The asset key as written.
    pub fn key(&self) -> &str {
        &self.written
    }

    /// The source for `services`' bundle, or why there is none.
    fn source(&self, services: &ImageServices) -> Result<ImageSource, ImageProviderError> {
        let key = self
            .key
            .clone()
            .map_err(|reason| ImageProviderError::InvalidAssetKey {
                key: self.written.to_string(),
                reason,
            })?;
        let bundle =
            services
                .asset_bundle()
                .ok_or_else(|| ImageProviderError::AssetBundleUnavailable {
                    key: self.written.to_string(),
                })?;
        Ok(ImageSource::asset(key, bundle))
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
    /// the same key and bundle. A hit refreshes recency without reading the
    /// asset. The cache reads and decodes on `services`' lanes, or where the
    /// future is polled without them. Without a cache it fails with
    /// [`ImageProviderError::ImageCacheUnavailable`], without a bundle with
    /// [`ImageProviderError::AssetBundleUnavailable`], and reads nothing.
    /// Failed loads are not cached and a later request can retry. Dropping
    /// the future cancels the load once no other request waits for it.
    fn resolve_async(
        &self,
        services: &ImageServices,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let Some(cache) = services.image_cache() else {
            let image = self.written.to_string();
            return Box::pin(
                async move { Err(ImageProviderError::ImageCacheUnavailable { image }) },
            );
        };
        let source = match self.source(services) {
            Ok(source) => source,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        let load = cache.load(&source, &services.load_services());
        Box::pin(async move {
            load.await
                .map_err(|error| ImageProviderError::from_load_error(&error))
        })
    }

    /// The cache's decoded image for this key; `None` without a cache, a
    /// bundle or a valid key.
    fn cached(&self, services: &ImageServices) -> Option<PixelImage> {
        services.image_cache()?.get(&self.source(services).ok()?)
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(ImageCacheKey::asset(Arc::clone(&self.written)))
    }
}
