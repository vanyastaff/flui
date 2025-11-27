//! SliverConstraints - Constraints for sliver layout

use crate::constraints::direction::{GrowthDirection, ScrollDirection};
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
/// use flui_types::constraints::ScrollDirection;
///
/// let constraints = SliverConstraints {
///     axis_direction: AxisDirection::TopToBottom,
///     growth_direction: GrowthDirection::Forward,
///     user_scroll_direction: ScrollDirection::Idle,
///     scroll_offset: 0.0,
///     preceding_scroll_extent: 0.0,
///     overlap: 0.0,
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
    /// The direction in which the sliver's contents are ordered
    ///
    /// This is the direction in which scroll offset increases.
    pub axis_direction: AxisDirection,

    /// The direction in which slivers and their content are laid out
    ///
    /// - `Forward`: Slivers are laid out in the positive scroll direction
    /// - `Reverse`: Slivers are laid out in the negative scroll direction
    ///
    /// This is used for bidirectional scrolling with a center sliver.
    pub growth_direction: GrowthDirection,

    /// The direction the user is attempting to scroll
    ///
    /// This is useful for slivers that want to respond to the user's
    /// scroll direction, e.g., to show/hide content based on scroll.
    pub user_scroll_direction: ScrollDirection,

    /// The scroll offset of the leading edge of the sliver
    ///
    /// 0.0 means the leading edge is at the leading edge of the viewport.
    /// Negative means the sliver is scrolled past the leading edge.
    pub scroll_offset: f32,

    /// The scroll distance consumed by all slivers that came before this one
    ///
    /// For the first sliver, this is 0.0. For subsequent slivers, it's the
    /// sum of all previous slivers' scroll extents.
    pub preceding_scroll_extent: f32,

    /// The overlap from the previous sliver painting beyond its layout extent
    ///
    /// This is positive when a previous sliver painted beyond its allocated
    /// space (e.g., a pinned header that overlays content).
    pub overlap: f32,

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
    /// Create new sliver constraints with all fields
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

    /// Get the axis (vertical or horizontal)
    #[inline]
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Check if sliver has infinite paint extent
    #[inline]
    pub fn has_infinite_paint_extent(&self) -> bool {
        self.remaining_paint_extent.is_infinite()
    }

    /// Get the normalized growth direction
    ///
    /// Returns the actual direction content grows, taking into account
    /// both the axis direction and growth direction.
    #[inline]
    pub fn normalized_growth_direction(&self) -> AxisDirection {
        apply_growth_direction_to_axis_direction(self.axis_direction, self.growth_direction)
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

    /// Copy with modified overlap
    pub fn copy_with_overlap(&self, overlap: f32) -> Self {
        Self { overlap, ..*self }
    }

    /// Copy with modified preceding scroll extent
    pub fn copy_with_preceding_scroll_extent(&self, preceding_scroll_extent: f32) -> Self {
        Self {
            preceding_scroll_extent,
            ..*self
        }
    }

    /// Check if these constraints are normalized (no negative scroll offset)
    #[inline]
    pub fn is_normalized(&self) -> bool {
        self.scroll_offset >= 0.0
            && self.overlap >= 0.0
            && self.remaining_paint_extent >= 0.0
            && self.remaining_cache_extent >= 0.0
            && self.cross_axis_extent >= 0.0
    }

    /// Get the amount this sliver would be scrolled beyond the leading edge
    ///
    /// Returns 0 if the sliver is not scrolled beyond the leading edge.
    #[inline]
    pub fn scroll_offset_beyond_leading_edge(&self) -> f32 {
        (-self.scroll_offset).max(0.0)
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

/// Apply growth direction to an axis direction
///
/// If growth is Forward, returns the axis direction unchanged.
/// If growth is Reverse, returns the flipped axis direction.
///
/// # Example
///
/// ```rust
/// use flui_types::layout::AxisDirection;
/// use flui_types::constraints::GrowthDirection;
/// use flui_types::sliver_constraints::apply_growth_direction_to_axis_direction;
///
/// // Forward keeps direction unchanged
/// assert_eq!(
///     apply_growth_direction_to_axis_direction(AxisDirection::TopToBottom, GrowthDirection::Forward),
///     AxisDirection::TopToBottom
/// );
///
/// // Reverse flips the direction
/// assert_eq!(
///     apply_growth_direction_to_axis_direction(AxisDirection::TopToBottom, GrowthDirection::Reverse),
///     AxisDirection::BottomToTop
/// );
/// ```
#[inline]
pub fn apply_growth_direction_to_axis_direction(
    axis_direction: AxisDirection,
    growth_direction: GrowthDirection,
) -> AxisDirection {
    match growth_direction {
        GrowthDirection::Forward => axis_direction,
        GrowthDirection::Reverse => axis_direction.opposite(),
    }
}

/// Apply growth direction to a scroll direction
///
/// If growth is Forward, returns the scroll direction unchanged.
/// If growth is Reverse, returns the flipped scroll direction.
///
/// # Example
///
/// ```rust
/// use flui_types::constraints::{GrowthDirection, ScrollDirection};
/// use flui_types::sliver_constraints::apply_growth_direction_to_scroll_direction;
///
/// // Forward keeps direction unchanged
/// assert_eq!(
///     apply_growth_direction_to_scroll_direction(ScrollDirection::Forward, GrowthDirection::Forward),
///     ScrollDirection::Forward
/// );
///
/// // Reverse flips the direction
/// assert_eq!(
///     apply_growth_direction_to_scroll_direction(ScrollDirection::Forward, GrowthDirection::Reverse),
///     ScrollDirection::Reverse
/// );
///
/// // Idle stays idle
/// assert_eq!(
///     apply_growth_direction_to_scroll_direction(ScrollDirection::Idle, GrowthDirection::Reverse),
///     ScrollDirection::Idle
/// );
/// ```
#[inline]
pub fn apply_growth_direction_to_scroll_direction(
    scroll_direction: ScrollDirection,
    growth_direction: GrowthDirection,
) -> ScrollDirection {
    match growth_direction {
        GrowthDirection::Forward => scroll_direction,
        GrowthDirection::Reverse => scroll_direction.flip(),
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
            ScrollDirection::Idle,
            100.0,
            50.0,
            10.0,
            500.0,
            400.0,
            AxisDirection::LeftToRight,
            600.0,
            1000.0,
            0.0,
        );

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.growth_direction, GrowthDirection::Forward);
        assert_eq!(constraints.user_scroll_direction, ScrollDirection::Idle);
        assert_eq!(constraints.scroll_offset, 100.0);
        assert_eq!(constraints.preceding_scroll_extent, 50.0);
        assert_eq!(constraints.overlap, 10.0);
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
    fn test_normalized_growth_direction() {
        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            ..Default::default()
        };
        assert_eq!(
            constraints.normalized_growth_direction(),
            AxisDirection::TopToBottom
        );

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Reverse,
            ..Default::default()
        };
        assert_eq!(
            constraints.normalized_growth_direction(),
            AxisDirection::BottomToTop
        );
    }

    #[test]
    fn test_scroll_offset_beyond_leading_edge() {
        let constraints = SliverConstraints {
            scroll_offset: -50.0,
            ..Default::default()
        };
        assert_eq!(constraints.scroll_offset_beyond_leading_edge(), 50.0);

        let constraints = SliverConstraints {
            scroll_offset: 100.0,
            ..Default::default()
        };
        assert_eq!(constraints.scroll_offset_beyond_leading_edge(), 0.0);
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
    fn test_copy_with_overlap() {
        let constraints = SliverConstraints::default();
        let modified = constraints.copy_with_overlap(25.0);

        assert_eq!(modified.overlap, 25.0);
    }

    #[test]
    fn test_copy_with_preceding_scroll_extent() {
        let constraints = SliverConstraints::default();
        let modified = constraints.copy_with_preceding_scroll_extent(500.0);

        assert_eq!(modified.preceding_scroll_extent, 500.0);
    }

    #[test]
    fn test_is_normalized() {
        let constraints = SliverConstraints {
            scroll_offset: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 100.0,
            remaining_cache_extent: 100.0,
            cross_axis_extent: 100.0,
            ..Default::default()
        };
        assert!(constraints.is_normalized());

        let constraints = SliverConstraints {
            scroll_offset: -10.0,
            ..Default::default()
        };
        assert!(!constraints.is_normalized());
    }

    #[test]
    fn test_default() {
        let constraints = SliverConstraints::default();

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.growth_direction, GrowthDirection::Forward);
        assert_eq!(constraints.user_scroll_direction, ScrollDirection::Idle);
        assert_eq!(constraints.scroll_offset, 0.0);
        assert_eq!(constraints.preceding_scroll_extent, 0.0);
        assert_eq!(constraints.overlap, 0.0);
        assert_eq!(constraints.remaining_paint_extent, 0.0);
    }

    #[test]
    fn test_apply_growth_direction_to_axis_direction() {
        assert_eq!(
            apply_growth_direction_to_axis_direction(
                AxisDirection::TopToBottom,
                GrowthDirection::Forward
            ),
            AxisDirection::TopToBottom
        );
        assert_eq!(
            apply_growth_direction_to_axis_direction(
                AxisDirection::TopToBottom,
                GrowthDirection::Reverse
            ),
            AxisDirection::BottomToTop
        );
        assert_eq!(
            apply_growth_direction_to_axis_direction(
                AxisDirection::LeftToRight,
                GrowthDirection::Reverse
            ),
            AxisDirection::RightToLeft
        );
    }

    #[test]
    fn test_apply_growth_direction_to_scroll_direction() {
        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Forward,
                GrowthDirection::Forward
            ),
            ScrollDirection::Forward
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Forward,
                GrowthDirection::Reverse
            ),
            ScrollDirection::Reverse
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Idle,
                GrowthDirection::Reverse
            ),
            ScrollDirection::Idle
        );
    }
}
