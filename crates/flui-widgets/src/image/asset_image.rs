//! [`AssetImage`] — an [`ImageProvider`] backed by `flui-assets`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use flui_assets::AssetRegistry;
use flui_types::painting::Image as PixelImage;

use super::cache_key::ImageCacheKey;
use super::decode_cache;
use super::provider::{ImageProvider, ImageProviderError};

/// An [`ImageProvider`] that loads and decodes an asset file through a
/// `flui-assets` [`AssetRegistry`], off the calling thread.
///
/// # No global registry
///
/// [`AssetRegistry::global()`] is deliberately never used here: the registry
/// is an explicit constructor argument, so the same registry (and therefore
/// the same background runtime, byte-loader cache, and lifetime) an
/// application already manages is the one `AssetImage` loads through. Two
/// `AssetImage`s built with different registries but the same `path` are
/// still coalesced and cached together at the `flui-widgets` layer (the
/// decode cache is keyed on the path alone, per [`ImageCacheKey::Asset`]) —
/// only the underlying *load* runs against whichever registry each provider
/// was given.
///
/// Prefer [`Image::asset`](crate::Image::asset) as the ergonomic constructor.
#[derive(Debug, Clone)]
pub struct AssetImage {
    registry: Arc<AssetRegistry>,
    path: String,
}

impl AssetImage {
    /// Creates a provider that loads `path` through `registry` when resolved
    /// asynchronously.
    pub fn new(registry: Arc<AssetRegistry>, path: impl Into<String>) -> Self {
        Self {
            registry,
            path: path.into(),
        }
    }

    /// The asset path this provider loads.
    pub fn path(&self) -> &str {
        &self.path
    }

    fn cache_key_value(&self) -> ImageCacheKey {
        ImageCacheKey::Asset(self.path.clone())
    }
}

impl ImageProvider for AssetImage {
    /// Returns the decode cache's current entry for this path, if any.
    ///
    /// `AssetImage` never performs blocking I/O here — the whole point of the
    /// provider is to load off-thread. A cache miss is
    /// [`ImageProviderError::RequiresAsyncResolve`]: the caller must go
    /// through [`resolve_async`](ImageProvider::resolve_async) (which
    /// [`Image`](crate::Image) does automatically via
    /// [`cache_key`](ImageProvider::cache_key)).
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        decode_cache::cached(&self.cache_key_value()).ok_or(
            ImageProviderError::RequiresAsyncResolve {
                provider_name: "AssetImage",
            },
        )
    }

    fn resolve_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let key = self.cache_key_value();
        let registry = Arc::clone(&self.registry);
        let path = self.path.clone();

        Box::pin(decode_cache::load_coalesced(key, move || async move {
            registry
                .load_image_bridged(path.clone())
                .await
                .map_err(|error| ImageProviderError::from_asset_error(path, error))
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
    fn asset_image_cache_key_is_namespaced_by_path() {
        let registry = Arc::new(AssetRegistry::default());
        let provider = AssetImage::new(registry, "logo.png");

        assert_eq!(
            provider.cache_key(),
            Some(ImageCacheKey::Asset("logo.png".to_string())),
        );
    }

    #[test]
    fn asset_image_path_returns_the_configured_path() {
        let registry = Arc::new(AssetRegistry::default());
        let provider = AssetImage::new(registry, "textures/wall.png");

        assert_eq!(provider.path(), "textures/wall.png");
    }

    #[test]
    fn asset_image_sync_resolve_reports_requires_async_resolve_on_a_cache_miss() {
        let registry = Arc::new(AssetRegistry::default());
        // A path guaranteed to never be in the decode cache.
        let provider = AssetImage::new(registry, "flui-widgets-test-never-cached-asset-image.png");

        let result = provider.resolve();
        assert!(
            matches!(result, Err(ImageProviderError::RequiresAsyncResolve { .. })),
            "a cache miss must report RequiresAsyncResolve, not silently succeed \
             or panic; got {result:?}",
        );
    }

    /// A missing file must never be reported as a decode failure — decoding
    /// never happens because the load itself fails first. Distinguishing
    /// this honestly (not flattening every `flui-assets` error into
    /// `DecodeFailed`) is what lets a caller tell "the file doesn't exist"
    /// apart from "the file exists but isn't a valid image".
    #[tokio::test]
    async fn asset_image_missing_file_is_not_reported_as_a_decode_failure() {
        let registry = Arc::new(AssetRegistry::default());
        let missing_path = "flui-widgets-test-this-file-does-not-exist-anywhere.png";
        let provider = AssetImage::new(registry, missing_path);

        let result = provider.resolve_async().await;

        assert!(
            !matches!(result, Err(ImageProviderError::DecodeFailed { .. })),
            "a missing file never reaches a decoder, so it must never be \
             reported as DecodeFailed; got {result:?}",
        );
        match result {
            Err(ImageProviderError::AssetLoadFailed { reason }) => {
                assert!(
                    !reason.to_lowercase().contains("decode"),
                    "the honest failure reason must not claim a decode \
                     failure that never happened: {reason:?}",
                );
            }
            Err(ImageProviderError::SourceNotFound { path }) => {
                assert_eq!(path, missing_path);
            }
            other => panic!(
                "expected AssetLoadFailed or SourceNotFound for a missing file, got {other:?}",
            ),
        }
    }
}
