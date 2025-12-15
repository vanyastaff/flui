//! Sliver constraints for scrollable viewport layout.
//!
//! This module provides [`SliverConstraints`], which describes the current scroll
//! state of the viewport from the point of view of the sliver receiving the constraints.
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `SliverConstraints` class from `rendering/sliver.dart`.

use super::{Constraints, GrowthDirection};
use flui_types::layout::{Axis, AxisDirection};

use crate::view::ScrollDirection;

/// Immutable layout constraints for sliver layout.
///
/// The `SliverConstraints` describe the current scroll state of the viewport
/// from the point of view of the sliver receiving the constraints. For example,
/// a `scroll_offset` of zero means that the leading edge of the sliver is
/// visible in the viewport, not that the viewport itself has a zero scroll offset.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SliverConstraints` class.
///
/// # Example
///
/// ```
/// use flui_rendering::constraints::SliverConstraints;
/// use flui_types::layout::AxisDirection;
/// use flui_rendering::constraints::GrowthDirection;
/// use flui_rendering::view::ScrollDirection;
///
/// let constraints = SliverConstraints::new(
///     AxisDirection::TopToBottom,
///     GrowthDirection::Forward,
///     ScrollDirection::Idle,
///     0.0,    // scroll_offset
///     0.0,    // preceding_scroll_extent
///     0.0,    // overlap
///     400.0,  // remaining_paint_extent
///     300.0,  // cross_axis_extent
///     AxisDirection::LeftToRight,
///     800.0,  // viewport_main_axis_extent
///     400.0,  // remaining_cache_extent
///     0.0,    // cache_origin
/// );
///
/// assert_eq!(constraints.axis(), Axis::Vertical);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverConstraints {
    /// The direction in which the `scroll_offset` and `remaining_paint_extent` increase.
    pub axis_direction: AxisDirection,

    /// The direction in which the contents of slivers are ordered, relative to
    /// the `axis_direction`.
    ///
    /// For example, if the `axis_direction` is `AxisDirection::Up`, and the
    /// `growth_direction` is `GrowthDirection::Forward`, then an alphabetical list
    /// will have A at the bottom, then B, then C, and so forth, with Z at the
    /// top, with the bottom of the A at scroll offset zero, and the top of the Z
    /// at the highest scroll offset.
    pub growth_direction: GrowthDirection,

    /// The direction in which the user is attempting to scroll, relative to the
    /// `axis_direction` and `growth_direction`.
    ///
    /// For example, if `growth_direction` is `GrowthDirection::Forward` and
    /// `axis_direction` is `AxisDirection::Down`, then a `ScrollDirection::Reverse`
    /// means that the user is scrolling down, in the positive `scroll_offset` direction.
    ///
    /// If the user is not scrolling, this will return `ScrollDirection::Idle`
    /// even if there is (for example) a scroll animation currently animating the position.
    pub user_scroll_direction: ScrollDirection,

    /// The scroll offset, in this sliver's coordinate system, that corresponds to
    /// the earliest visible part of this sliver.
    ///
    /// For example, if `axis_direction` is `AxisDirection::Down` and `growth_direction`
    /// is `GrowthDirection::Forward`, then scroll offset is the amount the top of
    /// the sliver has been scrolled past the top of the viewport.
    ///
    /// This value is typically used to compute whether this sliver should still
    /// protrude into the viewport via `SliverGeometry::paint_extent` and
    /// `SliverGeometry::layout_extent` considering how far the beginning of the
    /// sliver is above the beginning of the viewport.
    pub scroll_offset: f32,

    /// The scroll distance that has been consumed by all slivers that came before
    /// this sliver.
    ///
    /// # Edge Cases
    ///
    /// Slivers often lazily create their internal content as layout occurs. In this
    /// case, when slivers exceed the viewport, their children are built lazily, and
    /// the sliver does not have enough information to estimate its total extent.
    /// `preceding_scroll_extent` will be `f32::INFINITY` for all slivers that appear
    /// after the lazily constructed child.
    pub preceding_scroll_extent: f32,

    /// The number of pixels from where the pixels corresponding to the `scroll_offset`
    /// will be painted up to the first pixel that has not yet been painted on by an
    /// earlier sliver, in the `axis_direction`.
    ///
    /// For example, if the previous sliver had a `paint_extent` of 100.0 pixels but
    /// a `layout_extent` of only 50.0 pixels, then the `overlap` of this sliver
    /// will be 50.0.
    ///
    /// This is typically ignored unless the sliver is itself going to be pinned
    /// or floating and wants to avoid doing so under the previous sliver.
    pub overlap: f32,

    /// The number of pixels of content that the sliver should consider providing.
    /// (Providing more pixels than this is inefficient.)
    ///
    /// The actual number of pixels provided should be specified in the
    /// `SliverGeometry::paint_extent`.
    ///
    /// This value may be infinite, for example if the viewport is an
    /// unconstrained shrink-wrapping viewport.
    ///
    /// This value may be 0.0, for example if the sliver is scrolled off the
    /// bottom of a downwards vertical viewport.
    pub remaining_paint_extent: f32,

    /// The number of pixels in the cross-axis.
    ///
    /// For a vertical list, this is the width of the sliver.
    pub cross_axis_extent: f32,

    /// The direction in which children should be placed in the cross axis.
    ///
    /// Typically used in vertical lists to describe whether the ambient
    /// `TextDirection` is RTL or LTR.
    pub cross_axis_direction: AxisDirection,

    /// The number of pixels the viewport can display in the main axis.
    ///
    /// For a vertical list, this is the height of the viewport.
    pub viewport_main_axis_extent: f32,

    /// Describes how much content the sliver should provide starting from the
    /// `cache_origin`.
    ///
    /// Not all content in the `remaining_cache_extent` will be visible as some
    /// of it might fall into the cache area of the viewport.
    ///
    /// Each sliver should start laying out content at the `cache_origin` and
    /// try to provide as much content as the `remaining_cache_extent` allows.
    ///
    /// The `remaining_cache_extent` is always larger or equal to the
    /// `remaining_paint_extent`. Content that falls in the `remaining_cache_extent`,
    /// but is outside of the `remaining_paint_extent` is currently not visible
    /// in the viewport.
    pub remaining_cache_extent: f32,

    /// Where the cache area starts relative to the `scroll_offset`.
    ///
    /// Slivers that fall into the cache area located before the leading edge and
    /// after the trailing edge of the viewport should still render content
    /// because they are about to become visible when the user scrolls.
    ///
    /// The `cache_origin` describes where the `remaining_cache_extent` starts relative
    /// to the `scroll_offset`. A cache origin of 0 means that the sliver does not
    /// have to provide any content before the current `scroll_offset`. A
    /// `cache_origin` of -250.0 means that even though the first visible part of
    /// the sliver will be at the provided `scroll_offset`, the sliver should
    /// render content starting 250.0 before the `scroll_offset` to fill the
    /// cache area of the viewport.
    ///
    /// The `cache_origin` is always negative or zero and will never exceed
    /// `-scroll_offset`. In other words, a sliver is never asked to provide
    /// content before its zero `scroll_offset`.
    pub cache_origin: f32,
}

impl SliverConstraints {
    /// Creates new sliver constraints with all parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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

    /// Creates a copy with the given fields replaced.
    #[allow(clippy::too_many_arguments)]
    pub fn copy_with(
        &self,
        axis_direction: Option<AxisDirection>,
        growth_direction: Option<GrowthDirection>,
        user_scroll_direction: Option<ScrollDirection>,
        scroll_offset: Option<f32>,
        preceding_scroll_extent: Option<f32>,
        overlap: Option<f32>,
        remaining_paint_extent: Option<f32>,
        cross_axis_extent: Option<f32>,
        cross_axis_direction: Option<AxisDirection>,
        viewport_main_axis_extent: Option<f32>,
        remaining_cache_extent: Option<f32>,
        cache_origin: Option<f32>,
    ) -> Self {
        Self {
            axis_direction: axis_direction.unwrap_or(self.axis_direction),
            growth_direction: growth_direction.unwrap_or(self.growth_direction),
            user_scroll_direction: user_scroll_direction.unwrap_or(self.user_scroll_direction),
            scroll_offset: scroll_offset.unwrap_or(self.scroll_offset),
            preceding_scroll_extent: preceding_scroll_extent
                .unwrap_or(self.preceding_scroll_extent),
            overlap: overlap.unwrap_or(self.overlap),
            remaining_paint_extent: remaining_paint_extent.unwrap_or(self.remaining_paint_extent),
            cross_axis_extent: cross_axis_extent.unwrap_or(self.cross_axis_extent),
            cross_axis_direction: cross_axis_direction.unwrap_or(self.cross_axis_direction),
            viewport_main_axis_extent: viewport_main_axis_extent
                .unwrap_or(self.viewport_main_axis_extent),
            remaining_cache_extent: remaining_cache_extent.unwrap_or(self.remaining_cache_extent),
            cache_origin: cache_origin.unwrap_or(self.cache_origin),
        }
    }

    // =========================================================================
    // Derived Properties
    // =========================================================================

    /// The axis along which the `scroll_offset` and `remaining_paint_extent` are measured.
    #[inline]
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Return what the `growth_direction` would be if the `axis_direction` was
    /// either `AxisDirection::Down` or `AxisDirection::Right`.
    ///
    /// This is the same as `growth_direction` unless the `axis_direction` is either
    /// `AxisDirection::Up` or `AxisDirection::Left`, in which case it is the
    /// opposite growth direction.
    pub fn normalized_growth_direction(&self) -> GrowthDirection {
        if self.axis_direction.is_reversed() {
            self.growth_direction.flip()
        } else {
            self.growth_direction
        }
    }

    // =========================================================================
    // Convenience Builder Methods
    // =========================================================================

    /// Copy constraints with different scroll offset.
    #[inline]
    #[must_use]
    pub fn with_scroll_offset(&self, scroll_offset: f32) -> Self {
        Self {
            scroll_offset,
            ..*self
        }
    }

    /// Copy constraints with different remaining paint extent.
    #[inline]
    #[must_use]
    pub fn with_remaining_paint_extent(&self, remaining_paint_extent: f32) -> Self {
        Self {
            remaining_paint_extent,
            ..*self
        }
    }

    /// Copy constraints with different preceding scroll extent.
    #[inline]
    #[must_use]
    pub fn with_preceding_scroll_extent(&self, preceding_scroll_extent: f32) -> Self {
        Self {
            preceding_scroll_extent,
            ..*self
        }
    }

    /// Copy constraints with different overlap.
    #[inline]
    #[must_use]
    pub fn with_overlap(&self, overlap: f32) -> Self {
        Self { overlap, ..*self }
    }

    /// Copy constraints with different cross axis extent.
    #[inline]
    #[must_use]
    pub fn with_cross_axis_extent(&self, cross_axis_extent: f32) -> Self {
        Self {
            cross_axis_extent,
            ..*self
        }
    }

    /// Copy constraints with different user scroll direction.
    #[inline]
    #[must_use]
    pub fn with_user_scroll_direction(&self, user_scroll_direction: ScrollDirection) -> Self {
        Self {
            user_scroll_direction,
            ..*self
        }
    }

    /// Copy constraints with different cache origin.
    #[inline]
    #[must_use]
    pub fn with_cache_origin(&self, cache_origin: f32) -> Self {
        Self {
            cache_origin,
            ..*self
        }
    }

    /// Copy constraints with different remaining cache extent.
    #[inline]
    #[must_use]
    pub fn with_remaining_cache_extent(&self, remaining_cache_extent: f32) -> Self {
        Self {
            remaining_cache_extent,
            ..*self
        }
    }

    // =========================================================================
    // Box Constraints Conversion
    // =========================================================================

    /// Returns `BoxConstraints` that reflects the sliver constraints.
    ///
    /// The `min_extent` and `max_extent` are used as the constraints in the main
    /// axis. If provided, the given `cross_axis_extent` is used as a tight
    /// constraint in the cross axis. Otherwise, the `cross_axis_extent` from this
    /// object is used as a constraint in the cross axis.
    ///
    /// Useful for slivers that have `RenderBox` children.
    pub fn as_box_constraints(
        &self,
        min_extent: f32,
        max_extent: f32,
        cross_axis_extent: Option<f32>,
    ) -> super::BoxConstraints {
        let cross = cross_axis_extent.unwrap_or(self.cross_axis_extent);
        match self.axis() {
            Axis::Horizontal => super::BoxConstraints::new(min_extent, max_extent, cross, cross),
            Axis::Vertical => super::BoxConstraints::new(cross, cross, min_extent, max_extent),
        }
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Returns whether the sliver's leading edge is visible in the viewport.
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.remaining_paint_extent > 0.0
    }

    /// Returns whether the sliver can paint content.
    #[inline]
    pub fn can_paint(&self) -> bool {
        self.remaining_paint_extent > 0.0 && self.cross_axis_extent > 0.0
    }

    /// Returns whether the sliver is completely scrolled out of view.
    #[inline]
    pub fn is_offscreen(&self) -> bool {
        self.scroll_offset >= self.viewport_main_axis_extent
    }

    /// Returns the scroll offset without any overlap (clamped to >= 0).
    #[inline]
    pub fn scroll_offset_corrected(&self) -> f32 {
        self.scroll_offset.max(0.0)
    }

    /// Returns the maximum extent this sliver can paint.
    #[inline]
    pub fn max_paint_extent(&self) -> f32 {
        self.remaining_paint_extent
    }

    /// Clamp a proposed extent to valid paint range.
    #[inline]
    pub fn clamp_paint_extent(&self, extent: f32) -> f32 {
        extent.clamp(0.0, self.remaining_paint_extent)
    }

    /// Returns the visible portion of the sliver based on scroll offset.
    ///
    /// If `scroll_offset` is positive, some of the sliver is scrolled off-screen.
    /// Returns the extent that should be painted.
    #[inline]
    pub fn visible_extent(&self, total_extent: f32) -> f32 {
        let corrected_offset = self.scroll_offset_corrected();
        (total_extent - corrected_offset)
            .max(0.0)
            .min(self.remaining_paint_extent)
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

impl Constraints for SliverConstraints {
    /// Sliver constraints are never tight.
    ///
    /// Unlike box constraints which can force an exact size, sliver constraints
    /// allow slivers to choose their extent based on content and scrolling.
    #[inline]
    fn is_tight(&self) -> bool {
        false
    }

    /// Returns whether sliver constraints are in canonical form.
    ///
    /// Sliver constraints are normalized if:
    /// - `scroll_offset >= 0.0`
    /// - `cross_axis_extent >= 0.0`
    /// - `axis_direction` and `cross_axis_direction` are on different axes
    /// - `viewport_main_axis_extent >= 0.0`
    /// - `remaining_paint_extent >= 0.0`
    #[inline]
    fn is_normalized(&self) -> bool {
        self.scroll_offset >= 0.0
            && self.cross_axis_extent >= 0.0
            && self.axis_direction.axis() != self.cross_axis_direction.axis()
            && self.viewport_main_axis_extent >= 0.0
            && self.remaining_paint_extent >= 0.0
    }

    /// Validates sliver constraints (debug mode only).
    #[cfg(debug_assertions)]
    fn debug_assert_is_valid(&self, _is_applied_constraint: bool) -> bool {
        // Check for NaN values
        debug_assert!(
            !self.scroll_offset.is_nan(),
            "SliverConstraints.scroll_offset cannot be NaN"
        );
        debug_assert!(
            !self.overlap.is_nan(),
            "SliverConstraints.overlap cannot be NaN"
        );
        debug_assert!(
            !self.cross_axis_extent.is_nan(),
            "SliverConstraints.cross_axis_extent cannot be NaN"
        );
        debug_assert!(
            !self.viewport_main_axis_extent.is_nan(),
            "SliverConstraints.viewport_main_axis_extent cannot be NaN"
        );
        debug_assert!(
            !self.remaining_paint_extent.is_nan(),
            "SliverConstraints.remaining_paint_extent cannot be NaN"
        );
        debug_assert!(
            !self.remaining_cache_extent.is_nan(),
            "SliverConstraints.remaining_cache_extent cannot be NaN"
        );
        debug_assert!(
            !self.cache_origin.is_nan(),
            "SliverConstraints.cache_origin cannot be NaN"
        );
        debug_assert!(
            !self.preceding_scroll_extent.is_nan(),
            "SliverConstraints.preceding_scroll_extent cannot be NaN"
        );

        // Check positive constraints
        debug_assert!(
            self.scroll_offset >= 0.0,
            "SliverConstraints.scroll_offset must be >= 0.0, got {}",
            self.scroll_offset
        );
        debug_assert!(
            self.viewport_main_axis_extent >= 0.0,
            "SliverConstraints.viewport_main_axis_extent must be >= 0.0"
        );
        debug_assert!(
            self.remaining_paint_extent >= 0.0,
            "SliverConstraints.remaining_paint_extent must be >= 0.0"
        );
        debug_assert!(
            self.remaining_cache_extent >= 0.0,
            "SliverConstraints.remaining_cache_extent must be >= 0.0"
        );
        debug_assert!(
            self.cache_origin <= 0.0,
            "SliverConstraints.cache_origin must be <= 0.0"
        );
        debug_assert!(
            self.preceding_scroll_extent >= 0.0,
            "SliverConstraints.preceding_scroll_extent must be >= 0.0"
        );

        // Check axis orthogonality
        debug_assert!(
            self.axis_direction.axis() != self.cross_axis_direction.axis(),
            "axis_direction and cross_axis_direction must be on different axes"
        );

        debug_assert!(
            self.is_normalized(),
            "SliverConstraints must be normalized: {:?}",
            self
        );

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_constraints() -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            ScrollDirection::Idle,
            50.0,
            100.0,
            0.0,
            400.0,
            300.0,
            AxisDirection::LeftToRight,
            800.0,
            600.0,
            -50.0,
        )
    }

    #[test]
    fn test_new() {
        let constraints = sample_constraints();

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.growth_direction, GrowthDirection::Forward);
        assert_eq!(constraints.user_scroll_direction, ScrollDirection::Idle);
        assert_eq!(constraints.scroll_offset, 50.0);
        assert_eq!(constraints.preceding_scroll_extent, 100.0);
        assert_eq!(constraints.overlap, 0.0);
        assert_eq!(constraints.remaining_paint_extent, 400.0);
        assert_eq!(constraints.cross_axis_extent, 300.0);
        assert_eq!(constraints.cross_axis_direction, AxisDirection::LeftToRight);
        assert_eq!(constraints.viewport_main_axis_extent, 800.0);
        assert_eq!(constraints.remaining_cache_extent, 600.0);
        assert_eq!(constraints.cache_origin, -50.0);
    }

    #[test]
    fn test_default() {
        let constraints = SliverConstraints::default();

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.growth_direction, GrowthDirection::Forward);
        assert_eq!(constraints.user_scroll_direction, ScrollDirection::Idle);
        assert_eq!(constraints.scroll_offset, 0.0);
        assert_eq!(constraints.remaining_paint_extent, 0.0);
    }

    #[test]
    fn test_axis() {
        let vertical = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            ..Default::default()
        };
        assert_eq!(vertical.axis(), Axis::Vertical);

        let horizontal = SliverConstraints {
            axis_direction: AxisDirection::LeftToRight,
            cross_axis_direction: AxisDirection::TopToBottom,
            ..Default::default()
        };
        assert_eq!(horizontal.axis(), Axis::Horizontal);
    }

    #[test]
    fn test_normalized_growth_direction() {
        // Normal case - down with forward
        let normal = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            ..Default::default()
        };
        assert_eq!(
            normal.normalized_growth_direction(),
            GrowthDirection::Forward
        );

        // Reversed axis direction - up with forward becomes reverse
        let reversed = SliverConstraints {
            axis_direction: AxisDirection::BottomToTop,
            growth_direction: GrowthDirection::Forward,
            ..Default::default()
        };
        assert_eq!(
            reversed.normalized_growth_direction(),
            GrowthDirection::Reverse
        );
    }

    #[test]
    fn test_is_visible() {
        let visible = SliverConstraints {
            remaining_paint_extent: 100.0,
            ..Default::default()
        };
        assert!(visible.is_visible());

        let invisible = SliverConstraints {
            remaining_paint_extent: 0.0,
            ..Default::default()
        };
        assert!(!invisible.is_visible());
    }

    #[test]
    fn test_can_paint() {
        let can_paint = SliverConstraints {
            remaining_paint_extent: 100.0,
            cross_axis_extent: 100.0,
            ..Default::default()
        };
        assert!(can_paint.can_paint());

        let no_paint_extent = SliverConstraints {
            remaining_paint_extent: 0.0,
            cross_axis_extent: 100.0,
            ..Default::default()
        };
        assert!(!no_paint_extent.can_paint());

        let no_cross_extent = SliverConstraints {
            remaining_paint_extent: 100.0,
            cross_axis_extent: 0.0,
            ..Default::default()
        };
        assert!(!no_cross_extent.can_paint());
    }

    #[test]
    fn test_scroll_offset_corrected() {
        let positive = SliverConstraints {
            scroll_offset: 50.0,
            ..Default::default()
        };
        assert_eq!(positive.scroll_offset_corrected(), 50.0);

        let negative = SliverConstraints {
            scroll_offset: -20.0,
            ..Default::default()
        };
        assert_eq!(negative.scroll_offset_corrected(), 0.0);
    }

    #[test]
    fn test_visible_extent() {
        let constraints = SliverConstraints {
            scroll_offset: 20.0,
            remaining_paint_extent: 100.0,
            ..Default::default()
        };

        // 80 pixels of 100 total are visible (20 scrolled off)
        assert_eq!(constraints.visible_extent(100.0), 80.0);

        // All 50 pixels visible
        assert_eq!(constraints.visible_extent(50.0), 30.0);
    }

    #[test]
    fn test_clamp_paint_extent() {
        let constraints = SliverConstraints {
            remaining_paint_extent: 100.0,
            ..Default::default()
        };

        assert_eq!(constraints.clamp_paint_extent(50.0), 50.0);
        assert_eq!(constraints.clamp_paint_extent(150.0), 100.0);
        assert_eq!(constraints.clamp_paint_extent(-10.0), 0.0);
    }

    #[test]
    fn test_as_box_constraints() {
        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            cross_axis_extent: 300.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            ..Default::default()
        };

        // Vertical sliver - cross axis becomes width
        let box_constraints = constraints.as_box_constraints(0.0, 100.0, None);
        assert_eq!(box_constraints.min_width, 300.0);
        assert_eq!(box_constraints.max_width, 300.0);
        assert_eq!(box_constraints.min_height, 0.0);
        assert_eq!(box_constraints.max_height, 100.0);

        // With custom cross axis extent
        let box_constraints = constraints.as_box_constraints(50.0, 200.0, Some(250.0));
        assert_eq!(box_constraints.min_width, 250.0);
        assert_eq!(box_constraints.max_width, 250.0);
    }

    #[test]
    fn test_with_methods() {
        let base = sample_constraints();

        let modified = base.with_scroll_offset(100.0);
        assert_eq!(modified.scroll_offset, 100.0);
        assert_eq!(modified.remaining_paint_extent, base.remaining_paint_extent);

        let modified = base.with_remaining_paint_extent(500.0);
        assert_eq!(modified.remaining_paint_extent, 500.0);

        let modified = base.with_overlap(25.0);
        assert_eq!(modified.overlap, 25.0);

        let modified = base.with_user_scroll_direction(ScrollDirection::Forward);
        assert_eq!(modified.user_scroll_direction, ScrollDirection::Forward);
    }

    #[test]
    fn test_copy_with() {
        let base = sample_constraints();

        let modified = base.copy_with(
            None,
            None,
            Some(ScrollDirection::Reverse),
            Some(200.0),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(modified.user_scroll_direction, ScrollDirection::Reverse);
        assert_eq!(modified.scroll_offset, 200.0);
        // Unchanged fields
        assert_eq!(modified.axis_direction, base.axis_direction);
        assert_eq!(modified.remaining_paint_extent, base.remaining_paint_extent);
    }

    #[test]
    fn test_is_tight() {
        let constraints = sample_constraints();
        assert!(!constraints.is_tight());
    }

    #[test]
    fn test_is_normalized() {
        let valid = sample_constraints();
        assert!(valid.is_normalized());

        // Negative scroll offset
        let invalid = SliverConstraints {
            scroll_offset: -10.0,
            ..Default::default()
        };
        assert!(!invalid.is_normalized());

        // Same axis for both directions
        let invalid = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            cross_axis_direction: AxisDirection::BottomToTop,
            ..Default::default()
        };
        assert!(!invalid.is_normalized());
    }

    #[test]
    fn test_is_offscreen() {
        let onscreen = SliverConstraints {
            scroll_offset: 100.0,
            viewport_main_axis_extent: 800.0,
            ..Default::default()
        };
        assert!(!onscreen.is_offscreen());

        let offscreen = SliverConstraints {
            scroll_offset: 900.0,
            viewport_main_axis_extent: 800.0,
            ..Default::default()
        };
        assert!(offscreen.is_offscreen());
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SliverConstraints>();
    }
}
