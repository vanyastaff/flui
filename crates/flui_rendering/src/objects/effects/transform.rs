//! RenderTransform - applies matrix transformation to child

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Simple 2D transformation matrix
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix4 {
    /// Translation X
    pub translate_x: f32,
    /// Translation Y
    pub translate_y: f32,
    /// Scale X
    pub scale_x: f32,
    /// Scale Y
    pub scale_y: f32,
    /// Rotation in radians
    pub rotation: f32,
}

impl Matrix4 {
    /// Identity matrix (no transformation)
    pub fn identity() -> Self {
        Self {
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
        }
    }

    /// Translation matrix
    pub fn translation(x: f32, y: f32) -> Self {
        Self {
            translate_x: x,
            translate_y: y,
            ..Self::identity()
        }
    }

    /// Scale matrix
    pub fn scale(x: f32, y: f32) -> Self {
        Self {
            scale_x: x,
            scale_y: y,
            ..Self::identity()
        }
    }

    /// Rotation matrix (radians)
    pub fn rotation(radians: f32) -> Self {
        Self {
            rotation: radians,
            ..Self::identity()
        }
    }
}

impl Default for Matrix4 {
    fn default() -> Self {
        Self::identity()
    }
}

/// Data for RenderTransform
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformData {
    /// The transformation matrix
    pub transform: Matrix4,
    /// Alignment origin for transformation
    pub origin: Offset,
}

impl TransformData {
    /// Create new transform data
    pub fn new(transform: Matrix4) -> Self {
        Self {
            transform,
            origin: Offset::ZERO,
        }
    }

    /// Create with custom origin
    pub fn with_origin(transform: Matrix4, origin: Offset) -> Self {
        Self { transform, origin }
    }
}

/// RenderObject that applies a transformation to its child
///
/// The transformation is applied during painting. It doesn't affect layout,
/// so the child is laid out as if untransformed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::effects::{TransformData, Matrix4}};
///
/// let transform = Matrix4::scale(2.0, 2.0);
/// let mut render_transform = SingleRenderBox::new(TransformData::new(transform));
/// ```
pub type RenderTransform = SingleRenderBox<TransformData>;

// ===== Public API =====

impl RenderTransform {
    /// Get the transformation matrix
    pub fn transform(&self) -> Matrix4 {
        self.data().transform
    }

    /// Get the origin
    pub fn origin(&self) -> Offset {
        self.data().origin
    }

    /// Set new transformation matrix
    pub fn set_transform(&mut self, transform: Matrix4) {
        if self.data().transform != transform {
            self.data_mut().transform = transform;
            self.mark_needs_paint();
        }
    }

    /// Set new origin
    pub fn set_origin(&mut self, origin: Offset) {
        if self.data().origin != origin {
            self.data_mut().origin = origin;
            self.mark_needs_paint();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderTransform {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints (transform doesn't affect layout)
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child with transformation
        if let Some(child) = self.child() {
            let transform = self.data().transform;
            let origin = self.data().origin;

            // Calculate transformed offset
            // TODO: In a real implementation, we would:
            // 1. Save painter transform state
            // 2. Apply translation to origin
            // 3. Apply scale/rotation around origin
            // 4. Paint child
            // 5. Restore painter transform state

            // For now, just apply simple translation
            let transformed_offset = Offset::new(
                offset.dx + transform.translate_x + origin.dx,
                offset.dy + transform.translate_y + origin.dy,
            );

            child.paint(painter, transformed_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix4_identity() {
        let m = Matrix4::identity();
        assert_eq!(m.translate_x, 0.0);
        assert_eq!(m.translate_y, 0.0);
        assert_eq!(m.scale_x, 1.0);
        assert_eq!(m.scale_y, 1.0);
        assert_eq!(m.rotation, 0.0);
    }

    #[test]
    fn test_matrix4_translation() {
        let m = Matrix4::translation(10.0, 20.0);
        assert_eq!(m.translate_x, 10.0);
        assert_eq!(m.translate_y, 20.0);
        assert_eq!(m.scale_x, 1.0);
        assert_eq!(m.scale_y, 1.0);
    }

    #[test]
    fn test_matrix4_scale() {
        let m = Matrix4::scale(2.0, 3.0);
        assert_eq!(m.scale_x, 2.0);
        assert_eq!(m.scale_y, 3.0);
        assert_eq!(m.translate_x, 0.0);
    }

    #[test]
    fn test_matrix4_rotation() {
        let m = Matrix4::rotation(std::f32::consts::PI);
        assert_eq!(m.rotation, std::f32::consts::PI);
    }

    #[test]
    fn test_transform_data_new() {
        let transform = Matrix4::scale(2.0, 2.0);
        let data = TransformData::new(transform);
        assert_eq!(data.transform, transform);
        assert_eq!(data.origin, Offset::ZERO);
    }

    #[test]
    fn test_transform_data_with_origin() {
        let transform = Matrix4::identity();
        let origin = Offset::new(10.0, 20.0);
        let data = TransformData::with_origin(transform, origin);
        assert_eq!(data.origin, origin);
    }

    #[test]
    fn test_render_transform_new() {
        let transform = Matrix4::scale(2.0, 2.0);
        let render_transform = SingleRenderBox::new(TransformData::new(transform));
        assert_eq!(render_transform.transform(), transform);
    }

    #[test]
    fn test_render_transform_set_transform() {
        let transform1 = Matrix4::scale(1.0, 1.0);
        let mut render_transform = SingleRenderBox::new(TransformData::new(transform1));

        // Clear initial needs_layout flag
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let _ = render_transform.layout(constraints);

        let transform2 = Matrix4::scale(2.0, 2.0);
        render_transform.set_transform(transform2);

        assert_eq!(render_transform.transform(), transform2);
        assert!(render_transform.needs_paint());
        assert!(!render_transform.needs_layout());
    }

    #[test]
    fn test_render_transform_layout() {
        let transform = Matrix4::translation(50.0, 50.0);
        let mut render_transform = SingleRenderBox::new(TransformData::new(transform));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = render_transform.layout(constraints);

        // Transform doesn't affect layout
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
