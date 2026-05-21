//! GPU path cache for tessellated superellipse (iOS-squircle) geometry.
//!
//! Mirrors the [`PathCache`](super::path_cache::PathCache) shape exactly,
//! providing capacity- and frame-based eviction for the path-tessellation
//! route used by `ClipSuperellipseLayer::render`. Replaces the previously
//! unbounded `thread_local! { static SUPERELLIPSE_CACHE: ... }` that lived
//! in `layer_render.rs` and accumulated entries monotonically across the
//! application lifetime.
//!
//! # Eviction
//!
//! Entries not accessed for 120 frames are automatically evicted during
//! [`SuperellipsePathCache::advance_frame`]. When the cache reaches
//! `max_entries`, [`SuperellipsePathCache::insert`] removes the LRU entry
//! (by `last_used_frame`) before adding the new one.
//!
//! # Ownership
//!
//! Owned by [`WgpuPainter`](super::WgpuPainter), single-threaded usage per
//! Painter (one Painter per render thread). Eliminates the static-mutable
//! smell of the prior `thread_local!` cache while preserving thread-safety
//! the same way `PathCache` does.

use std::collections::HashMap;

use flui_types::painting::Path;

/// Number of idle frames before a cached entry is evicted.
///
/// Matches [`PathCache`](super::path_cache::PathCache)'s threshold —
/// 2 seconds at 60fps. No reason to diverge; superellipse path lifetimes
/// follow the same access pattern as general-purpose paths.
const EVICTION_THRESHOLD: u64 = 120;

/// Cache key for superellipse paths, using f32-to-bits for Hash/Eq.
///
/// Captures rect bounds and all 4 corner radii. Identical to the prior
/// thread-local cache's key shape; moved here from `layer_render.rs`
/// to keep the cache module self-contained.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SuperellipseKey {
    /// Outer-rect left edge, as `f32::to_bits()`.
    pub left: u32,
    /// Outer-rect top edge, as `f32::to_bits()`.
    pub top: u32,
    /// Outer-rect right edge, as `f32::to_bits()`.
    pub right: u32,
    /// Outer-rect bottom edge, as `f32::to_bits()`.
    pub bottom: u32,
    /// Top-left corner radius `x`, as `f32::to_bits()`.
    pub tl_x: u32,
    /// Top-left corner radius `y`, as `f32::to_bits()`.
    pub tl_y: u32,
    /// Top-right corner radius `x`, as `f32::to_bits()`.
    pub tr_x: u32,
    /// Top-right corner radius `y`, as `f32::to_bits()`.
    pub tr_y: u32,
    /// Bottom-right corner radius `x`, as `f32::to_bits()`.
    pub br_x: u32,
    /// Bottom-right corner radius `y`, as `f32::to_bits()`.
    pub br_y: u32,
    /// Bottom-left corner radius `x`, as `f32::to_bits()`.
    pub bl_x: u32,
    /// Bottom-left corner radius `y`, as `f32::to_bits()`.
    pub bl_y: u32,
}

impl SuperellipseKey {
    /// Create a cache key from an `RSuperellipse`.
    #[must_use]
    pub fn from_superellipse(s: &flui_types::geometry::RSuperellipse) -> Self {
        let rect = s.outer_rect();
        let tl = s.tl_radius();
        let tr = s.tr_radius();
        let br = s.br_radius();
        let bl = s.bl_radius();

        Self {
            left: rect.left().0.to_bits(),
            top: rect.top().0.to_bits(),
            right: rect.right().0.to_bits(),
            bottom: rect.bottom().0.to_bits(),
            tl_x: tl.x.0.to_bits(),
            tl_y: tl.y.0.to_bits(),
            tr_x: tr.x.0.to_bits(),
            tr_y: tr.y.0.to_bits(),
            br_x: br.x.0.to_bits(),
            br_y: br.y.0.to_bits(),
            bl_x: bl.x.0.to_bits(),
            bl_y: bl.y.0.to_bits(),
        }
    }
}

/// A single cached superellipse path with frame-based eviction metadata.
#[derive(Debug, Clone)]
struct CachedSuperellipsePath {
    /// The tessellated path with iOS-squircle corner curves.
    path: Path,
    /// Frame number when this entry was last accessed.
    last_used_frame: u64,
}

/// Bounded cache for tessellated superellipse paths.
///
/// Stores pre-generated iOS-squircle paths so that identical superellipses
/// (same bounds, same corner radii) do not require re-tessellation across
/// frames. Eviction policy mirrors [`PathCache`](super::path_cache::PathCache):
/// entries idle for `EVICTION_THRESHOLD` frames are dropped during
/// `advance_frame`, and the LRU entry is evicted on insertion when at
/// `max_entries` capacity.
pub struct SuperellipsePathCache {
    entries: HashMap<SuperellipseKey, CachedSuperellipsePath>,
    max_entries: usize,
    hits: u64,
    misses: u64,
    current_frame: u64,
}

impl SuperellipsePathCache {
    /// Create a new superellipse path cache with the given maximum entry count.
    ///
    /// A reasonable default for most UIs is 256 (smaller than `PathCache`'s
    /// 512 because typical UIs use fewer distinct superellipse shapes than
    /// distinct paths). A `max_entries` of 0 disables caching entirely —
    /// matches [`PathCache::new`](super::path_cache::PathCache::new) semantics
    /// for the 0-capacity case.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries.min(256)),
            max_entries,
            hits: 0,
            misses: 0,
            current_frame: 0,
        }
    }

    /// Look up a cached superellipse path by key.
    ///
    /// Returns `Some(Path)` on a cache hit and updates the last-used frame
    /// counter. Returns `None` on a cache miss. The returned path is cloned
    /// because `Path` ownership crosses the cache boundary to downstream
    /// clipping code that consumes the value.
    pub fn get(&mut self, key: &SuperellipseKey) -> Option<Path> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used_frame = self.current_frame;
            self.hits += 1;
            Some(entry.path.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert a tessellated superellipse path into the cache.
    ///
    /// If the cache is at capacity, the LRU entry (by `last_used_frame`)
    /// is evicted before insertion. Matches
    /// [`PathCache::insert`](super::path_cache::PathCache::insert) logic.
    pub fn insert(&mut self, key: SuperellipseKey, path: Path) {
        // Evict LRU entry when at capacity (skip if key already exists —
        // it's an update, not an insertion).
        if self.entries.len() >= self.max_entries
            && !self.entries.contains_key(&key)
            && let Some(&oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.last_used_frame)
                .map(|(k, _)| k)
        {
            self.entries.remove(&oldest_key);
        }

        self.entries.insert(
            key,
            CachedSuperellipsePath {
                path,
                last_used_frame: self.current_frame,
            },
        );
    }

    /// Advance the frame counter and evict stale entries.
    ///
    /// Entries not accessed within the last [`EVICTION_THRESHOLD`] frames
    /// are removed. Call this once per frame (typically at the start of
    /// the painter's `render` method, next to `PathCache::advance_frame`).
    pub fn advance_frame(&mut self) {
        self.current_frame += 1;

        let threshold = self.current_frame.saturating_sub(EVICTION_THRESHOLD);
        let before = self.entries.len();
        self.entries.retain(|_, v| v.last_used_frame >= threshold);
        let evicted = before - self.entries.len();

        if evicted > 0 {
            tracing::debug!(
                evicted,
                remaining = self.entries.len(),
                "Superellipse path cache frame eviction"
            );
        }
    }

    /// Return cache statistics: `(hits, misses, current_entries)`.
    #[must_use]
    pub fn stats(&self) -> (u64, u64, usize) {
        (self.hits, self.misses, self.entries.len())
    }

    /// Remove all cached entries and reset hit/miss counters.
    ///
    /// Leaves `current_frame` unchanged (matches `PathCache::clear`
    /// semantics — clearing entries is a state reset, not a time reset).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }
}

impl std::fmt::Debug for SuperellipsePathCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuperellipsePathCache")
            .field("entries", &self.entries.len())
            .field("max_entries", &self.max_entries)
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .field("current_frame", &self.current_frame)
            .finish()
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test code: expect/panic IS the assertion path"
)]
mod tests {
    use flui_types::geometry::{Radius, Rect, px};
    use flui_types::painting::Path;

    use super::*;

    fn make_key(seed: u32) -> SuperellipseKey {
        // Vary `left` by seed to produce unique keys. All bit-patterns
        // come from f32 sources via `to_bits()` so that real-world
        // SuperellipseKey instances (produced by `from_superellipse`)
        // can compare equal against test keys with matching f32 values.
        SuperellipseKey {
            left: (seed as f32).to_bits(),
            top: 0.0_f32.to_bits(),
            right: 100.0_f32.to_bits(),
            bottom: 100.0_f32.to_bits(),
            tl_x: 8.0_f32.to_bits(),
            tl_y: 8.0_f32.to_bits(),
            tr_x: 8.0_f32.to_bits(),
            tr_y: 8.0_f32.to_bits(),
            br_x: 8.0_f32.to_bits(),
            br_y: 8.0_f32.to_bits(),
            bl_x: 8.0_f32.to_bits(),
            bl_y: 8.0_f32.to_bits(),
        }
    }

    fn make_path() -> Path {
        Path::new()
    }

    #[test]
    fn cache_hit_miss_happy_path() {
        let mut cache = SuperellipsePathCache::new(64);
        let key = make_key(1);

        // Miss before insert
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.stats(), (0, 1, 0));

        // Insert + hit
        cache.insert(key, make_path());
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.stats(), (1, 1, 1));
    }

    #[test]
    fn insert_evicts_lru_at_capacity() {
        let mut cache = SuperellipsePathCache::new(2);
        cache.insert(make_key(1), make_path());
        cache.insert(make_key(2), make_path());

        // Advance a frame and touch only key 2
        cache.advance_frame();
        let _ = cache.get(&make_key(2));

        // Insert third entry — should evict key 1 (LRU)
        cache.insert(make_key(3), make_path());
        assert!(cache.get(&make_key(1)).is_none(), "key 1 should be evicted");
        assert!(cache.get(&make_key(2)).is_some(), "key 2 stays");
        assert!(cache.get(&make_key(3)).is_some(), "key 3 stays");
    }

    #[test]
    fn advance_frame_evicts_stale_entries() {
        let mut cache = SuperellipsePathCache::new(64);
        cache.insert(make_key(1), make_path());

        // Advance past threshold without re-accessing
        for _ in 0..=EVICTION_THRESHOLD {
            cache.advance_frame();
        }

        assert!(cache.get(&make_key(1)).is_none(), "stale entry evicted");
        assert_eq!(cache.stats().2, 0, "cache empty after frame-eviction");
    }

    #[test]
    fn hits_misses_counters_increment_correctly() {
        let mut cache = SuperellipsePathCache::new(64);
        let key = make_key(1);

        // miss, insert, hit, hit → (2, 1, 1)
        let _ = cache.get(&key);
        cache.insert(key, make_path());
        let _ = cache.get(&key);
        let _ = cache.get(&key);

        assert_eq!(cache.stats(), (2, 1, 1));
    }

    #[test]
    fn clear_empties_entries_and_resets_stats() {
        let mut cache = SuperellipsePathCache::new(64);
        cache.insert(make_key(1), make_path());
        cache.insert(make_key(2), make_path());
        let _ = cache.get(&make_key(1));
        assert_eq!(cache.stats(), (1, 0, 2));

        cache.clear();
        assert_eq!(cache.stats(), (0, 0, 0));
    }

    #[test]
    fn superellipse_key_from_real_rsuperellipse() {
        // Smoke test: SuperellipseKey::from_superellipse round-trips
        // a real RSuperellipse without panicking, and produces a key
        // that's stable across repeated calls.
        let rse = flui_types::geometry::RSuperellipse::from_rect_and_radius(
            Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0)),
            Radius::circular(px(8.0)),
        );

        let k1 = SuperellipseKey::from_superellipse(&rse);
        let k2 = SuperellipseKey::from_superellipse(&rse);
        assert_eq!(k1, k2, "key stable across calls");
    }
}
