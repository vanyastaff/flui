//! Layout results for sliver (scrollable) content.
//!
//! Describes the space occupied by a sliver after layout, including
//! paint extent, scroll extent, and cache regions.

use std::{
    fmt,
    hash::{Hash, Hasher},
};

const PRECISION_ERROR_TOLERANCE: f32 = flui_foundation::EPSILON_F32;

/// Layout output describing space occupied by a sliver.
///
/// After a sliver performs layout, it returns geometry describing:
/// - How much scrollable space it occupies (`scroll_extent`)
/// - How much it actually paints (`paint_extent`)
/// - Where painting starts (`paint_origin`)
/// - Cache extent for off-screen content
///
/// # Cache Support
///
/// Implements `Hash` and `Eq` for caching layout results:
///
/// ```ignore
/// cache.insert(sliver_id, geometry);
/// ```
///
/// # Flutter Equivalence
///
/// Maps directly to Flutter's `SliverGeometry` class.
#[derive(Clone, Copy, PartialEq)]
pub struct SliverGeometry {
    /// Total scrollable extent consumed by this sliver.
    pub scroll_extent: f32,

    /// Extent that's actually painted in the viewport.
    pub paint_extent: f32,

    /// Offset from the sliver's natural position where painting starts.
    /// Typically 0.0, but can be negative for effects like pinned headers.
    pub paint_origin: f32,

    /// Extent that affects layout of subsequent slivers.
    /// Usually equals paint_extent but may differ for special cases.
    pub layout_extent: f32,

    /// Maximum extent this sliver could paint if unconstrained.
    pub max_paint_extent: f32,

    /// Maximum extent that should block scrolling (for pinned elements).
    pub max_scroll_obstruction_extent: f32,

    /// Cross-axis extent if this sliver affects cross-axis sizing.
    pub cross_axis_extent: Option<f32>,

    /// Extent used for hit testing (usually equals paint_extent).
    pub hit_test_extent: f32,

    /// Whether this sliver is currently visible in the viewport.
    pub visible: bool,

    /// Whether painting extends beyond layout bounds.
    pub has_visual_overflow: bool,

    /// If set, requests a scroll offset correction.
    pub scroll_offset_correction: Option<f32>,

    /// Total extent to keep alive in the cache (on and off screen).
    pub cache_extent: f32,
}

// ============================================================================
// HASH + EQ FOR CACHING
// ============================================================================

impl Hash for SliverGeometry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.scroll_extent.to_bits().hash(state);
        self.paint_extent.to_bits().hash(state);
        self.paint_origin.to_bits().hash(state);
        self.layout_extent.to_bits().hash(state);
        self.max_paint_extent.to_bits().hash(state);
        self.max_scroll_obstruction_extent.to_bits().hash(state);

        if let Some(extent) = self.cross_axis_extent {
            extent.to_bits().hash(state);
        }

        self.hit_test_extent.to_bits().hash(state);
        self.visible.hash(state);
        self.has_visual_overflow.hash(state);

        if let Some(correction) = self.scroll_offset_correction {
            correction.to_bits().hash(state);
        }

        self.cache_extent.to_bits().hash(state);
    }
}

impl Eq for SliverGeometry {}

// ============================================================================
// CONSTRUCTORS
// ============================================================================

impl SliverGeometry {
    /// Zero geometry - no space occupied.
    pub const ZERO: Self = Self {
        scroll_extent: 0.0,
        paint_extent: 0.0,
        paint_origin: 0.0,
        layout_extent: 0.0,
        max_paint_extent: 0.0,
        max_scroll_obstruction_extent: 0.0,
        cross_axis_extent: None,
        hit_test_extent: 0.0,
        visible: false,
        has_visual_overflow: false,
        scroll_offset_correction: None,
        cache_extent: 0.0,
    };

    /// Creates geometry with basic extents.
    ///
    /// Sets paint_extent as layout and cache extent, visible if paint_extent >
    /// 0.
    #[inline]
    #[must_use]
    pub const fn new(scroll_extent: f32, paint_extent: f32, paint_origin: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            paint_origin,
            layout_extent: paint_extent,
            max_paint_extent: paint_extent,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent: paint_extent,
        }
    }

    /// Creates geometry with explicit layout and cache extents.
    #[inline]
    #[must_use]
    pub const fn with_extents(
        scroll_extent: f32,
        paint_extent: f32,
        layout_extent: f32,
        cache_extent: f32,
    ) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
            max_paint_extent: paint_extent,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: false,
            scroll_offset_correction: None,
            cache_extent,
        }
    }

    /// Creates geometry requesting a scroll offset correction.
    ///
    /// Used when a sliver determines the scroll position needs adjustment.
    #[inline]
    #[must_use]
    pub const fn scroll_offset_correction(correction: f32) -> Self {
        Self {
            scroll_extent: 0.0,
            paint_extent: 0.0,
            paint_origin: 0.0,
            layout_extent: 0.0,
            max_paint_extent: 0.0,
            max_scroll_obstruction_extent: 0.0,
            cross_axis_extent: None,
            hit_test_extent: 0.0,
            visible: false,
            has_visual_overflow: false,
            scroll_offset_correction: Some(correction),
            cache_extent: 0.0,
        }
    }

    // ============================================================================
    // BUILDER PATTERN
    // ============================================================================

    /// Sets max paint extent.
    #[inline]
    #[must_use]
    pub const fn with_max_paint_extent(mut self, extent: f32) -> Self {
        self.max_paint_extent = extent;
        self
    }

    /// Sets paint origin.
    #[inline]
    #[must_use]
    pub const fn with_paint_origin(mut self, origin: f32) -> Self {
        self.paint_origin = origin;
        self
    }

    /// Sets hit test extent.
    #[inline]
    #[must_use]
    pub const fn with_hit_test_extent(mut self, extent: f32) -> Self {
        self.hit_test_extent = extent;
        self
    }

    /// Sets cross axis extent.
    #[inline]
    #[must_use]
    pub const fn with_cross_axis_extent(mut self, extent: f32) -> Self {
        self.cross_axis_extent = Some(extent);
        self
    }

    /// Marks as having visual overflow.
    #[inline]
    #[must_use]
    pub const fn with_visual_overflow(mut self) -> Self {
        self.has_visual_overflow = true;
        self
    }

    /// Sets max scroll obstruction extent (for pinned headers).
    #[inline]
    #[must_use]
    pub const fn with_max_scroll_obstruction(mut self, extent: f32) -> Self {
        self.max_scroll_obstruction_extent = extent;
        self
    }

    // ============================================================================
    // QUERIES
    // ============================================================================

    /// Returns whether geometry represents zero space.
    #[inline]
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.scroll_extent == 0.0 && self.paint_extent == 0.0 && self.layout_extent == 0.0
    }

    /// Returns whether this geometry requests a scroll correction.
    #[inline]
    #[must_use]
    pub const fn needs_scroll_correction(&self) -> bool {
        self.scroll_offset_correction.is_some()
    }

    /// Returns whether this sliver consumes layout space.
    #[inline]
    #[must_use]
    pub const fn consumes_layout_space(&self) -> bool {
        self.layout_extent > 0.0
    }

    /// Returns whether this sliver is painted.
    #[inline]
    #[must_use]
    pub const fn is_painted(&self) -> bool {
        self.paint_extent > 0.0
    }

    /// Returns whether this sliver is in the cache region.
    #[inline]
    #[must_use]
    pub const fn is_in_cache(&self) -> bool {
        self.cache_extent > 0.0
    }

    /// Returns layout extent that doesn't paint (dead space).
    #[inline]
    #[must_use]
    pub fn non_painted_layout_extent(&self) -> f32 {
        (self.layout_extent - self.paint_extent).max(0.0)
    }

    /// Returns cache extent beyond layout bounds.
    #[inline]
    #[must_use]
    pub fn cache_beyond_layout(&self) -> f32 {
        (self.cache_extent - self.layout_extent).max(0.0)
    }

    // ============================================================================
    // VALIDATION
    // ============================================================================

    /// Returns the first Flutter-compatible geometry invariant violation.
    #[inline]
    #[must_use]
    pub fn validation_error(&self) -> Option<&'static str> {
        if self.scroll_extent.is_nan() {
            return Some("scroll_extent is NaN");
        }
        if !self.paint_extent.is_finite() {
            return Some("paint_extent is not finite");
        }
        if !self.paint_origin.is_finite() {
            return Some("paint_origin is not finite");
        }
        if !self.layout_extent.is_finite() {
            return Some("layout_extent is not finite");
        }
        if self.max_paint_extent.is_nan() {
            return Some("max_paint_extent is NaN");
        }
        if !self.max_scroll_obstruction_extent.is_finite() {
            return Some("max_scroll_obstruction_extent is not finite");
        }
        if !self.hit_test_extent.is_finite() {
            return Some("hit_test_extent is not finite");
        }
        if !self.cache_extent.is_finite() {
            return Some("cache_extent is not finite");
        }

        if self.scroll_extent < 0.0 {
            return Some("scroll_extent is negative");
        }
        if self.paint_extent < 0.0 {
            return Some("paint_extent is negative");
        }
        if self.layout_extent < 0.0 {
            return Some("layout_extent is negative");
        }
        if self.max_paint_extent < 0.0 {
            return Some("max_paint_extent is negative");
        }
        if self.max_scroll_obstruction_extent < 0.0 {
            return Some("max_scroll_obstruction_extent is negative");
        }
        if self.hit_test_extent < 0.0 {
            return Some("hit_test_extent is negative");
        }
        if self.cache_extent < 0.0 {
            return Some("cache_extent is negative");
        }

        if let Some(extent) = self.cross_axis_extent {
            if !extent.is_finite() {
                return Some("cross_axis_extent is not finite");
            }
            if extent < 0.0 {
                return Some("cross_axis_extent is negative");
            }
        }

        if self.layout_extent > self.paint_extent {
            return Some("layout_extent exceeds paint_extent");
        }
        if self.paint_extent - self.max_paint_extent > PRECISION_ERROR_TOLERANCE {
            return Some("paint_extent exceeds max_paint_extent");
        }

        if let Some(correction) = self.scroll_offset_correction {
            if !correction.is_finite() {
                return Some("scroll_offset_correction is not finite");
            }
            if correction == 0.0 {
                return Some("scroll_offset_correction is zero");
            }
        }

        None
    }

    /// Validates geometry invariants in debug builds.
    #[cfg(debug_assertions)]
    pub fn debug_assert_valid(&self) {
        if let Some(reason) = self.validation_error() {
            debug_assert!(false, "SliverGeometry is not valid: {reason}: {self:?}");
        }
    }

    /// No-op in release builds.
    #[cfg(not(debug_assertions))]
    #[inline]
    pub fn debug_assert_valid(&self) {}
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Default for SliverGeometry {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Debug for SliverGeometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("SliverGeometry");

        debug.field("scroll_extent", &self.scroll_extent);
        debug.field("paint_extent", &self.paint_extent);

        if self.paint_origin != 0.0 {
            debug.field("paint_origin", &self.paint_origin);
        }

        if self.layout_extent != self.paint_extent {
            debug.field("layout_extent", &self.layout_extent);
        }

        if self.max_paint_extent != self.paint_extent {
            debug.field("max_paint_extent", &self.max_paint_extent);
        }

        if self.max_scroll_obstruction_extent > 0.0 {
            debug.field(
                "max_scroll_obstruction",
                &self.max_scroll_obstruction_extent,
            );
        }

        if let Some(extent) = self.cross_axis_extent {
            debug.field("cross_axis_extent", &extent);
        }

        if !self.visible {
            debug.field("visible", &false);
        }

        if self.has_visual_overflow {
            debug.field("has_overflow", &true);
        }

        if let Some(correction) = self.scroll_offset_correction {
            debug.field("scroll_correction", &correction);
        }

        debug.finish()
    }
}

impl fmt::Display for SliverGeometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_hash_equality() {
        let g1 = SliverGeometry::new(100.0, 50.0, 0.0);
        let g2 = SliverGeometry::new(100.0, 50.0, 0.0);
        let g3 = SliverGeometry::new(200.0, 50.0, 0.0);

        assert_eq!(g1, g2);
        assert_ne!(g1, g3);

        let mut set = HashSet::new();
        set.insert(g1);
        assert!(set.contains(&g2));
        assert!(!set.contains(&g3));
    }

    #[test]
    fn test_zero_constant() {
        assert!(SliverGeometry::ZERO.is_zero());
        const { assert!(!SliverGeometry::ZERO.visible) };
        assert_eq!(SliverGeometry::ZERO.scroll_extent, 0.0);
    }

    #[test]
    fn test_builder_pattern() {
        let geometry = SliverGeometry::new(100.0, 50.0, 0.0)
            .with_max_paint_extent(150.0)
            .with_hit_test_extent(60.0)
            .with_cross_axis_extent(300.0)
            .with_visual_overflow();

        assert_eq!(geometry.max_paint_extent, 150.0);
        assert_eq!(geometry.hit_test_extent, 60.0);
        assert_eq!(geometry.cross_axis_extent, Some(300.0));
        assert!(geometry.has_visual_overflow);
    }

    #[test]
    fn test_queries() {
        let geometry = SliverGeometry::with_extents(100.0, 50.0, 40.0, 60.0);

        assert!(geometry.consumes_layout_space());
        assert!(geometry.is_painted());
        assert!(geometry.is_in_cache());
        assert_eq!(geometry.non_painted_layout_extent(), 0.0);
        assert_eq!(geometry.cache_beyond_layout(), 20.0);
    }

    #[test]
    fn test_scroll_correction() {
        let geometry = SliverGeometry::scroll_offset_correction(100.0);

        assert!(geometry.needs_scroll_correction());
        assert_eq!(geometry.scroll_offset_correction, Some(100.0));
        assert!(geometry.is_zero());
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_validation() {
        let valid = SliverGeometry::new(100.0, 50.0, 0.0);
        valid.debug_assert_valid(); // Should not panic
    }

    #[test]
    fn validation_rejects_non_finite_and_negative_extents() {
        let geometry = SliverGeometry {
            hit_test_extent: -1.0,
            ..SliverGeometry::new(100.0, 50.0, 0.0)
        };
        assert_eq!(
            geometry.validation_error(),
            Some("hit_test_extent is negative")
        );

        let geometry = SliverGeometry {
            paint_origin: f32::NAN,
            ..SliverGeometry::new(100.0, 50.0, 0.0)
        };
        assert_eq!(
            geometry.validation_error(),
            Some("paint_origin is not finite")
        );
    }

    #[test]
    fn validation_allows_precision_tolerance_for_max_paint_extent() {
        let geometry = SliverGeometry {
            paint_extent: 1.0,
            layout_extent: 1.0,
            max_paint_extent: 1.0 - flui_foundation::EPSILON_F32 / 2.0,
            ..SliverGeometry::ZERO
        };
        assert_eq!(geometry.validation_error(), None);

        let geometry = SliverGeometry {
            paint_extent: 1.0,
            layout_extent: 1.0,
            max_paint_extent: 1.0 - flui_foundation::EPSILON_F32 * 2.0,
            ..SliverGeometry::ZERO
        };
        assert_eq!(
            geometry.validation_error(),
            Some("paint_extent exceeds max_paint_extent")
        );
    }

    #[test]
    fn validation_allows_infinite_scroll_and_max_paint_extents() {
        let geometry = SliverGeometry {
            scroll_extent: f32::INFINITY,
            paint_extent: 100.0,
            layout_extent: 100.0,
            max_paint_extent: f32::INFINITY,
            hit_test_extent: 100.0,
            cache_extent: 120.0,
            visible: true,
            ..SliverGeometry::ZERO
        };

        assert_eq!(geometry.validation_error(), None);
    }
}
