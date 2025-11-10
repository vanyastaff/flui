//! Asset loader trait definition.

use std::future::Future;

use crate::core::Asset;

/// Trait for loading assets from different sources.
///
/// Loaders abstract over different asset sources like filesystems, networks,
/// memory, or asset bundles. Each loader can provide assets of any type that
/// implements the `Asset` trait.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::core::{Asset, AssetLoader};
///
/// struct FileLoader {
///     base_path: PathBuf,
/// }
///
/// impl<T: Asset> AssetLoader<T> for FileLoader {
///     async fn load(&self, key: &T::Key) -> Result<T::Data, T::Error> {
///         // Load from filesystem
///         todo!()
///     }
/// }
/// ```
pub trait AssetLoader<T: Asset>: Send + Sync {
    /// Load an asset by its key.
    ///
    /// This method should perform all necessary I/O and decoding operations
    /// to produce the asset data.
    ///
    /// # Errors
    ///
    /// Returns an error if the asset cannot be loaded or decoded.
    fn load(&self, key: &T::Key) -> impl Future<Output = Result<T::Data, T::Error>> + Send;

    /// Check if an asset exists without loading it.
    ///
    /// This is useful for validation or preloading logic. The default
    /// implementation returns `true` (optimistic).
    ///
    /// # Errors
    ///
    /// Returns an error if the existence check fails.
    fn exists(&self, _key: &T::Key) -> impl Future<Output = Result<bool, T::Error>> + Send {
        async { Ok(true) }
    }

    /// Get metadata for an asset without loading it.
    ///
    /// The default implementation returns `None`.
    fn metadata(
        &self,
        _key: &T::Key,
    ) -> impl Future<Output = Result<Option<crate::core::AssetMetadata>, T::Error>> + Send {
        async { Ok(None) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Asset;
    use std::error::Error as StdError;
    use std::fmt;

    // Test types
    #[derive(Debug)]
    struct TestAsset;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct TestKey(String);

    #[derive(Debug)]
    struct TestData(String);

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
            TestKey("test".to_string())
        }

        async fn load(&self) -> Result<Self::Data, Self::Error> {
            Ok(TestData("data".to_string()))
        }
    }

    // Test loader
    struct TestLoader;

    impl AssetLoader<TestAsset> for TestLoader {
        async fn load(&self, key: &TestKey) -> Result<TestData, TestError> {
            Ok(TestData(format!("loaded: {}", key.0)))
        }
    }

    #[tokio::test]
    async fn test_loader_load() {
        let loader = TestLoader;
        let key = TestKey("test123".to_string());

        let data = loader.load(&key).await.unwrap();
        assert_eq!(data.0, "loaded: test123");
    }

    #[tokio::test]
    async fn test_loader_exists_default() {
        let loader = TestLoader;
        let key = TestKey("test".to_string());

        // Default implementation should return true
        let exists = loader.exists(&key).await.unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn test_loader_metadata_default() {
        let loader = TestLoader;
        let key = TestKey("test".to_string());

        // Default implementation should return None
        let metadata = loader.metadata(&key).await.unwrap();
        assert!(metadata.is_none());
    }
}
