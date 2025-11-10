//! Layout caching system
//!
//! LRU cache for layout results with statistics tracking

use flui_types::constraints::BoxConstraints;
use flui_types::Size;
use moka::sync::Cache;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Layout cache key
///
/// Uniquely identifies a layout computation based on:
/// - Element ID
/// - Constraints
/// - Optionally, child count (for multi-child layouts)
#[derive(Debug, Clone, Copy)]
pub struct LayoutCacheKey {
    element_id: crate::ElementId,
    constraints: BoxConstraints,
    child_count: Option<usize>,
}

impl PartialEq for LayoutCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.element_id == other.element_id
            && self.child_count == other.child_count
            && self.constraints.min_width == other.constraints.min_width
            && self.constraints.max_width == other.constraints.max_width
            && self.constraints.min_height == other.constraints.min_height
            && self.constraints.max_height == other.constraints.max_height
    }
}

impl Eq for LayoutCacheKey {}

impl std::hash::Hash for LayoutCacheKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.element_id.hash(state);
        self.child_count.hash(state);

        // Hash constraints using ordered float bits
        self.constraints.min_width.to_bits().hash(state);
        self.constraints.max_width.to_bits().hash(state);
        self.constraints.min_height.to_bits().hash(state);
        self.constraints.max_height.to_bits().hash(state);
    }
}

impl LayoutCacheKey {
    /// Create a new cache key
    pub fn new(element_id: crate::ElementId, constraints: BoxConstraints) -> Self {
        Self {
            element_id,
            constraints,
            child_count: None,
        }
    }

    /// Add child count to the key (for multi-child layouts)
    pub fn with_child_count(mut self, count: usize) -> Self {
        self.child_count = Some(count);
        self
    }
}

/// Layout result stored in cache
#[derive(Debug, Clone, Copy)]
pub struct LayoutResult {
    /// Computed size
    pub size: Size,

    /// Whether layout needs to be recomputed
    pub needs_layout: bool,
}

impl LayoutResult {
    /// Create a new layout result
    pub fn new(size: Size) -> Self {
        Self {
            size,
            needs_layout: false,
        }
    }
}

/// Layout cache with statistics tracking
///
/// Uses moka for thread-safe LRU caching with TTL, plus lock-free
/// statistics tracking for cache hits and misses.
///
/// # Performance
///
/// - Cache operations: O(1) amortized (LRU + hash map)
/// - Statistics: Lock-free atomic operations
/// - Max capacity: 10,000 entries
/// - TTL: 60 seconds
pub struct LayoutCache {
    cache: Cache<LayoutCacheKey, LayoutResult>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl LayoutCache {
    /// Create a new layout cache
    fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(60))
                .build(),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Get a layout result from cache
    ///
    /// Tracks cache hits and misses for statistics.
    pub fn get(&self, key: &LayoutCacheKey) -> Option<LayoutResult> {
        let result = self.cache.get(key);
        if result.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// Insert a layout result into cache
    pub fn insert(&self, key: LayoutCacheKey, value: LayoutResult) {
        self.cache.insert(key, value);
    }

    /// Invalidate a specific cache entry
    pub fn invalidate(&self, key: &LayoutCacheKey) {
        self.cache.invalidate(key);
    }

    /// Clear all cache entries
    pub fn clear(&self) {
        self.cache.invalidate_all();
    }

    /// Get number of entries in cache
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }

    /// Get detailed cache statistics
    ///
    /// Returns (hits, misses, total_requests, hit_rate_percent)
    pub fn detailed_stats(&self) -> (u64, u64, u64, f64) {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            (hits as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        (hits, misses, total, hit_rate)
    }

    /// Print cache statistics to stderr
    ///
    /// Useful for debugging and performance analysis.
    pub fn print_stats(&self) {
        let (hits, misses, total, hit_rate) = self.detailed_stats();
        tracing::info!(
            entries = self.entry_count(),
            total = total,
            hits = hits,
            misses = misses,
            hit_rate = format_args!("{:.1}%", hit_rate),
            "LayoutCache Statistics"
        );
    }

    /// Reset statistics counters
    ///
    /// Useful for benchmarking specific code sections.
    pub fn reset_stats(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for LayoutCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (hits, misses, total, hit_rate) = self.detailed_stats();
        f.debug_struct("LayoutCache")
            .field("entries", &self.entry_count())
            .field("hits", &hits)
            .field("misses", &misses)
            .field("total_requests", &total)
            .field("hit_rate", &format!("{:.1}%", hit_rate))
            .finish()
    }
}

/// Get the global layout cache
pub fn layout_cache() -> &'static LayoutCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<LayoutCache> = OnceLock::new();

    CACHE.get_or_init(LayoutCache::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementId;

    #[test]
    fn test_cache_key() {
        let key1 = LayoutCacheKey::new(ElementId::new(0), BoxConstraints::tight(Size::ZERO));
        let key2 = LayoutCacheKey::new(ElementId::new(0), BoxConstraints::tight(Size::ZERO));

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_with_child_count() {
        let key1 = LayoutCacheKey::new(ElementId::new(0), BoxConstraints::tight(Size::ZERO));
        let key2 = key1.with_child_count(5);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_operations() {
        let cache = layout_cache();
        cache.reset_stats(); // Clear any previous stats

        let key = LayoutCacheKey::new(ElementId::new(42), BoxConstraints::tight(Size::new(100.0, 100.0)));
        let result = LayoutResult::new(Size::new(50.0, 50.0));

        cache.insert(key, result);

        let cached = cache.get(&key).unwrap();
        assert_eq!(cached.size, Size::new(50.0, 50.0));
    }

    #[test]
    fn test_cache_statistics() {
        let cache = LayoutCache::new();

        // Initially no requests
        let (hits, misses, total, hit_rate) = cache.detailed_stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
        assert_eq!(total, 0);
        assert_eq!(hit_rate, 0.0);

        // Insert and get (hit)
        let key = LayoutCacheKey::new(ElementId::new(1), BoxConstraints::tight(Size::ZERO));
        cache.insert(key, LayoutResult::new(Size::new(10.0, 10.0)));

        assert!(cache.get(&key).is_some());
        let (hits, misses, total, hit_rate) = cache.detailed_stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 0);
        assert_eq!(total, 1);
        assert_eq!(hit_rate, 100.0);

        // Get non-existent (miss)
        let key2 = LayoutCacheKey::new(ElementId::new(2), BoxConstraints::tight(Size::ZERO));
        assert!(cache.get(&key2).is_none());

        let (hits, misses, total, hit_rate) = cache.detailed_stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
        assert_eq!(total, 2);
        assert_eq!(hit_rate, 50.0);
    }

    #[test]
    fn test_cache_reset_stats() {
        let cache = LayoutCache::new();

        let key = LayoutCacheKey::new(ElementId::new(1), BoxConstraints::tight(Size::ZERO));
        cache.insert(key, LayoutResult::new(Size::ZERO));
        cache.get(&key);

        let (hits, _, _, _) = cache.detailed_stats();
        assert_eq!(hits, 1);

        cache.reset_stats();

        let (hits, misses, total, hit_rate) = cache.detailed_stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
        assert_eq!(total, 0);
        assert_eq!(hit_rate, 0.0);
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = LayoutCache::new();

        let key = LayoutCacheKey::new(ElementId::new(1), BoxConstraints::tight(Size::ZERO));
        cache.insert(key, LayoutResult::new(Size::ZERO));

        assert!(cache.get(&key).is_some());

        cache.invalidate(&key);

        assert!(cache.get(&key).is_none());

        // Should count as miss after invalidation
        let (hits, misses, _, _) = cache.detailed_stats();
        assert_eq!(hits, 1); // First get
        assert_eq!(misses, 1); // Second get after invalidate
    }

    #[test]
    fn test_cache_clear() {
        let cache = LayoutCache::new();

        let key1 = LayoutCacheKey::new(ElementId::new(1), BoxConstraints::tight(Size::ZERO));
        let key2 = LayoutCacheKey::new(ElementId::new(2), BoxConstraints::tight(Size::ZERO));

        cache.insert(key1, LayoutResult::new(Size::ZERO));
        cache.insert(key2, LayoutResult::new(Size::ZERO));

        assert_eq!(cache.entry_count(), 2);

        cache.clear();

        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_cache_debug_format() {
        let cache = LayoutCache::new();
        cache.reset_stats();

        let key = LayoutCacheKey::new(ElementId::new(1), BoxConstraints::tight(Size::ZERO));
        cache.insert(key, LayoutResult::new(Size::ZERO));
        cache.get(&key);

        let debug_str = format!("{:?}", cache);

        // Should contain statistics
        assert!(debug_str.contains("LayoutCache"));
        assert!(debug_str.contains("hits"));
        assert!(debug_str.contains("misses"));
        assert!(debug_str.contains("hit_rate"));
    }
}
