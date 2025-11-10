//! Asset caching system with Moka.
//!
//! Provides high-performance caching using Moka's TinyLFU eviction algorithm.
//! The cache is async-friendly and lock-free for maximum concurrency.

use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache as MokaCache;

use crate::core::Asset;
use crate::types::AssetHandle;

pub mod stats;

pub use stats::CacheStats;

/// High-performance asset cache using Moka.
///
/// This cache uses the TinyLFU admission policy which provides better hit rates
/// than traditional LRU caches. It's also completely lock-free and async-friendly.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::cache::AssetCache;
///
/// let cache = AssetCache::<ImageAsset>::new(100_000_000); // 100 MB
///
/// // Insert an asset
/// cache.insert(key, data).await;
///
/// // Get from cache
/// if let Some(handle) = cache.get(&key).await {
///     println!("Cache hit!");
/// }
/// ```
pub struct AssetCache<T: Asset> {
    /// The Moka cache instance.
    cache: MokaCache<T::Key, Arc<T::Data>>,

    /// Cache statistics.
    stats: Arc<parking_lot::RwLock<CacheStats>>,
}

impl<T: Asset> AssetCache<T> {
    /// Creates a new asset cache with the given capacity in bytes.
    ///
    /// # Arguments
    ///
    /// * `capacity_bytes` - Maximum cache size in bytes (approximate)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 100 MB cache
    /// let cache = AssetCache::<ImageAsset>::new(100 * 1024 * 1024);
    /// ```
    pub fn new(capacity_bytes: usize) -> Self {
        // Estimate capacity in number of items (rough heuristic)
        // Assume average asset is ~10KB
        let estimated_items = (capacity_bytes / 10_240).max(100);

        Self::with_config(estimated_items, Duration::from_secs(300))
    }

    /// Creates a cache with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `max_capacity` - Maximum number of items to cache
    /// * `time_to_live` - How long items stay in cache after insertion
    pub fn with_config(max_capacity: usize, time_to_live: Duration) -> Self {
        let cache = MokaCache::builder()
            .max_capacity(max_capacity as u64)
            .time_to_live(time_to_live)
            .time_to_idle(Duration::from_secs(60))
            .build();

        Self {
            cache,
            stats: Arc::new(parking_lot::RwLock::new(CacheStats::default())),
        }
    }

    /// Gets an asset from the cache.
    ///
    /// Returns `None` if the asset is not in the cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(handle) = cache.get(&key).await {
    ///     println!("Width: {}", handle.width());
    /// }
    /// ```
    pub async fn get(&self, key: &T::Key) -> Option<AssetHandle<T::Data, T::Key>> {
        let result = self.cache.get(key).await;

        // Update stats
        {
            let mut stats = self.stats.write();
            if result.is_some() {
                stats.hits += 1;
            } else {
                stats.misses += 1;
            }
        }

        result.map(|data| AssetHandle::new(data, key.clone()))
    }

    /// Inserts an asset into the cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let data = Image::new(100, 100);
    /// let handle = cache.insert(key, data).await;
    /// ```
    pub async fn insert(&self, key: T::Key, data: T::Data) -> AssetHandle<T::Data, T::Key> {
        let arc_data = Arc::new(data);
        self.cache.insert(key.clone(), arc_data.clone()).await;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.insertions += 1;
        }

        AssetHandle::new(arc_data, key)
    }

    /// Gets an asset, or inserts it if not present.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let handle = cache.get_or_insert_with(key, || async {
    ///     // Load the asset
    ///     load_image("test.png").await
    /// }).await?;
    /// ```
    pub async fn get_or_insert_with<F, Fut>(
        &self,
        key: T::Key,
        f: F,
    ) -> Result<AssetHandle<T::Data, T::Key>, T::Error>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T::Data, T::Error>>,
    {
        // Try to get from cache first
        if let Some(handle) = self.get(&key).await {
            return Ok(handle);
        }

        // Not in cache, load it
        let data = f().await?;
        Ok(self.insert(key, data).await)
    }

    /// Invalidates (removes) an asset from the cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// cache.invalidate(&key).await;
    /// ```
    pub async fn invalidate(&self, key: &T::Key) {
        self.cache.invalidate(key).await;

        let mut stats = self.stats.write();
        stats.evictions += 1;
    }

    /// Clears all assets from the cache.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// cache.clear().await;
    /// ```
    pub async fn clear(&self) {
        self.cache.invalidate_all();
        self.cache.run_pending_tasks().await;

        let mut stats = self.stats.write();
        stats.evictions = 0;
        stats.hits = 0;
        stats.misses = 0;
        stats.insertions = 0;
    }

    /// Runs any pending maintenance tasks.
    ///
    /// This is useful for tests to ensure all async operations complete.
    pub async fn sync(&self) {
        self.cache.run_pending_tasks().await;
    }

    /// Returns the number of items currently in the cache.
    pub fn len(&self) -> usize {
        self.cache.entry_count() as usize
    }

    /// Returns whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns cache statistics.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let stats = cache.stats();
    /// println!("Hit rate: {:.2}%", stats.hit_rate() * 100.0);
    /// ```
    pub fn stats(&self) -> CacheStats {
        *self.stats.read()
    }

    /// Resets cache statistics.
    pub fn reset_stats(&self) {
        let mut stats = self.stats.write();
        *stats = CacheStats::default();
    }
}

// Clone creates a new cache that shares no state
impl<T: Asset> Clone for AssetCache<T> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            stats: Arc::new(parking_lot::RwLock::new(CacheStats::default())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AssetKey;
    use std::error::Error as StdError;
    use std::fmt;

    // Test asset type
    #[derive(Debug, Clone)]
    struct TestAsset;

    #[derive(Debug, Clone, PartialEq)]
    struct TestData {
        value: i32,
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
        type Key = AssetKey;
        type Error = TestError;

        fn key(&self) -> AssetKey {
            AssetKey::new("test")
        }

        async fn load(&self) -> Result<TestData, TestError> {
            Ok(TestData { value: 42 })
        }
    }

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);
        let key = AssetKey::new("test");
        let data = TestData { value: 42 };

        // Insert
        let handle = cache.insert(key, data.clone()).await;
        assert_eq!(*handle, data);

        // Get
        let retrieved = cache.get(&key).await.unwrap();
        assert_eq!(*retrieved, data);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);
        let key = AssetKey::new("nonexistent");

        let result = cache.get(&key).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_get_or_insert_with() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);
        let key = AssetKey::new("test");

        let handle = cache
            .get_or_insert_with(key, || async { Ok(TestData { value: 99 }) })
            .await
            .unwrap();

        assert_eq!(handle.value, 99);

        // Second call should return cached value
        let handle2 = cache
            .get_or_insert_with(key, || async { Ok(TestData { value: 123 }) })
            .await
            .unwrap();

        assert_eq!(handle2.value, 99); // Original value, not 123
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);
        let key = AssetKey::new("test");
        let data = TestData { value: 42 };

        cache.insert(key, data).await;
        assert!(cache.get(&key).await.is_some());

        cache.invalidate(&key).await;
        assert!(cache.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);

        for i in 0..10 {
            let key = AssetKey::new(&format!("test{}", i));
            cache.insert(key, TestData { value: i }).await;
        }

        cache.sync().await;
        assert_eq!(cache.len(), 10);

        cache.clear().await;
        assert_eq!(cache.len(), 0);
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);
        let key = AssetKey::new("test");

        // Insert
        cache.insert(key, TestData { value: 42 }).await;

        // Hit
        cache.get(&key).await;

        // Miss
        cache.get(&AssetKey::new("nonexistent")).await;

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.insertions, 1);
        assert!(stats.hit_rate() > 0.0);
    }

    #[tokio::test]
    async fn test_cache_len_and_empty() {
        let cache = AssetCache::<TestAsset>::new(1024 * 1024);

        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.insert(AssetKey::new("test"), TestData { value: 1 }).await;
        cache.sync().await;

        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }
}
