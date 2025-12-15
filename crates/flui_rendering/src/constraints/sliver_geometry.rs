//! Sliver geometry for scrollable layout output.
//!
//! This module provides [`SliverGeometry`], which describes the amount of space
//! occupied by a sliver.
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `SliverGeometry` class from `rendering/sliver.dart`.

/// Describes the amount of space occupied by a sliver.
///
/// A sliver can occupy space in several different ways, which is why this struct
/// contains multiple values.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SliverGeometry` class.
///
/// # Example
///
/// ```
/// use flui_rendering::constraints::SliverGeometry;
///
/// let geometry = SliverGeometry::new(100.0, 100.0, 0.0);
/// assert_eq!(geometry.scroll_extent, 100.0);
/// assert!(geometry.visible);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverGeometry {
    /// The (estimated) total scrollable extent that this sliver has content for.
    ///
    /// This is the amount of scrolling the user needs to do to get from the
    /// beginning of this sliver to the end of this sliver.
    ///
    /// The value is used to calculate the scroll offset of all slivers in the
    /// scrollable and thus should be provided whether the sliver is currently
    /// in the viewport or not.
    pub scroll_extent: f32,

    /// The amount of currently visible visual space that was taken by the sliver
    /// to render the subset of the sliver that covers all or part of the
    /// `remaining_paint_extent` in the current viewport.
    ///
    /// This value does not affect how the next sliver is positioned. In other
    /// words, if this value was 100 and `layout_extent` was 0, typical slivers
    /// placed after it would end up drawing in the same 100 pixel space while
    /// painting.
    ///
    /// This must be between zero and `SliverConstraints::remaining_paint_extent`.
    pub paint_extent: f32,

    /// The visual location of the first visible part of this sliver relative to
    /// its layout position.
    ///
    /// For example, if the sliver wishes to paint visually before its layout
    /// position, the `paint_origin` is negative.
    ///
    /// Defaults to 0.0, which means slivers start painting at their layout
    /// position by default.
    pub paint_origin: f32,

    /// The distance from the first visible part of this sliver to the first
    /// visible part of the next sliver.
    ///
    /// This must be between zero and `paint_extent`. It defaults to `paint_extent`.
    pub layout_extent: f32,

    /// The (estimated) total paint extent that this sliver would be able to
    /// provide if the `remaining_paint_extent` was infinite.
    ///
    /// This is used by viewports that implement shrink-wrapping.
    ///
    /// By definition, this cannot be less than `paint_extent`.
    pub max_paint_extent: f32,

    /// The maximum extent by which this sliver can reduce the area in which
    /// content can scroll if the sliver were pinned at the edge.
    ///
    /// Slivers that never get pinned at the edge should return zero.
    ///
    /// A pinned app bar is an example of a sliver that would use this setting:
    /// When the app bar is pinned to the top, the area in which content can
    /// actually scroll is reduced by the height of the app bar.
    pub max_scroll_obstruction_extent: f32,

    /// The amount of space allocated to the cross axis.
    ///
    /// This value will typically be `None` unless it is different from
    /// `SliverConstraints::cross_axis_extent`. If `None`, then the cross axis
    /// extent of the sliver is assumed to be the same as the constraints.
    pub cross_axis_extent: Option<f32>,

    /// The distance from where this sliver started painting to the bottom of
    /// where it should accept hits.
    ///
    /// Defaults to `paint_extent`.
    pub hit_test_extent: f32,

    /// Whether this sliver should be painted.
    ///
    /// By default, this is true if `paint_extent` is greater than zero.
    pub visible: bool,

    /// Whether this sliver has visual overflow.
    ///
    /// By default, this is false, which means the viewport does not need to clip
    /// its children. If any slivers have visual overflow, the viewport will apply
    /// a clip to its children.
    pub has_visual_overflow: bool,

    /// If this is non-zero after `perform_layout` returns, the scroll offset
    /// will be adjusted by the parent and then the entire layout of the parent
    /// will be rerun.
    ///
    /// When the value is non-null, the sliver does not need to compute the rest
    /// of the values when constructing the geometry since `perform_layout` will
    /// be called again on this sliver in the same frame after the scroll offset
    /// correction has been applied.
    ///
    /// This value must not be zero if set.
    pub scroll_offset_correction: Option<f32>,

    /// How many pixels the sliver has consumed in the `remaining_cache_extent`.
    ///
    /// This value should be equal to or larger than the `layout_extent` because
    /// the sliver always consumes at least the `layout_extent` from the
    /// `remaining_cache_extent` and possibly more if it falls into the cache
    /// area of the viewport.
    pub cache_extent: f32,
}

impl SliverGeometry {
    /// Creates a new sliver geometry with the given extents.
    ///
    /// The `layout_extent`, `hit_test_extent`, and `cache_extent` default to
    /// `paint_extent`. The `max_paint_extent` defaults to `paint_extent`.
    /// `visible` defaults to `paint_extent > 0.0`.
    #[inline]
    pub fn new(scroll_extent: f32, paint_extent: f32, paint_origin: f32) -> Self {
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

    /// Creates a geometry for a zero-size sliver.
    #[inline]
    pub const fn zero() -> Self {
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
            scroll_offset_correction: None,
            cache_extent: 0.0,
        }
    }

    /// Creates a copy with the given fields replaced.
    #[allow(clippy::too_many_arguments)]
    pub fn copy_with(
        &self,
        scroll_extent: Option<f32>,
        paint_extent: Option<f32>,
        paint_origin: Option<f32>,
        layout_extent: Option<f32>,
        max_paint_extent: Option<f32>,
        max_scroll_obstruction_extent: Option<f32>,
        cross_axis_extent: Option<Option<f32>>,
        hit_test_extent: Option<f32>,
        visible: Option<bool>,
        has_visual_overflow: Option<bool>,
        cache_extent: Option<f32>,
    ) -> Self {
        Self {
            scroll_extent: scroll_extent.unwrap_or(self.scroll_extent),
            paint_extent: paint_extent.unwrap_or(self.paint_extent),
            paint_origin: paint_origin.unwrap_or(self.paint_origin),
            layout_extent: layout_extent.unwrap_or(self.layout_extent),
            max_paint_extent: max_paint_extent.unwrap_or(self.max_paint_extent),
            max_scroll_obstruction_extent: max_scroll_obstruction_extent
                .unwrap_or(self.max_scroll_obstruction_extent),
            cross_axis_extent: cross_axis_extent.unwrap_or(self.cross_axis_extent),
            hit_test_extent: hit_test_extent.unwrap_or(self.hit_test_extent),
            visible: visible.unwrap_or(self.visible),
            has_visual_overflow: has_visual_overflow.unwrap_or(self.has_visual_overflow),
            scroll_offset_correction: self.scroll_offset_correction, // Not copyable
            cache_extent: cache_extent.unwrap_or(self.cache_extent),
        }
    }

    // =========================================================================
    // Builder Methods
    // =========================================================================

    /// Builder method to set the layout extent.
    #[inline]
    #[must_use]
    pub fn with_layout_extent(mut self, extent: f32) -> Self {
        self.layout_extent = extent;
        self
    }

    /// Builder method to set the max paint extent.
    #[inline]
    #[must_use]
    pub fn with_max_paint_extent(mut self, extent: f32) -> Self {
        self.max_paint_extent = extent;
        self
    }

    /// Builder method to set the max scroll obstruction extent.
    #[inline]
    #[must_use]
    pub fn with_max_scroll_obstruction_extent(mut self, extent: f32) -> Self {
        self.max_scroll_obstruction_extent = extent;
        self
    }

    /// Builder method to set the cross axis extent.
    #[inline]
    #[must_use]
    pub fn with_cross_axis_extent(mut self, extent: f32) -> Self {
        self.cross_axis_extent = Some(extent);
        self
    }

    /// Builder method to set the hit test extent.
    #[inline]
    #[must_use]
    pub fn with_hit_test_extent(mut self, extent: f32) -> Self {
        self.hit_test_extent = extent;
        self
    }

    /// Builder method to set visibility.
    #[inline]
    #[must_use]
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Builder method to set visual overflow.
    #[inline]
    #[must_use]
    pub fn with_visual_overflow(mut self, has_overflow: bool) -> Self {
        self.has_visual_overflow = has_overflow;
        self
    }

    /// Builder method to set the scroll offset correction.
    #[inline]
    #[must_use]
    pub fn with_scroll_offset_correction(mut self, correction: f32) -> Self {
        debug_assert!(
            correction != 0.0,
            "scroll_offset_correction must not be zero"
        );
        self.scroll_offset_correction = Some(correction);
        self
    }

    /// Builder method to set the cache extent.
    #[inline]
    #[must_use]
    pub fn with_cache_extent(mut self, extent: f32) -> Self {
        self.cache_extent = extent;
        self
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Returns whether this sliver is visible.
    #[inline]
    pub const fn is_visible(&self) -> bool {
        self.visible && self.paint_extent > 0.0
    }

    /// Returns whether this sliver is hit testable.
    #[inline]
    pub fn is_hit_testable(&self) -> bool {
        self.visible && self.hit_test_extent > 0.0
    }

    /// Returns whether this sliver is empty (has no extent).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.scroll_extent == 0.0 && self.paint_extent == 0.0
    }

    /// Returns whether this geometry represents a scrollable sliver.
    #[inline]
    pub fn is_scrollable(&self) -> bool {
        self.scroll_extent > 0.0
    }

    /// Returns the actual painted area bounds (accounting for paint origin).
    #[inline]
    pub const fn paint_bounds(&self) -> (f32, f32) {
        (self.paint_origin, self.paint_origin + self.paint_extent)
    }

    /// Returns the ratio of paint extent to scroll extent.
    ///
    /// This can be useful for determining how much of the sliver is visible.
    /// Returns 1.0 if the entire sliver is visible, < 1.0 if partially visible.
    #[inline]
    pub fn visibility_ratio(&self) -> f32 {
        if self.scroll_extent > 0.0 {
            (self.paint_extent / self.scroll_extent).min(1.0)
        } else {
            1.0
        }
    }

    /// Returns whether the sliver extends beyond its paint extent.
    #[inline]
    pub fn extends_beyond_viewport(&self) -> bool {
        self.scroll_extent > self.paint_extent
    }

    /// Returns whether a scroll offset correction is needed.
    #[inline]
    pub fn needs_scroll_offset_correction(&self) -> bool {
        self.scroll_offset_correction.is_some()
    }

    // =========================================================================
    // Validation
    // =========================================================================

    /// Asserts that this geometry is internally consistent.
    ///
    /// Does nothing if asserts are disabled. Always returns true.
    #[cfg(debug_assertions)]
    pub fn debug_assert_is_valid(&self) -> bool {
        debug_assert!(
            self.scroll_extent >= 0.0,
            "scroll_extent ({}) must be >= 0",
            self.scroll_extent
        );
        debug_assert!(
            self.paint_extent >= 0.0,
            "paint_extent ({}) must be >= 0",
            self.paint_extent
        );
        debug_assert!(
            self.layout_extent >= 0.0,
            "layout_extent ({}) must be >= 0",
            self.layout_extent
        );
        debug_assert!(
            self.cache_extent >= 0.0,
            "cache_extent ({}) must be >= 0",
            self.cache_extent
        );
        debug_assert!(
            self.layout_extent <= self.paint_extent,
            "layout_extent ({}) must be <= paint_extent ({})",
            self.layout_extent,
            self.paint_extent
        );
        // Allow small floating point errors
        const TOLERANCE: f32 = 1e-5;
        debug_assert!(
            self.paint_extent <= self.max_paint_extent + TOLERANCE,
            "paint_extent ({}) must be <= max_paint_extent ({})",
            self.paint_extent,
            self.max_paint_extent
        );
        debug_assert!(
            self.hit_test_extent >= 0.0,
            "hit_test_extent ({}) must be >= 0",
            self.hit_test_extent
        );
        debug_assert!(
            self.scroll_offset_correction != Some(0.0),
            "scroll_offset_correction must not be zero"
        );
        true
    }
}

impl Default for SliverGeometry {
    fn default() -> Self {
        Self::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0);

        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 80.0);
        assert_eq!(geometry.paint_origin, 0.0);
        assert_eq!(geometry.layout_extent, 80.0);
        assert_eq!(geometry.max_paint_extent, 80.0);
        assert_eq!(geometry.max_scroll_obstruction_extent, 0.0);
        assert_eq!(geometry.hit_test_extent, 80.0);
        assert!(geometry.visible);
        assert!(!geometry.has_visual_overflow);
        assert!(geometry.scroll_offset_correction.is_none());
        assert_eq!(geometry.cache_extent, 80.0);
    }

    #[test]
    fn test_zero() {
        let geometry = SliverGeometry::zero();

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
        assert!(geometry.is_empty());
    }

    #[test]
    fn test_default() {
        let geometry = SliverGeometry::default();
        assert_eq!(geometry, SliverGeometry::zero());
    }

    #[test]
    fn test_builder_methods() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0)
            .with_layout_extent(60.0)
            .with_max_paint_extent(120.0)
            .with_max_scroll_obstruction_extent(50.0)
            .with_cross_axis_extent(300.0)
            .with_hit_test_extent(70.0)
            .with_visible(false)
            .with_visual_overflow(true)
            .with_cache_extent(100.0);

        assert_eq!(geometry.layout_extent, 60.0);
        assert_eq!(geometry.max_paint_extent, 120.0);
        assert_eq!(geometry.max_scroll_obstruction_extent, 50.0);
        assert_eq!(geometry.cross_axis_extent, Some(300.0));
        assert_eq!(geometry.hit_test_extent, 70.0);
        assert!(!geometry.visible);
        assert!(geometry.has_visual_overflow);
        assert_eq!(geometry.cache_extent, 100.0);
    }

    #[test]
    fn test_scroll_offset_correction() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0).with_scroll_offset_correction(10.0);

        assert_eq!(geometry.scroll_offset_correction, Some(10.0));
        assert!(geometry.needs_scroll_offset_correction());
    }

    #[test]
    fn test_is_visible() {
        let visible = SliverGeometry::new(100.0, 80.0, 0.0);
        assert!(visible.is_visible());

        let not_visible_extent = SliverGeometry::new(100.0, 0.0, 0.0);
        assert!(!not_visible_extent.is_visible());

        let not_visible_flag = SliverGeometry::new(100.0, 80.0, 0.0).with_visible(false);
        assert!(!not_visible_flag.is_visible());
    }

    #[test]
    fn test_is_hit_testable() {
        let testable = SliverGeometry::new(100.0, 80.0, 0.0);
        assert!(testable.is_hit_testable());

        let not_testable = SliverGeometry::new(100.0, 80.0, 0.0).with_hit_test_extent(0.0);
        assert!(!not_testable.is_hit_testable());
    }

    #[test]
    fn test_is_empty() {
        let empty = SliverGeometry::zero();
        assert!(empty.is_empty());

        let not_empty_scroll = SliverGeometry::new(100.0, 0.0, 0.0);
        assert!(!not_empty_scroll.is_empty());

        let not_empty_paint = SliverGeometry::new(0.0, 80.0, 0.0);
        assert!(!not_empty_paint.is_empty());
    }

    #[test]
    fn test_paint_bounds() {
        let geometry = SliverGeometry::new(100.0, 80.0, 10.0);
        assert_eq!(geometry.paint_bounds(), (10.0, 90.0));

        let negative_origin = SliverGeometry::new(100.0, 80.0, -5.0);
        assert_eq!(negative_origin.paint_bounds(), (-5.0, 75.0));
    }

    #[test]
    fn test_visibility_ratio() {
        let full_visible = SliverGeometry::new(100.0, 100.0, 0.0);
        assert!((full_visible.visibility_ratio() - 1.0).abs() < f32::EPSILON);

        let half_visible = SliverGeometry::new(100.0, 50.0, 0.0);
        assert!((half_visible.visibility_ratio() - 0.5).abs() < f32::EPSILON);

        let zero_scroll = SliverGeometry::new(0.0, 0.0, 0.0);
        assert!((zero_scroll.visibility_ratio() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_extends_beyond_viewport() {
        let extends = SliverGeometry::new(200.0, 100.0, 0.0);
        assert!(extends.extends_beyond_viewport());

        let fits = SliverGeometry::new(100.0, 100.0, 0.0);
        assert!(!fits.extends_beyond_viewport());
    }

    #[test]
    fn test_is_scrollable() {
        let scrollable = SliverGeometry::new(100.0, 80.0, 0.0);
        assert!(scrollable.is_scrollable());

        let not_scrollable = SliverGeometry::new(0.0, 80.0, 0.0);
        assert!(!not_scrollable.is_scrollable());
    }

    #[test]
    fn test_copy_with() {
        let base = SliverGeometry::new(100.0, 80.0, 0.0);

        let modified = base.copy_with(
            Some(200.0),
            None,
            Some(5.0),
            None,
            None,
            Some(30.0),
            None,
            None,
            Some(false),
            None,
            None,
        );

        assert_eq!(modified.scroll_extent, 200.0);
        assert_eq!(modified.paint_extent, 80.0);
        assert_eq!(modified.paint_origin, 5.0);
        assert_eq!(modified.max_scroll_obstruction_extent, 30.0);
        assert!(!modified.visible);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_debug_assert_is_valid() {
        let valid = SliverGeometry::new(100.0, 80.0, 0.0);
        assert!(valid.debug_assert_is_valid());
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SliverGeometry>();
    }
}
