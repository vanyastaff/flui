//! RenderAlign - positions its child according to an alignment.
//!
//! This render object aligns its child within its own bounds using
//! `Alignment` coordinates. It can optionally scale the child's size
//! using width and height factors.

use flui_types::{Alignment, BoxConstraints, Offset, Size};

use crate::containers::AligningBox;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// A render object that aligns its child within itself.
///
/// The child is positioned according to the alignment property.
/// If width/height factors are provided, the child is given those
/// factors of the available space.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderAlign;
/// use flui_types::Alignment;
///
/// // Center alignment
/// let mut align = RenderAlign::new(Alignment::CENTER);
///
/// // Top-right with 50% width
/// let mut align = RenderAlign::new(Alignment::TOP_RIGHT)
///     .with_width_factor(0.5);
/// ```
#[derive(Debug)]
pub struct RenderAlign {
    /// Container holding the child, alignment, and geometry.
    aligning: AligningBox,
}

impl RenderAlign {
    /// Creates a new render align with the given alignment.
    pub fn new(alignment: Alignment) -> Self {
        Self {
            aligning: AligningBox::new(alignment),
        }
    }

    /// Returns the current alignment.
    pub fn alignment(&self) -> Alignment {
        self.aligning.alignment()
    }

    /// Sets the alignment.
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.aligning.set_alignment(alignment);
        // In real implementation: self.mark_needs_layout();
    }

    /// Returns the width factor, if any.
    pub fn width_factor(&self) -> Option<f32> {
        self.aligning.width_factor()
    }

    /// Sets the width factor.
    ///
    /// If non-null, the child is given this fraction of the available width.
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        self.aligning.set_width_factor(factor);
        // In real implementation: self.mark_needs_layout();
    }

    /// Returns the height factor, if any.
    pub fn height_factor(&self) -> Option<f32> {
        self.aligning.height_factor()
    }

    /// Sets the height factor.
    ///
    /// If non-null, the child is given this fraction of the available height.
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        self.aligning.set_height_factor(factor);
        // In real implementation: self.mark_needs_layout();
    }

    /// Builder method to set width factor.
    pub fn with_width_factor(mut self, factor: f32) -> Self {
        self.set_width_factor(Some(factor));
        self
    }

    /// Builder method to set height factor.
    pub fn with_height_factor(mut self, factor: f32) -> Self {
        self.set_height_factor(Some(factor));
        self
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.aligning.geometry()
    }

    /// Returns the child offset.
    pub fn child_offset(&self) -> Offset {
        self.aligning.offset()
    }

    /// Computes the aligned offset for a child of the given size.
    pub fn compute_aligned_offset(&self, my_size: Size, child_size: Size) -> Offset {
        let alignment = self.alignment();

        // Convert alignment (-1 to 1) to offset
        let half_width_delta = (my_size.width - child_size.width) / 2.0;
        let half_height_delta = (my_size.height - child_size.height) / 2.0;

        Offset::new(
            half_width_delta + alignment.x * half_width_delta,
            half_height_delta + alignment.y * half_height_delta,
        )
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let shrink_wrap_width =
            self.width_factor().is_some() || constraints.max_width == f32::INFINITY;
        let shrink_wrap_height =
            self.height_factor().is_some() || constraints.max_height == f32::INFINITY;

        let size = constraints.constrain(Size::new(
            if shrink_wrap_width {
                0.0
            } else {
                constraints.max_width
            },
            if shrink_wrap_height {
                0.0
            } else {
                constraints.max_height
            },
        ));
        self.aligning.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        let shrink_wrap_width =
            self.width_factor().is_some() || constraints.max_width == f32::INFINITY;
        let shrink_wrap_height =
            self.height_factor().is_some() || constraints.max_height == f32::INFINITY;

        // Compute our size
        let my_size = Size::new(
            if shrink_wrap_width {
                self.width_factor()
                    .map(|f| child_size.width * f)
                    .unwrap_or(child_size.width)
            } else {
                constraints.max_width
            },
            if shrink_wrap_height {
                self.height_factor()
                    .map(|f| child_size.height * f)
                    .unwrap_or(child_size.height)
            } else {
                constraints.max_height
            },
        );

        let my_size = constraints.constrain(my_size);

        // Position child using alignment
        let offset = self.compute_aligned_offset(my_size, child_size);
        self.aligning.set_offset(offset);
        self.aligning.set_geometry(my_size);

        my_size
    }

    /// Returns constraints for the child (loosened).
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.loosen()
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Child would be painted at offset + child_offset
        let _ = (context, offset);
    }

    /// Computes intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        self.compute_min_intrinsic_width(height, child_width)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        self.compute_min_intrinsic_height(width, child_height)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        let child_offset = self.child_offset();
        child_baseline.map(|distance| distance + child_offset.dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_center() {
        let mut align = RenderAlign::new(Alignment::CENTER);
        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));

        let size = align.perform_layout(constraints);

        assert_eq!(size, Size::new(200.0, 200.0));
    }

    #[test]
    fn test_align_with_factors() {
        let align = RenderAlign::new(Alignment::CENTER)
            .with_width_factor(0.5)
            .with_height_factor(0.5);

        assert_eq!(align.width_factor(), Some(0.5));
        assert_eq!(align.height_factor(), Some(0.5));
    }

    #[test]
    fn test_align_no_child() {
        let mut align = RenderAlign::new(Alignment::TOP_LEFT);
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = align.perform_layout(constraints);

        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_computed_offset_center() {
        let align = RenderAlign::new(Alignment::CENTER);

        // Center a 50x50 child in a 100x100 parent
        let offset = align.compute_aligned_offset(Size::new(100.0, 100.0), Size::new(50.0, 50.0));

        assert_eq!(offset.dx, 25.0);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    fn test_computed_offset_top_left() {
        let align = RenderAlign::new(Alignment::TOP_LEFT);

        let offset = align.compute_aligned_offset(Size::new(100.0, 100.0), Size::new(50.0, 50.0));

        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 0.0);
    }

    #[test]
    fn test_computed_offset_bottom_right() {
        let align = RenderAlign::new(Alignment::BOTTOM_RIGHT);

        let offset = align.compute_aligned_offset(Size::new(100.0, 100.0), Size::new(50.0, 50.0));

        assert_eq!(offset.dx, 50.0);
        assert_eq!(offset.dy, 50.0);
    }

    #[test]
    fn test_layout_with_child() {
        let mut align = RenderAlign::new(Alignment::CENTER);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let child_size = Size::new(50.0, 50.0);

        let size = align.perform_layout_with_child(constraints, child_size);

        assert_eq!(size, Size::new(200.0, 200.0));

        let child_offset = align.child_offset();
        assert_eq!(child_offset.dx, 75.0);
        assert_eq!(child_offset.dy, 75.0);
    }
}
