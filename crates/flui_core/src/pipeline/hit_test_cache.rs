//! Hit test result caching
//!
//! Caches hit test results when the tree is unchanged to avoid expensive traversals.
//!
//! # Performance Benefits
//!
//! - Avoids O(n) tree traversal when tree is stable
//! - Typical scenarios: hover states, cursor changes, tooltip positioning
//! - Expected savings: 5-15% CPU during mouse movement over static UI
//!
//! # Cache Invalidation
//!
//! Cache is invalidated whenever:
//! - Layout changes (elements move/resize)
//! - Paint changes (element visibility changes)
//! - Tree structure changes (elements added/removed)

use crate::element::{ElementHitTestResult, ElementId};
use flui_types::Offset;
use std::collections::HashMap;

/// Hit test cache key combining position and root
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    /// Quantized position (to avoid floating point precision issues)
    /// Each unit = 0.1 pixels
    x_quantized: i32,
    y_quantized: i32,
    /// Root element being tested
    root_id: ElementId,
}

impl CacheKey {
    /// Create cache key from position and root
    ///
    /// Quantizes position to avoid cache misses from floating point precision.
    /// Groups coordinates into 0.1px buckets: [0.0..0.1), [0.1..0.2), etc.
    fn new(position: Offset, root_id: ElementId) -> Self {
        // Quantize to 0.1 pixel precision by flooring
        // This groups positions into 0.1px buckets, so small jitter within
        // the same bucket hits the cache (e.g., 10.05 and 10.09 both → 100)
        let x_quantized = (position.dx * 10.0).floor() as i32;
        let y_quantized = (position.dy * 10.0).floor() as i32;

        Self {
            x_quantized,
            y_quantized,
            root_id,
        }
    }
}

/// Hit test result cache
///
/// Caches results of expensive hit test traversals.
/// Automatically invalidates when tree state changes.
#[derive(Debug, Default)]
pub struct HitTestCache {
    /// Cached results indexed by (position, root_id)
    cache: HashMap<CacheKey, ElementHitTestResult>,

    /// Tree generation counter (incremented on any tree change)
    /// When this doesn't match cached generation, cache is stale
    tree_generation: u64,

    /// Generation value stored with cached results
    cached_generation: u64,

    /// Cache statistics (debug builds only)
    #[cfg(debug_assertions)]
    stats: CacheStats,
}

#[cfg(debug_assertions)]
#[derive(Debug, Default)]
struct CacheStats {
    hits: u64,
    misses: u64,
    invalidations: u64,
}

impl HitTestCache {
    /// Create new cache
    pub fn new() -> Self {
        Self::default()
    }

    /// Get cached result if valid
    ///
    /// Returns Some(result) if cache is valid and contains entry for this position.
    /// Returns None if cache is stale or entry not found.
    pub fn get(&mut self, position: Offset, root_id: ElementId) -> Option<ElementHitTestResult> {
        // Check if cache is stale
        if self.cached_generation != self.tree_generation {
            #[cfg(debug_assertions)]
            tracing::debug!(
                "HitTestCache::get: cache stale (gen {} != {})",
                self.cached_generation,
                self.tree_generation
            );
            self.cache.clear();
            self.cached_generation = self.tree_generation;
            #[cfg(debug_assertions)]
            {
                self.stats.invalidations += 1;
            }
            return None;
        }

        let key = CacheKey::new(position, root_id);
        let result = self.cache.get(&key).cloned();

        #[cfg(debug_assertions)]
        {
            if result.is_some() {
                self.stats.hits += 1;
                tracing::trace!(
                    "HitTestCache::get: cache HIT (pos={:?}, root={:?})",
                    position,
                    root_id
                );
            } else {
                self.stats.misses += 1;
            }
        }

        result
    }

    /// Insert result into cache
    ///
    /// Stores result for future lookups at this position.
    pub fn insert(&mut self, position: Offset, root_id: ElementId, result: ElementHitTestResult) {
        let key = CacheKey::new(position, root_id);

        #[cfg(debug_assertions)]
        let count = result.entries().len();

        self.cache.insert(key, result);

        #[cfg(debug_assertions)]
        tracing::trace!(
            "HitTestCache::insert: cached result (pos={:?}, root={:?}, count={})",
            position,
            root_id,
            count
        );
    }

    /// Invalidate cache when tree changes
    ///
    /// Call this whenever layout, paint, or tree structure changes.
    /// This increments the generation counter, marking all cached entries as stale.
    pub fn invalidate(&mut self) {
        self.tree_generation += 1;

        #[cfg(debug_assertions)]
        tracing::debug!(
            "HitTestCache::invalidate: tree generation now {}",
            self.tree_generation
        );
    }

    /// Clear the cache (for memory management)
    pub fn clear(&mut self) {
        self.cache.clear();
        #[cfg(debug_assertions)]
        tracing::debug!("HitTestCache::clear: cache cleared");
    }

    /// Get cache statistics (debug builds only)
    #[cfg(debug_assertions)]
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.stats.hits, self.stats.misses, self.stats.invalidations)
    }

    /// Get cache size
    pub fn size(&self) -> usize {
        self.cache.len()
    }

    /// Get current generation counter
    pub fn generation(&self) -> u64 {
        self.tree_generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_quantization() {
        let root = ElementId::new(1);

        // Same position should produce same key
        let key1 = CacheKey::new(Offset::new(10.0, 20.0), root);
        let key2 = CacheKey::new(Offset::new(10.0, 20.0), root);
        assert_eq!(key1, key2);

        // Small jitter (<0.1px) should produce same key
        let key3 = CacheKey::new(Offset::new(10.05, 20.05), root);
        assert_eq!(key1, key3);

        // Larger difference should produce different key
        let key4 = CacheKey::new(Offset::new(10.2, 20.2), root);
        assert_ne!(key1, key4);
    }

    #[test]
    fn test_cache_invalidation() {
        let mut cache = HitTestCache::new();
        let root = ElementId::new(1);
        let pos = Offset::new(10.0, 20.0);
        let result = ElementHitTestResult::new();

        // Insert and retrieve
        cache.insert(pos, root, result.clone());
        assert!(cache.get(pos, root).is_some());

        // Invalidate
        cache.invalidate();

        // Should miss now
        assert!(cache.get(pos, root).is_none());
    }

    #[test]
    fn test_generation_counter() {
        let mut cache = HitTestCache::new();
        assert_eq!(cache.generation(), 0);

        cache.invalidate();
        assert_eq!(cache.generation(), 1);

        cache.invalidate();
        assert_eq!(cache.generation(), 2);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = HitTestCache::new();
        let root = ElementId::new(1);
        let result = ElementHitTestResult::new();

        // Fill cache
        cache.insert(Offset::new(10.0, 20.0), root, result.clone());
        cache.insert(Offset::new(30.0, 40.0), root, result.clone());
        assert_eq!(cache.size(), 2);

        // Clear
        cache.clear();
        assert_eq!(cache.size(), 0);
    }
}
