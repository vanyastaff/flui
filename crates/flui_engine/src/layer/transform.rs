//! Transform layer - applies matrix transform to child layer

use crate::layer::{base_single_child::SingleChildLayerBase, BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult};
use flui_types::{Offset, Rect};

/// Type of transform to apply
#[derive(Debug, Clone, Copy)]
pub enum Transform {
    /// Translate by offset
    Translate(Offset),

    /// Rotate by angle (radians)
    Rotate(f32),

    /// Scale uniformly
    Scale(f32),

    /// Scale non-uniformly
    ScaleXY { sx: f32, sy: f32 },

    /// Skew (shear) transform - creates parallelogram effect
    /// - `skew_x`: horizontal skew angle in radians
    /// - `skew_y`: vertical skew angle in radians
    Skew { skew_x: f32, skew_y: f32 },

    /// Full 2D affine transformation matrix
    /// [a, b, c, d, tx, ty] represents:
    /// | a  c  tx |
    /// | b  d  ty |
    /// | 0  0  1  |
    Matrix {
        a: f32,  // x scale / horizontal stretch
        b: f32,  // vertical skew
        c: f32,  // horizontal skew
        d: f32,  // y scale / vertical stretch
        tx: f32, // x translation
        ty: f32, // y translation
    },

    /// Trapezoid/Perspective transform - applies vertical gradient scaling
    /// Creates pyramid or trapezoid effect by scaling differently at top and bottom
    /// - `top_scale`: horizontal scale factor at the top (1.0 = normal width)
    /// - `bottom_scale`: horizontal scale factor at the bottom (1.0 = normal width)
    ///
    /// Example: top_scale=0.5, bottom_scale=1.0 creates pyramid (narrow top, wide bottom)
    Trapezoid { top_scale: f32, bottom_scale: f32 },
}

/// Layer that applies a transform to its child
///
/// Transforms affect both layout and painting. The child is painted
/// in the transformed coordinate space.
///
/// # Example
///
/// ```text
/// TransformLayer (rotate 45°)
///   └─ PictureLayer (draws square)
/// Result: Rotated square
/// ```
pub struct TransformLayer {
    /// Base single-child layer functionality
    base: SingleChildLayerBase,

    /// The transform to apply
    transform: Transform,
}

impl TransformLayer {
    /// Create a new transform layer
    pub fn new(child: BoxedLayer, transform: Transform) -> Self {
        Self {
            base: SingleChildLayerBase::new(child),
            transform,
        }
    }

    /// Create a translation transform layer
    pub fn translate(child: BoxedLayer, offset: Offset) -> Self {
        Self::new(child, Transform::Translate(offset))
    }

    /// Create a rotation transform layer
    pub fn rotate(child: BoxedLayer, angle: f32) -> Self {
        Self::new(child, Transform::Rotate(angle))
    }

    /// Create a scale transform layer
    pub fn scale(child: BoxedLayer, scale: f32) -> Self {
        Self::new(child, Transform::Scale(scale))
    }

    /// Create a non-uniform scale transform layer
    pub fn scale_xy(child: BoxedLayer, sx: f32, sy: f32) -> Self {
        Self::new(child, Transform::ScaleXY { sx, sy })
    }

    /// Create a skew transform layer
    pub fn skew(child: BoxedLayer, skew_x: f32, skew_y: f32) -> Self {
        Self::new(child, Transform::Skew { skew_x, skew_y })
    }

    /// Create a skew X transform layer (horizontal skew only)
    pub fn skew_x(child: BoxedLayer, angle: f32) -> Self {
        Self::new(
            child,
            Transform::Skew {
                skew_x: angle,
                skew_y: 0.0,
            },
        )
    }

    /// Create a skew Y transform layer (vertical skew only)
    pub fn skew_y(child: BoxedLayer, angle: f32) -> Self {
        Self::new(
            child,
            Transform::Skew {
                skew_x: 0.0,
                skew_y: angle,
            },
        )
    }

    /// Create a matrix transform layer
    pub fn matrix(child: BoxedLayer, a: f32, b: f32, c: f32, d: f32, tx: f32, ty: f32) -> Self {
        Self::new(child, Transform::Matrix { a, b, c, d, tx, ty })
    }

    /// Create a trapezoid/perspective transform layer
    /// - `top_scale`: horizontal scale at the top (< 1.0 makes top narrow)
    /// - `bottom_scale`: horizontal scale at the bottom (> 1.0 makes bottom wide)
    ///
    /// Example: trapezoid(child, 0.5, 1.0) creates pyramid (narrow top, wide bottom)
    pub fn trapezoid(child: BoxedLayer, top_scale: f32, bottom_scale: f32) -> Self {
        Self::new(
            child,
            Transform::Trapezoid {
                top_scale,
                bottom_scale,
            },
        )
    }

    /// Get the transform
    pub fn transform(&self) -> Transform {
        self.transform
    }

    /// Set the transform
    pub fn set_transform(&mut self, transform: Transform) {
        self.transform = transform;
    }

    /// Get the child layer
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.base.child()
    }

    /// Apply the transform to a point (forward transform)
    fn transform_point(&self, point: Offset) -> Offset {
        match self.transform {
            Transform::Translate(offset) => Offset::new(point.dx + offset.dx, point.dy + offset.dy),
            Transform::Rotate(angle) => {
                let cos = angle.cos();
                let sin = angle.sin();
                Offset::new(
                    point.dx * cos - point.dy * sin,
                    point.dx * sin + point.dy * cos,
                )
            }
            Transform::Scale(scale) => Offset::new(point.dx * scale, point.dy * scale),
            Transform::ScaleXY { sx, sy } => Offset::new(point.dx * sx, point.dy * sy),
            Transform::Skew { skew_x, skew_y } => {
                let tan_x = skew_x.tan();
                let tan_y = skew_y.tan();
                Offset::new(point.dx + tan_x * point.dy, tan_y * point.dx + point.dy)
            }
            Transform::Matrix { a, b, c, d, tx, ty } => Offset::new(
                a * point.dx + c * point.dy + tx,
                b * point.dx + d * point.dy + ty,
            ),
            Transform::Trapezoid {
                top_scale,
                bottom_scale,
            } => {
                // For trapezoid, we need the child bounds to calculate normalized y
                // For bounds calculation, we'll approximate by using the larger scale
                // This gives a conservative bounding box
                let max_scale = top_scale.max(bottom_scale);
                Offset::new(point.dx * max_scale, point.dy)
            }
        }
    }
}

impl Layer for TransformLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        let Some(child) = self.base.child() else {
            return;
        };

        painter.save();

        // Apply transform
        match self.transform {
            Transform::Translate(offset) => {
                painter.translate(offset);
            }
            Transform::Rotate(angle) => {
                painter.rotate(angle);
            }
            Transform::Scale(scale) => {
                painter.scale(scale, scale);
            }
            Transform::ScaleXY { sx, sy } => {
                painter.scale(sx, sy);
            }
            Transform::Skew { skew_x, skew_y } => {
                painter.skew(skew_x, skew_y);
            }
            Transform::Matrix { a, b, c, d, tx, ty } => {
                painter.transform_matrix(a, b, c, d, tx, ty);
            }
            Transform::Trapezoid { .. } => {
                // Trapezoid is a non-affine transform (non-linear gradient scaling)
                // that cannot be represented by standard painter methods.
                //
                // To achieve trapezoid/pyramid text effects, use per-character rendering
                // with flui_types::text_path::vertical_scale() helper:
                //
                // Example:
                //   for (i, ch) in text.chars().enumerate() {
                //       let y_norm = i as f32 / total as f32;
                //       let scale_x = vertical_scale(y_norm, 0.5, 1.0);
                //       painter.save();
                //       painter.scale(scale_x, 1.0);
                //       painter.text(&ch.to_string(), pos, size, &paint);
                //       painter.restore();
                //   }
                //
                // For this variant, child is painted without transform.
            }
        }

        // Paint child in transformed space
        child.paint(painter);

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        let child_bounds = self.base.child_bounds();

        // Get the four corners of the child bounds
        let corners = child_bounds.corners();

        // Transform each corner point
        let transformed_corners: Vec<Offset> = corners
            .iter()
            .map(|&corner| self.transform_point(Offset::new(corner.x, corner.y)))
            .collect();

        // Find the axis-aligned bounding box of the transformed corners
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for point in &transformed_corners {
            min_x = min_x.min(point.dx);
            min_y = min_y.min(point.dy);
            max_x = max_x.max(point.dx);
            max_y = max_y.max(point.dy);
        }

        // Return the bounding rect
        Rect::from_ltrb(min_x, min_y, max_x, max_y)
    }

    fn is_visible(&self) -> bool {
        self.base.is_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Transform the position to child's coordinate space
        let local_position = match self.transform {
            Transform::Translate(offset) => {
                // Inverse translation: subtract offset
                Offset::new(position.dx - offset.dx, position.dy - offset.dy)
            }
            Transform::Rotate(angle) => {
                // Inverse rotation: rotate by -angle around origin
                let cos = (-angle).cos();
                let sin = (-angle).sin();
                Offset::new(
                    position.dx * cos - position.dy * sin,
                    position.dx * sin + position.dy * cos,
                )
            }
            Transform::Scale(scale) => {
                // Inverse scale: divide by scale
                if scale.abs() < 0.001 {
                    return false; // Degenerate scale, no hit
                }
                Offset::new(position.dx / scale, position.dy / scale)
            }
            Transform::ScaleXY { sx, sy } => {
                // Inverse non-uniform scale
                if sx.abs() < 0.001 || sy.abs() < 0.001 {
                    return false; // Degenerate scale, no hit
                }
                Offset::new(position.dx / sx, position.dy / sy)
            }
            Transform::Skew { skew_x, skew_y } => {
                // Inverse skew transformation
                // For skew matrix: [1, tan_x], [tan_y, 1]
                // Inverse: [1/(1-tan_x*tan_y), -tan_x/(1-tan_x*tan_y)], [-tan_y/(1-tan_x*tan_y), 1/(1-tan_x*tan_y)]
                let tan_x = skew_x.tan();
                let tan_y = skew_y.tan();
                let det = 1.0 - tan_x * tan_y;
                if det.abs() < 0.001 {
                    return false; // Degenerate transform
                }
                let inv_det = 1.0 / det;
                Offset::new(
                    (position.dx - tan_x * position.dy) * inv_det,
                    (position.dy - tan_y * position.dx) * inv_det,
                )
            }
            Transform::Matrix { a, b, c, d, tx, ty } => {
                // Inverse of 2D affine matrix
                // First subtract translation
                let px = position.dx - tx;
                let py = position.dy - ty;

                // Then apply inverse of [a, c; b, d]
                let det = a * d - b * c;
                if det.abs() < 0.001 {
                    return false; // Degenerate transform
                }
                let inv_det = 1.0 / det;
                Offset::new((d * px - c * py) * inv_det, (-b * px + a * py) * inv_det)
            }
            Transform::Trapezoid { .. } => {
                // Trapezoid transform is complex (non-linear gradient)
                // For hit testing, approximate as identity for now
                // TODO: Implement proper inverse trapezoid transform
                position
            }
        };

        // Test child with transformed position
        self.base.child_hit_test(local_position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.child_handle_event(event)
    }

    fn dispose(&mut self) {
        self.base.dispose_child();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn mark_needs_paint(&mut self) {
        self.base.invalidate_cache();
        if let Some(child) = self.base.child_mut() {
            child.mark_needs_paint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::picture::PictureLayer;
    use std::f32::consts::PI;

    fn create_test_layer() -> BoxedLayer {
        // Create a picture layer with known bounds (0,0 -> 100,100)
        let mut picture = PictureLayer::new();
        // Draw a rectangle to set bounds
        picture.draw_rect(
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            crate::painter::Paint::default(),
        );
        Box::new(picture)
    }

    #[test]
    fn test_translate_bounds() {
        let child = create_test_layer();
        let layer = TransformLayer::translate(child, Offset::new(50.0, 30.0));

        let bounds = layer.bounds();
        // Original: (0,0)-(100,100)
        // After translate by (50,30): (50,30)-(150,130)
        assert_eq!(bounds.left(), 50.0);
        assert_eq!(bounds.top(), 30.0);
        assert_eq!(bounds.right(), 150.0);
        assert_eq!(bounds.bottom(), 130.0);
    }

    #[test]
    fn test_scale_bounds() {
        let child = create_test_layer();
        let layer = TransformLayer::scale(child, 2.0);

        let bounds = layer.bounds();
        // Original: (0,0)-(100,100)
        // After scale by 2: (0,0)-(200,200)
        assert_eq!(bounds.left(), 0.0);
        assert_eq!(bounds.top(), 0.0);
        assert_eq!(bounds.right(), 200.0);
        assert_eq!(bounds.bottom(), 200.0);
    }

    #[test]
    fn test_scale_xy_bounds() {
        let child = create_test_layer();
        let layer = TransformLayer::scale_xy(child, 2.0, 0.5);

        let bounds = layer.bounds();
        // Original: (0,0)-(100,100)
        // After scale by (2, 0.5): (0,0)-(200,50)
        assert_eq!(bounds.left(), 0.0);
        assert_eq!(bounds.top(), 0.0);
        assert_eq!(bounds.right(), 200.0);
        assert_eq!(bounds.bottom(), 50.0);
    }

    #[test]
    fn test_rotate_90_bounds() {
        let child = create_test_layer();
        // Rotate 90 degrees counter-clockwise
        let layer = TransformLayer::rotate(child, PI / 2.0);

        let bounds = layer.bounds();
        // Original: (0,0)-(100,100)
        // After 90° rotation: corners become (-100,0), (0,0), (0,100), (-100,100)
        // Bounding box: (-100,0)-(0,100)
        assert!((bounds.left() - (-100.0)).abs() < 0.01);
        assert!((bounds.top() - 0.0).abs() < 0.01);
        assert!((bounds.right() - 0.0).abs() < 0.01);
        assert!((bounds.bottom() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_rotate_45_bounds() {
        let child = create_test_layer();
        // Rotate 45 degrees
        let layer = TransformLayer::rotate(child, PI / 4.0);

        let bounds = layer.bounds();
        // The bounding box of a rotated square should expand
        // Width/height should be approximately 100 * sqrt(2) ≈ 141.42
        let width = bounds.width();
        let height = bounds.height();
        assert!((width - 141.42).abs() < 0.1);
        assert!((height - 141.42).abs() < 0.1);
    }

    #[test]
    fn test_skew_x_bounds() {
        let child = create_test_layer();
        // Skew horizontally by 45 degrees
        let layer = TransformLayer::skew_x(child, PI / 4.0);

        let bounds = layer.bounds();
        // Original: (0,0)-(100,100)
        // After skew_x(45°): corners become (0,0), (100,0), (100+100*tan45,100), (100*tan45,100)
        // tan(45°) = 1.0
        // So: (0,0), (100,0), (200,100), (100,100)
        // Bounding box: (0,0)-(200,100)
        assert!((bounds.left() - 0.0).abs() < 0.01);
        assert!((bounds.top() - 0.0).abs() < 0.01);
        assert!((bounds.right() - 200.0).abs() < 0.1);
        assert!((bounds.bottom() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_matrix_bounds() {
        let child = create_test_layer();
        // Identity matrix should not change bounds
        let layer = TransformLayer::matrix(child, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

        let bounds = layer.bounds();
        assert_eq!(bounds.left(), 0.0);
        assert_eq!(bounds.top(), 0.0);
        assert_eq!(bounds.right(), 100.0);
        assert_eq!(bounds.bottom(), 100.0);
    }

    #[test]
    fn test_matrix_scale_translate_bounds() {
        let child = create_test_layer();
        // Scale by 2 and translate by (10, 20)
        // Matrix: [2, 0, 0, 2, 10, 20]
        let layer = TransformLayer::matrix(child, 2.0, 0.0, 0.0, 2.0, 10.0, 20.0);

        let bounds = layer.bounds();
        // Original: (0,0)-(100,100)
        // After scale by 2: (0,0)-(200,200)
        // After translate by (10,20): (10,20)-(210,220)
        assert_eq!(bounds.left(), 10.0);
        assert_eq!(bounds.top(), 20.0);
        assert_eq!(bounds.right(), 210.0);
        assert_eq!(bounds.bottom(), 220.0);
    }

    #[test]
    fn test_trapezoid_bounds() {
        let child = create_test_layer();
        // Trapezoid with top_scale=0.5, bottom_scale=1.5
        let layer = TransformLayer::trapezoid(child, 0.5, 1.5);

        let bounds = layer.bounds();
        // Conservative approximation uses max_scale = 1.5
        // Width should be 100 * 1.5 = 150
        assert_eq!(bounds.width(), 150.0);
        assert_eq!(bounds.height(), 100.0);
    }
}
