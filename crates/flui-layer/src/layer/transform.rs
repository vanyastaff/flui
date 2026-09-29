//! `TransformLayer` — transforms its subtree by a 4×4 matrix.

use flui_foundation::geometry::{Matrix4, Point};

/// Layer that applies a full matrix transformation to its children.
///
/// Unlike `OffsetLayer` which only supports translation, `TransformLayer`
/// supports the full range of affine and perspective transformations:
/// - Translation
/// - Rotation
/// - Scaling
/// - Skewing
/// - Perspective
///
/// # Performance
///
/// `TransformLayer` is more expensive than `OffsetLayer`. Use `OffsetLayer`
/// when only translation is needed.
///
/// # Architecture
///
/// ```text
/// TransformLayer
///   │
///   │ Apply 4x4 matrix transform
///   ▼
/// Children rendered with transformation
/// ```
///
/// # Example
///
/// ```rust
/// use std::f64::consts::PI;
///
/// use flui_layer::TransformLayer;
/// use flui_foundation::geometry::Matrix4;
///
/// // Create a rotation transform (45 degrees)
/// let layer = TransformLayer::rotation(PI / 4.0);
///
/// // Or use a custom matrix
/// let matrix = Matrix4::translation(100.0, 50.0, 0.0);
/// let layer = TransformLayer::new(matrix);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TransformLayer {
    /// The transformation matrix
    transform: Matrix4,
}

impl TransformLayer {
    /// Transforms the subtree by `transform`.
    #[inline]
    pub const fn new(transform: Matrix4) -> Self {
        Self { transform }
    }

    /// The identity transform.
    #[inline]
    pub fn identity() -> Self {
        Self::new(Matrix4::identity())
    }

    /// A pure translation by `(dx, dy)` device pixels.
    #[inline]
    pub fn translation(dx: f64, dy: f64) -> Self {
        Self::new(Matrix4::translation(dx, dy, 0.0))
    }

    /// A rotation about the Z axis by `angle` radians.
    #[inline]
    pub fn rotation(angle: f64) -> Self {
        Self::new(Matrix4::rotation_z(angle))
    }

    /// A uniform scale by `s` about the origin.
    #[inline]
    pub fn scale(s: f64) -> Self {
        Self::new(Matrix4::scaling(s, s, 1.0))
    }

    /// The matrix applied to the subtree.
    #[inline]
    pub const fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Whether the matrix is the identity (the engine pushes no transform for it).
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.transform.is_identity()
    }

    /// `point` mapped through the matrix.
    #[inline]
    pub fn transform_point(&self, point: Point<f64>) -> Point<f64> {
        let (x, y) = self.transform.transform_point(point.x, point.y);
        Point::new(x, y)
    }

    /// The inverse transform; `None` when the matrix is singular.
    #[inline]
    pub fn try_inverse(&self) -> Option<TransformLayer> {
        self.transform.try_inverse().map(TransformLayer::new)
    }
}

impl Default for TransformLayer {
    fn default() -> Self {
        Self::identity()
    }
}
