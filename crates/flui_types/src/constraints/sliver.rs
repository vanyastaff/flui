//! Sliver constraints and geometry
//!
//! Slivers are scrollable areas that can lay out their children in a scrollable viewport.
//! This module provides constraint and geometry types for slivers.

use crate::layout::{Axis, AxisDirection};
use super::direction::GrowthDirection;

/// Immutable layout constraints for slivers
///
/// Similar to Flutter's `SliverConstraints`. Describes the constraints for a sliver
/// in a scrollable viewport.
///
/// # Examples
///
/// ```
/// use flui_types::constraints::{SliverConstraints, GrowthDirection};
/// use flui_types::layout::{Axis, AxisDirection};
///
/// let constraints = SliverConstraints::new(
///     AxisDirection::TopToBottom,
///     GrowthDirection::Forward,
///     Axis::Vertical,
///     0.0,
///     400.0,
///     800.0,
///     100.0,
/// );
///
/// assert_eq!(constraints.axis, Axis::Vertical);
/// assert_eq!(constraints.remaining_paint_extent, 400.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SliverConstraints {
    /// The direction in which the sliver's content is ordered
    pub axis_direction: AxisDirection,

    /// The direction in which content grows
    pub growth_direction: GrowthDirection,

    /// The axis along which the sliver scrolls
    pub axis: Axis,

    /// The scroll offset of the sliver
    ///
    /// This is the number of pixels from the first visible part of the sliver
    /// to the scroll offset.
    pub scroll_offset: f32,

    /// The amount of space remaining in the viewport
    ///
    /// This is the amount of the viewport that has not yet been filled by slivers.
    pub remaining_paint_extent: f32,

    /// The maximum extent the sliver can have in the main axis
    pub viewport_main_axis_extent: f32,

    /// The extent before the leading edge of the sliver
    ///
    /// This is the amount of the viewport before the sliver's leading edge.
    pub preceding_scroll_extent: f32,

    /// The cross-axis extent of the viewport
    pub cross_axis_extent: f32,

    /// The directionality of the viewport's cross axis
    pub cross_axis_direction: AxisDirection,
}

impl SliverConstraints {
    /// Creates new sliver constraints
    #[inline]
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        axis_direction: AxisDirection,
        growth_direction: GrowthDirection,
        axis: Axis,
        scroll_offset: f32,
        remaining_paint_extent: f32,
        viewport_main_axis_extent: f32,
        cross_axis_extent: f32,
    ) -> Self {
        Self {
            axis_direction,
            growth_direction,
            axis,
            scroll_offset,
            remaining_paint_extent,
            viewport_main_axis_extent,
            preceding_scroll_extent: 0.0,
            cross_axis_extent,
            cross_axis_direction: axis_direction.flip(),
        }
    }

    /// Returns the axis along which the sliver scrolls
    #[inline]
    #[must_use]
    pub const fn axis(&self) -> Axis {
        self.axis
    }

    /// Returns the direction in which content grows
    #[inline]
    #[must_use]
    pub const fn growth_direction(&self) -> GrowthDirection {
        self.growth_direction
    }

    /// Returns whether the sliver's leading edge is visible in the viewport
    #[inline]
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.remaining_paint_extent > 0.0
    }

    /// Returns the amount of overlap from the previous sliver
    ///
    /// If the sliver's scroll offset is negative, it means the previous sliver
    /// extended past its normal extent and is overlapping this sliver.
    #[inline]
    #[must_use]
    pub fn overlap(&self) -> f32 {
        if self.scroll_offset < 0.0 {
            -self.scroll_offset
        } else {
            0.0
        }
    }

    /// Returns the scroll offset without any overlap
    #[inline]
    #[must_use]
    pub fn scroll_offset_corrected(&self) -> f32 {
        self.scroll_offset.max(0.0)
    }

    /// Returns whether the sliver is normalized (valid)
    #[inline]
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        self.remaining_paint_extent >= 0.0
            && self.viewport_main_axis_extent >= 0.0
            && self.cross_axis_extent >= 0.0
    }

    // ===== Helper methods for layout and rendering =====

    /// Returns whether the sliver can paint content
    #[inline]
    #[must_use]
    pub const fn can_paint(&self) -> bool {
        self.remaining_paint_extent > 0.0 && self.cross_axis_extent > 0.0
    }

    /// Returns whether the sliver is completely scrolled out of view
    #[inline]
    #[must_use]
    pub fn is_offscreen(&self) -> bool {
        self.scroll_offset >= self.viewport_main_axis_extent
    }

    /// Returns the maximum extent this sliver can paint
    #[inline]
    #[must_use]
    pub const fn max_paint_extent(&self) -> f32 {
        self.remaining_paint_extent
    }

    /// Clamp a proposed extent to valid paint range
    #[inline]
    #[must_use]
    pub fn clamp_paint_extent(&self, extent: f32) -> f32 {
        extent.clamp(0.0, self.remaining_paint_extent)
    }

    /// Returns the visible portion of the sliver based on scroll offset
    ///
    /// If scroll_offset is positive, some of the sliver is scrolled off-screen.
    /// Returns the extent that should be painted.
    #[inline]
    #[must_use]
    pub fn visible_extent(&self, total_extent: f32) -> f32 {
        let corrected_offset = self.scroll_offset_corrected();
        (total_extent - corrected_offset).max(0.0).min(self.remaining_paint_extent)
    }

    /// Copy constraints with different scroll offset
    #[inline]
    #[must_use]
    pub fn with_scroll_offset(&self, scroll_offset: f32) -> Self {
        Self {
            scroll_offset,
            ..*self
        }
    }

    /// Copy constraints with different remaining paint extent
    #[inline]
    #[must_use]
    pub fn with_remaining_paint_extent(&self, remaining_paint_extent: f32) -> Self {
        Self {
            remaining_paint_extent,
            ..*self
        }
    }

    /// Copy constraints with different preceding scroll extent
    #[inline]
    #[must_use]
    pub fn with_preceding_scroll_extent(&self, preceding_scroll_extent: f32) -> Self {
        Self {
            preceding_scroll_extent,
            ..*self
        }
    }
}

impl Default for SliverConstraints {
    fn default() -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            axis: Axis::Vertical,
            scroll_offset: 0.0,
            remaining_paint_extent: 0.0,
            viewport_main_axis_extent: 0.0,
            preceding_scroll_extent: 0.0,
            cross_axis_extent: 0.0,
            cross_axis_direction: AxisDirection::LeftToRight,
        }
    }
}

/// Describes the geometry of a sliver
///
/// Similar to Flutter's `SliverGeometry`. This is returned by a sliver's
/// `performLayout` method to describe how much space the sliver occupies.
///
/// # Examples
///
/// ```
/// use flui_types::constraints::SliverGeometry;
///
/// let geometry = SliverGeometry::new(
///     100.0,  // scroll_extent
///     100.0,  // paint_extent
///     0.0,    // paint_origin
/// );
///
/// assert_eq!(geometry.scroll_extent, 100.0);
/// assert!(geometry.is_hit_testable());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SliverGeometry {
    /// The amount of space the sliver occupies in the scrollable area
    ///
    /// This is the "scroll extent" - how much the user would need to scroll
    /// to get past this sliver.
    pub scroll_extent: f32,

    /// The amount of currently visible space the sliver occupies
    ///
    /// This is the "paint extent" - how many pixels of the viewport the sliver
    /// currently occupies. Must be between 0 and `remaining_paint_extent` from constraints.
    pub paint_extent: f32,

    /// The distance from the first visible part of the sliver to its leading edge
    ///
    /// Usually 0.0. Can be negative if the sliver paints before its first visible pixel.
    pub paint_origin: f32,

    /// The amount of space the sliver extends before its scroll offset
    ///
    /// This is used for slivers that paint content before their scroll offset,
    /// such as pinned headers.
    pub layout_extent: Option<f32>,

    /// The maximum extent the sliver could have painted
    ///
    /// If not specified, defaults to `paint_extent`.
    pub max_paint_extent: Option<f32>,

    /// The maximum extent the sliver could scroll
    ///
    /// If not specified, defaults to `scroll_extent`.
    pub max_scroll_extent: Option<f32>,

    /// Whether the sliver should be hit tested
    ///
    /// If false, pointer events will pass through this sliver.
    pub hit_test_extent: Option<f32>,

    /// Whether the sliver is visible
    ///
    /// If false, the sliver will not be painted even if it has a paint extent.
    pub visible: bool,

    /// Whether this sliver has visual overflow
    ///
    /// If true, the sliver painted outside its allocated paint extent.
    pub has_visual_overflow: bool,

    /// Cache extent before the leading edge
    pub cache_extent: Option<f32>,
}

impl SliverGeometry {
    /// Creates a new sliver geometry
    #[inline]
    #[must_use]
    pub fn new(scroll_extent: f32, paint_extent: f32, paint_origin: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            paint_origin,
            layout_extent: None,
            max_paint_extent: None,
            max_scroll_extent: None,
            hit_test_extent: None,
            visible: true,
            has_visual_overflow: false,
            cache_extent: None,
        }
    }

    /// Creates a geometry for a zero-size sliver
    #[inline]
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            scroll_extent: 0.0,
            paint_extent: 0.0,
            paint_origin: 0.0,
            layout_extent: Some(0.0),
            max_paint_extent: Some(0.0),
            max_scroll_extent: Some(0.0),
            hit_test_extent: Some(0.0),
            visible: false,
            has_visual_overflow: false,
            cache_extent: Some(0.0),
        }
    }

    /// Returns the actual layout extent
    ///
    /// This is either the explicitly set layout extent, or the paint extent.
    #[inline]
    #[must_use]
    pub fn layout_extent(&self) -> f32 {
        self.layout_extent.unwrap_or(self.paint_extent)
    }

    /// Returns the actual max paint extent
    #[inline]
    #[must_use]
    pub fn max_paint_extent(&self) -> f32 {
        self.max_paint_extent.unwrap_or(self.paint_extent)
    }

    /// Returns the actual max scroll extent
    #[inline]
    #[must_use]
    pub fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent.unwrap_or(self.scroll_extent)
    }

    /// Returns the actual hit test extent
    #[inline]
    #[must_use]
    pub fn hit_test_extent(&self) -> f32 {
        self.hit_test_extent.unwrap_or(self.paint_extent)
    }

    /// Returns whether this sliver is hit testable
    #[inline]
    #[must_use]
    pub fn is_hit_testable(&self) -> bool {
        self.visible && self.hit_test_extent() > 0.0
    }

    /// Returns whether this sliver is visible
    #[inline]
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible && self.paint_extent > 0.0
    }

    /// Returns whether this sliver is empty (has no extent)
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scroll_extent == 0.0 && self.paint_extent == 0.0
    }

    /// Builder method to set the layout extent
    #[inline]
    #[must_use]
    pub fn with_layout_extent(mut self, extent: f32) -> Self {
        self.layout_extent = Some(extent);
        self
    }

    /// Builder method to set the max paint extent
    #[inline]
    #[must_use]
    pub fn with_max_paint_extent(mut self, extent: f32) -> Self {
        self.max_paint_extent = Some(extent);
        self
    }

    /// Builder method to set the max scroll extent
    #[inline]
    #[must_use]
    pub fn with_max_scroll_extent(mut self, extent: f32) -> Self {
        self.max_scroll_extent = Some(extent);
        self
    }

    /// Builder method to set the hit test extent
    #[inline]
    #[must_use]
    pub fn with_hit_test_extent(mut self, extent: f32) -> Self {
        self.hit_test_extent = Some(extent);
        self
    }

    /// Builder method to set visibility
    #[inline]
    #[must_use]
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Builder method to set visual overflow
    #[inline]
    #[must_use]
    pub fn with_visual_overflow(mut self, has_overflow: bool) -> Self {
        self.has_visual_overflow = has_overflow;
        self
    }

    /// Builder method to set cache extent
    #[inline]
    #[must_use]
    pub fn with_cache_extent(mut self, extent: f32) -> Self {
        self.cache_extent = Some(extent);
        self
    }

    // ===== Helper methods for rendering =====

    /// Returns the actual painted area bounds (accounting for paint origin)
    #[inline]
    #[must_use]
    pub const fn paint_bounds(&self) -> (f32, f32) {
        (self.paint_origin, self.paint_origin + self.paint_extent)
    }

    /// Returns whether this geometry represents a scrollable sliver
    #[inline]
    #[must_use]
    pub fn is_scrollable(&self) -> bool {
        self.scroll_extent > 0.0
    }

    /// Returns the ratio of paint extent to scroll extent
    ///
    /// This can be useful for determining how much of the sliver is visible.
    /// Returns 1.0 if the entire sliver is visible, < 1.0 if partially visible.
    #[inline]
    #[must_use]
    pub fn visibility_ratio(&self) -> f32 {
        if self.scroll_extent > 0.0 {
            (self.paint_extent / self.scroll_extent).min(1.0)
        } else {
            1.0
        }
    }

    /// Returns whether the sliver extends beyond its paint extent
    #[inline]
    #[must_use]
    pub fn extends_beyond_viewport(&self) -> bool {
        self.scroll_extent > self.paint_extent
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
    fn test_sliver_constraints_new() {
        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            400.0,
            800.0,
            300.0,
        );

        assert_eq!(constraints.axis, Axis::Vertical);
        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.growth_direction, GrowthDirection::Forward);
        assert_eq!(constraints.scroll_offset, 0.0);
        assert_eq!(constraints.remaining_paint_extent, 400.0);
        assert_eq!(constraints.viewport_main_axis_extent, 800.0);
        assert_eq!(constraints.cross_axis_extent, 300.0);
    }

    #[test]
    fn test_sliver_constraints_is_visible() {
        let visible = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            400.0,
            800.0,
            300.0,
        );
        assert!(visible.is_visible());

        let invisible = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,
            0.0,
            800.0,
            300.0,
        );
        assert!(!invisible.is_visible());
    }

    #[test]
    fn test_sliver_constraints_overlap() {
        let no_overlap = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            50.0,
            400.0,
            800.0,
            300.0,
        );
        assert_eq!(no_overlap.overlap(), 0.0);

        let with_overlap = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            -20.0,
            400.0,
            800.0,
            300.0,
        );
        assert_eq!(with_overlap.overlap(), 20.0);
    }

    #[test]
    fn test_sliver_constraints_corrected_offset() {
        let negative_offset = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            -20.0,
            400.0,
            800.0,
            300.0,
        );
        assert_eq!(negative_offset.scroll_offset_corrected(), 0.0);

        let positive_offset = SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            50.0,
            400.0,
            800.0,
            300.0,
        );
        assert_eq!(positive_offset.scroll_offset_corrected(), 50.0);
    }

    #[test]
    fn test_sliver_geometry_new() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0);

        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 80.0);
        assert_eq!(geometry.paint_origin, 0.0);
        assert!(geometry.visible);
        assert!(!geometry.has_visual_overflow);
    }

    #[test]
    fn test_sliver_geometry_zero() {
        let geometry = SliverGeometry::zero();

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
        assert!(geometry.is_empty());
    }

    #[test]
    fn test_sliver_geometry_extents() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0)
            .with_layout_extent(75.0)
            .with_max_paint_extent(120.0)
            .with_max_scroll_extent(150.0)
            .with_hit_test_extent(80.0);

        assert_eq!(geometry.layout_extent(), 75.0);
        assert_eq!(geometry.max_paint_extent(), 120.0);
        assert_eq!(geometry.max_scroll_extent(), 150.0);
        assert_eq!(geometry.hit_test_extent(), 80.0);
    }

    #[test]
    fn test_sliver_geometry_defaults() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0);

        // When not explicitly set, should default to paint/scroll extents
        assert_eq!(geometry.layout_extent(), 80.0);
        assert_eq!(geometry.max_paint_extent(), 80.0);
        assert_eq!(geometry.max_scroll_extent(), 100.0);
        assert_eq!(geometry.hit_test_extent(), 80.0);
    }

    #[test]
    fn test_sliver_geometry_visibility() {
        let visible = SliverGeometry::new(100.0, 80.0, 0.0);
        assert!(visible.is_visible());
        assert!(visible.is_hit_testable());

        let invisible = SliverGeometry::new(100.0, 80.0, 0.0).with_visible(false);
        assert!(!invisible.is_visible());
        assert!(!invisible.is_hit_testable());
    }

    #[test]
    fn test_sliver_geometry_builder() {
        let geometry = SliverGeometry::new(100.0, 80.0, 0.0)
            .with_visible(false)
            .with_visual_overflow(true)
            .with_cache_extent(200.0);

        assert!(!geometry.visible);
        assert!(geometry.has_visual_overflow);
        assert_eq!(geometry.cache_extent, Some(200.0));
    }
}
