//! RenderTransform - applies matrix transformation to child

use crate::core::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, BoxProtocol, RenderBox, Single};
use flui_interaction::HitTestResult;
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

    /// Transform a point from parent coordinates to child coordinates.
    ///
    /// This applies the inverse transform to convert a hit test position
    /// into the child's coordinate space.
    fn transform_point_to_child(&self, point: Offset) -> Option<Offset> {
        // Try to compute inverse transform
        let inverse_transform = self.transform.inverse()?;
        let inverse_matrix: Matrix4 = inverse_transform.into();

        // Account for alignment offset when transforming position
        let mut transformed_position = point;

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

        Some(final_position)
    }
}

impl RenderBox<Single> for RenderTransform {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> Size {
        // Layout child with same constraints (transform doesn't affect layout)
        ctx.layout_single_child()
            .unwrap_or_else(|_| ctx.constraints.smallest())
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Read offset before taking mutable borrow
        let offset = ctx.offset;

        // Apply transform using Canvas API
        ctx.canvas_mut().save();

        // Move to offset first
        ctx.canvas_mut().translate(offset.dx, offset.dy);

        // Apply alignment if needed
        if self.alignment != Offset::ZERO {
            ctx.canvas_mut()
                .translate(self.alignment.dx, self.alignment.dy);
        }

        // Use the Canvas::transform() method
        ctx.canvas_mut().transform(&self.transform);

        // Reverse alignment
        if self.alignment != Offset::ZERO {
            ctx.canvas_mut()
                .translate(-self.alignment.dx, -self.alignment.dy);
        }

        // Paint child at origin (transform already applied)
        let _ = ctx.paint_single_child(Offset::ZERO);

        ctx.canvas_mut().restore();
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        // To hit test a transformed child, we need to transform the hit position
        // by the INVERSE of our transform, then test the child with that position.
        //
        // Example: If we rotate a button 45°, and the user clicks at (100, 100),
        // we need to rotate that click position -45° before testing the button.

        // Transform the position to child coordinates
        let child_position = match self.transform_point_to_child(ctx.position) {
            Some(pos) => pos,
            None => {
                // Transform is singular (non-invertible), e.g., scale by 0
                // Cannot hit test - return false
                return false;
            }
        };

        // Create new context with transformed position and test children
        let child_ctx = ctx.with_position(child_position);
        child_ctx.hit_test_children(result)
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

    #[test]
    fn test_transform_point_identity() {
        let transform = RenderTransform::new(Transform::identity());
        let point = Offset::new(100.0, 50.0);
        let result = transform.transform_point_to_child(point);
        assert!(result.is_some());
        let transformed = result.unwrap();
        assert!((transformed.dx - point.dx).abs() < 0.001);
        assert!((transformed.dy - point.dy).abs() < 0.001);
    }

    #[test]
    fn test_transform_point_translation() {
        let transform = RenderTransform::new(Transform::translate(10.0, 20.0));
        let point = Offset::new(100.0, 50.0);
        let result = transform.transform_point_to_child(point);
        assert!(result.is_some());
        let transformed = result.unwrap();
        // Inverse of translate(10, 20) is translate(-10, -20)
        assert!((transformed.dx - 90.0).abs() < 0.001);
        assert!((transformed.dy - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_point_scale() {
        let transform = RenderTransform::new(Transform::scale(2.0));
        let point = Offset::new(100.0, 50.0);
        let result = transform.transform_point_to_child(point);
        assert!(result.is_some());
        let transformed = result.unwrap();
        // Inverse of scale(2) is scale(0.5)
        assert!((transformed.dx - 50.0).abs() < 0.001);
        assert!((transformed.dy - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_point_zero_scale() {
        // Scale by 0 is non-invertible
        let transform = RenderTransform::new(Transform::scale(0.0));
        let point = Offset::new(100.0, 50.0);
        let result = transform.transform_point_to_child(point);
        assert!(result.is_none());
    }
}
