//! RenderSizedBox - forces a specific size on its child.
//!
//! This render object forces its child to have a specific width and/or height.
//! Unlike ConstrainedBox, it always uses tight constraints for the specified
//! dimensions.

use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ProxyBox;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// A render object that forces a specific size.
///
/// If width or height is None, that dimension uses the child's size
/// (or 0 if there's no child).
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderSizedBox;
///
/// // Fixed 100x100 size
/// let mut sized = RenderSizedBox::new(Some(100.0), Some(100.0));
///
/// // Only fixed width
/// let mut sized = RenderSizedBox::new(Some(100.0), None);
/// ```
#[derive(Debug)]
pub struct RenderSizedBox {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// The fixed width, if any.
    width: Option<f32>,

    /// The fixed height, if any.
    height: Option<f32>,
}

impl RenderSizedBox {
    /// Creates a new sized box with optional fixed dimensions.
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            proxy: ProxyBox::new(),
            width,
            height,
        }
    }

    /// Creates a sized box with both dimensions fixed.
    pub fn fixed(width: f32, height: f32) -> Self {
        Self::new(Some(width), Some(height))
    }

    /// Creates an expand box that fills available space.
    pub fn expand() -> Self {
        Self::new(Some(f32::INFINITY), Some(f32::INFINITY))
    }

    /// Creates a shrink box that takes minimum space.
    pub fn shrink() -> Self {
        Self::new(Some(0.0), Some(0.0))
    }

    /// Returns the fixed width, if any.
    pub fn width(&self) -> Option<f32> {
        self.width
    }

    /// Sets the fixed width.
    pub fn set_width(&mut self, width: Option<f32>) {
        if self.width != width {
            self.width = width;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the fixed height, if any.
    pub fn height(&self) -> Option<f32> {
        self.height
    }

    /// Sets the fixed height.
    pub fn set_height(&mut self, height: Option<f32>) {
        if self.height != height {
            self.height = height;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    /// Computes the effective constraints.
    ///
    /// - None: pass through parent constraints (loose)
    /// - Some(INFINITY): use max from parent (expand)
    /// - Some(0.0): use min from parent (shrink)
    /// - Some(value): use the fixed value (clamped to parent)
    fn get_effective_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        let (min_width, max_width) = match self.width {
            Some(w) if w == f32::INFINITY => (constraints.max_width, constraints.max_width),
            Some(w) => {
                let clamped = w.clamp(constraints.min_width, constraints.max_width);
                (clamped, clamped)
            }
            None => (constraints.min_width, constraints.max_width),
        };
        let (min_height, max_height) = match self.height {
            Some(h) if h == f32::INFINITY => (constraints.max_height, constraints.max_height),
            Some(h) => {
                let clamped = h.clamp(constraints.min_height, constraints.max_height);
                (clamped, clamped)
            }
            None => (constraints.min_height, constraints.max_height),
        };

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let effective = self.get_effective_constraints(constraints);
        let size = effective.constrain(Size::new(
            self.width.unwrap_or(0.0),
            self.height.unwrap_or(0.0),
        ));
        self.proxy.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        let effective = self.get_effective_constraints(constraints);
        let size = effective.constrain(Size::new(
            self.width.unwrap_or(child_size.width),
            self.height.unwrap_or(child_size.height),
        ));
        self.proxy.set_geometry(size);
        size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        self.get_effective_constraints(constraints)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Child would be painted at offset
        let _ = (context, offset);
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        self.width.unwrap_or_else(|| child_width.unwrap_or(0.0))
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        self.compute_min_intrinsic_width(height, child_width)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        self.height.unwrap_or_else(|| child_height.unwrap_or(0.0))
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
        child_baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sized_box_fixed() {
        let mut sized = RenderSizedBox::fixed(100.0, 50.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let size = sized.perform_layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_sized_box_shrink() {
        let mut sized = RenderSizedBox::shrink();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let size = sized.perform_layout(constraints);

        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_sized_box_expand() {
        let mut sized = RenderSizedBox::expand();
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);

        let size = sized.perform_layout(constraints);

        assert_eq!(size, Size::new(200.0, 150.0));
    }

    #[test]
    fn test_sized_box_partial() {
        let mut sized = RenderSizedBox::new(Some(100.0), None);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let size = sized.perform_layout(constraints);

        // Width is fixed, height is 0 (no child)
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 0.0);
    }

    #[test]
    fn test_sized_box_intrinsics() {
        let sized = RenderSizedBox::fixed(100.0, 50.0);

        assert_eq!(sized.compute_min_intrinsic_width(0.0, None), 100.0);
        assert_eq!(sized.compute_max_intrinsic_width(0.0, None), 100.0);
        assert_eq!(sized.compute_min_intrinsic_height(0.0, None), 50.0);
        assert_eq!(sized.compute_max_intrinsic_height(0.0, None), 50.0);
    }

    #[test]
    fn test_layout_with_child() {
        let mut sized = RenderSizedBox::new(Some(100.0), None);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let child_size = Size::new(50.0, 75.0);

        let size = sized.perform_layout_with_child(constraints, child_size);

        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 75.0);
    }
}
