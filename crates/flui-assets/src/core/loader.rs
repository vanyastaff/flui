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
