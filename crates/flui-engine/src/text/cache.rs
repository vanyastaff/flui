//! Glyph cache for reusing rasterized glyphs across frames.

use std::collections::HashMap;

// ─── TextCacheKey ────────────────────────────────────────────────────────────

/// Key for looking up cached shaped text runs.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct TextCacheKey {
    /// Hash of the text content.
    pub text_hash: u64,
    /// Font size stored as raw bits for exact equality.
    pub font_size_bits: u32,
    /// Hash of the font family name.
    pub font_family_hash: u64,
    /// Font weight (e.g., 400 for normal, 700 for bold).
    pub font_weight: u16,
}

impl TextCacheKey {
    /// Create a new cache key from text properties.
    pub fn new(text: &str, font_size: f32, font_family: &str, font_weight: u16) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        let text_hash = hasher.finish();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        font_family.hash(&mut hasher);
        let font_family_hash = hasher.finish();
        Self {
            text_hash,
            font_size_bits: font_size.to_bits(),
            font_family_hash,
            font_weight,
        }
    }
}

// ─── CachedEntry ─────────────────────────────────────────────────────────────

/// An entry in the shape cache, tracking when it was last used.
pub struct CachedEntry<T> {
    /// The cached value.
    pub value: T,
    /// Frame number when this entry was last accessed.
    pub last_used_frame: u64,
}

// ─── ShapeCache ──────────────────────────────────────────────────────────────

/// LRU cache for shaped text buffers.
///
/// Eviction strategy: when inserting at capacity, first evict stale entries
/// (those not used within `eviction_ttl_frames`). If still at capacity,
/// evict the entry with the smallest `last_used_frame`.
pub struct ShapeCache<T> {
    entries: HashMap<TextCacheKey, CachedEntry<T>>,
    max_entries: usize,
    eviction_ttl_frames: u64,
}

impl<T> ShapeCache<T> {
    /// Create an empty cache with the given capacity and TTL.
    pub fn new(max_entries: usize, eviction_ttl_frames: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
            eviction_ttl_frames,
        }
    }

    /// Look up a cached value, updating its last-used frame.
    pub fn get(&mut self, key: &TextCacheKey, current_frame: u64) -> Option<&T> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used_frame = current_frame;
            // Re-borrow as immutable to return &T
            Some(&self.entries[key].value)
        } else {
            None
        }
    }

    /// Insert a value into the cache, evicting if at capacity.
    pub fn insert(&mut self, key: TextCacheKey, value: T, current_frame: u64) {
        // If key already present, just update
        if self.entries.contains_key(&key) {
            self.entries.insert(
                key,
                CachedEntry {
                    value,
                    last_used_frame: current_frame,
                },
            );
            return;
        }

        // Evict if at capacity
        if self.entries.len() >= self.max_entries {
            self.evict_stale(current_frame);
        }
        if self.entries.len() >= self.max_entries {
            // Evict the oldest entry (smallest last_used_frame)
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used_frame)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }

        self.entries.insert(
            key,
            CachedEntry {
                value,
                last_used_frame: current_frame,
            },
        );
    }

    /// Look up a cached value without updating its last-used frame.
    ///
    /// Useful when you need multiple simultaneous immutable borrows of
    /// different cached entries (e.g. building a slice of `TextArea`
    /// references).
    pub fn get_ref(&self, key: &TextCacheKey) -> Option<&T> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    /// Touch all provided keys, updating their last-used frame.
    ///
    /// Call this before or after a batch of `get_ref` lookups to keep
    /// the entries alive in the LRU.
    pub fn touch_keys<'a>(
        &mut self,
        keys: impl IntoIterator<Item = &'a TextCacheKey>,
        current_frame: u64,
    ) {
        for key in keys {
            if let Some(entry) = self.entries.get_mut(key) {
                entry.last_used_frame = current_frame;
            }
        }
    }

    /// Check if the cache contains a key.
    pub fn contains(&self, key: &TextCacheKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Return the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Remove entries where `current_frame - last_used_frame > eviction_ttl_frames`.
    pub fn evict_stale(&mut self, current_frame: u64) {
        self.entries.retain(|_, entry| {
            current_frame.saturating_sub(entry.last_used_frame) <= self.eviction_ttl_frames
        });
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache() {
        let mut cache: ShapeCache<String> = ShapeCache::new(10, 100);
        assert_eq!(cache.len(), 0);
        let key = TextCacheKey::new("hello", 16.0, "Arial", 400);
        assert!(cache.get(&key, 0).is_none());
    }

    #[test]
    fn insert_and_get() {
        let mut cache: ShapeCache<String> = ShapeCache::new(10, 100);
        let key = TextCacheKey::new("hello", 16.0, "Arial", 400);
        cache.insert(key.clone(), "shaped_hello".to_string(), 0);
        assert_eq!(cache.len(), 1);
        let val = cache.get(&key, 1);
        assert!(val.is_some());
        assert_eq!(val.unwrap(), "shaped_hello");
    }

    #[test]
    fn lru_eviction() {
        let mut cache: ShapeCache<String> = ShapeCache::new(2, 1000);
        let key_a = TextCacheKey::new("a", 16.0, "Arial", 400);
        let key_b = TextCacheKey::new("b", 16.0, "Arial", 400);
        let key_c = TextCacheKey::new("c", 16.0, "Arial", 400);

        cache.insert(key_a.clone(), "A".into(), 0);
        cache.insert(key_b.clone(), "B".into(), 1);
        // Cache full (2). Inserting c should evict a (oldest).
        cache.insert(key_c.clone(), "C".into(), 2);

        assert_eq!(cache.len(), 2);
        assert!(!cache.contains(&key_a));
        assert!(cache.contains(&key_b));
        assert!(cache.contains(&key_c));
    }

    #[test]
    fn stale_eviction() {
        let mut cache: ShapeCache<String> = ShapeCache::new(10, 10);
        let key = TextCacheKey::new("hello", 16.0, "Arial", 400);
        cache.insert(key.clone(), "shaped".into(), 0);
        assert!(cache.contains(&key));

        // Frame 15: 15 - 0 = 15 > 10 ttl → stale
        cache.evict_stale(15);
        assert!(!cache.contains(&key));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn get_updates_last_used() {
        let mut cache: ShapeCache<String> = ShapeCache::new(10, 10);
        let key = TextCacheKey::new("hello", 16.0, "Arial", 400);
        cache.insert(key.clone(), "shaped".into(), 0);

        // Access at frame 5 → last_used_frame becomes 5
        let _ = cache.get(&key, 5);

        // Evict at frame 12: 12 - 5 = 7 ≤ 10 ttl → NOT stale
        cache.evict_stale(12);
        assert!(cache.contains(&key));
    }
}
