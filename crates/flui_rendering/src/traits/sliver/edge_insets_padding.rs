//! RenderSliverEdgeInsetsPadding trait - sliver padding with edge insets.
//!
//! This module provides the `RenderSliverEdgeInsetsPadding` trait which corresponds to
//! Flutter's `RenderSliverEdgeInsetsPadding` class - an abstract class for slivers
//! that apply padding using edge insets.

use crate::constraints::GrowthDirection;
use flui_types::layout::{Axis, AxisDirection, EdgeInsets};

use super::RenderSliver;

/// Trait for slivers that apply padding using edge insets.
///
/// RenderSliverEdgeInsetsPadding contains a single sliver child and pads it
/// with the specified edge insets. The padding is resolved relative to the
/// scroll direction.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliverEdgeInsetsPadding` abstract class.
///
/// # Padding Direction
///
/// - `before_padding`: Padding on the side nearest scroll offset 0
/// - `after_padding`: Padding on the side furthest from scroll offset 0
/// - `main_axis_padding`: Total padding along the scroll direction
/// - `cross_axis_padding`: Total padding perpendicular to scroll direction
///
/// # Usage
///
/// Implement this trait for slivers that need to apply padding, such as
/// `RenderSliverPadding`.
pub trait RenderSliverEdgeInsetsPadding: RenderSliver {
    /// Returns the resolved padding.
    ///
    /// The offsets are specified in visual edges (left, top, right, bottom),
    /// not affected by text direction.
    ///
    /// Returns `None` if padding hasn't been resolved yet.
    fn resolved_padding(&self) -> Option<EdgeInsets>;

    /// Returns the padding in the scroll direction on the side nearest scroll offset 0.
    ///
    /// Only valid after layout has started.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliverEdgeInsetsPadding.beforePadding`.
    fn before_padding(&self) -> f32 {
        let padding = self
            .resolved_padding()
            .expect("resolved_padding must be set before layout");
        let constraints = self.constraints();

        let effective_direction = apply_growth_direction_to_axis_direction(
            constraints.axis_direction,
            constraints.growth_direction,
        );

        match effective_direction {
            AxisDirection::BottomToTop => padding.bottom,
            AxisDirection::LeftToRight => padding.left,
            AxisDirection::TopToBottom => padding.top,
            AxisDirection::RightToLeft => padding.right,
        }
    }

    /// Returns the padding in the scroll direction on the side furthest from scroll offset 0.
    ///
    /// Only valid after layout has started.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliverEdgeInsetsPadding.afterPadding`.
    fn after_padding(&self) -> f32 {
        let padding = self
            .resolved_padding()
            .expect("resolved_padding must be set before layout");
        let constraints = self.constraints();

        let effective_direction = apply_growth_direction_to_axis_direction(
            constraints.axis_direction,
            constraints.growth_direction,
        );

        match effective_direction {
            AxisDirection::BottomToTop => padding.top,
            AxisDirection::LeftToRight => padding.right,
            AxisDirection::TopToBottom => padding.bottom,
            AxisDirection::RightToLeft => padding.left,
        }
    }

    /// Returns the total padding in the main axis direction.
    ///
    /// For a vertical list, this is `padding.top + padding.bottom`.
    /// For a horizontal list, this is `padding.left + padding.right`.
    ///
    /// Only valid after layout has started.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliverEdgeInsetsPadding.mainAxisPadding`.
    fn main_axis_padding(&self) -> f32 {
        let padding = self
            .resolved_padding()
            .expect("resolved_padding must be set before layout");
        let constraints = self.constraints();

        match constraints.axis() {
            Axis::Horizontal => padding.horizontal_total(),
            Axis::Vertical => padding.vertical_total(),
        }
    }

    /// Returns the total padding in the cross axis direction.
    ///
    /// For a vertical list, this is `padding.left + padding.right`.
    /// For a horizontal list, this is `padding.top + padding.bottom`.
    ///
    /// Only valid after layout has started.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliverEdgeInsetsPadding.crossAxisPadding`.
    fn cross_axis_padding(&self) -> f32 {
        let padding = self
            .resolved_padding()
            .expect("resolved_padding must be set before layout");
        let constraints = self.constraints();

        match constraints.axis() {
            Axis::Horizontal => padding.vertical_total(),
            Axis::Vertical => padding.horizontal_total(),
        }
    }
}

/// Flips the axis direction if the growth direction is reverse.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `applyGrowthDirectionToAxisDirection`.
pub fn apply_growth_direction_to_axis_direction(
    axis_direction: AxisDirection,
    growth_direction: GrowthDirection,
) -> AxisDirection {
    match growth_direction {
        GrowthDirection::Forward => axis_direction,
        GrowthDirection::Reverse => flip_axis_direction(axis_direction),
    }
}

/// Flips an axis direction to its opposite.
///
/// - TopToBottom ↔ BottomToTop
/// - LeftToRight ↔ RightToLeft
fn flip_axis_direction(direction: AxisDirection) -> AxisDirection {
    match direction {
        AxisDirection::BottomToTop => AxisDirection::TopToBottom,
        AxisDirection::TopToBottom => AxisDirection::BottomToTop,
        AxisDirection::RightToLeft => AxisDirection::LeftToRight,
        AxisDirection::LeftToRight => AxisDirection::RightToLeft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flip_axis_direction() {
        assert_eq!(
            flip_axis_direction(AxisDirection::BottomToTop),
            AxisDirection::TopToBottom
        );
        assert_eq!(
            flip_axis_direction(AxisDirection::TopToBottom),
            AxisDirection::BottomToTop
        );
        assert_eq!(
            flip_axis_direction(AxisDirection::RightToLeft),
            AxisDirection::LeftToRight
        );
        assert_eq!(
            flip_axis_direction(AxisDirection::LeftToRight),
            AxisDirection::RightToLeft
        );
    }

    #[test]
    fn test_apply_growth_direction_forward() {
        assert_eq!(
            apply_growth_direction_to_axis_direction(
                AxisDirection::TopToBottom,
                GrowthDirection::Forward
            ),
            AxisDirection::TopToBottom
        );
        assert_eq!(
            apply_growth_direction_to_axis_direction(
                AxisDirection::BottomToTop,
                GrowthDirection::Forward
            ),
            AxisDirection::BottomToTop
        );
    }

    #[test]
    fn test_apply_growth_direction_reverse() {
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
}
