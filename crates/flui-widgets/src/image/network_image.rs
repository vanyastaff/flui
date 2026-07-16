//! [`NetworkImage`] — an [`ImageProvider`] backed by `flui-assets`' network
//! loader.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::AssetRegistry;
use flui_types::painting::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::decode_cache;
use super::provider::{ImageProvider, ImageProviderError};

/// An [`ImageProvider`] that fetches and decodes an image over HTTP/HTTPS
/// through a `flui-assets` [`AssetRegistry`]'s network loader, off the
/// calling thread.
///
/// Like [`AssetImage`](super::AssetImage), the registry is an explicit
/// constructor argument — never [`AssetRegistry::global()`] — so the request
/// runs on whichever background runtime and byte-loader machinery the
/// application already owns.
///
/// Prefer [`Image::network`](crate::Image::network) as the ergonomic
/// constructor.
#[derive(Debug, Clone)]
pub struct NetworkImage {
    registry: Arc<AssetRegistry>,
    url: String,
}

impl NetworkImage {
    /// Creates a provider that fetches `url` through `registry` when
    /// resolved asynchronously.
    pub fn new(registry: Arc<AssetRegistry>, url: impl Into<String>) -> Self {
        Self {
            registry,
            url: url.into(),
        }
    }

    /// The URL this provider fetches.
    pub fn url(&self) -> &str {
        &self.url
    }

    fn cache_key_value(&self) -> ImageCacheKey {
        ImageCacheKey::Network(self.url.clone())
    }
}

impl ImageProvider for NetworkImage {
    /// Returns the decode cache's current entry for this URL, if any.
    ///
    /// `NetworkImage` never performs a blocking network request here — see
    /// [`AssetImage::resolve`](super::AssetImage) for the same contract on
    /// the asset-file provider.
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        decode_cache::cached(&self.cache_key_value()).ok_or(
            ImageProviderError::RequiresAsyncResolve {
                provider_name: "NetworkImage",
            },
        )
    }

    fn resolve_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let key = self.cache_key_value();
        let registry = Arc::clone(&self.registry);
        let url = self.url.clone();

        Box::pin(decode_cache::load_coalesced(key, move || async move {
            registry
                .load_network_image_bridged(url)
                .await
                .map_err(|error| ImageProviderError::DecodeFailed {
                    reason: error.to_string(),
                })
        }))
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(self.cache_key_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_image_cache_key_is_namespaced_by_url() {
        let registry = Arc::new(AssetRegistry::default());
        let provider = NetworkImage::new(registry, "https://example.com/img.png");

        assert_eq!(
            provider.cache_key(),
            Some(ImageCacheKey::Network(
                "https://example.com/img.png".to_string()
            )),
        );
    }

    #[test]
    fn network_image_url_returns_the_configured_url() {
        let registry = Arc::new(AssetRegistry::default());
        let provider = NetworkImage::new(registry, "https://example.com/a.png");

        assert_eq!(provider.url(), "https://example.com/a.png");
    }

    #[test]
    fn network_image_sync_resolve_reports_requires_async_resolve_on_a_cache_miss() {
        let registry = Arc::new(AssetRegistry::default());
        let provider = NetworkImage::new(
            registry,
            "https://example.com/flui-widgets-test-never-cached.png",
        );

        let result = provider.resolve();
        assert!(
            matches!(result, Err(ImageProviderError::RequiresAsyncResolve { .. })),
            "a cache miss must report RequiresAsyncResolve, not silently succeed \
             or panic; got {result:?}",
        );
    }

    #[test]
    fn asset_and_network_keys_for_the_same_text_are_distinct() {
        assert_ne!(
            ImageCacheKey::Asset("shared.png".to_string()),
            ImageCacheKey::Network("shared.png".to_string()),
        );
    }
}
