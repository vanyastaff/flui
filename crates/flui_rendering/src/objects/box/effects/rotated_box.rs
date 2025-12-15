//! RenderRotatedBox - rotates child by quarter turns.
//!
//! This render object rotates its child by 90-degree increments.
//! For arbitrary rotations, use RenderTransform instead.

use flui_types::{Offset, Point, Rect, Size};

use crate::constraints::BoxConstraints;

use crate::containers::Single;
use crate::pipeline::PaintingContext;
use crate::protocol::BoxProtocol;
use crate::traits::TextBaseline;

/// Number of clockwise quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum QuarterTurns {
    /// No rotation (0°).
    #[default]
    Zero = 0,
    /// One quarter turn clockwise (90°).
    One = 1,
    /// Two quarter turns (180°).
    Two = 2,
    /// Three quarter turns clockwise (270° or -90°).
    Three = 3,
}

impl QuarterTurns {
    /// Creates from an integer (mod 4).
    pub fn from_int(turns: i32) -> Self {
        match turns.rem_euclid(4) {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            _ => unreachable!(),
        }
    }

    /// Returns the number of quarter turns as an integer.
    pub fn as_int(&self) -> u8 {
        *self as u8
    }

    /// Returns whether the axes are swapped (90° or 270°).
    pub fn swaps_axes(&self) -> bool {
        matches!(self, Self::One | Self::Three)
    }

    /// Returns the angle in radians.
    pub fn radians(&self) -> f32 {
        std::f32::consts::FRAC_PI_2 * (*self as u8 as f32)
    }
}

/// A render object that rotates its child by quarter turns.
///
/// The child is rotated around its center. For 90° and 270° rotations,
/// the width and height constraints are swapped for the child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::{RenderRotatedBox, QuarterTurns};
///
/// // Rotate 90 degrees clockwise
/// let rotated = RenderRotatedBox::new(QuarterTurns::One);
///
/// // Rotate 180 degrees (upside down)
/// let flipped = RenderRotatedBox::new(QuarterTurns::Two);
/// ```
#[derive(Debug)]
pub struct RenderRotatedBox {
    /// Container for the child.
    single: Single<BoxProtocol>,

    /// Number of quarter turns.
    quarter_turns: QuarterTurns,

    /// Cached size.
    size: Size,
}

impl RenderRotatedBox {
    /// Creates a new rotated box.
    pub fn new(quarter_turns: QuarterTurns) -> Self {
        Self {
            single: Single::new(),
            quarter_turns,
            size: Size::ZERO,
        }
    }

    /// Creates with no rotation.
    pub fn no_rotation() -> Self {
        Self::new(QuarterTurns::Zero)
    }

    /// Creates rotated 90 degrees clockwise.
    pub fn clockwise() -> Self {
        Self::new(QuarterTurns::One)
    }

    /// Creates rotated 180 degrees.
    pub fn upside_down() -> Self {
        Self::new(QuarterTurns::Two)
    }

    /// Creates rotated 90 degrees counter-clockwise.
    pub fn counter_clockwise() -> Self {
        Self::new(QuarterTurns::Three)
    }

    /// Returns the number of quarter turns.
    pub fn quarter_turns(&self) -> QuarterTurns {
        self.quarter_turns
    }

    /// Sets the number of quarter turns.
    pub fn set_quarter_turns(&mut self, quarter_turns: QuarterTurns) {
        if self.quarter_turns != quarter_turns {
            self.quarter_turns = quarter_turns;
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Transforms constraints for the child based on rotation.
    fn transform_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        if self.quarter_turns.swaps_axes() {
            // Swap width and height constraints
            BoxConstraints::new(
                constraints.min_height,
                constraints.max_height,
                constraints.min_width,
                constraints.max_width,
            )
        } else {
            constraints
        }
    }

    /// Transforms size from child to parent coordinates.
    fn transform_size(&self, child_size: Size) -> Size {
        if self.quarter_turns.swaps_axes() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
        }
    }

    /// Computes the child offset for painting.
    fn compute_child_offset(&self) -> Offset {
        let size = self.size;
        match self.quarter_turns {
            QuarterTurns::Zero => Offset::ZERO,
            QuarterTurns::One => Offset::new(size.width, 0.0),
            QuarterTurns::Two => Offset::new(size.width, size.height),
            QuarterTurns::Three => Offset::new(0.0, size.height),
        }
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = constraints.smallest();
        self.size = size;
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        _constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        let size = self.transform_size(child_size);
        self.size = size;
        size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        self.transform_constraints(constraints)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.quarter_turns == QuarterTurns::Zero {
            // No rotation needed
            let _ = (context, offset);
            // In real implementation: context.paint_child(child, offset);
            return;
        }

        let child_offset = self.compute_child_offset();
        let radians = self.quarter_turns.radians();

        // In real implementation:
        // context.push_transform(
        //     offset,
        //     Matrix4::rotation_z(radians) * Matrix4::translation(child_offset),
        //     |ctx| ctx.paint_child(child, Offset::ZERO)
        // );
        let _ = (context, offset, child_offset, radians);
    }

    /// Transforms a point from parent to child coordinates.
    pub fn parent_to_child(&self, point: Point, parent_size: Size, child_size: Size) -> Point {
        match self.quarter_turns {
            QuarterTurns::Zero => point,
            QuarterTurns::One => Point::new(point.y, parent_size.width - point.x),
            QuarterTurns::Two => {
                Point::new(parent_size.width - point.x, parent_size.height - point.y)
            }
            QuarterTurns::Three => Point::new(child_size.width - point.y, point.x),
        }
    }

    /// Hit test with rotation applied.
    pub fn hit_test(&self, position: Offset, child_size: Size) -> bool {
        let parent_size = self.size;
        let child_point = self.parent_to_child(
            Point::new(position.dx, position.dy),
            parent_size,
            child_size,
        );
        let child_rect = Rect::from_origin_size(Point::ZERO, child_size);
        child_rect.contains(child_point)
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        if self.quarter_turns.swaps_axes() {
            // Child's height becomes our width
            child_width.unwrap_or(0.0)
        } else {
            // Child's width is our width
            let _ = height;
            child_width.unwrap_or(0.0)
        }
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        self.compute_min_intrinsic_width(height, child_width)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        if self.quarter_turns.swaps_axes() {
            // Child's width becomes our height
            child_height.unwrap_or(0.0)
        } else {
            // Child's height is our height
            let _ = width;
            child_height.unwrap_or(0.0)
        }
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        self.compute_min_intrinsic_height(width, child_height)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        _child_baseline: Option<f32>,
    ) -> Option<f32> {
        // Baseline doesn't make sense for rotated content
        None
    }
}

impl Default for RenderRotatedBox {
    fn default() -> Self {
        Self::no_rotation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quarter_turns_from_int() {
        assert_eq!(QuarterTurns::from_int(0), QuarterTurns::Zero);
        assert_eq!(QuarterTurns::from_int(1), QuarterTurns::One);
        assert_eq!(QuarterTurns::from_int(2), QuarterTurns::Two);
        assert_eq!(QuarterTurns::from_int(3), QuarterTurns::Three);
        assert_eq!(QuarterTurns::from_int(4), QuarterTurns::Zero);
        assert_eq!(QuarterTurns::from_int(-1), QuarterTurns::Three);
    }

    #[test]
    fn test_quarter_turns_swaps_axes() {
        assert!(!QuarterTurns::Zero.swaps_axes());
        assert!(QuarterTurns::One.swaps_axes());
        assert!(!QuarterTurns::Two.swaps_axes());
        assert!(QuarterTurns::Three.swaps_axes());
    }

    #[test]
    fn test_transform_constraints_no_rotation() {
        let rotated = RenderRotatedBox::no_rotation();
        let constraints = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);

        let transformed = rotated.transform_constraints(constraints);

        assert_eq!(transformed, constraints);
    }

    #[test]
    fn test_transform_constraints_quarter_turn() {
        let rotated = RenderRotatedBox::clockwise();
        let constraints = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);

        let transformed = rotated.transform_constraints(constraints);

        assert_eq!(transformed.min_width, 20.0);
        assert_eq!(transformed.max_width, 200.0);
        assert_eq!(transformed.min_height, 10.0);
        assert_eq!(transformed.max_height, 100.0);
    }

    #[test]
    fn test_transform_size() {
        let rotated = RenderRotatedBox::clockwise();
        let child_size = Size::new(100.0, 50.0);

        let parent_size = rotated.transform_size(child_size);

        assert_eq!(parent_size.width, 50.0);
        assert_eq!(parent_size.height, 100.0);
    }

    #[test]
    fn test_layout() {
        let mut rotated = RenderRotatedBox::clockwise();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);
        let child_size = Size::new(100.0, 50.0);

        let size = rotated.perform_layout_with_child(constraints, child_size);

        // Child 100x50 rotated 90° becomes 50x100
        assert_eq!(size.width, 50.0);
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn test_parent_to_child_no_rotation() {
        let rotated = RenderRotatedBox::no_rotation();
        let point = Point::new(10.0, 20.0);

        let child_point =
            rotated.parent_to_child(point, Size::new(100.0, 100.0), Size::new(100.0, 100.0));

        assert_eq!(child_point, point);
    }

    #[test]
    fn test_parent_to_child_half_rotation() {
        let rotated = RenderRotatedBox::upside_down();
        let point = Point::new(10.0, 20.0);
        let parent_size = Size::new(100.0, 100.0);

        let child_point = rotated.parent_to_child(point, parent_size, Size::new(100.0, 100.0));

        assert!((child_point.x - 90.0).abs() < f32::EPSILON);
        assert!((child_point.y - 80.0).abs() < f32::EPSILON);
    }
}
