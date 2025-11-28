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
    pub(crate) default_capacity: usize,
}

impl std::fmt::Debug for AssetRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetRegistry")
            .field("cache_count", &self.caches.read().len())
            .field("default_capacity", &self.default_capacity)
            .finish()
    }
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
    /// If the asset is already cached, returns the cached version immediately.
    /// Otherwise, loads the asset and adds it to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The asset cannot be loaded (`AssetError::LoadFailed`)
    /// - The asset data is invalid (`AssetError::DecodeFailed`)
    /// - The asset format is unsupported (`AssetError::UnsupportedFormat`)
    /// - Any I/O error occurs (`AssetError::Io`)
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

// ===== Type State Builder Pattern =====

/// Type-state marker: Capacity not yet set.
#[derive(Debug, Clone, Copy)]
pub struct NoCapacity;

/// Type-state marker: Capacity has been set.
#[derive(Debug, Clone, Copy)]
pub struct HasCapacity(pub(crate) usize);

/// Builder for constructing an asset registry with compile-time validation.
///
/// This builder uses the type-state pattern to ensure required configuration
/// is provided at compile-time. The `build()` method is only available after
/// capacity has been set.
///
/// # Type States
///
/// - `AssetRegistryBuilder<NoCapacity>` - Initial state, capacity must be set
/// - `AssetRegistryBuilder<HasCapacity>` - Ready to build
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::AssetRegistryBuilder;
///
/// // This compiles - capacity is set
/// let registry = AssetRegistryBuilder::new()
///     .with_capacity(200 * 1024 * 1024) // 200 MB
///     .build();
///
/// // This won't compile - capacity not set
/// // let registry = AssetRegistryBuilder::new().build(); // ❌ ERROR
/// ```
///
/// # Default Capacity
///
/// If you want a registry with default capacity, use `with_default_capacity()`:
///
/// ```rust,ignore
/// let registry = AssetRegistryBuilder::new()
///     .with_default_capacity() // 100 MB
///     .build();
/// ```
pub struct AssetRegistryBuilder<C = NoCapacity> {
    capacity: C,
}

// ===== Initial State: NoCapacity =====

impl AssetRegistryBuilder<NoCapacity> {
    /// Creates a new registry builder.
    ///
    /// You must call `with_capacity()` or `with_default_capacity()` before building.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let builder = AssetRegistryBuilder::new();
    /// // builder.build(); // ❌ Won't compile - capacity not set
    /// ```
    pub fn new() -> Self {
        Self {
            capacity: NoCapacity,
        }
    }

    /// Sets a custom cache capacity in bytes.
    ///
    /// This capacity is used for each asset type's cache.
    ///
    /// # Arguments
    ///
    /// * `capacity_bytes` - Cache capacity in bytes (must be > 0)
    ///
    /// # Panics
    ///
    /// Panics if `capacity_bytes` is 0.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(500 * 1024 * 1024) // 500 MB
    ///     .build();
    /// ```
    pub fn with_capacity(self, capacity_bytes: usize) -> AssetRegistryBuilder<HasCapacity> {
        assert!(capacity_bytes > 0, "Capacity must be greater than 0");
        AssetRegistryBuilder {
            capacity: HasCapacity(capacity_bytes),
        }
    }

    /// Sets the default cache capacity (100 MB).
    ///
    /// This is a convenience method for the common case.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_default_capacity()
    ///     .build();
    /// ```
    pub fn with_default_capacity(self) -> AssetRegistryBuilder<HasCapacity> {
        AssetRegistryBuilder {
            capacity: HasCapacity(100 * 1024 * 1024), // 100 MB
        }
    }
}

// ===== Final State: HasCapacity =====

impl AssetRegistryBuilder<HasCapacity> {
    /// Builds the asset registry.
    ///
    /// This method is only available after capacity has been set.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(200 * 1024 * 1024)
    ///     .build();
    /// ```
    pub fn build(self) -> AssetRegistry {
        AssetRegistry::new(self.capacity.0)
    }

    /// Updates the capacity after it has been set.
    ///
    /// This allows changing the capacity even after calling `with_capacity()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let registry = AssetRegistryBuilder::new()
    ///     .with_capacity(100 * 1024 * 1024)
    ///     .with_capacity(200 * 1024 * 1024) // Override previous value
    ///     .build();
    /// ```
    pub fn with_capacity(self, capacity_bytes: usize) -> AssetRegistryBuilder<HasCapacity> {
        assert!(capacity_bytes > 0, "Capacity must be greater than 0");
        AssetRegistryBuilder {
            capacity: HasCapacity(capacity_bytes),
        }
    }
}

// ===== Convenience: Default Implementation =====

impl Default for AssetRegistryBuilder<NoCapacity> {
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

    // ===== Type State Builder Tests =====

    #[test]
    fn test_builder_with_capacity() {
        let registry = AssetRegistryBuilder::new()
            .with_capacity(50 * 1024 * 1024)
            .build();

        // Registry should work correctly
        assert_eq!(registry.default_capacity, 50 * 1024 * 1024);
    }

    #[test]
    fn test_builder_with_default_capacity() {
        let registry = AssetRegistryBuilder::new()
            .with_default_capacity()
            .build();

        // Should use default capacity (100 MB)
        assert_eq!(registry.default_capacity, 100 * 1024 * 1024);
    }

    #[test]
    fn test_builder_capacity_override() {
        let registry = AssetRegistryBuilder::new()
            .with_capacity(100 * 1024 * 1024)
            .with_capacity(200 * 1024 * 1024) // Override
            .build();

        // Should use the last capacity set
        assert_eq!(registry.default_capacity, 200 * 1024 * 1024);
    }

    #[test]
    #[should_panic(expected = "Capacity must be greater than 0")]
    fn test_builder_zero_capacity_panics() {
        let _registry = AssetRegistryBuilder::new()
            .with_capacity(0) // Should panic
            .build();
    }

    #[test]
    fn test_builder_default() {
        let builder = AssetRegistryBuilder::default();
        let registry = builder.with_default_capacity().build();

        assert_eq!(registry.default_capacity, 100 * 1024 * 1024);
    }

    // This test demonstrates compile-time safety
    // Uncommenting this should cause a compile error:
    // #[test]
    // fn test_builder_without_capacity_does_not_compile() {
    //     let _registry = AssetRegistryBuilder::new().build(); // ❌ ERROR: no method `build` on NoCapacity
    // }

    #[test]
    fn test_registry_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<AssetRegistry>();
        assert_sync::<AssetRegistry>();
    }

    #[test]
    fn test_registry_debug() {
        let registry = AssetRegistry::default();
        let debug_str = format!("{:?}", registry);
        assert!(debug_str.contains("AssetRegistry"));
        assert!(debug_str.contains("cache_count"));
        assert!(debug_str.contains("default_capacity"));
    }

    #[test]
    fn test_type_state_markers_debug() {
        let no_cap = NoCapacity;
        let has_cap = HasCapacity(100);

        let debug1 = format!("{:?}", no_cap);
        let debug2 = format!("{:?}", has_cap);

        assert!(debug1.contains("NoCapacity"));
        assert!(debug2.contains("HasCapacity"));
    }
}
