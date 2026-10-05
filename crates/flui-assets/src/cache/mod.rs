//! Asset caching system with Moka.
//!
//! Provides high-performance caching using Moka's TinyLFU eviction algorithm.
//! Moka provides concurrent async operations; statistics use a short lock.

use std::sync::Arc;

use moka::future::Cache as MokaCache;

use crate::core::Asset;
use crate::types::AssetHandle;

mod config;
mod initialization;
pub mod stats;

pub use config::{AssetCacheConfig, CacheCapacity, CacheExpiration, ExpirationTooLong};
pub use stats::CacheStats;

/// High-performance asset cache using Moka.
///
/// This cache uses the TinyLFU admission policy which provides better hit rates
/// than traditional LRU caches for frequency-biased workloads. Moka's admission
/// and maintenance are concurrent and eventually consistent.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::cache::AssetCache;
///
/// let cache = AssetCache::<ImageAsset>::new(flui_assets::CacheCapacity::default()); // entry count
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
    initializers: Arc<initialization::Initializers<T::Key>>,
}

impl<T: Asset> std::fmt::Debug for AssetCache<T>
where
    T::Key: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetCache")
            .field("entry_count", &self.len())
            .field("stats", &self.stats())
            .finish()
    }
}

impl<T: Asset> AssetCache<T> {
    /// Creates a count-bounded cache with default expiration.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Completed-entry retention; it does not bound decoded bytes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // entry count cache
    /// let cache = AssetCache::<ImageAsset>::new(flui_assets::CacheCapacity::default());
    /// ```
    pub fn new(capacity: CacheCapacity) -> Self {
        Self::with_config(AssetCacheConfig {
            capacity,
            ..AssetCacheConfig::default()
        })
    }

    /// Creates a cache with custom configuration.
    ///
    pub fn with_config(config: AssetCacheConfig) -> Self {
        let mut builder = MokaCache::builder().max_capacity(config.capacity.limit());
        if let Some(duration) = config.time_to_live.duration() {
            builder = builder.time_to_live(duration);
        }
        if let Some(duration) = config.time_to_idle.duration() {
            builder = builder.time_to_idle(duration);
        }
        let cache = builder.build();

        Self {
            cache,
            stats: Arc::new(parking_lot::RwLock::new(CacheStats::default())),
            initializers: Arc::new(initialization::Initializers::new()),
        }
    }

    /// Observes presence without cloning data, recording requests, promoting
    /// popularity, or refreshing idle expiration. Concurrent mutation may
    /// change the result immediately after this observation.
    pub fn contains(&self, key: &T::Key) -> bool {
        self.cache.contains_key(key)
    }

    /// Configured completed-entry retention policy.
    pub fn capacity(&self) -> CacheCapacity {
        let limit = self
            .cache
            .policy()
            .max_capacity()
            .expect("BUG: asset caches always configure a maximum capacity");
        std::num::NonZeroU64::new(limit).map_or(CacheCapacity::Disabled, CacheCapacity::Entries)
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
                stats.hits = stats.hits.saturating_add(1);
            } else {
                stats.misses = stats.misses.saturating_add(1);
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
            stats.insertions = stats.insertions.saturating_add(1);
        }

        AssetHandle::new(arc_data, key)
    }

    /// Gets an asset, or coalesces concurrent cold initializers for its key.
    ///
    /// Waiters share one allocation or one `Arc` error, without requiring the
    /// error to implement `Clone`. Errors are not cached. Moka allows a waiter
    /// to restart after the elected initializer is cancelled or panics.
    ///
    /// A same-key call polled inside this cache's initializer (including through
    /// a clone, and after suspension) runs its own initializer independently.
    /// It returns an uncached handle; the outer initializer remains responsible
    /// for publication. This avoids waiting on its own in-flight entry. Other
    /// keys and independent requests still use Moka. Dependency cycles through
    /// separately spawned tasks are not detected by poll-scoped ancestry.
    ///
    /// Hit/miss counters describe the initial presence observation; concurrent
    /// changes may race that probe. Fresh returned entries count as completed
    /// insertions. Cancellation after backend publication can leave an entry
    /// without a completed insertion count.
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
    ) -> Result<AssetHandle<T::Data, T::Key>, Arc<T::Error>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T::Data, T::Error>>,
    {
        let present = self.contains(&key);
        {
            let mut stats = self.stats.write();
            if present {
                stats.hits = stats.hits.saturating_add(1);
            } else {
                stats.misses = stats.misses.saturating_add(1);
            }
        }
        if self.initializers.is_reentrant(&key) {
            if let Some(data) = self.cache.get(&key).await {
                return Ok(AssetHandle::new(data, key));
            }
            let data = f().await.map(Arc::new).map_err(Arc::new)?;
            return Ok(AssetHandle::new(data, key));
        }
        let initializer_key = Arc::new(key.clone());
        let entry = self
            .cache
            .entry_by_ref(&key)
            .or_try_insert_with(
                self.initializers
                    .run(&initializer_key, async { f().await.map(Arc::new) }),
            )
            .await?;
        if entry.is_fresh() {
            let mut stats = self.stats.write();
            stats.insertions = stats.insertions.saturating_add(1);
        }
        Ok(AssetHandle::new(entry.into_value(), key))
    }

    /// Invalidates a completed entry. This does not cancel an in-flight
    /// initializer, which may insert after invalidation.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// cache.invalidate(&key).await;
    /// ```
    pub async fn invalidate(&self, key: &T::Key) {
        self.cache.invalidate(key).await;

        let mut stats = self.stats.write();
        stats.invalidations = stats.invalidations.saturating_add(1);
    }

    /// Invalidates completed entries and resets operation counters after
    /// pending maintenance. In-flight initializers may insert afterward;
    /// this is not a generation barrier or immediate physical deallocation.
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
        stats.invalidations = 0;
        stats.hits = 0;
        stats.misses = 0;
        stats.insertions = 0;
    }

    /// Runs any pending maintenance tasks.
    ///
    /// This improves estimated counts after quiescence. Concurrent operations
    /// can continue; this is not a snapshot or physical deallocation barrier.
    pub async fn sync(&self) {
        self.cache.run_pending_tasks().await;
    }

    /// Returns Moka's eventually consistent estimate of the entry count.
    pub fn len(&self) -> u64 {
        self.cache.entry_count()
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

// ===== Extension Traits Pattern =====

/// Sealed trait module to prevent external implementations.
#[doc(hidden)]
pub mod sealed {
    use super::{Asset, AssetCache};

    /// Sealed trait to prevent external implementations of AssetCacheCore.
    pub trait Sealed {}

    impl<T: Asset> Sealed for AssetCache<T> {}
    impl<T: Asset> Sealed for &AssetCache<T> {}
    impl<T: Asset> Sealed for &mut AssetCache<T> {}
}

/// Core AssetCache API providing fundamental operations.
///
/// This trait is sealed to prevent external implementations, allowing
/// the API to evolve without breaking changes.
pub trait AssetCacheCore<T: Asset>: sealed::Sealed {
    /// Observes presence without recording a retrieving cache read.
    fn contains(&self, key: &T::Key) -> bool;

    /// Configured completed-entry retention policy.
    fn capacity(&self) -> CacheCapacity;
    /// Gets an asset from the cache.
    fn get(
        &self,
        key: &T::Key,
    ) -> impl std::future::Future<Output = Option<AssetHandle<T::Data, T::Key>>> + Send;

    /// Inserts an asset into the cache.
    fn insert(
        &self,
        key: T::Key,
        data: T::Data,
    ) -> impl std::future::Future<Output = AssetHandle<T::Data, T::Key>> + Send;

    /// Returns cache statistics.
    fn stats(&self) -> CacheStats;

    /// Returns the number of items in the cache.
    fn len(&self) -> u64;

    /// Returns whether the cache is empty.
    fn is_empty(&self) -> bool;
}

impl<T: Asset> AssetCacheCore<T> for AssetCache<T> {
    fn contains(&self, key: &T::Key) -> bool {
        self.contains(key)
    }

    fn capacity(&self) -> CacheCapacity {
        self.capacity()
    }
    #[inline]
    async fn get(&self, key: &T::Key) -> Option<AssetHandle<T::Data, T::Key>> {
        self.get(key).await
    }

    #[inline]
    async fn insert(&self, key: T::Key, data: T::Data) -> AssetHandle<T::Data, T::Key> {
        self.insert(key, data).await
    }

    #[inline]
    fn stats(&self) -> CacheStats {
        self.stats()
    }

    #[inline]
    fn len(&self) -> u64 {
        self.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Extension trait providing convenient cache operations.
///
/// This trait is automatically implemented for all types that implement
/// [`AssetCacheCore`]. It provides high-level convenience methods.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::{AssetCache, AssetCacheExt};
///
/// let cache = AssetCache::<ImageAsset>::new(flui_assets::CacheCapacity::default());
///
/// // Check hit rate
/// let hit_rate = cache.hit_rate();
/// println!("Cache efficiency: {:.1}%", hit_rate * 100.0);
///
/// // Batch insert
/// cache.insert_many(vec![
///     (key1, image1),
///     (key2, image2),
/// ]).await;
///
/// // Check if cached
/// if cache.contains(&key) {
///     println!("Asset is cached!");
/// }
/// ```
pub trait AssetCacheExt<T: Asset>: AssetCacheCore<T> {
    /// Returns the cache hit rate as a fraction (0.0 - 1.0).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let hit_rate = cache.hit_rate();
    /// println!("Hit rate: {:.1}%", hit_rate * 100.0);
    /// ```
    #[inline]
    fn hit_rate(&self) -> f64 {
        self.stats().hit_rate()
    }

    /// Returns the cache miss rate as a fraction (0.0 - 1.0).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let miss_rate = cache.miss_rate();
    /// if miss_rate > 0.5 {
    ///     println!("Cache is not very effective");
    /// }
    /// ```
    #[inline]
    fn miss_rate(&self) -> f64 {
        self.stats().miss_rate()
    }

    /// Inserts assets sequentially and returns handles in input order.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// cache.insert_many(vec![
    ///     (key1, image1),
    ///     (key2, image2),
    ///     (key3, image3),
    /// ]).await;
    /// ```
    #[inline]
    fn insert_many(
        &self,
        items: Vec<(T::Key, T::Data)>,
    ) -> impl std::future::Future<Output = Vec<AssetHandle<T::Data, T::Key>>> + Send
    where
        Self: Sized + Sync,
    {
        async move {
            let mut handles = Vec::with_capacity(items.len());
            for (key, data) in items {
                handles.push(self.insert(key, data).await);
            }
            handles
        }
    }

    /// Returns the capacity utilization as a fraction (0.0 - 1.0).
    ///
    /// Uses the configured entry capacity and Moka's estimated count. Returns
    /// zero when retention is disabled and clamps concurrent maintenance lag
    /// to one. This does not measure decoded bytes or process memory.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let utilization = cache.utilization();
    /// if utilization > 0.9 {
    ///     println!("Cache is nearly full");
    /// }
    /// ```
    #[inline]
    fn utilization(&self) -> f64 {
        match self.capacity() {
            CacheCapacity::Disabled => 0.0,
            CacheCapacity::Entries(capacity) => {
                (self.len() as f64 / capacity.get() as f64).min(1.0)
            }
        }
    }

    /// Returns `true` if the cache is performing well (hit rate > 70%).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if !cache.is_efficient() {
    ///     println!("Consider increasing cache size");
    /// }
    /// ```
    #[inline]
    fn is_efficient(&self) -> bool {
        self.hit_rate() > 0.7
    }
}

// Blanket implementation for all types implementing AssetCacheCore
impl<C, T: Asset> AssetCacheExt<T> for C where C: AssetCacheCore<T> + ?Sized {}

// Clones share cached entries and their operation counters.
impl<T: Asset> Clone for AssetCache<T> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            initializers: Arc::clone(&self.initializers),
            stats: Arc::clone(&self.stats),
        }
    }
}
