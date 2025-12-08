//! RenderTransform - applies matrix transformation to child
//!
//! This module provides [`RenderTransform`], a render object that applies
//! 2D/3D transformations to its child, following Flutter's RenderTransform protocol.
//!
//! # Flutter Equivalence
//!
//! This implementation matches Flutter's `RenderTransform` class from
//! `package:flutter/src/rendering/proxy_box.dart`.
//!
//! **Flutter API:**
//! ```dart
//! class RenderTransform extends RenderProxyBox {
//!   RenderTransform({
//!     required Matrix4 transform,
//!     Offset? origin,
//!     AlignmentGeometry? alignment,
//!     RenderBox? child,
//!   });
//!
//!   @override
//!   bool get alwaysNeedsCompositing => child != null;
//! }
//! ```
//!
//! # Transform Operations
//!
//! FLUI provides a high-level Transform API for common operations:
//!
//! - **Translation**: `Transform::translate(x, y)` - Move by offset
//! - **Rotation**: `Transform::rotate(angle)` - Rotate around Z-axis
//! - **Scaling**: `Transform::scale(s)` / `Transform::scale_xy(sx, sy)` - Scale uniformly/non-uniformly
//! - **Skewing**: `Transform::skew(sx, sy)` - Shear transformation
//! - **Composition**: `transform.then(other)` - Chain multiple transforms
//!
//! # Layout Behavior
//!
//! Transform is **layout-transparent**:
//! - Child laid out with original constraints (untransformed)
//! - Transform applied only during paint
//! - Child size becomes parent size (no size change)
//!
//! This means a 100×100 child rotated 45° still reports size as 100×100,
//! even though its visual bounds may extend beyond this.
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(1) - hardware-accelerated matrix multiplication
//! - **Hit Testing**: O(1) - matrix inversion (cached)
//! - **Memory**: 80 bytes (Matrix4 = 16 × f32 + metadata)

use flui_rendering::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_interaction::HitTestResult;
use flui_types::{geometry::Transform, Matrix4, Offset, Size};

/// RenderObject that applies a transformation to its child.
///
/// The transformation is applied during painting. It doesn't affect layout,
/// so the child is laid out as if untransformed.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged to child, applies transform during paint only.
///
/// # Flutter Compliance
///
/// This implementation follows Flutter's RenderTransform protocol:
///
/// | Flutter Property | FLUI Equivalent | Behavior |
/// |------------------|-----------------|----------|
/// | `transform` | `transform` | Transformation matrix |
/// | `origin` / `alignment` | `alignment` | Transform origin point |
/// | `performLayout()` | `layout()` | Pass-through (untransformed) |
/// | `paint()` | `paint()` | Apply transform, paint child |
/// | `hitTestChildren()` | `hit_test()` | Inverse transform position |
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderTransform;
/// use flui_types::geometry::Transform;
/// use std::f32::consts::PI;
///
/// // Rotation (common for icons, loading spinners)
/// let rotate = RenderTransform::new(Transform::rotate(PI / 4.0));
///
/// // Scaling (common for zoom effects, thumbnails)
/// let scale = RenderTransform::new(Transform::scale(2.0));
/// let scale_xy = RenderTransform::new(Transform::scale_xy(1.5, 0.5));
///
/// // Translation (common for slide animations)
/// let translate = RenderTransform::new(Transform::translate(50.0, 100.0));
///
/// // Skew (common for italic text, perspective effects)
/// let italic = RenderTransform::new(Transform::skew(0.2, 0.0));
///
/// // Composition (combine multiple transforms)
/// let composed = Transform::translate(50.0, 50.0)
///     .then(Transform::rotate(PI / 4.0))
///     .then(Transform::scale(2.0));
/// let transform = RenderTransform::new(composed);
///
/// // Custom alignment (rotate around specific point)
/// let centered_rotate = RenderTransform::with_alignment(
///     Transform::rotate(PI / 4.0),
///     Offset::new(50.0, 50.0), // Rotate around (50, 50)
/// );
/// ```
///
/// # Transform Order
///
/// Transforms are applied in **reverse order** when composed:
///
/// ```text
/// transform.then(other) → Apply transform first, then other
///
/// Example:
///   Transform::translate(100, 0)
///     .then(Transform::rotate(PI/4))
///
/// Execution:
///   1. Translate by (100, 0)
///   2. Rotate by 45° around origin
///
/// Result: Point moves right, then rotates
/// ```
///
/// # Hit Testing
///
/// Hit testing applies the **inverse transform** to the hit position:
///
/// ```text
/// User clicks at (100, 100) on screen
///   ↓
/// Transform: rotate 45°
///   ↓
/// Inverse: rotate -45°
///   ↓
/// Hit test child at rotated position
/// ```
///
/// If the transform is singular (non-invertible, e.g., scale by 0),
/// hit testing returns `false`.
#[derive(Debug)]
pub struct RenderTransform {
    /// The transformation to apply
    ///
    /// Use high-level Transform API for common operations:
    /// - `Transform::translate(x, y)`
    /// - `Transform::rotate(angle)`
    /// - `Transform::scale(s)`
    /// - `Transform::skew(sx, sy)`
    transform: Transform,

    /// Origin point for rotation/scale (relative to child size)
    ///
    /// - `Offset::ZERO` - Transform around top-left corner (default)
    /// - `Offset::new(width/2, height/2)` - Transform around center
    /// - Custom offset - Transform around specific point
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

impl RenderObject for RenderTransform {}

impl RenderBox<Single> for RenderTransform {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Layout child with same constraints (transform doesn't affect layout)
        ctx.layout_single_child()
            .map_err(|e| flui_rendering::RenderError::Layout(e.to_string()))
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
        ctx.paint_single_child(Offset::ZERO);

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
