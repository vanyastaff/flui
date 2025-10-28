//! RenderOpacity - applies opacity to a child using OpacityLayer

use flui_types::Size;
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{OpacityLayer, BoxedLayer};

/// RenderObject that applies opacity to its child
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// Changing opacity only affects painting, not layout.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOpacity;
///
/// let opacity = RenderOpacity::new(0.5);
/// ```
#[derive(Debug)]
pub struct RenderOpacity {
    /// Opacity value (0.0 = fully transparent, 1.0 = fully opaque)
    pub opacity: f32,
}

impl RenderOpacity {
    /// Create new RenderOpacity
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0)
        }
    }

    /// Set new opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

impl RenderObject for RenderOpacity {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Capture child layer
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        // Wrap in OpacityLayer
        Box::new(OpacityLayer::new(child_layer, self.opacity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::render::LayoutCx;
    use flui_types::constraints::BoxConstraints;

    #[test]
    fn test_render_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert_eq!(opacity.opacity, 0.5);
    }

    #[test]
    fn test_render_opacity_clamping() {
        let opacity1 = RenderOpacity::new(-0.5);
        assert_eq!(opacity1.opacity, 0.0);

        let opacity2 = RenderOpacity::new(1.5);
        assert_eq!(opacity2.opacity, 1.0);
    }

    #[test]
    fn test_render_opacity_set_opacity() {
        let mut opacity = RenderOpacity::new(0.5);
        opacity.set_opacity(0.8);
        assert_eq!(opacity.opacity, 0.8);
    }
}
