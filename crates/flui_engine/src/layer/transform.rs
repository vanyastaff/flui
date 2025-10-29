//! Transform layer - applies matrix transform to child layer

use crate::layer::{BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::{Offset, Rect};
use flui_types::events::{Event, HitTestResult};

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
    /// The child layer to transform
    child: BoxedLayer,

    /// The transform to apply
    transform: Transform,
}

impl TransformLayer {
    /// Create a new transform layer
    pub fn new(child: BoxedLayer, transform: Transform) -> Self {
        Self { child, transform }
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
    pub fn child(&self) -> &BoxedLayer {
        &self.child
    }
}

impl Layer for TransformLayer {
    fn paint(&self, painter: &mut dyn Painter) {
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
        self.child.paint(painter);

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        // TODO: Transform the child bounds by the matrix
        // For now, just return child bounds (conservative approximation)
        self.child.bounds()
    }

    fn is_visible(&self) -> bool {
        self.child.is_visible()
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
        self.child.hit_test(local_position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Forward event to child
        self.child.handle_event(event)
    }
}
