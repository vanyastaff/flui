//! Layout constraints for sliver (scrollable) layout.
//!
//! Slivers are scrollable content boxes that handle viewport-aware layout
//! with features like infinite scrolling, lazy loading, and cache regions.

use super::{Constraints, GrowthDirection};
use crate::view::ScrollDirection;
use flui_types::layout::{Axis, AxisDirection};
use std::fmt;
use std::hash::{Hash, Hasher};

/// Layout constraints for sliver (scrollable) content.
///
/// Provides viewport-aware constraints that slivers use to determine their
/// size and paint extent within a scrolling viewport.
///
/// # Cache Support
///
/// Implements `Hash` and `Eq` for use as cache keys. Use `normalize()` before
/// caching to ensure consistent floating-point comparisons:
///
/// ```ignore
/// let key = constraints.normalize();
/// cache.insert(key, geometry);
/// ```
///
/// # Normalization
///
/// The `normalize()` method rounds floating-point values to 2 decimal places
/// (0.01 precision). This precision level:
/// - Matches typical display pixel precision (sub-pixel rendering is rare)
/// - Avoids cache thrashing from floating-point rounding errors
/// - Maintains sufficient precision for accurate layout
///
/// # Flutter Equivalence
///
/// Maps directly to Flutter's `SliverConstraints` class with identical semantics.
#[derive(Clone, Copy, PartialEq)]
pub struct SliverConstraints {
    /// Direction along the main axis (e.g., Down for vertical scroll).
    pub axis_direction: AxisDirection,

    /// Direction in which content grows (forward/reverse).
    pub growth_direction: GrowthDirection,

    /// Current user scroll direction (for scroll-dependent effects).
    pub user_scroll_direction: ScrollDirection,

    /// Current scroll offset in the viewport.
    pub scroll_offset: f32,

    /// Scroll extent already occupied by preceding slivers.
    pub preceding_scroll_extent: f32,

    /// Overlap with the previous sliver (for effects like pinned headers).
    pub overlap: f32,

    /// Remaining extent available for painting in the viewport.
    pub remaining_paint_extent: f32,

    /// Extent available in the cross axis.
    pub cross_axis_extent: f32,

    /// Direction along the cross axis.
    pub cross_axis_direction: AxisDirection,

    /// Total extent of the viewport along the main axis.
    pub viewport_main_axis_extent: f32,

    /// Remaining extent available for caching (typically larger than paint extent).
    pub remaining_cache_extent: f32,

    /// Offset from scroll position where caching starts (typically negative).
    pub cache_origin: f32,
}

// ============================================================================
// HASH + EQ FOR CACHING
// ============================================================================

impl Hash for SliverConstraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash enums directly
        self.axis_direction.hash(state);
        self.growth_direction.hash(state);
        self.user_scroll_direction.hash(state);
        self.cross_axis_direction.hash(state);

        // Hash floats as bit patterns (NaN-safe)
        self.scroll_offset.to_bits().hash(state);
        self.preceding_scroll_extent.to_bits().hash(state);
        self.overlap.to_bits().hash(state);
        self.remaining_paint_extent.to_bits().hash(state);
        self.cross_axis_extent.to_bits().hash(state);
        self.viewport_main_axis_extent.to_bits().hash(state);
        self.remaining_cache_extent.to_bits().hash(state);
        self.cache_origin.to_bits().hash(state);
    }
}

impl Eq for SliverConstraints {}

// ============================================================================
// CONSTRUCTORS
// ============================================================================

impl SliverConstraints {
    /// Creates new sliver constraints with all parameters.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    #[must_use]
    pub const fn new(
        axis_direction: AxisDirection,
        growth_direction: GrowthDirection,
        user_scroll_direction: ScrollDirection,
        scroll_offset: f32,
        preceding_scroll_extent: f32,
        overlap: f32,
        remaining_paint_extent: f32,
        cross_axis_extent: f32,
        cross_axis_direction: AxisDirection,
        viewport_main_axis_extent: f32,
        remaining_cache_extent: f32,
        cache_origin: f32,
    ) -> Self {
        Self {
            axis_direction,
            growth_direction,
            user_scroll_direction,
            scroll_offset,
            preceding_scroll_extent,
            overlap,
            remaining_paint_extent,
            cross_axis_extent,
            cross_axis_direction,
            viewport_main_axis_extent,
            remaining_cache_extent,
            cache_origin,
        }
    }

    // ============================================================================
    // NORMALIZATION FOR CACHING
    // ============================================================================

    /// Normalizes constraints for use as cache keys.
    ///
    /// Rounds finite floating-point values to 0.01 precision (2 decimal places).
    /// This precision matches typical display requirements and prevents cache
    /// misses due to floating-point rounding errors.
    ///
    /// Infinite values are preserved unchanged.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Self {
        Self {
            axis_direction: self.axis_direction,
            growth_direction: self.growth_direction,
            user_scroll_direction: self.user_scroll_direction,
            cross_axis_direction: self.cross_axis_direction,
            scroll_offset: round_to_hundredths_runtime(self.scroll_offset),
            preceding_scroll_extent: round_to_hundredths_runtime(self.preceding_scroll_extent),
            overlap: round_to_hundredths_runtime(self.overlap),
            remaining_paint_extent: round_to_hundredths_runtime(self.remaining_paint_extent),
            cross_axis_extent: round_to_hundredths_runtime(self.cross_axis_extent),
            viewport_main_axis_extent: round_to_hundredths_runtime(self.viewport_main_axis_extent),
            remaining_cache_extent: round_to_hundredths_runtime(self.remaining_cache_extent),
            cache_origin: round_to_hundredths_runtime(self.cache_origin),
        }
    }

    /// Checks if constraints are already normalized.
    ///
    /// More efficient than comparing with `normalize()` as it checks
    /// each field individually without allocation.
    #[inline]
    #[must_use]
    pub fn is_normalized_for_cache(&self) -> bool {
        is_normalized(self.scroll_offset)
            && is_normalized(self.preceding_scroll_extent)
            && is_normalized(self.overlap)
            && is_normalized(self.remaining_paint_extent)
            && is_normalized(self.cross_axis_extent)
            && is_normalized(self.viewport_main_axis_extent)
            && is_normalized(self.remaining_cache_extent)
            && is_normalized(self.cache_origin)
    }

    // ============================================================================
    // VIEWPORT QUERIES
    // ============================================================================

    /// Returns the scroll axis (horizontal or vertical).
    #[inline]
    #[must_use]
    pub const fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Returns whether content is at or before the viewport start.
    #[inline]
    #[must_use]
    pub const fn is_at_viewport_start(&self) -> bool {
        self.scroll_offset <= 0.0
    }

    /// Returns whether there is remaining paint extent available.
    #[inline]
    #[must_use]
    pub fn has_remaining_paint_extent(&self) -> bool {
        self.remaining_paint_extent > 0.0 && self.remaining_paint_extent.is_finite()
    }

    /// Returns whether there is remaining cache extent available.
    #[inline]
    #[must_use]
    pub fn has_remaining_cache_extent(&self) -> bool {
        self.remaining_cache_extent > 0.0 && self.remaining_cache_extent.is_finite()
    }

    /// Returns whether this sliver is completely scrolled out of view.
    #[inline]
    #[must_use]
    pub fn is_scrolled_out_of_view(&self) -> bool {
        self.scroll_offset >= self.remaining_paint_extent
    }

    /// Returns whether viewport is shrink-wrapping (infinite paint extent).
    #[inline]
    #[must_use]
    pub fn is_shrink_wrapping(&self) -> bool {
        self.remaining_paint_extent.is_infinite()
    }

    // ============================================================================
    // BUILDER PATTERN
    // ============================================================================

    /// Creates a copy with modified scroll offset.
    #[inline]
    #[must_use]
    pub const fn with_scroll_offset(mut self, scroll_offset: f32) -> Self {
        self.scroll_offset = scroll_offset;
        self
    }

    /// Creates a copy with modified remaining paint extent.
    #[inline]
    #[must_use]
    pub const fn with_remaining_paint_extent(mut self, extent: f32) -> Self {
        self.remaining_paint_extent = extent;
        self
    }

    /// Creates a copy with modified cross axis extent.
    #[inline]
    #[must_use]
    pub const fn with_cross_axis_extent(mut self, extent: f32) -> Self {
        self.cross_axis_extent = extent;
        self
    }

    /// Creates a copy with modified overlap.
    #[inline]
    #[must_use]
    pub const fn with_overlap(mut self, overlap: f32) -> Self {
        self.overlap = overlap;
        self
    }
}

// ============================================================================
// NORMALIZATION HELPERS
// ============================================================================

/// Rounds value to hundredths (0.01 precision).
///
fn round_to_hundredths_runtime(value: f32) -> f32 {
    if value.is_finite() {
        (value * 100.0).round() / 100.0
    } else {
        value
    }
}

/// Checks if value is already normalized to hundredths precision.
#[inline]
fn is_normalized(value: f32) -> bool {
    if !value.is_finite() {
        true // Infinity/NaN are "normalized"
    } else {
        value == round_to_hundredths_runtime(value)
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Constraints for SliverConstraints {
    fn is_tight(&self) -> bool {
        // Sliver constraints are never tight - they describe available space
        // but don't force a specific size
        false
    }

    fn is_normalized(&self) -> bool {
        self.scroll_offset >= 0.0
            && self.preceding_scroll_extent >= 0.0
            && self.remaining_paint_extent >= 0.0
            && self.cross_axis_extent >= 0.0
            && self.viewport_main_axis_extent >= 0.0
            && self.remaining_cache_extent >= 0.0
            && self.cache_origin <= 0.0
            && self.cache_origin >= -self.scroll_offset
            && self.remaining_cache_extent >= self.remaining_paint_extent
            && !self.scroll_offset.is_nan()
            && !self.remaining_paint_extent.is_nan()
    }

    #[cfg(debug_assertions)]
    fn debug_assert_is_valid(&self, is_applied_constraint: bool) -> bool {
        debug_assert!(
            self.is_normalized(),
            "SliverConstraints must be normalized: {:?}",
            self
        );

        if is_applied_constraint {
            debug_assert!(
                self.remaining_paint_extent.is_finite() || self.is_shrink_wrapping(),
                "remaining_paint_extent must be finite unless shrink-wrapping"
            );
        }

        true
    }
}

impl Default for SliverConstraints {
    fn default() -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 0.0,
            cross_axis_extent: 0.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 0.0,
            remaining_cache_extent: 0.0,
            cache_origin: 0.0,
        }
    }
}

impl fmt::Debug for SliverConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverConstraints")
            .field("axis", &self.axis())
            .field("scroll_offset", &self.scroll_offset)
            .field("remaining_paint", &self.remaining_paint_extent)
            .field("cross_extent", &self.cross_axis_extent)
            .finish()
    }
}

impl fmt::Display for SliverConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_hash_equality() {
        let c1 = SliverConstraints::default()
            .with_scroll_offset(100.0)
            .with_remaining_paint_extent(500.0);

        let c2 = c1;
        let c3 = c1.with_scroll_offset(200.0);

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);

        let mut set = HashSet::new();
        set.insert(c1);
        assert!(set.contains(&c2));
        assert!(!set.contains(&c3));
    }

    #[test]
    fn test_normalization() {
        let c = SliverConstraints::default()
            .with_scroll_offset(100.123456)
            .with_remaining_paint_extent(500.987654);

        let normalized = c.normalize();

        assert_eq!(normalized.scroll_offset, 100.12);
        assert_eq!(normalized.remaining_paint_extent, 500.99);
    }

    #[test]
    fn test_viewport_queries() {
        let c = SliverConstraints::default()
            .with_scroll_offset(0.0)
            .with_remaining_paint_extent(500.0);

        assert!(c.is_at_viewport_start());
        assert!(c.has_remaining_paint_extent());
        assert!(!c.is_scrolled_out_of_view());
    }

    #[test]
    fn test_builder_pattern() {
        let c = SliverConstraints::default()
            .with_scroll_offset(100.0)
            .with_remaining_paint_extent(500.0)
            .with_cross_axis_extent(300.0)
            .with_overlap(10.0);

        assert_eq!(c.scroll_offset, 100.0);
        assert_eq!(c.remaining_paint_extent, 500.0);
        assert_eq!(c.cross_axis_extent, 300.0);
        assert_eq!(c.overlap, 10.0);
    }

    #[test]
    fn test_is_normalized_for_cache() {
        let normalized = SliverConstraints::default()
            .with_scroll_offset(100.12)
            .with_remaining_paint_extent(500.99);

        assert!(normalized.is_normalized_for_cache());

        let unnormalized = SliverConstraints::default().with_scroll_offset(100.123456);

        assert!(!unnormalized.is_normalized_for_cache());
    }
}
