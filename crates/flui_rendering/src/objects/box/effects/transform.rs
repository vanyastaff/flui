//! RenderTransform - applies a matrix transformation to its child.
//!
//! This render object applies a 4x4 transformation matrix to its child,
//! enabling rotation, scaling, translation, and perspective effects.

use flui_types::{BoxConstraints, Matrix4, Offset, Point, Rect, Size};

use crate::containers::ProxyBox;
use crate::pipeline::PaintingContext;
use crate::traits::TextBaseline;

/// Alignment for transform origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformAlignment {
    /// X alignment (-1.0 = left, 0.0 = center, 1.0 = right)
    pub x: f32,
    /// Y alignment (-1.0 = top, 0.0 = center, 1.0 = bottom)
    pub y: f32,
}

impl TransformAlignment {
    /// Top-left corner.
    pub const TOP_LEFT: Self = Self { x: -1.0, y: -1.0 };
    /// Top-center.
    pub const TOP_CENTER: Self = Self { x: 0.0, y: -1.0 };
    /// Top-right corner.
    pub const TOP_RIGHT: Self = Self { x: 1.0, y: -1.0 };
    /// Center-left.
    pub const CENTER_LEFT: Self = Self { x: -1.0, y: 0.0 };
    /// Center.
    pub const CENTER: Self = Self { x: 0.0, y: 0.0 };
    /// Center-right.
    pub const CENTER_RIGHT: Self = Self { x: 1.0, y: 0.0 };
    /// Bottom-left corner.
    pub const BOTTOM_LEFT: Self = Self { x: -1.0, y: 1.0 };
    /// Bottom-center.
    pub const BOTTOM_CENTER: Self = Self { x: 0.0, y: 1.0 };
    /// Bottom-right corner.
    pub const BOTTOM_RIGHT: Self = Self { x: 1.0, y: 1.0 };

    /// Creates a new alignment.
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Computes the offset for a given size.
    pub fn along_size(&self, size: Size) -> Offset {
        Offset::new(
            size.width * (1.0 + self.x) / 2.0,
            size.height * (1.0 + self.y) / 2.0,
        )
    }
}

impl Default for TransformAlignment {
    fn default() -> Self {
        Self::CENTER
    }
}

/// A render object that applies a transformation matrix.
///
/// The transformation is applied around the `origin` point, which defaults
/// to the center of the render object.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::effects::RenderTransform;
/// use flui_types::Matrix4;
///
/// // Rotate 45 degrees around center
/// let angle = std::f32::consts::FRAC_PI_4;
/// let transform = RenderTransform::rotation_z(angle);
///
/// // Scale 2x from top-left
/// let mut scale = RenderTransform::scale(2.0, 2.0);
/// scale.set_alignment(TransformAlignment::TOP_LEFT);
/// ```
#[derive(Debug)]
pub struct RenderTransform {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// The transformation matrix.
    transform: Matrix4,

    /// The origin alignment for the transformation.
    alignment: TransformAlignment,

    /// Additional origin offset (in logical pixels).
    origin: Option<Offset>,

    /// Whether to apply hit test transformation.
    transform_hit_tests: bool,
}

impl RenderTransform {
    /// Creates a new transform render object with the given matrix.
    pub fn new(transform: Matrix4) -> Self {
        Self {
            proxy: ProxyBox::new(),
            transform,
            alignment: TransformAlignment::CENTER,
            origin: None,
            transform_hit_tests: true,
        }
    }

    /// Creates an identity transform (no transformation).
    pub fn identity() -> Self {
        Self::new(Matrix4::IDENTITY)
    }

    /// Creates a rotation around the Z axis.
    pub fn rotation_z(angle: f32) -> Self {
        Self::new(Matrix4::rotation_z(angle))
    }

    /// Creates a uniform scale transformation.
    pub fn scale(sx: f32, sy: f32) -> Self {
        Self::new(Matrix4::scaling(sx, sy, 1.0))
    }

    /// Creates a translation transformation.
    pub fn translation(tx: f32, ty: f32) -> Self {
        Self::new(Matrix4::translation(tx, ty, 0.0))
    }

    /// Returns the transformation matrix.
    pub fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Sets the transformation matrix.
    pub fn set_transform(&mut self, transform: Matrix4) {
        if self.transform != transform {
            self.transform = transform;
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Returns the alignment.
    pub fn alignment(&self) -> TransformAlignment {
        self.alignment
    }

    /// Sets the alignment for the transformation origin.
    pub fn set_alignment(&mut self, alignment: TransformAlignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Returns the origin offset.
    pub fn origin(&self) -> Option<Offset> {
        self.origin
    }

    /// Sets an additional origin offset.
    pub fn set_origin(&mut self, origin: Option<Offset>) {
        if self.origin != origin {
            self.origin = origin;
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Returns whether hit tests are transformed.
    pub fn transform_hit_tests(&self) -> bool {
        self.transform_hit_tests
    }

    /// Sets whether hit tests should be transformed.
    pub fn set_transform_hit_tests(&mut self, value: bool) {
        self.transform_hit_tests = value;
    }

    /// Computes the effective origin for the current size.
    fn effective_origin(&self, size: Size) -> Offset {
        let alignment_offset = self.alignment.along_size(size);
        if let Some(origin) = self.origin {
            Offset::new(
                alignment_offset.dx + origin.dx,
                alignment_offset.dy + origin.dy,
            )
        } else {
            alignment_offset
        }
    }

    /// Computes the effective transform matrix with origin applied.
    pub fn effective_transform(&self, size: Size) -> Matrix4 {
        let origin = self.effective_origin(size);

        if origin.dx.abs() < f32::EPSILON && origin.dy.abs() < f32::EPSILON {
            self.transform
        } else {
            // Translate to origin, apply transform, translate back
            let to_origin = Matrix4::translation(-origin.dx, -origin.dy, 0.0);
            let from_origin = Matrix4::translation(origin.dx, origin.dy, 0.0);
            from_origin * self.transform * to_origin
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        *self.proxy.geometry()
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = constraints.smallest();
        self.proxy.set_geometry(size);
        size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        _constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        self.proxy.set_geometry(child_size);
        child_size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let size = self.size();
        let transform = self.effective_transform(size);

        if transform == Matrix4::IDENTITY {
            // No transformation needed
            let _ = (context, offset);
            // In real implementation: context.paint_child(child, offset);
        } else {
            // Apply transformation
            // In real implementation:
            // context.push_transform(offset, transform, |ctx| {
            //     ctx.paint_child(child, offset);
            // });
            let _ = (context, offset, transform);
        }
    }

    /// Transforms a point from parent coordinates to local coordinates.
    pub fn global_to_local(&self, point: Point) -> Option<Point> {
        let size = self.size();
        let transform = self.effective_transform(size);
        let inverse = transform.try_inverse()?;

        let (tx, ty) = inverse.transform_point(point.x, point.y);
        Some(Point::new(tx, ty))
    }

    /// Transforms a point from local coordinates to parent coordinates.
    pub fn local_to_global(&self, point: Point) -> Point {
        let size = self.size();
        let transform = self.effective_transform(size);

        let (tx, ty) = transform.transform_point(point.x, point.y);
        Point::new(tx, ty)
    }

    /// Hit test with transformation applied.
    pub fn hit_test(&self, position: Offset) -> bool {
        if !self.transform_hit_tests {
            // Pass through without transformation
            let size = self.size();
            let rect = Rect::from_origin_size(Point::ZERO, size);
            return rect.contains(Point::new(position.dx, position.dy));
        }

        // Transform the position
        if let Some(local) = self.global_to_local(Point::new(position.dx, position.dy)) {
            let size = self.size();
            let rect = Rect::from_origin_size(Point::ZERO, size);
            rect.contains(local)
        } else {
            false
        }
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, child_width: Option<f32>) -> f32 {
        child_width.unwrap_or(0.0)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, child_height: Option<f32>) -> f32 {
        child_height.unwrap_or(0.0)
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
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn test_transform_identity() {
        let transform = RenderTransform::identity();
        assert_eq!(*transform.transform(), Matrix4::IDENTITY);
    }

    #[test]
    fn test_transform_alignment() {
        let size = Size::new(100.0, 80.0);

        let center = TransformAlignment::CENTER.along_size(size);
        assert!((center.dx - 50.0).abs() < f32::EPSILON);
        assert!((center.dy - 40.0).abs() < f32::EPSILON);

        let top_left = TransformAlignment::TOP_LEFT.along_size(size);
        assert!((top_left.dx - 0.0).abs() < f32::EPSILON);
        assert!((top_left.dy - 0.0).abs() < f32::EPSILON);

        let bottom_right = TransformAlignment::BOTTOM_RIGHT.along_size(size);
        assert!((bottom_right.dx - 100.0).abs() < f32::EPSILON);
        assert!((bottom_right.dy - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_transform_rotation() {
        let mut transform = RenderTransform::rotation_z(FRAC_PI_2);
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        transform.perform_layout(constraints);

        // After 90 degree rotation, the effective transform should rotate points
        let effective = transform.effective_transform(transform.size());
        assert_ne!(effective, Matrix4::IDENTITY);
    }

    #[test]
    fn test_transform_scale() {
        let mut transform = RenderTransform::scale(2.0, 2.0);
        let child_size = Size::new(50.0, 50.0);
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));

        let size = transform.perform_layout_with_child(constraints, child_size);
        assert_eq!(size, child_size);
    }

    #[test]
    fn test_hit_test_identity() {
        let mut transform = RenderTransform::identity();
        transform.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(transform.hit_test(Offset::new(50.0, 50.0)));
        assert!(!transform.hit_test(Offset::new(150.0, 50.0)));
    }

    #[test]
    fn test_hit_test_disabled() {
        let mut transform = RenderTransform::identity();
        transform.set_transform_hit_tests(false);
        transform.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        assert!(transform.hit_test(Offset::new(50.0, 50.0)));
    }

    #[test]
    fn test_local_to_global() {
        let mut transform = RenderTransform::translation(10.0, 20.0);
        transform.set_alignment(TransformAlignment::TOP_LEFT);
        transform.perform_layout_with_child(
            BoxConstraints::tight(Size::new(100.0, 100.0)),
            Size::new(100.0, 100.0),
        );

        let global = transform.local_to_global(Point::new(0.0, 0.0));
        assert!((global.x - 10.0).abs() < 0.001);
        assert!((global.y - 20.0).abs() < 0.001);
    }
}
