//! RenderTransform - applies matrix transformation to child

use flui_core::element::hit_test::BoxHitTestResult;
use flui_core::render::{
    RenderBox, Single, {BoxProtocol, HitTestContext, LayoutContext, PaintContext},
};
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

impl RenderBox<Single> for RenderTransform {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        // Layout child with same constraints (transform doesn't affect layout)
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();

        // Read offset before taking mutable borrow
        let offset = ctx.offset;

        // Apply transform using Canvas API
        ctx.canvas().save();

        // Move to offset first
        ctx.canvas().translate(offset.dx, offset.dy);

        // Apply alignment if needed
        if self.alignment != Offset::ZERO {
            ctx.canvas().translate(self.alignment.dx, self.alignment.dy);
        }

        // Use the new Canvas::transform() method
        ctx.canvas().transform(&self.transform);

        // Reverse alignment
        if self.alignment != Offset::ZERO {
            ctx.canvas()
                .translate(-self.alignment.dx, -self.alignment.dy);
        }

        // Paint child at origin (transform already applied)
        ctx.paint_child(child_id, Offset::ZERO);

        ctx.canvas().restore();
    }

    fn hit_test(
        &self,
        ctx: HitTestContext<'_, Single, BoxProtocol>,
        result: &mut BoxHitTestResult,
    ) -> bool {
        // To hit test a transformed child, we need to transform the hit position
        // by the INVERSE of our transform, then test the child with that position.
        //
        // Example: If we rotate a button 45°, and the user clicks at (100, 100),
        // we need to rotate that click position -45° before testing the button.

        // Try to compute inverse transform
        let inverse_transform = match self.transform.inverse() {
            Some(inv) => inv,
            None => {
                // Transform is singular (non-invertible), e.g., scale by 0
                // Cannot hit test - return false
                return false;
            }
        };

        // Convert inverse transform to matrix and apply to position
        let inverse_matrix: Matrix4 = inverse_transform.into();

        // Account for alignment offset when transforming position
        let mut transformed_position = ctx.position;

        // Apply inverse alignment (reverse of paint order)
        if self.alignment != Offset::ZERO {
            transformed_position = Offset::new(
                transformed_position.dx + self.alignment.dx,
                transformed_position.dy + self.alignment.dy,
            );
        }

        // Apply inverse transform to position
        let x = transformed_position.dx;
        let y = transformed_position.dy;
        let transformed_x =
            inverse_matrix.m[0] * x + inverse_matrix.m[4] * y + inverse_matrix.m[12];
        let transformed_y =
            inverse_matrix.m[1] * x + inverse_matrix.m[5] * y + inverse_matrix.m[13];

        // Reverse inverse alignment
        let final_position = if self.alignment != Offset::ZERO {
            Offset::new(
                transformed_x - self.alignment.dx,
                transformed_y - self.alignment.dy,
            )
        } else {
            Offset::new(transformed_x, transformed_y)
        };

        // Create new context with transformed position
        let new_ctx = ctx.with_position(final_position);

        // Test child with transformed position
        self.hit_test_children(&new_ctx, result)
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
