//! TransformLayer - Full matrix transformation layer
//!
//! This layer applies a full 4x4 transformation matrix to its children.
//! Corresponds to Flutter's `TransformLayer`.

use flui_types::geometry::{Point, Rect};
use flui_types::Matrix4;

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
/// use flui_layer::TransformLayer;
/// use flui_types::Matrix4;
/// use std::f32::consts::PI;
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
    /// Creates a new transform layer with the given matrix.
    #[inline]
    pub const fn new(transform: Matrix4) -> Self {
        Self { transform }
    }

    /// Creates an identity transform layer (no transformation).
    #[inline]
    pub fn identity() -> Self {
        Self::new(Matrix4::identity())
    }

    /// Creates a translation transform layer.
    #[inline]
    pub fn translation(dx: f32, dy: f32) -> Self {
        Self::new(Matrix4::translation(dx, dy, 0.0))
    }

    /// Creates a rotation transform layer (around Z axis).
    ///
    /// # Arguments
    ///
    /// * `angle` - Rotation angle in radians
    #[inline]
    pub fn rotation(angle: f32) -> Self {
        Self::new(Matrix4::rotation_z(angle))
    }

    /// Creates a rotation transform layer around an anchor point.
    ///
    /// # Arguments
    ///
    /// * `angle` - Rotation angle in radians
    /// * `anchor` - The point to rotate around
    pub fn rotation_around(angle: f32, anchor: Point) -> Self {
        // Translate to origin, rotate, translate back
        let translate_to_origin = Matrix4::translation(-anchor.x, -anchor.y, 0.0);
        let rotate = Matrix4::rotation_z(angle);
        let translate_back = Matrix4::translation(anchor.x, anchor.y, 0.0);

        Self::new(translate_back * rotate * translate_to_origin)
    }

    /// Creates a uniform scale transform layer.
    #[inline]
    pub fn scale(s: f32) -> Self {
        Self::new(Matrix4::scaling(s, s, 1.0))
    }

    /// Creates a non-uniform scale transform layer.
    #[inline]
    pub fn scale_xy(sx: f32, sy: f32) -> Self {
        Self::new(Matrix4::scaling(sx, sy, 1.0))
    }

    /// Creates a scale transform layer around an anchor point.
    pub fn scale_around(sx: f32, sy: f32, anchor: Point) -> Self {
        let translate_to_origin = Matrix4::translation(-anchor.x, -anchor.y, 0.0);
        let scale = Matrix4::scaling(sx, sy, 1.0);
        let translate_back = Matrix4::translation(anchor.x, anchor.y, 0.0);

        Self::new(translate_back * scale * translate_to_origin)
    }

    /// Creates a skew transform layer.
    ///
    /// # Arguments
    ///
    /// * `skew_x` - Horizontal skew factor (radians)
    /// * `skew_y` - Vertical skew factor (radians)
    #[inline]
    pub fn skew(skew_x: f32, skew_y: f32) -> Self {
        Self::new(Matrix4::skew_2d(skew_x, skew_y))
    }

    /// Returns a reference to the transformation matrix.
    #[inline]
    pub const fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Sets the transformation matrix.
    #[inline]
    pub fn set_transform(&mut self, transform: Matrix4) {
        self.transform = transform;
    }

    /// Returns true if this is an identity transform.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.transform.is_identity()
    }

    /// Returns true if this transform only contains translation.
    ///
    /// A translation-only transform has identity rotation/scale components
    /// and only non-zero translation values.
    #[inline]
    pub fn is_translation_only(&self) -> bool {
        self.transform.is_translation_only()
    }

    /// Transforms a point by the matrix.
    #[inline]
    pub fn transform_point(&self, point: Point) -> Point {
        let (x, y) = self.transform.transform_point(point.x, point.y);
        Point::new(x, y)
    }

    /// Transforms bounds by the matrix.
    ///
    /// Returns the axis-aligned bounding box of the transformed rectangle.
    /// This may be larger than the original bounds if the transform includes
    /// rotation.
    pub fn transform_bounds(&self, bounds: Rect) -> Rect {
        // Transform all four corners
        let corners = [
            self.transform_point(Point::new(bounds.left(), bounds.top())),
            self.transform_point(Point::new(bounds.right(), bounds.top())),
            self.transform_point(Point::new(bounds.right(), bounds.bottom())),
            self.transform_point(Point::new(bounds.left(), bounds.bottom())),
        ];

        // Find bounding box of transformed corners
        let mut min_x = corners[0].x;
        let mut min_y = corners[0].y;
        let mut max_x = corners[0].x;
        let mut max_y = corners[0].y;

        for corner in &corners[1..] {
            min_x = min_x.min(corner.x);
            min_y = min_y.min(corner.y);
            max_x = max_x.max(corner.x);
            max_y = max_y.max(corner.y);
        }

        Rect::from_min_max(Point::new(min_x, min_y), Point::new(max_x, max_y))
    }

    /// Concatenates another transform to this one.
    ///
    /// The result is equivalent to applying `other` first, then `self`.
    #[inline]
    pub fn concat(&mut self, other: &Matrix4) {
        self.transform *= *other;
    }

    /// Pre-concatenates another transform to this one.
    ///
    /// The result is equivalent to applying `self` first, then `other`.
    #[inline]
    pub fn pre_concat(&mut self, other: &Matrix4) {
        self.transform = *other * self.transform;
    }

    /// Returns the inverse of this transform, if it exists.
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

// Thread safety
unsafe impl Send for TransformLayer {}
unsafe impl Sync for TransformLayer {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    #[test]
    fn test_transform_layer_new() {
        let matrix = Matrix4::translation(10.0, 20.0, 0.0);
        let layer = TransformLayer::new(matrix);

        assert_eq!(layer.transform(), &matrix);
    }

    #[test]
    fn test_transform_layer_identity() {
        let layer = TransformLayer::identity();

        assert!(layer.is_identity());
    }

    #[test]
    fn test_transform_layer_translation() {
        let layer = TransformLayer::translation(10.0, 20.0);

        assert!(layer.is_translation_only());

        let point = layer.transform_point(Point::new(5.0, 5.0));
        assert!((point.x - 15.0).abs() < 0.001);
        assert!((point.y - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_rotation() {
        let layer = TransformLayer::rotation(FRAC_PI_2); // 90 degrees

        let point = layer.transform_point(Point::new(1.0, 0.0));
        assert!(point.x.abs() < 0.001);
        assert!((point.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_rotation_around() {
        let center = Point::new(50.0, 50.0);
        let layer = TransformLayer::rotation_around(PI, center); // 180 degrees

        // Point at (100, 50) should rotate to (0, 50) around center (50, 50)
        let point = layer.transform_point(Point::new(100.0, 50.0));
        assert!((point.x - 0.0).abs() < 0.001);
        assert!((point.y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_scale() {
        let layer = TransformLayer::scale(2.0);

        let point = layer.transform_point(Point::new(10.0, 20.0));
        assert!((point.x - 20.0).abs() < 0.001);
        assert!((point.y - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_scale_xy() {
        let layer = TransformLayer::scale_xy(2.0, 3.0);

        let point = layer.transform_point(Point::new(10.0, 10.0));
        assert!((point.x - 20.0).abs() < 0.001);
        assert!((point.y - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_scale_around() {
        let center = Point::new(50.0, 50.0);
        let layer = TransformLayer::scale_around(2.0, 2.0, center);

        // Point at (100, 100) scaled 2x around (50, 50) should be at (150, 150)
        let point = layer.transform_point(Point::new(100.0, 100.0));
        assert!((point.x - 150.0).abs() < 0.001);
        assert!((point.y - 150.0).abs() < 0.001);

        // Center should remain unchanged
        let center_result = layer.transform_point(center);
        assert!((center_result.x - 50.0).abs() < 0.001);
        assert!((center_result.y - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_transform_bounds() {
        let layer = TransformLayer::translation(10.0, 20.0);
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);

        let transformed = layer.transform_bounds(bounds);
        assert!((transformed.left() - 10.0).abs() < 0.001);
        assert!((transformed.top() - 20.0).abs() < 0.001);
        assert!((transformed.width() - 100.0).abs() < 0.001);
        assert!((transformed.height() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_transform_bounds_rotation() {
        let layer = TransformLayer::rotation(FRAC_PI_2); // 90 degrees
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);

        let transformed = layer.transform_bounds(bounds);
        // After 90 degree rotation, width and height should swap
        assert!((transformed.width() - 50.0).abs() < 0.001);
        assert!((transformed.height() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_set_transform() {
        let mut layer = TransformLayer::identity();

        layer.set_transform(Matrix4::scaling(2.0, 2.0, 1.0));
        assert!(!layer.is_identity());
    }

    #[test]
    fn test_transform_layer_concat() {
        let mut layer = TransformLayer::translation(10.0, 0.0);
        let scale = Matrix4::scaling(2.0, 2.0, 1.0);

        layer.concat(&scale);

        // Point at (5, 0): scale first (10, 0), then translate (20, 0)
        let point = layer.transform_point(Point::new(5.0, 0.0));
        assert!((point.x - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_try_inverse() {
        let layer = TransformLayer::scale(2.0);
        let inverse = layer.try_inverse().unwrap();

        // Applying transform then inverse should give identity
        let point = Point::new(10.0, 20.0);
        let transformed = layer.transform_point(point);
        let back = inverse.transform_point(transformed);

        assert!((back.x - point.x).abs() < 0.001);
        assert!((back.y - point.y).abs() < 0.001);
    }

    #[test]
    fn test_transform_layer_default() {
        let layer = TransformLayer::default();
        assert!(layer.is_identity());
    }

    #[test]
    fn test_transform_layer_clone() {
        let layer = TransformLayer::translation(10.0, 20.0);
        let cloned = layer.clone();

        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_transform_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<TransformLayer>();
        assert_sync::<TransformLayer>();
    }
}
