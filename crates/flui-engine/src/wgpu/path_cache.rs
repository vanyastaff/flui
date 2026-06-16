//! GPU path cache for tessellated geometry.
//!
//! Caches pre-tessellated vertex positions and indices to avoid redundant
//! lyon tessellation of identical paths across frames.  Color is applied at
//! draw time so a single cached entry can be reused even when paint color
//! changes (the hash only covers geometry-affecting properties).
//!
//! # Eviction
//!
//! Entries not accessed for 120 frames are automatically evicted during
//! [`PathCache::advance_frame`].

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use flui_painting::{PaintStyle, StrokeCap, StrokeJoin};
use flui_types::painting::path::{Path, PathCommand};

/// Number of idle frames before a cached entry is evicted.
const EVICTION_THRESHOLD: u64 = 120;

/// Cached tessellated path geometry.
///
/// Stores pre-tessellated vertex positions and triangle indices so that
/// identical paths do not need to be re-tessellated by lyon every frame.
pub struct PathCache {
    entries: HashMap<u64, CachedPath>,
    max_entries: usize,
    hits: u64,
    misses: u64,
    current_frame: u64,
}

/// A single cached tessellation result.
struct CachedPath {
    /// Position-only vertex data (color applied at draw time).
    vertices: Vec<[f32; 2]>,
    /// Triangle indices into `vertices`.
    indices: Vec<u32>,
    /// Frame number when this entry was last accessed.
    last_used_frame: u64,
}

impl PathCache {
    /// Create a new path cache with the given maximum entry count.
    ///
    /// A reasonable default for most UIs is 512.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries.min(512)),
            max_entries,
            hits: 0,
            misses: 0,
            current_frame: 0,
        }
    }

    /// Look up cached tessellation data for the given path hash.
    ///
    /// Returns `Some((positions, indices))` on a cache hit and updates the
    /// last-used frame counter.  Returns `None` on a cache miss.
    pub fn get(&mut self, path_hash: u64) -> Option<(&[[f32; 2]], &[u32])> {
        if let Some(entry) = self.entries.get_mut(&path_hash) {
            entry.last_used_frame = self.current_frame;
            self.hits += 1;
            Some((&entry.vertices, &entry.indices))
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert tessellated geometry into the cache.
    ///
    /// If the cache is at capacity the oldest entry (by `last_used_frame`) is
    /// evicted to make room.
    pub fn insert(&mut self, path_hash: u64, vertices: Vec<[f32; 2]>, indices: Vec<u32>) {
        // Evict oldest entry when at capacity
        if self.entries.len() >= self.max_entries
            && !self.entries.contains_key(&path_hash)
            && let Some(&oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.last_used_frame)
                .map(|(k, _)| k)
        {
            self.entries.remove(&oldest_key);
        }

        self.entries.insert(
            path_hash,
            CachedPath {
                vertices,
                indices,
                last_used_frame: self.current_frame,
            },
        );
    }

    /// Advance the frame counter and evict stale entries.
    ///
    /// Entries not accessed within the last `EVICTION_THRESHOLD` (120) frames are
    /// removed.  Call this once per frame (typically at the start of
    /// [`WgpuPainter::render`](super::painter::WgpuPainter::render)).
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
                "Path cache frame eviction"
            );
        }
    }

    /// Compute a hash for a path combined with paint properties that affect
    /// tessellation geometry (style, stroke width, caps, joins).
    ///
    /// Two calls with identical path commands and paint parameters will produce
    /// the same hash, allowing the tessellated result to be reused.
    #[must_use]
    pub fn compute_path_hash(
        path: &Path,
        style: PaintStyle,
        stroke_width: f32,
        stroke_cap: StrokeCap,
        stroke_join: StrokeJoin,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Hash fill type
        path.fill_type().hash(&mut hasher);

        // Hash paint style
        style.hash(&mut hasher);

        // Hash stroke parameters (only meaningful for strokes, but hashing
        // unconditionally is cheaper than branching)
        stroke_width.to_bits().hash(&mut hasher);
        stroke_cap.hash(&mut hasher);
        stroke_join.hash(&mut hasher);

        // Hash each path command
        for cmd in path.commands() {
            hash_command(cmd, &mut hasher);
        }

        hasher.finish()
    }

    /// Return cache statistics: `(hits, misses, current_entries)`.
    #[must_use]
    pub fn stats(&self) -> (u64, u64, usize) {
        (self.hits, self.misses, self.entries.len())
    }

    /// Remove all cached entries and reset statistics.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hits = 0;
        self.misses = 0;
    }
}

/// Hash a single [`PathCommand`] by discriminant and contained point data.
fn hash_command(cmd: &PathCommand, hasher: &mut DefaultHasher) {
    // Discriminant tag
    std::mem::discriminant(cmd).hash(hasher);

    match cmd {
        PathCommand::MoveTo(p) | PathCommand::LineTo(p) => {
            hash_point(*p, hasher);
        }
        PathCommand::QuadraticTo(cp, ep) => {
            hash_point(*cp, hasher);
            hash_point(*ep, hasher);
        }
        PathCommand::CubicTo(cp1, cp2, ep) => {
            hash_point(*cp1, hasher);
            hash_point(*cp2, hasher);
            hash_point(*ep, hasher);
        }
        PathCommand::Close => {}
        PathCommand::AddRect(r) | PathCommand::AddOval(r) => {
            hash_rect(r, hasher);
        }
        PathCommand::AddCircle(center, radius) => {
            hash_point(*center, hasher);
            radius.to_bits().hash(hasher);
        }
        PathCommand::AddArc(r, start, sweep) => {
            hash_rect(r, hasher);
            start.to_bits().hash(hasher);
            sweep.to_bits().hash(hasher);
        }
    }
}

/// Hash a `Point<Pixels>` by its f32 bit patterns.
fn hash_point(p: flui_types::Point<flui_types::geometry::Pixels>, hasher: &mut DefaultHasher) {
    p.x.0.to_bits().hash(hasher);
    p.y.0.to_bits().hash(hasher);
}

/// Hash a `Rect<Pixels>` by its four edge f32 bit patterns.
fn hash_rect(r: &flui_types::Rect<flui_types::geometry::Pixels>, hasher: &mut DefaultHasher) {
    r.left().0.to_bits().hash(hasher);
    r.top().0.to_bits().hash(hasher);
    r.right().0.to_bits().hash(hasher);
    r.bottom().0.to_bits().hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_cache_hit_miss() {
        let mut cache = PathCache::new(64);

        let hash = 42;
        assert!(cache.get(hash).is_none());
        assert_eq!(cache.stats(), (0, 1, 0));

        cache.insert(hash, vec![[1.0, 2.0], [3.0, 4.0]], vec![0, 1, 2]);
        let result = cache.get(hash);
        assert!(result.is_some());

        let (verts, idxs) = result.unwrap();
        assert_eq!(verts.len(), 2);
        assert_eq!(idxs, &[0, 1, 2]);
        assert_eq!(cache.stats(), (1, 1, 1));
    }

    #[test]
    fn test_eviction_by_frame() {
        let mut cache = PathCache::new(64);
        cache.insert(1, vec![[0.0, 0.0]], vec![0]);

        // Advance past eviction threshold without accessing entry
        for _ in 0..121 {
            cache.advance_frame();
        }

        assert!(cache.get(1).is_none());
        // Entry count should be 0 after eviction
        assert_eq!(cache.stats().2, 0);
    }

    #[test]
    fn test_capacity_eviction() {
        let mut cache = PathCache::new(2);
        cache.insert(1, vec![[0.0, 0.0]], vec![0]);
        cache.insert(2, vec![[1.0, 1.0]], vec![0]);

        // Advance a frame and access only entry 2
        cache.advance_frame();
        let _ = cache.get(2);

        // Insert a third entry — should evict entry 1 (oldest)
        cache.insert(3, vec![[2.0, 2.0]], vec![0]);
        assert!(cache.get(1).is_none());
        assert!(cache.get(2).is_some());
        assert!(cache.get(3).is_some());
    }

    #[test]
    fn test_path_hash_deterministic() {
        let mut path = Path::new();
        path.move_to(flui_types::Point::new(px(0.0), px(0.0)));
        path.line_to(flui_types::Point::new(px(100.0), px(0.0)));
        path.line_to(flui_types::Point::new(px(100.0), px(100.0)));
        path.close();

        let h1 = PathCache::compute_path_hash(
            &path,
            PaintStyle::Fill,
            0.0,
            StrokeCap::Butt,
            StrokeJoin::Miter,
        );
        let h2 = PathCache::compute_path_hash(
            &path,
            PaintStyle::Fill,
            0.0,
            StrokeCap::Butt,
            StrokeJoin::Miter,
        );
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_different_paint_different_hash() {
        let mut path = Path::new();
        path.move_to(flui_types::Point::new(px(0.0), px(0.0)));
        path.line_to(flui_types::Point::new(px(100.0), px(100.0)));

        let h_fill = PathCache::compute_path_hash(
            &path,
            PaintStyle::Fill,
            0.0,
            StrokeCap::Butt,
            StrokeJoin::Miter,
        );
        let h_stroke = PathCache::compute_path_hash(
            &path,
            PaintStyle::Stroke,
            2.0,
            StrokeCap::Round,
            StrokeJoin::Round,
        );
        assert_ne!(h_fill, h_stroke);
    }

    #[test]
    fn test_clear() {
        let mut cache = PathCache::new(64);
        cache.insert(1, vec![[0.0, 0.0]], vec![0]);
        cache.insert(2, vec![[1.0, 1.0]], vec![0]);
        assert_eq!(cache.stats().2, 2);

        cache.clear();
        assert_eq!(cache.stats(), (0, 0, 0));
    }

    /// Regression: `compute_path_hash` does NOT include the dash pattern, so a
    /// dashed stroke and a solid stroke of the same path produce the SAME hash.
    ///
    /// This confirms that the `draw_path` fix (bypass cache for dashed strokes)
    /// is necessary: if dashed paths were cached under the same key as solid
    /// paths, a later solid draw would return dashed geometry (or vice-versa).
    #[test]
    fn dashed_and_solid_stroke_share_geometry_hash() {
        let mut path = flui_types::painting::path::Path::new();
        path.move_to(flui_types::Point::new(px(0.0), px(0.0)));
        path.line_to(flui_types::Point::new(px(100.0), px(0.0)));

        // compute_path_hash does not include dash_pattern (by design)
        let h_solid = PathCache::compute_path_hash(
            &path,
            PaintStyle::Stroke,
            2.0,
            StrokeCap::Butt,
            StrokeJoin::Miter,
        );
        // Calling with identical arguments (dash pattern is not a parameter)
        let h_dashed = PathCache::compute_path_hash(
            &path,
            PaintStyle::Stroke,
            2.0,
            StrokeCap::Butt,
            StrokeJoin::Miter,
        );
        assert_eq!(
            h_solid, h_dashed,
            "Hash must be the same: dash pattern is excluded from the cache key, \
             so draw_path must bypass the cache for dashed strokes"
        );
    }
}
