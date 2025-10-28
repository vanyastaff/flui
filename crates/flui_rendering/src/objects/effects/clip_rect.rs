//! RenderClipRect - clips child to a rectangle

use flui_types::{Rect, Size, painting::Clip};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{ClipRectLayer, BoxedLayer};

/// RenderObject that clips its child to a rectangle
///
/// The clipping is applied during painting. It doesn't affect layout,
/// so the child is laid out normally and then clipped to its bounds.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderClipRect;
/// use flui_types::painting::Clip;
///
/// let clip_rect = RenderClipRect::new(Clip::AntiAlias);
/// ```
#[derive(Debug)]
pub struct RenderClipRect {
    /// The clipping behavior (None, HardEdge, AntiAlias, etc.)
    pub clip_behavior: Clip,
}

impl RenderClipRect {
    /// Create new RenderClipRect with specified clip behavior
    pub fn new(clip_behavior: Clip) -> Self {
        Self { clip_behavior }
    }

    /// Create with hard edge clipping (default)
    pub fn hard_edge() -> Self {
        Self {
            clip_behavior: Clip::HardEdge,
        }
    }

    /// Create with anti-aliased clipping
    pub fn anti_alias() -> Self {
        Self {
            clip_behavior: Clip::AntiAlias,
        }
    }

    /// Set new clip behavior
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        self.clip_behavior = clip_behavior;
    }
}

impl Default for RenderClipRect {
    fn default() -> Self {
        Self::hard_edge()
    }
}

impl RenderObject for RenderClipRect {
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
        let clip_rect = Rect::from_xywh(0.0, 0.0, 1000.0, 1000.0);

        // Wrap in ClipRectLayer
        let mut clip_layer = ClipRectLayer::new(clip_rect);
        clip_layer.add_child(child_layer);

        Box::new(clip_layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_rect_new() {
        let clip = RenderClipRect::new(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rect_default() {
        let clip = RenderClipRect::default();
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_rect_hard_edge() {
        let clip = RenderClipRect::hard_edge();
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_rect_anti_alias() {
        let clip = RenderClipRect::anti_alias();
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_rect_set_clip_behavior() {
        let mut clip = RenderClipRect::hard_edge();
        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }
}
