//! Asset registry for central asset management.
//!
//! The registry provides a centralized system for loading and caching assets.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::cache::AssetCache;
use crate::core::Asset;
use crate::error::{AssetError, Result};
use crate::types::AssetHandle;

/// Asset registry for central asset management.
///
/// The registry manages caches for different asset types and provides
/// a unified API for loading assets.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::{AssetRegistry, ImageAsset};
///
/// // Get the global registry
/// let registry = AssetRegistry::global();
///
/// // Load an image
/// let image = ImageAsset::file("logo.png");
/// let handle = registry.load(image).await?;
///
/// println!("Loaded: {}x{}", handle.width(), handle.height());
/// ```
pub struct AssetRegistry {
    /// Type-erased caches for different asset types.
    /// Key: TypeId of the Asset type
    /// Value: `Box<dyn Any>` containing `AssetCache<T>`
    caches: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,

    /// Default cache capacity in bytes.
    default_capacity: usize,
}

impl AssetRegistry {
    /// Returns the global asset registry instance.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistry::global();
    /// let image = registry.load(ImageAsset::file("logo.png")).await?;
    /// ```
    pub fn global() -> &'static Self {
        use once_cell::sync::Lazy;
        static REGISTRY: Lazy<AssetRegistry> = Lazy::new(|| {
            AssetRegistryBuilder::new()
                .with_capacity(100 * 1024 * 1024) // 100 MB default
                .build()
        });
        &REGISTRY
    }

    /// Creates a new empty registry with the given default capacity.
    fn new(default_capacity: usize) -> Self {
        Self {
            caches: Arc::new(RwLock::new(HashMap::new())),
            default_capacity,
        }
    }

    /// Loads an asset, using the cache if available.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let image = ImageAsset::file("logo.png");
    /// let handle = registry.load(image).await?;
    /// ```
    pub async fn load<T>(&self, asset: T) -> Result<AssetHandle<T::Data, T::Key>>
    where
        T: Asset<Error = AssetError>,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        let key = asset.key();
        let cache = self.get_or_create_cache::<T>();

        // Try to get from cache first
        if let Some(handle) = cache.get(&key).await {
            return Ok(handle);
        }

        // Not in cache, load the asset
        let data = asset.load().await?;

        // Insert into cache and return handle
        Ok(cache.insert(key, data).await)
    }

    /// Gets an asset from cache without loading.
    ///
    /// Returns `None` if the asset is not cached.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key = AssetKey::new("logo.png");
    /// if let Some(handle) = registry.get::<ImageAsset>(&key).await {
    ///     println!("Found in cache!");
    /// }
    /// ```
    pub async fn get<T>(&self, key: &T::Key) -> Option<AssetHandle<T::Data, T::Key>>
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        self.get_cache::<T>()?.get(key).await
    }

    /// Preloads an asset into the cache.
    ///
    /// This is useful for warming up the cache before assets are needed.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Preload critical assets at startup
    /// registry.preload(ImageAsset::file("logo.png")).await?;
    /// registry.preload(FontAsset::file("Roboto-Regular.ttf")).await?;
    /// ```
    pub async fn preload<T>(&self, asset: T) -> Result<()>
    where
        T: Asset<Error = AssetError>,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        self.load(asset).await?;
        Ok(())
    }

    /// Invalidates (removes) an asset from the cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key = AssetKey::new("logo.png");
    /// registry.invalidate::<ImageAsset>(&key).await;
    /// ```
    pub async fn invalidate<T>(&self, key: &T::Key)
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        if let Some(cache) = self.get_cache::<T>() {
            cache.invalidate(key).await;
        }
    }

    /// Clears all cached assets of a specific type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Clear all cached images
    /// registry.clear::<ImageAsset>().await;
    /// ```
    pub async fn clear<T>(&self)
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        if let Some(cache) = self.get_cache::<T>() {
            cache.clear().await;
        }
    }

    /// Clears all caches in the registry.
    pub async fn clear_all(&self) {
        let mut caches = self.caches.write();
        caches.clear();
    }

    /// Gets the cache for a specific asset type, if it exists.
    fn get_cache<T>(&self) -> Option<AssetCache<T>>
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        let caches = self.caches.read();
        let type_id = TypeId::of::<T>();

        caches
            .get(&type_id)
            .and_then(|any| any.downcast_ref::<AssetCache<T>>().cloned())
    }

    /// Gets or creates the cache for a specific asset type.
    fn get_or_create_cache<T>(&self) -> AssetCache<T>
    where
        T: Asset,
        T::Key: std::hash::Hash + Eq + Clone,
        T::Data: Clone,
    {
        let type_id = TypeId::of::<T>();

        // Fast path: cache already exists
        {
            let caches = self.caches.read();
            if let Some(any) = caches.get(&type_id) {
                if let Some(cache) = any.downcast_ref::<AssetCache<T>>() {
                    return cache.clone();
                }
            }
        }

        // Slow path: create new cache
        let mut caches = self.caches.write();

        // Double-check in case another thread created it
        if let Some(any) = caches.get(&type_id) {
            if let Some(cache) = any.downcast_ref::<AssetCache<T>>() {
                return cache.clone();
            }
        }

        // Create new cache
        let cache = AssetCache::<T>::new(self.default_capacity);
        caches.insert(type_id, Box::new(cache.clone()));
        cache
    }

    /// Returns statistics for all caches.
    ///
    /// Returns a map of asset type names to their cache stats.
    pub fn stats(&self) -> Vec<(String, crate::cache::CacheStats)> {
        // Note: We can't easily get type names from TypeId at runtime,
        // so this is a simplified version. In a real implementation,
        // you might want to track type names explicitly.
        vec![]
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self::new(100 * 1024 * 1024) // 100 MB
    }
}

/// Builder for constructing an asset registry.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::AssetRegistryBuilder;
///
/// let registry = AssetRegistryBuilder::new()
///     .with_capacity(200 * 1024 * 1024) // 200 MB
///     .build();
/// ```
pub struct AssetRegistryBuilder {
    default_capacity: usize,
}

impl AssetRegistryBuilder {
    /// Creates a new registry builder.
    pub fn new() -> Self {
        Self {
            default_capacity: 100 * 1024 * 1024, // 100 MB default
        }
    }

    /// Sets the default cache capacity in bytes.
    ///
    /// This capacity is used for each asset type's cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(500 * 1024 * 1024) // 500 MB
    ///     .build();
    /// ```
    pub fn with_capacity(mut self, capacity_bytes: usize) -> Self {
        self.default_capacity = capacity_bytes;
        self
    }

    /// Builds the registry.
    pub fn build(self) -> AssetRegistry {
        AssetRegistry::new(self.default_capacity)
    }
}

impl Default for AssetRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{FontAsset, ImageAsset};
    use crate::types::AssetKey;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = AssetRegistryBuilder::new()
            .with_capacity(50 * 1024 * 1024)
            .build();

        // Registry should be empty initially
        let key = AssetKey::new("test");
        assert!(registry.get::<ImageAsset>(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_registry_load_font() {
        let registry = AssetRegistry::default();

        // Create a minimal TTF font
        let ttf_bytes = vec![
            0x00, 0x01, 0x00, 0x00, // TrueType version
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let font = FontAsset::from_bytes("test.ttf", ttf_bytes);
        let handle = registry.load(font).await.unwrap();

        // Should be in cache now
        let key = AssetKey::new("test.ttf");
        assert!(registry.get::<FontAsset>(&key).await.is_some());

        // Verify font data
        assert!(handle.bytes.len() >= 10);
    }

    #[tokio::test]
    async fn test_registry_invalidate() {
        let registry = AssetRegistry::default();

        let ttf_bytes = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let font = FontAsset::from_bytes("test.ttf", ttf_bytes);

        let _handle = registry.load(font).await.unwrap();

        let key = AssetKey::new("test.ttf");
        assert!(registry.get::<FontAsset>(&key).await.is_some());

        // Invalidate
        registry.invalidate::<FontAsset>(&key).await;
        assert!(registry.get::<FontAsset>(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_registry_clear() {
        let registry = AssetRegistry::default();

        // Load multiple fonts
        for i in 0..3 {
            let ttf_bytes = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
            let font = FontAsset::from_bytes(format!("test{}.ttf", i), ttf_bytes);
            let _handle = registry.load(font).await.unwrap();
        }

        // All should be cached
        assert!(registry
            .get::<FontAsset>(&AssetKey::new("test0.ttf"))
            .await
            .is_some());
        assert!(registry
            .get::<FontAsset>(&AssetKey::new("test1.ttf"))
            .await
            .is_some());

        // Clear all FontAssets
        registry.clear::<FontAsset>().await;

        // Should all be gone
        assert!(registry
            .get::<FontAsset>(&AssetKey::new("test0.ttf"))
            .await
            .is_none());
        assert!(registry
            .get::<FontAsset>(&AssetKey::new("test1.ttf"))
            .await
            .is_none());
    }

    #[test]
    fn test_global_registry() {
        let registry1 = AssetRegistry::global();
        let registry2 = AssetRegistry::global();

        // Should be the same instance
        assert!(std::ptr::eq(registry1, registry2));
    }
}
