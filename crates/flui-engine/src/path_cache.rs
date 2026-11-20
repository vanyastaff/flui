//! GPU path cache for tessellated geometry.
//!
//! Caches pre-tessellated vertex positions and indices to avoid redundant
//! lyon tessellation of identical paths across frames. Color is applied at
//! draw time so a single cached entry can be reused even when paint color
//! changes: the key covers only what shapes the geometry.
//!
//! # Keys
//!
//! An entry is keyed by the path itself and the paint's geometry parameters
//! ([`TessellationKey`]), compared bit for bit on every hit. The 64-bit hash
//! only picks the bucket: two paths that hash alike are two entries, never
//! one path drawn with the other's triangles. Paths come from the
//! application and its content (an SVG), so the hash is std's SipHash under a
//! per-cache random seed.
//!
//! # Eviction
//!
//! Entries not accessed for 120 frames are evicted by
//! [`PathCache::advance_frame`](crate::path_cache::PathCache::advance_frame); at capacity an insert
//! evicts the least recently used entry.

use std::hash::{BuildHasher, Hash, Hasher, RandomState};

use flui_painting::paint::{Paint, PaintStyle, Path, PathCommand, StrokeCap, StrokeJoin};
use hashbrown::HashTable;

/// Number of idle frames before a cached entry is evicted.
const EVICTION_THRESHOLD: u64 = 120;

/// Cached tessellated path geometry.
///
/// Stores pre-tessellated vertex positions and triangle indices so that
/// identical paths do not need to be re-tessellated by lyon every frame.
///
/// `S` builds the bucket hasher; the default seeds SipHash at random.
#[cfg_attr(
    not(feature = "testing"),
    expect(
        unreachable_pub,
        reason = "the crate root re-exports it only under `testing`, for the benches"
    )
)]
pub struct PathCache<S = RandomState> {
    entries: HashTable<CachedPath>,
    hasher: S,
    max_entries: usize,
    current_frame: u64,
}

/// What a tessellation depends on: a path, the paint parameters that shape
/// its triangles, and the world-scale bucket its curves are flattened for.
///
/// Made by [`PathCache::key`], which hashes it once for the lookup and the
/// insert that may follow.
#[cfg_attr(
    not(feature = "testing"),
    expect(
        unreachable_pub,
        reason = "the crate root re-exports it only under `testing`, for the benches"
    )
)]
pub struct TessellationKey<'a> {
    path: &'a Path,
    params: GeometryParams,
    hash: u64,
}

/// The paint parameters a tessellation's triangles depend on, with the scale
/// bucket. Stroke parameters are kept for fills too: comparing them is
/// cheaper than branching on the style.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GeometryParams {
    style: PaintStyle,
    /// The stroke width's `f32` bits, the precision the tessellator reads.
    stroke_width_bits: u32,
    stroke_cap: StrokeCap,
    stroke_join: StrokeJoin,
    scale_bucket: u64,
}

/// A single cached tessellation result and the key it answers.
struct CachedPath {
    /// A clone of the drawn path: it shares the drawn path's curves, so the
    /// entry copies no geometry.
    path: Path,
    params: GeometryParams,
    hash: u64,
    /// Position-only vertex data (color applied at draw time).
    vertices: Vec<[f32; 2]>,
    /// Triangle indices into `vertices`.
    indices: Vec<u32>,
    /// Frame number when this entry was last accessed.
    last_used_frame: u64,
}

impl CachedPath {
    fn answers(&self, key: &TessellationKey<'_>) -> bool {
        self.hash == key.hash && self.params == key.params && same_geometry(&self.path, key.path)
    }
}

impl PathCache {
    /// Create a new path cache with the given maximum entry count.
    ///
    /// A reasonable default for most UIs is 512.
    #[must_use]
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            unreachable_pub,
            reason = "the crate root re-exports it only under `testing`, for the benches"
        )
    )]
    pub fn new(max_entries: usize) -> Self {
        Self::with_hasher(max_entries, RandomState::new())
    }
}

impl<S: BuildHasher> PathCache<S> {
    /// Create a cache whose buckets hash with `hasher`.
    #[must_use]
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            unreachable_pub,
            reason = "the crate root re-exports it only under `testing`, for the benches"
        )
    )]
    pub fn with_hasher(max_entries: usize, hasher: S) -> Self {
        Self {
            entries: HashTable::with_capacity(max_entries.min(512)),
            hasher,
            max_entries,
            current_frame: 0,
        }
    }

    /// The key of `path` drawn with `paint` at world scale `max_scale`.
    ///
    /// The style, stroke width, caps and joins shape the triangles, and so
    /// does the scale: cached local geometry carries the chord density tuned
    /// for the scale it was flattened at, so an entry made at scale 1 is
    /// never reused at scale 8, where it would facet. A dash pattern also
    /// shapes them and is not in the key: dashed strokes are not cached.
    #[must_use]
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            unreachable_pub,
            reason = "the crate root re-exports it only under `testing`, for the benches"
        )
    )]
    pub fn key<'a>(&self, path: &'a Path, paint: &Paint, max_scale: f32) -> TessellationKey<'a> {
        let params = GeometryParams {
            style: paint.style,
            stroke_width_bits: (paint.stroke_width as f32).to_bits(),
            stroke_cap: paint.stroke_cap,
            stroke_join: paint.stroke_join,
            scale_bucket: quantize_scale(max_scale),
        };
        let mut hasher = self.hasher.build_hasher();
        params.hash(&mut hasher);
        path.fill_type().hash(&mut hasher);
        for command in path.commands() {
            hash_command(command, &mut hasher);
        }
        TessellationKey {
            path,
            params,
            hash: hasher.finish(),
        }
    }

    /// Look up cached tessellation data for `key`.
    ///
    /// Returns `Some((positions, indices))` on a cache hit and updates the
    /// last-used frame counter. Returns `None` on a cache miss.
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            unreachable_pub,
            reason = "the crate root re-exports it only under `testing`, for the benches"
        )
    )]
    pub fn get(&mut self, key: &TessellationKey<'_>) -> Option<(&[[f32; 2]], &[u32])> {
        let entry = self
            .entries
            .find_mut(key.hash, |entry| entry.answers(key))?;
        entry.last_used_frame = self.current_frame;
        Some((&entry.vertices, &entry.indices))
    }

    /// Insert tessellated geometry for `key`, replacing an entry it already has.
    ///
    /// If the cache is at capacity the least recently used entry is evicted
    /// to make room.
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            unreachable_pub,
            reason = "the crate root re-exports it only under `testing`, for the benches"
        )
    )]
    pub fn insert(
        &mut self,
        key: &TessellationKey<'_>,
        vertices: Vec<[f32; 2]>,
        indices: Vec<u32>,
    ) {
        if let Some(entry) = self.entries.find_mut(key.hash, |entry| entry.answers(key)) {
            entry.vertices = vertices;
            entry.indices = indices;
            entry.last_used_frame = self.current_frame;
            return;
        }
        if self.entries.len() >= self.max_entries {
            self.evict_least_recently_used();
        }
        if self.max_entries == 0 {
            return;
        }
        self.entries.insert_unique(
            key.hash,
            CachedPath {
                path: key.path.clone(),
                params: key.params,
                hash: key.hash,
                vertices,
                indices,
                last_used_frame: self.current_frame,
            },
            |entry| entry.hash,
        );
    }

    /// Advance the frame counter and evict stale entries.
    ///
    /// Entries not accessed within the last `EVICTION_THRESHOLD` (120) frames are
    /// removed. Call this once per frame (typically at the start of
    /// `WgpuPainter::render`).
    #[cfg_attr(
        not(feature = "testing"),
        expect(
            unreachable_pub,
            reason = "the crate root re-exports it only under `testing`, for the benches"
        )
    )]
    pub fn advance_frame(&mut self) {
        self.current_frame += 1;

        let threshold = self.current_frame.saturating_sub(EVICTION_THRESHOLD);
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.last_used_frame >= threshold);
        let evicted = before - self.entries.len();

        if evicted > 0 {
            tracing::debug!(
                evicted,
                remaining = self.entries.len(),
                "Path cache frame eviction"
            );
        }
    }

    /// Removes one entry of the oldest last-used frame.
    fn evict_least_recently_used(&mut self) {
        let Some(oldest) = self.entries.iter().map(|entry| entry.last_used_frame).min() else {
            return;
        };
        let mut evicted = false;
        self.entries.retain(|entry| {
            let evict = !evicted && entry.last_used_frame == oldest;
            evicted |= evict;
            !evict
        });
    }
}

/// Quantize a world scale into a coarse bucket for cache keying.
///
/// Cached geometry stores the *local* (untransformed) tessellation, but the
/// chord density baked into it was chosen for the scale it was first
/// tessellated at (see the scale-aware tolerance in
/// `crate::tessellator::Tessellator`). Reusing a scale-1 tessellation at
/// scale 8 would show facets, so the scale must participate in the key —
/// quantized so that nearby scales (which need indistinguishable density)
/// still share an entry and avoid cache churn during smooth zoom.
///
/// Rounds to two decimal places: 1.00 and 8.00 land in distinct buckets,
/// 1.001 and 1.004 do not. Non-finite or non-positive scales collapse to the
/// identity bucket.
#[must_use]
fn quantize_scale(max_scale: f32) -> u64 {
    // Mirror `Tessellator::set_max_scale`'s guard (`> f32::EPSILON`) so the
    // cache bucket and the tessellation tolerance agree on the effective
    // scale: a degenerate scale in `(0, EPSILON]` falls back to 1.0 in both,
    // keeping the cached geometry consistent with its key.
    let s = if max_scale.is_finite() && max_scale > f32::EPSILON {
        max_scale
    } else {
        1.0
    };
    // Two-decimal bucket; `round` keeps adjacent sub-pixel scales together.
    (s * 100.0).round() as u64
}

/// Whether two paths have the same fill rule and the same commands, point
/// coordinates compared by their bits, as the hash reads them.
fn same_geometry(cached: &Path, drawn: &Path) -> bool {
    cached.fill_type() == drawn.fill_type()
        && (cached.shares_commands_with(drawn) || {
            let (mut cached, mut drawn) = (cached.commands(), drawn.commands());
            cached.len() == drawn.len()
                && cached
                    .by_ref()
                    .zip(drawn.by_ref())
                    .all(|(a, b)| command_bits(a) == command_bits(b))
        })
}

/// A command's variant and the bits of its points, unused slots zero.
fn command_bits(command: PathCommand) -> (u8, [u64; 6]) {
    let bits = |p: flui_foundation::geometry::Point<f64>| [p.x.to_bits(), p.y.to_bits()];
    match command {
        PathCommand::MoveTo(p) => {
            let [x, y] = bits(p);
            (0, [x, y, 0, 0, 0, 0])
        }
        PathCommand::LineTo(p) => {
            let [x, y] = bits(p);
            (1, [x, y, 0, 0, 0, 0])
        }
        PathCommand::QuadraticTo(c, p) => {
            let ([cx, cy], [x, y]) = (bits(c), bits(p));
            (2, [cx, cy, x, y, 0, 0])
        }
        PathCommand::CubicTo(c1, c2, p) => {
            let ([ax, ay], [bx, by], [x, y]) = (bits(c1), bits(c2), bits(p));
            (3, [ax, ay, bx, by, x, y])
        }
        PathCommand::Close => (4, [0; 6]),
    }
}

/// Hash a command as [`command_bits`] reads it, so equal keys hash alike.
fn hash_command(command: PathCommand, hasher: &mut impl Hasher) {
    let (variant, bits) = command_bits(command);
    variant.hash(hasher);
    let points = match variant {
        0 | 1 => 2,
        2 => 4,
        3 => 6,
        _ => 0,
    };
    bits[..points].hash(hasher);
}

#[cfg(test)]
mod tests {
    use std::hash::BuildHasherDefault;

    use flui_foundation::geometry::{Point, Rect};
    use flui_painting::paint::{Paint, Path};

    use super::PathCache;

    /// A hasher under which every key lands in one bucket.
    #[derive(Default)]
    struct OneBucket;

    impl std::hash::Hasher for OneBucket {
        fn finish(&self) -> u64 {
            7
        }

        fn write(&mut self, _bytes: &[u8]) {}
    }

    fn triangle(apex: f64) -> Path {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(10.0, 0.0));
        path.line_to(Point::new(5.0, apex));
        path.close();
        path
    }

    #[test]
    fn paths_that_hash_alike_keep_their_own_geometry() {
        let mut cache = PathCache::with_hasher(8, BuildHasherDefault::<OneBucket>::default());
        let paint = Paint::default();
        let (low, high) = (triangle(5.0), triangle(9.0));

        let low_key = cache.key(&low, &paint, 1.0);
        cache.insert(
            &low_key,
            vec![[0.0, 0.0], [10.0, 0.0], [5.0, 5.0]],
            vec![0, 1, 2],
        );
        let high_key = cache.key(&high, &paint, 1.0);
        assert!(
            cache.get(&high_key).is_none(),
            "a path is answered with another path's triangles"
        );

        cache.insert(
            &high_key,
            vec![[0.0, 0.0], [10.0, 0.0], [5.0, 9.0]],
            vec![0, 1, 2],
        );
        let (low_vertices, _) = cache
            .get(&cache.key(&low, &paint, 1.0))
            .expect("low cached");
        assert_eq!(low_vertices[2], [5.0, 5.0]);
        let (high_vertices, _) = cache
            .get(&cache.key(&high, &paint, 1.0))
            .expect("high cached");
        assert_eq!(high_vertices[2], [5.0, 9.0]);
    }

    #[test]
    fn an_equal_path_built_apart_hits() {
        let mut cache = PathCache::new(8);
        let paint = Paint::default();
        let mut drawn = Path::new();
        drawn.add_rect(Rect::from_ltrb(0.0, 0.0, 4.0, 4.0));
        let key = cache.key(&drawn, &paint, 1.0);
        cache.insert(&key, vec![[0.0, 0.0]], vec![0]);

        let mut again = Path::new();
        again.add_rect(Rect::from_ltrb(0.0, 0.0, 4.0, 4.0));
        assert!(cache.get(&cache.key(&again, &paint, 1.0)).is_some());
        assert!(cache.get(&cache.key(&again, &paint, 8.0)).is_none());
    }
}
