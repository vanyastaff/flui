//! Layout caching system
//!
//! Simple LRU cache for layout results

use flui_types::Size;
use flui_types::constraints::BoxConstraints;
use moka::sync::Cache;
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

/// Global layout cache
///
/// Uses moka for thread-safe LRU caching with TTL.
pub type LayoutCache = Cache<LayoutCacheKey, LayoutResult>;

/// Get the global layout cache
pub fn layout_cache() -> &'static LayoutCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<LayoutCache> = OnceLock::new();

    CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(60))
            .build()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key() {
        let key1 = LayoutCacheKey::new(0, BoxConstraints::tight(Size::ZERO));
        let key2 = LayoutCacheKey::new(0, BoxConstraints::tight(Size::ZERO));

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_cache_with_child_count() {
        let key1 = LayoutCacheKey::new(0, BoxConstraints::tight(Size::ZERO));
        let key2 = key1.with_child_count(5);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_cache_operations() {
        let cache = layout_cache();

        let key = LayoutCacheKey::new(42, BoxConstraints::tight(Size::new(100.0, 100.0)));
        let result = LayoutResult::new(Size::new(50.0, 50.0));

        cache.insert(key, result);

        let cached = cache.get(&key).unwrap();
        assert_eq!(cached.size, Size::new(50.0, 50.0));
    }
}
