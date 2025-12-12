//! Sliver constraints for scrollable content layout

use std::fmt;

/// Direction of the scroll axis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    /// Scroll up (decreasing vertical offset)
    Up,
    /// Scroll down (increasing vertical offset)
    Down,
    /// Scroll left (decreasing horizontal offset)
    Left,
    /// Scroll right (increasing horizontal offset)
    Right,
}

impl AxisDirection {
    /// Returns the axis (horizontal or vertical)
    pub fn axis(&self) -> Axis {
        match self {
            AxisDirection::Up | AxisDirection::Down => Axis::Vertical,
            AxisDirection::Left | AxisDirection::Right => Axis::Horizontal,
        }
    }

    /// Returns whether this is a reversed direction (up or left)
    pub fn is_reversed(&self) -> bool {
        matches!(self, AxisDirection::Up | AxisDirection::Left)
    }
}

/// Axis type (horizontal or vertical)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Direction in which the content grows relative to the scroll axis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrowthDirection {
    /// Content grows in the forward direction (same as scroll direction)
    Forward,
    /// Content grows in the reverse direction (opposite of scroll direction)
    Reverse,
}

/// Direction of user scrolling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    /// User is scrolling away from the end of the content
    Idle,
    /// User is scrolling toward the end of the content
    Forward,
    /// User is scrolling toward the start of the content
    Reverse,
}

/// Immutable layout constraints for sliver protocol
///
/// SliverConstraints describe the viewport and scrolling context for a sliver.
/// Unlike BoxConstraints which describe size ranges, SliverConstraints provide
/// viewport information and the current scroll position.
///
/// # Coordinate System
/// - **Main Axis**: The axis along which scrolling occurs
/// - **Cross Axis**: The axis perpendicular to scrolling
/// - **Scroll Offset**: Current scroll position along main axis
/// - **Remaining Extent**: Space remaining in viewport
///
/// # Examples
///
/// ```ignore
/// let constraints = SliverConstraints {
///     axis_direction: AxisDirection::Down,
///     growth_direction: GrowthDirection::Forward,
///     user_scroll_direction: ScrollDirection::Forward,
///     scroll_offset: 100.0,
///     preceding_scroll_extent: 200.0,
///     overlap: 0.0,
///     remaining_paint_extent: 600.0,
///     cross_axis_extent: 400.0,
///     cross_axis_direction: AxisDirection::Right,
///     viewport_main_axis_extent: 800.0,
///     remaining_cache_extent: 1000.0,
///     cache_origin: 0.0,
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SliverConstraints {
    /// Direction of the scroll axis
    pub axis_direction: AxisDirection,

    /// Direction in which content grows
    pub growth_direction: GrowthDirection,

    /// Direction the user is currently scrolling
    pub user_scroll_direction: ScrollDirection,

    /// Current scroll offset along the main axis
    ///
    /// This is the number of pixels that have been scrolled past the leading
    /// edge of this sliver.
    pub scroll_offset: f32,

    /// Total scroll extent of all preceding slivers
    ///
    /// This is used for scroll position calculations.
    pub preceding_scroll_extent: f32,

    /// Overlap with previous sliver, in pixels
    ///
    /// When a previous sliver (like a floating header) overlaps into this
    /// sliver's space, this value indicates how much.
    pub overlap: f32,

    /// Remaining space in the viewport along the main axis
    ///
    /// This is how much space is left for this sliver to paint into, accounting
    /// for what previous slivers have already used.
    pub remaining_paint_extent: f32,

    /// Size of the viewport along the cross axis
    ///
    /// This is typically the width for a vertical scrollable, or height for
    /// a horizontal scrollable.
    pub cross_axis_extent: f32,

    /// Direction of the cross axis
    pub cross_axis_direction: AxisDirection,

    /// Total size of the viewport along the main axis
    ///
    /// This is the full viewport size, not just the remaining space.
    pub viewport_main_axis_extent: f32,

    /// Remaining cache extent along the main axis
    ///
    /// Slivers can render content beyond the visible viewport up to this limit
    /// for smoother scrolling.
    pub remaining_cache_extent: f32,

    /// Cache origin offset
    ///
    /// This is typically 0.0 but can be negative for slivers that render
    /// before the leading edge of the viewport.
    pub cache_origin: f32,
}

impl SliverConstraints {
    /// Creates new sliver constraints with common defaults
    pub fn new(
        axis_direction: AxisDirection,
        scroll_offset: f32,
        remaining_paint_extent: f32,
        cross_axis_extent: f32,
        viewport_main_axis_extent: f32,
    ) -> Self {
        Self {
            axis_direction,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent,
            cross_axis_extent,
            cross_axis_direction: match axis_direction.axis() {
                Axis::Vertical => AxisDirection::Right,
                Axis::Horizontal => AxisDirection::Down,
            },
            viewport_main_axis_extent,
            remaining_cache_extent: remaining_paint_extent,
            cache_origin: 0.0,
        }
    }

    /// Returns the scroll axis
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Returns the axis extent for layout purposes
    pub fn axis_extent(&self, width: f32, height: f32) -> f32 {
        match self.axis() {
            Axis::Horizontal => width,
            Axis::Vertical => height,
        }
    }

    /// Returns the cross axis extent for layout purposes
    pub fn cross_axis_extent_for(&self, width: f32, height: f32) -> f32 {
        match self.axis() {
            Axis::Horizontal => height,
            Axis::Vertical => width,
        }
    }

    /// Returns whether the sliver is normalized (no negative overlap)
    pub fn is_normalized(&self) -> bool {
        self.scroll_offset >= 0.0
            && self.cross_axis_extent >= 0.0
            && self.axis_direction == AxisDirection::Down
                || self.axis_direction == AxisDirection::Right
    }

    /// Creates a copy with adjusted scroll offset
    pub fn copy_with_scroll_offset(&self, scroll_offset: f32) -> Self {
        Self {
            scroll_offset,
            ..self.clone()
        }
    }

    /// Creates a copy with adjusted remaining extents
    pub fn copy_with_remaining_extents(
        &self,
        remaining_paint_extent: f32,
        remaining_cache_extent: f32,
    ) -> Self {
        Self {
            remaining_paint_extent,
            remaining_cache_extent,
            ..self.clone()
        }
    }

    /// Returns whether this sliver has any visible area
    pub fn has_visual_overflow(&self, paint_extent: f32, scroll_extent: f32) -> bool {
        paint_extent < scroll_extent || paint_extent < 0.0
    }
}

impl fmt::Display for SliverConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SliverConstraints({:?}, offset: {:.1}, remaining: {:.1}, cross: {:.1})",
            self.axis_direction,
            self.scroll_offset,
            self.remaining_paint_extent,
            self.cross_axis_extent
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axis_direction() {
        assert_eq!(AxisDirection::Down.axis(), Axis::Vertical);
        assert_eq!(AxisDirection::Up.axis(), Axis::Vertical);
        assert_eq!(AxisDirection::Right.axis(), Axis::Horizontal);
        assert_eq!(AxisDirection::Left.axis(), Axis::Horizontal);

        assert!(!AxisDirection::Down.is_reversed());
        assert!(AxisDirection::Up.is_reversed());
        assert!(!AxisDirection::Right.is_reversed());
        assert!(AxisDirection::Left.is_reversed());
    }

    #[test]
    fn test_sliver_constraints_new() {
        let constraints = SliverConstraints::new(
            AxisDirection::Down,
            100.0,
            500.0,
            400.0,
            800.0,
        );

        assert_eq!(constraints.axis_direction, AxisDirection::Down);
        assert_eq!(constraints.scroll_offset, 100.0);
        assert_eq!(constraints.remaining_paint_extent, 500.0);
        assert_eq!(constraints.cross_axis_extent, 400.0);
        assert_eq!(constraints.viewport_main_axis_extent, 800.0);
        assert_eq!(constraints.growth_direction, GrowthDirection::Forward);
    }

    #[test]
    fn test_copy_with_scroll_offset() {
        let constraints = SliverConstraints::new(
            AxisDirection::Down,
            100.0,
            500.0,
            400.0,
            800.0,
        );

        let modified = constraints.copy_with_scroll_offset(200.0);
        assert_eq!(modified.scroll_offset, 200.0);
        assert_eq!(modified.remaining_paint_extent, 500.0);
    }
}
