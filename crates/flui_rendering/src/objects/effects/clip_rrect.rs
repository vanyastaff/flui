//! RenderClipRRect - clips child to rounded rectangle

use flui_types::{Size, styling::BorderRadius, painting::Clip};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{ClipRRectLayer, BoxedLayer};

/// RenderObject that clips its child to a rounded rectangle
///
/// The clipping is applied during painting with rounded corners.
/// It doesn't affect layout, so the child is laid out normally
/// and then clipped to its bounds with rounded corners.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderClipRRect;
/// use flui_types::styling::BorderRadius;
/// use flui_types::painting::Clip;
///
/// let clip = RenderClipRRect::circular(10.0);
/// ```
#[derive(Debug)]
pub struct RenderClipRRect {
    /// Border radius for rounded corners
    pub border_radius: BorderRadius,
    /// The clipping behavior (None, HardEdge, AntiAlias, etc.)
    pub clip_behavior: Clip,
}

impl RenderClipRRect {
    /// Create new RenderClipRRect with border radius and clip behavior
    pub fn new(border_radius: BorderRadius, clip_behavior: Clip) -> Self {
        Self {
            border_radius,
            clip_behavior,
        }
    }

    /// Create with circular radius (all corners same)
    pub fn circular(radius: f32) -> Self {
        Self::new(BorderRadius::circular(radius), Clip::AntiAlias)
    }

    /// Set new border radius
    pub fn set_border_radius(&mut self, border_radius: BorderRadius) {
        self.border_radius = border_radius;
    }

    /// Set new clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        self.clip_behavior = clip_behavior;
    }
}

impl Default for RenderClipRRect {
    fn default() -> Self {
        Self::circular(0.0)
    }
}

impl RenderObject for RenderClipRRect {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Layout child with same constraints
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // If no clipping needed, just return child layer
        if !self.clip_behavior.clips() {
            let child = cx.child();
            return cx.capture_child_layer(child);
        }

        // Get child layer
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);

        // TODO: Get actual size from layout context instead of using placeholder
        // The PaintCx should provide access to the laid-out size from the layout phase
        let size = 1000.0;

        // Create RRect from border radius
        // Note: In a real implementation, we'd use the actual child bounds
        // For now, use a uniform radius (average of all corners)
        let avg_radius = (self.border_radius.top_left.x + self.border_radius.top_right.x +
                         self.border_radius.bottom_right.x + self.border_radius.bottom_left.x) / 4.0;

        let rrect = flui_engine::painter::RRect {
            rect: flui_types::Rect::from_xywh(0.0, 0.0, size, size),
            corner_radius: avg_radius,
        };

        // Wrap in ClipRRectLayer
        let mut clip_layer = ClipRRectLayer::new(rrect);
        clip_layer.add_child(child_layer);

        Box::new(clip_layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_rrect_new() {
        let radius = BorderRadius::circular(10.0);
        let clip = RenderClipRRect::new(radius, Clip::AntiAlias);
        assert_eq!(clip.border_radius, radius);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_circular() {
        let clip = RenderClipRRect::circular(15.0);
        assert_eq!(clip.border_radius, BorderRadius::circular(15.0));
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_default() {
        let clip = RenderClipRRect::default();
        assert_eq!(clip.border_radius, BorderRadius::circular(0.0));
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rrect_set_border_radius() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.set_border_radius(BorderRadius::circular(20.0));
        assert_eq!(clip.border_radius, BorderRadius::circular(20.0));
    }

    #[test]
    fn test_render_clip_rrect_set_clip_behavior() {
        let mut clip = RenderClipRRect::circular(10.0);
        clip.set_clip_behavior(Clip::HardEdge);
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }
}
