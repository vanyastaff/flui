//! RenderTransform - applies matrix transformation to child

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::{Offset, Size};

/// Transform operations
#[derive(Debug, Clone, PartialEq)]
pub enum Transform {
    /// Translate by offset
    Translate(Offset),
    /// Rotate by angle (radians)
    Rotate(f32),
    /// Scale uniformly
    Scale(f32),
    /// Scale non-uniformly
    ScaleXY {
        /// X-axis scale factor
        sx: f32,
        /// Y-axis scale factor
        sy: f32
    },
    /// Skew (shear) transform
    Skew {
        /// X-axis skew angle in radians
        skew_x: f32,
        /// Y-axis skew angle in radians
        skew_y: f32
    },
    /// Arbitrary 2D affine transformation matrix
    Matrix {
        /// X-axis scale/rotation component
        a: f32,
        /// Y-axis shear component
        b: f32,
        /// X-axis shear component
        c: f32,
        /// Y-axis scale/rotation component
        d: f32,
        /// X-axis translation
        tx: f32,
        /// Y-axis translation
        ty: f32,
    },
}

/// RenderObject that applies a transformation to its child
///
/// The transformation is applied during painting. It doesn't affect layout,
/// so the child is laid out as if untransformed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderTransform;
/// use flui_rendering::objects::Transform;
///
/// let transform = RenderTransform::new(Transform::Rotate(std::f32::consts::PI / 4.0));
/// ```
#[derive(Debug)]
pub struct RenderTransform {
    /// The transformation to apply
    pub transform: Transform,

    /// Origin point for rotation/scale (relative to child size)
    pub alignment: Offset,
}

impl RenderTransform {
    /// Create new RenderTransform
    pub fn new(transform: Transform) -> Self {
        Self {
            transform,
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

        // Apply the specific transform based on type
        match self.transform {
            Transform::Translate(t_offset) => {
                canvas.translate(t_offset.dx, t_offset.dy);
            }
            Transform::Rotate(angle) => {
                // Apply alignment offset, rotate, then reverse alignment
                canvas.translate(self.alignment.dx, self.alignment.dy);
                canvas.rotate(angle);
                canvas.translate(-self.alignment.dx, -self.alignment.dy);
            }
            Transform::Scale(scale) => {
                canvas.translate(self.alignment.dx, self.alignment.dy);
                canvas.scale(scale, None);
                canvas.translate(-self.alignment.dx, -self.alignment.dy);
            }
            Transform::ScaleXY { sx, sy } => {
                canvas.translate(self.alignment.dx, self.alignment.dy);
                canvas.scale(sx, Some(sy));
                canvas.translate(-self.alignment.dx, -self.alignment.dy);
            }
            Transform::Skew { skew_x, skew_y } => {
                canvas.translate(self.alignment.dx, self.alignment.dy);
                // Use Matrix4::skew_2d for skew transformation
                use flui_types::Matrix4;
                let skew_matrix = Matrix4::skew_2d(skew_x, skew_y);
                canvas.set_transform(skew_matrix);
                canvas.translate(-self.alignment.dx, -self.alignment.dy);
            }
            Transform::Matrix { a, b, c, d, tx, ty } => {
                // Apply 2D affine transformation matrix
                // Matrix4 stores in column-major order: [m00, m10, m20, m30, m01, m11, ...]
                use flui_types::Matrix4;
                let matrix = Matrix4::new(
                    a, b, 0.0, 0.0,      // column 0
                    c, d, 0.0, 0.0,      // column 1
                    0.0, 0.0, 1.0, 0.0,  // column 2
                    tx, ty, 0.0, 1.0,    // column 3
                );
                canvas.set_transform(matrix);
            }
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

    #[test]
    fn test_render_transform_new() {
        let transform = RenderTransform::new(Transform::Rotate(1.0));
        assert!(matches!(transform.transform, Transform::Rotate(_)));
    }

    #[test]
    fn test_render_transform_with_alignment() {
        let transform =
            RenderTransform::with_alignment(Transform::Scale(2.0), Offset::new(0.5, 0.5));
        assert!(matches!(transform.transform, Transform::Scale(_)));
        assert_eq!(transform.alignment, Offset::new(0.5, 0.5));
    }

    #[test]
    fn test_render_transform_set_transform() {
        let mut transform = RenderTransform::new(Transform::Translate(Offset::ZERO));
        transform.set_transform(Transform::Rotate(1.5));
        assert!(matches!(transform.transform, Transform::Rotate(_)));

        
    }
}
