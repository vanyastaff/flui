//! SliverConstraints - Constraints for sliver layout

use crate::layout::{Axis, AxisDirection};

/// Immutable layout constraints for slivers
///
/// Slivers use a different constraint/sizing model than boxes.
/// Instead of BoxConstraints, they use SliverConstraints which
/// describe the scroll state from the sliver's perspective.
///
/// # Coordinate System
///
/// - **scroll offset**: Distance from the leading edge of the sliver to the
///   leading edge of the viewport. Negative means the sliver is scrolled
///   beyond the leading edge.
/// - **paint extent**: The amount of visual space that should be painted
/// - **cache extent**: The amount that should be cached for smooth scrolling
///
/// # Example
///
/// ```rust,ignore
/// use flui_types::{SliverConstraints, Axis, AxisDirection};
///
/// let constraints = SliverConstraints {
///     axis_direction: AxisDirection::TopToBottom,
///     grow_direction_reversed: false,
///     scroll_offset: 0.0,
///     remaining_paint_extent: 600.0,
///     cross_axis_extent: 400.0,
///     cross_axis_direction: AxisDirection::LeftToRight,
///     viewport_main_axis_extent: 600.0,
///     remaining_cache_extent: 1000.0,
///     cache_origin: 0.0,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverConstraints {
    /// The direction in which the sliver is growing
    pub axis_direction: AxisDirection,

    /// Whether the sliver grows in the reverse direction
    pub grow_direction_reversed: bool,

    /// The scroll offset of the leading edge of the sliver
    ///
    /// 0.0 means the leading edge is at the leading edge of the viewport.
    /// Negative means the sliver is scrolled past the leading edge.
    pub scroll_offset: f32,

    /// The amount of space available for painting
    ///
    /// This is the portion of the viewport that hasn't been consumed
    /// by previous slivers.
    pub remaining_paint_extent: f32,

    /// The extent of the cross axis
    ///
    /// For a vertical scrolling viewport, this is the width.
    /// For a horizontal scrolling viewport, this is the height.
    pub cross_axis_extent: f32,

    /// The direction of the cross axis
    pub cross_axis_direction: AxisDirection,

    /// The total extent of the viewport's main axis
    pub viewport_main_axis_extent: f32,

    /// The cache extent remaining for this sliver
    ///
    /// Slivers should create geometry for all children that would
    /// fit in this extent, even if they're not visible.
    pub remaining_cache_extent: f32,

    /// The cache origin
    ///
    /// Distance from the leading edge of the cache extent to the
    /// leading edge of the sliver.
    pub cache_origin: f32,
}

impl SliverConstraints {
    /// Create new sliver constraints
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        axis_direction: AxisDirection,
        grow_direction_reversed: bool,
        scroll_offset: f32,
        remaining_paint_extent: f32,
        cross_axis_extent: f32,
        cross_axis_direction: AxisDirection,
        viewport_main_axis_extent: f32,
        remaining_cache_extent: f32,
        cache_origin: f32,
    ) -> Self {
        Self {
            axis_direction,
            grow_direction_reversed,
            scroll_offset,
            remaining_paint_extent,
            cross_axis_extent,
            cross_axis_direction,
            viewport_main_axis_extent,
            remaining_cache_extent,
            cache_origin,
        }
    }

    /// Get the axis (vertical or horizontal)
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Check if sliver has infinite paint extent
    pub fn has_infinite_paint_extent(&self) -> bool {
        self.remaining_paint_extent.is_infinite()
    }

    /// Get the overlap amount
    ///
    /// Positive when the sliver is scrolled beyond the leading edge
    pub fn overlap(&self) -> f32 {
        -self.scroll_offset.min(0.0)
    }

    /// Copy with modified scroll offset
    pub fn copy_with_scroll_offset(&self, scroll_offset: f32) -> Self {
        Self {
            scroll_offset,
            ..*self
        }
    }

    /// Copy with modified remaining paint extent
    pub fn copy_with_remaining_paint_extent(&self, remaining_paint_extent: f32) -> Self {
        Self {
            remaining_paint_extent,
            ..*self
        }
    }
}

impl Default for SliverConstraints {
    fn default() -> Self {
        Self {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 0.0,
            cross_axis_extent: 0.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 0.0,
            remaining_cache_extent: 0.0,
            cache_origin: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_constraints_new() {
        let constraints = SliverConstraints::new(
            AxisDirection::TopToBottom,
            false,
            100.0,
            500.0,
            400.0,
            AxisDirection::LeftToRight,
            600.0,
            1000.0,
            0.0,
        );

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert!(!constraints.grow_direction_reversed);
        assert_eq!(constraints.scroll_offset, 100.0);
        assert_eq!(constraints.remaining_paint_extent, 500.0);
    }

    #[test]
    fn test_axis() {
        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            ..Default::default()
        };

        assert_eq!(constraints.axis(), Axis::Vertical);
    }

    #[test]
    fn test_has_infinite_paint_extent() {
        let mut constraints = SliverConstraints::default();

        assert!(!constraints.has_infinite_paint_extent());

        constraints.remaining_paint_extent = f32::INFINITY;
        assert!(constraints.has_infinite_paint_extent());
    }

    #[test]
    fn test_overlap_positive() {
        let constraints = SliverConstraints {
            scroll_offset: -50.0,
            ..Default::default()
        };

        assert_eq!(constraints.overlap(), 50.0);
    }

    #[test]
    fn test_overlap_zero() {
        let constraints = SliverConstraints {
            scroll_offset: 100.0,
            ..Default::default()
        };

        assert_eq!(constraints.overlap(), 0.0);
    }

    #[test]
    fn test_copy_with_scroll_offset() {
        let constraints = SliverConstraints::default();
        let modified = constraints.copy_with_scroll_offset(250.0);

        assert_eq!(modified.scroll_offset, 250.0);
        assert_eq!(
            modified.remaining_paint_extent,
            constraints.remaining_paint_extent
        );
    }

    #[test]
    fn test_copy_with_remaining_paint_extent() {
        let constraints = SliverConstraints::default();
        let modified = constraints.copy_with_remaining_paint_extent(350.0);

        assert_eq!(modified.remaining_paint_extent, 350.0);
        assert_eq!(modified.scroll_offset, constraints.scroll_offset);
    }

    #[test]
    fn test_default() {
        let constraints = SliverConstraints::default();

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert!(!constraints.grow_direction_reversed);
        assert_eq!(constraints.scroll_offset, 0.0);
        assert_eq!(constraints.remaining_paint_extent, 0.0);
    }
}
