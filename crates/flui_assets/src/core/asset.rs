//! Core `Asset` trait definition.

use std::future::Future;
use std::hash::Hash;

use crate::core::AssetMetadata;

/// The fundamental trait that all assets must implement.
///
/// This trait defines how an asset is identified, loaded, and validated.
/// It uses associated types to allow each asset type to define its own
/// data representation, key type, and error type.
///
/// # Type Parameters
///
/// - `Data`: The decoded asset data (e.g., `Image`, `FontData`, `AudioBuffer`)
/// - `Key`: The unique identifier type (typically `AssetKey`)
/// - `Error`: The error type for loading operations
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::core::Asset;
/// use flui_assets::{AssetKey, AssetError};
///
/// pub struct ImageAsset {
///     path: String,
/// }
///
/// impl Asset for ImageAsset {
///     type Data = Image;
///     type Key = AssetKey;
///     type Error = AssetError;
///
///     fn key(&self) -> AssetKey {
///         AssetKey::new(&self.path)
///     }
///
///     async fn load(&self) -> Result<Image, AssetError> {
///         // Load and decode image
///         todo!()
///     }
/// }
/// ```
pub trait Asset: Send + Sync + 'static {
    /// The type of data this asset produces when loaded.
    ///
    /// This must be `Send + Sync` to allow sharing across threads.
    type Data: Send + Sync;

    /// The type used to uniquely identify this asset.
    ///
    /// This is used as the cache key, so it must be hashable and comparable.
    type Key: Hash + Eq + Clone + Send + Sync;

    /// The error type for loading operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Returns the unique key for this asset.
    ///
    /// This key is used for caching and deduplication. Two assets with the same
    /// key are considered identical and will share the same cached data.
    fn key(&self) -> Self::Key;

    /// Loads and decodes the asset asynchronously.
    ///
    /// This method performs all I/O and decoding operations. The result will be
    /// cached by the asset registry, so expensive operations are only performed once.
    ///
    /// # Errors
    ///
    /// Returns an error if the asset cannot be loaded or decoded.
    fn load(&self) -> impl Future<Output = Result<Self::Data, Self::Error>> + Send;

    /// Returns optional metadata about the asset without loading it.
    ///
    /// This is useful for UI previews, progress indicators, or preloading logic.
    /// The default implementation returns `None`.
    ///
    /// # Performance
    ///
    /// Implementations should try to extract metadata without fully decoding the asset.
    /// For example, an image asset might read just the file header to get dimensions.
    fn metadata(&self) -> Option<AssetMetadata> {
        None
    }

    /// Validates the asset before loading.
    ///
    /// This is called before `load()` and can be used for early validation,
    /// such as checking file extensions, magic numbers, or size limits.
    ///
    /// The default implementation always returns `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails.
    fn validate(&self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;
    use std::fmt;

    // Test asset implementation
    #[derive(Debug)]
    struct TestAsset {
        id: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct TestKey(String);

    #[derive(Debug)]
    struct TestData {
        content: String,
    }

    #[derive(Debug)]
    struct TestError;

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "test error")
        }
    }

    impl StdError for TestError {}

    impl Asset for TestAsset {
        type Data = TestData;
        type Key = TestKey;
        type Error = TestError;

        fn key(&self) -> Self::Key {
            TestKey(self.id.clone())
        }

        async fn load(&self) -> Result<Self::Data, Self::Error> {
            Ok(TestData {
                content: format!("loaded: {}", self.id),
            })
        }
    }

    #[tokio::test]
    async fn test_asset_load() {
        let asset = TestAsset {
            id: "test123".to_string(),
        };

        let key = asset.key();
        assert_eq!(key.0, "test123");

        let data = asset.load().await.unwrap();
        assert_eq!(data.content, "loaded: test123");
    }

    #[test]
    fn test_asset_validate() {
        let asset = TestAsset {
            id: "test".to_string(),
        };

        // Default validation should pass
        assert!(asset.validate().is_ok());
    }

    #[test]
    fn test_asset_metadata() {
        let asset = TestAsset {
            id: "test".to_string(),
        };

        // Default metadata is None
        assert!(asset.metadata().is_none());
    }
}
