//! RenderSliverHelpers trait - utility methods for sliver rendering.
//!
//! This module provides the `RenderSliverHelpers` trait which corresponds to
//! Flutter's `RenderSliverHelpers` mixin - helper methods for slivers that
//! contain RenderBox children.

use crate::constraints::GrowthDirection;
use flui_types::layout::{Axis, AxisDirection};
use flui_types::Offset;

use super::RenderSliver;
use crate::traits::r#box::BoxHitTestResult;
use crate::traits::RenderBox;

/// Trait providing utility methods for slivers that contain RenderBox children.
///
/// This trait provides helper methods for:
/// - Converting between sliver and box coordinate systems
/// - Hit testing box children within a sliver
/// - Applying paint transforms for box children
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliverHelpers` mixin.
///
/// # Usage
///
/// Implement this trait for slivers that wrap RenderBox children, such as
/// `RenderSliverSingleBoxAdapter` and `RenderSliverMultiBoxAdaptor`.
pub trait RenderSliverHelpers: RenderSliver {
    /// Returns the main axis position of the given child.
    ///
    /// This is the distance from the start of the sliver to the start of
    /// the child in the main axis direction.
    ///
    /// Note: This is named differently from `RenderSliver::child_main_axis_position`
    /// because that method takes `&dyn RenderObject`, while this takes `&dyn RenderBox`.
    fn box_child_main_axis_position(&self, child: &dyn RenderBox) -> f32;

    /// Returns the cross axis position of the given child.
    ///
    /// Defaults to 0.0 (child starts at the cross axis origin).
    fn box_child_cross_axis_position(&self, _child: &dyn RenderBox) -> f32 {
        0.0
    }

    /// Determines if the sliver is laid out "right way up" based on constraints.
    ///
    /// Returns true if content grows in the same direction as the axis direction,
    /// false if they are reversed.
    fn get_right_way_up(&self) -> bool {
        let constraints = self.constraints();
        let reversed = axis_direction_is_reversed(constraints.axis_direction);
        match constraints.growth_direction {
            GrowthDirection::Forward => !reversed,
            GrowthDirection::Reverse => reversed,
        }
    }

    /// Hit tests a box child within this sliver.
    ///
    /// This method converts the position from the sliver coordinate system
    /// to the Cartesian coordinate system used by RenderBox.
    ///
    /// # Arguments
    ///
    /// * `result` - The hit test result to add entries to
    /// * `child` - The box child to hit test
    /// * `main_axis_position` - Position in the main axis (sliver coordinates)
    /// * `cross_axis_position` - Position in the cross axis (sliver coordinates)
    ///
    /// # Returns
    ///
    /// True if the child was hit, false otherwise.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliverHelpers.hitTestBoxChild`.
    fn hit_test_box_child(
        &self,
        result: &mut BoxHitTestResult,
        child: &dyn RenderBox,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        let constraints = self.constraints();
        let geometry = self.geometry();

        let right_way_up = self.get_right_way_up();
        let mut delta = self.box_child_main_axis_position(child);
        let cross_axis_delta = self.box_child_cross_axis_position(child);
        let mut absolute_position = main_axis_position - delta;
        let absolute_cross_axis_position = cross_axis_position - cross_axis_delta;

        let child_size = child.size();

        let transformed_position = match constraints.axis() {
            Axis::Horizontal => {
                if !right_way_up {
                    absolute_position = child_size.width - absolute_position;
                    delta = geometry.paint_extent - child_size.width - delta;
                }
                let _paint_offset = Offset::new(delta, cross_axis_delta);
                Offset::new(absolute_position, absolute_cross_axis_position)
            }
            Axis::Vertical => {
                if !right_way_up {
                    absolute_position = child_size.height - absolute_position;
                    delta = geometry.paint_extent - child_size.height - delta;
                }
                let _paint_offset = Offset::new(cross_axis_delta, delta);
                Offset::new(absolute_cross_axis_position, absolute_position)
            }
        };

        child.hit_test(result, transformed_position)
    }

    /// Applies the paint transform for a box child.
    ///
    /// This method converts the child's position from sliver coordinates
    /// to the Cartesian coordinate system and applies the appropriate
    /// translation to the transform matrix.
    ///
    /// # Arguments
    ///
    /// * `child` - The box child
    /// * `transform` - The transform matrix to modify (column-major 4x4)
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderSliverHelpers.applyPaintTransformForBoxChild`.
    fn apply_paint_transform_for_box_child(
        &self,
        child: &dyn RenderBox,
        transform: &mut [f32; 16],
    ) {
        let constraints = self.constraints();
        let geometry = self.geometry();

        let right_way_up = self.get_right_way_up();
        let mut delta = self.box_child_main_axis_position(child);
        let cross_axis_delta = self.box_child_cross_axis_position(child);

        let child_size = child.size();

        match constraints.axis() {
            Axis::Horizontal => {
                if !right_way_up {
                    delta = geometry.paint_extent - child_size.width - delta;
                }
                translate_transform(transform, delta, cross_axis_delta);
            }
            Axis::Vertical => {
                if !right_way_up {
                    delta = geometry.paint_extent - child_size.height - delta;
                }
                translate_transform(transform, cross_axis_delta, delta);
            }
        }
    }

    /// Computes the paint offset for a box child.
    ///
    /// Returns the offset at which to paint the child, accounting for
    /// axis direction and growth direction.
    ///
    /// # Arguments
    ///
    /// * `child` - The box child
    ///
    /// # Returns
    ///
    /// The offset at which to paint the child.
    fn compute_child_paint_offset(&self, child: &dyn RenderBox) -> Offset {
        let constraints = self.constraints();
        let geometry = self.geometry();

        let right_way_up = self.get_right_way_up();
        let mut delta = self.box_child_main_axis_position(child);
        let cross_axis_delta = self.box_child_cross_axis_position(child);

        let child_size = child.size();

        match constraints.axis() {
            Axis::Horizontal => {
                if !right_way_up {
                    delta = geometry.paint_extent - child_size.width - delta;
                }
                Offset::new(delta, cross_axis_delta)
            }
            Axis::Vertical => {
                if !right_way_up {
                    delta = geometry.paint_extent - child_size.height - delta;
                }
                Offset::new(cross_axis_delta, delta)
            }
        }
    }
}

/// Returns true if the axis direction is reversed (BottomToTop or RightToLeft).
fn axis_direction_is_reversed(direction: AxisDirection) -> bool {
    matches!(
        direction,
        AxisDirection::BottomToTop | AxisDirection::RightToLeft
    )
}

/// Applies a translation to a 4x4 column-major transform matrix.
fn translate_transform(transform: &mut [f32; 16], dx: f32, dy: f32) {
    // Column-major 4x4 matrix translation:
    // transform[12] is the x translation
    // transform[13] is the y translation
    transform[12] += dx;
    transform[13] += dy;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axis_direction_is_reversed() {
        assert!(!axis_direction_is_reversed(AxisDirection::TopToBottom));
        assert!(!axis_direction_is_reversed(AxisDirection::LeftToRight));
        assert!(axis_direction_is_reversed(AxisDirection::BottomToTop));
        assert!(axis_direction_is_reversed(AxisDirection::RightToLeft));
    }

    #[test]
    fn test_translate_transform() {
        let mut transform = [
            1.0, 0.0, 0.0, 0.0, // column 0
            0.0, 1.0, 0.0, 0.0, // column 1
            0.0, 0.0, 1.0, 0.0, // column 2
            0.0, 0.0, 0.0, 1.0, // column 3
        ];

        translate_transform(&mut transform, 10.0, 20.0);

        assert_eq!(transform[12], 10.0);
        assert_eq!(transform[13], 20.0);
    }
}
