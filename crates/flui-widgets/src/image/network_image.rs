//! [`NetworkImage`] — an [`ImageProvider`] backed by `flui-assets`' network
//! loader.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::AssetRegistry;
use flui_painting::paint::Image as PixelImage;

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
                .load_network_image_bridged(url.clone())
                .await
                .map_err(|error| ImageProviderError::from_asset_error(url, error))
        }))
    }

    fn cache_key(&self) -> Option<ImageCacheKey> {
        Some(self.cache_key_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused connection must never be reported as a decode failure — no
    /// bytes ever arrive for a decoder to fail on. Hermetic: binds an
    /// ephemeral loopback port and closes it immediately, so the connection
    /// attempt is refused deterministically with no external network
    /// involved.
    #[tokio::test]
    async fn network_image_connection_refused_is_not_reported_as_a_decode_failure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("binding an ephemeral port must succeed");
        let addr = listener
            .local_addr()
            .expect("a bound listener must report its local address");
        drop(listener); // nothing is listening on `addr` from here on.

        let registry = Arc::new(AssetRegistry::default());
        let provider = NetworkImage::new(registry, format!("http://{addr}/unreachable.png"));

        let result = provider.resolve_async().await;

        assert!(
            !matches!(result, Err(ImageProviderError::DecodeFailed { .. })),
            "a refused connection never reaches a decoder, so it must never \
             be reported as DecodeFailed; got {result:?}",
        );
        match result {
            Err(ImageProviderError::AssetLoadFailed { reason }) => {
                assert!(
                    !reason.to_lowercase().contains("decode"),
                    "the honest failure reason must not claim a decode \
                     failure that never happened: {reason:?}",
                );
            }
            other => panic!("expected AssetLoadFailed for a refused connection, got {other:?}"),
        }
    }
}
