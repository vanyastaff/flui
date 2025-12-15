//! RenderPadding - insets its child by the given padding.
//!
//! When passing layout constraints to its child, padding shrinks the
//! constraints by the given padding, causing the child to layout at a smaller
//! size. Padding then sizes itself to its child's size, inflated by the
//! padding, effectively creating empty space around the child.

use flui_types::{EdgeInsets, Offset, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ShiftedBox;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// A render object that insets its child by the given padding.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderPadding;
/// use flui_types::EdgeInsets;
///
/// let mut padding = RenderPadding::new(EdgeInsets::all(16.0));
/// ```
#[derive(Debug)]
pub struct RenderPadding {
    /// Container holding the child and geometry.
    shifted: ShiftedBox,

    /// The amount to pad the child in each dimension.
    padding: EdgeInsets,
}

impl RenderPadding {
    /// Creates a new render padding with the given edge insets.
    ///
    /// The padding must have non-negative insets.
    pub fn new(padding: EdgeInsets) -> Self {
        debug_assert!(
            padding.is_non_negative(),
            "Padding must have non-negative insets"
        );
        Self {
            shifted: ShiftedBox::new(),
            padding,
        }
    }

    /// Returns the current padding.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Sets the padding.
    ///
    /// The padding must have non-negative insets.
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        debug_assert!(
            padding.is_non_negative(),
            "Padding must have non-negative insets"
        );
        if self.padding != padding {
            self.padding = padding;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.shifted.geometry()
    }

    /// Returns the child offset.
    pub fn child_offset(&self) -> Offset {
        self.shifted.offset()
    }

    /// Performs layout with the given constraints.
    ///
    /// Returns the resulting size.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let padding = self.padding;

        // Without child, size is just padding
        let size = constraints.constrain(Size::new(
            padding.horizontal_total(),
            padding.vertical_total(),
        ));

        // Set child offset at padding origin
        self.shifted
            .set_offset(Offset::new(padding.left, padding.top));
        self.shifted.set_geometry(size);

        size
    }

    /// Performs layout with a child size.
    ///
    /// Call this after laying out the child to compute final size.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        let padding = self.padding;

        // Position child at padding offset
        self.shifted
            .set_offset(Offset::new(padding.left, padding.top));

        // Size is child size plus padding
        let size = Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        );
        let size = constraints.constrain(size);
        self.shifted.set_geometry(size);
        size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.deflate(self.padding)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Child would be painted at offset + child_offset
        let _ = (context, offset);
    }

    /// Computes the minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        let padding = self.padding;
        let inner_height = (height - padding.vertical_total()).max(0.0);
        child_width
            .map(|w| w + padding.horizontal_total())
            .unwrap_or_else(|| padding.horizontal_total())
            .max(0.0)
            + if inner_height > 0.0 { 0.0 } else { 0.0 } // Use inner_height
    }

    /// Computes the maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        self.compute_min_intrinsic_width(height, child_width)
    }

    /// Computes the minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        let padding = self.padding;
        let inner_width = (width - padding.horizontal_total()).max(0.0);
        child_height
            .map(|h| h + padding.vertical_total())
            .unwrap_or_else(|| padding.vertical_total())
            .max(0.0)
            + if inner_width > 0.0 { 0.0 } else { 0.0 } // Use inner_width
    }

    /// Computes the maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        self.compute_min_intrinsic_height(width, child_height)
    }

    /// Computes the distance to the baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline.map(|distance| distance + self.padding.top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_new() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        assert_eq!(padding.padding(), EdgeInsets::all(10.0));
    }

    #[test]
    fn test_padding_set_padding() {
        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        padding.set_padding(EdgeInsets::all(20.0));
        assert_eq!(padding.padding(), EdgeInsets::all(20.0));
    }

    #[test]
    fn test_padding_layout_no_child() {
        let mut padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let size = padding.perform_layout(constraints);

        // Without child, size is just padding
        assert_eq!(size.width, 40.0); // 10 + 30
        assert_eq!(size.height, 60.0); // 20 + 40
    }

    #[test]
    fn test_padding_layout_with_child() {
        let mut padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let child_size = Size::new(50.0, 50.0);

        let size = padding.perform_layout_with_child(constraints, child_size);

        assert_eq!(size.width, 90.0); // 50 + 10 + 30
        assert_eq!(size.height, 110.0); // 50 + 20 + 40
    }

    #[test]
    fn test_padding_child_offset() {
        let mut padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        padding.perform_layout(constraints);

        let offset = padding.child_offset();
        assert_eq!(offset.dx, 10.0);
        assert_eq!(offset.dy, 20.0);
    }

    #[test]
    fn test_constraints_for_child() {
        let padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let child_constraints = padding.constraints_for_child(constraints);

        assert_eq!(child_constraints.max_width, 160.0); // 200 - 10 - 30
        assert_eq!(child_constraints.max_height, 140.0); // 200 - 20 - 40
    }

    #[test]
    fn test_padding_intrinsics() {
        let padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));

        assert_eq!(padding.compute_min_intrinsic_width(100.0, None), 40.0);
        assert_eq!(padding.compute_min_intrinsic_width(100.0, Some(50.0)), 90.0);
        assert_eq!(padding.compute_min_intrinsic_height(100.0, None), 60.0);
        assert_eq!(
            padding.compute_min_intrinsic_height(100.0, Some(50.0)),
            110.0
        );
    }
}
