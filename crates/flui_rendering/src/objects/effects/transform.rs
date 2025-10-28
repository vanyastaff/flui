//! RenderTransform - applies matrix transformation to child

use flui_types::{Size, Offset};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{TransformLayer, Transform, BoxedLayer};

/// RenderObject that applies a transformation to its child
///
/// The transformation is applied during painting. It doesn't affect layout,
/// so the child is laid out as if untransformed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderTransform;
/// use flui_engine::Transform;
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

impl RenderObject for RenderTransform {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints (transform doesn't affect layout)
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Capture child layer
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        // Wrap in TransformLayer
        Box::new(TransformLayer::new(child_layer, self.transform))
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
        let transform = RenderTransform::with_alignment(
            Transform::Scale(2.0),
            Offset::new(0.5, 0.5)
        );
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
