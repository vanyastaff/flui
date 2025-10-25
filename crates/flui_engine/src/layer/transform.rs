//! Transform layer - applies matrix transform to child layer

use flui_types::{Rect, Offset};
use crate::layer::{Layer, BoxedLayer};
use crate::painter::Painter;

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

    // TODO: Add full 2D matrix transform when needed
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
        Self {
            child,
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
}
