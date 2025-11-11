//! RenderTransform - applies matrix transformation to child

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::{geometry::Transform, Matrix4, Offset, Size};

/// RenderObject that applies a transformation to its child
///
/// The transformation is applied during painting. It doesn't affect layout,
/// so the child is laid out as if untransformed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderTransform;
/// use flui_types::geometry::Transform;
/// use std::f32::consts::PI;
///
/// // High-level Transform API (recommended)
/// let rotate = RenderTransform::new(Transform::rotate(PI / 4.0));
///
/// // Composing transforms
/// let composed = Transform::translate(50.0, 50.0)
///     .then(Transform::rotate(PI / 4.0))
///     .then(Transform::scale(2.0));
/// let transform = RenderTransform::new(composed);
///
/// // Skew for italic text
/// let italic = RenderTransform::new(Transform::skew(0.2, 0.0));
/// ```
#[derive(Debug)]
pub struct RenderTransform {
    /// The transformation to apply
    transform: Transform,

    /// Origin point for rotation/scale (relative to child size)
    pub alignment: Offset,
}

impl RenderTransform {
    /// Create new RenderTransform from high-level Transform API
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::geometry::Transform;
    /// use std::f32::consts::PI;
    ///
    /// // Simple transforms
    /// let translate = RenderTransform::new(Transform::translate(10.0, 20.0));
    /// let rotate = RenderTransform::new(Transform::rotate(PI / 4.0));
    /// let scale = RenderTransform::new(Transform::scale(2.0));
    ///
    /// // Composed transforms
    /// let composed = Transform::translate(50.0, 50.0)
    ///     .then(Transform::rotate(PI / 4.0))
    ///     .then(Transform::scale(2.0));
    /// let transform = RenderTransform::new(composed);
    /// ```
    pub fn new(transform: Transform) -> Self {
        Self {
            transform,
            alignment: Offset::ZERO,
        }
    }

    /// Create from Matrix4 (backward compatibility)
    pub fn from_matrix(matrix: Matrix4) -> Self {
        Self {
            transform: matrix.into(),
            alignment: Offset::ZERO,
        }
    }

    /// Create with custom alignment/origin
    pub fn with_alignment(transform: Transform, alignment: Offset) -> Self {
        Self {
            transform,
            alignment,
        }
    }

    /// Set new transformation
    pub fn set_transform(&mut self, transform: Transform) {
        self.transform = transform;
    }

    /// Set alignment/origin
    pub fn set_alignment(&mut self, alignment: Offset) {
        self.alignment = alignment;
    }
}

impl Render for RenderTransform {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child with same constraints (transform doesn't affect layout)
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // Create a new canvas for the transformed content
        let mut canvas = Canvas::new();

        // Apply transform using Canvas API
        canvas.save();

        // Apply alignment if needed
        if self.alignment != Offset::ZERO {
            canvas.translate(self.alignment.dx, self.alignment.dy);
        }

        // Use the new Canvas::transform() method
        canvas.transform(&self.transform);

        // Reverse alignment
        if self.alignment != Offset::ZERO {
            canvas.translate(-self.alignment.dx, -self.alignment.dy);
        }

        // Paint child and append its canvas
        let child_canvas = tree.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);

        canvas.restore();

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_render_transform_new() {
        let transform = RenderTransform::new(Transform::rotate(PI / 4.0));
        assert_eq!(transform.alignment, Offset::ZERO);
    }

    #[test]
    fn test_render_transform_from_matrix() {
        let matrix = Matrix4::rotation_z(PI / 4.0);
        let transform = RenderTransform::from_matrix(matrix);
        assert_eq!(transform.alignment, Offset::ZERO);
    }

    #[test]
    fn test_render_transform_with_alignment() {
        let transform =
            RenderTransform::with_alignment(Transform::scale(2.0), Offset::new(0.5, 0.5));
        assert_eq!(transform.alignment, Offset::new(0.5, 0.5));
    }

    #[test]
    fn test_render_transform_set_transform() {
        let mut transform = RenderTransform::new(Transform::translate(10.0, 20.0));
        transform.set_transform(Transform::rotate(1.5));
        // Transform set successfully
    }

    #[test]
    fn test_render_transform_composition() {
        let composed = Transform::translate(50.0, 50.0)
            .then(Transform::rotate(PI / 4.0))
            .then(Transform::scale(2.0));
        let transform = RenderTransform::new(composed);
        assert_eq!(transform.alignment, Offset::ZERO);
    }

    #[test]
    fn test_render_transform_skew() {
        let transform = RenderTransform::new(Transform::skew(0.2, 0.0));
        assert_eq!(transform.alignment, Offset::ZERO);
    }
}
