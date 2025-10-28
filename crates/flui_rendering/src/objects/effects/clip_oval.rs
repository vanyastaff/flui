//! RenderClipOval - clips child to an oval shape

use flui_types::{Size, painting::Clip};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::BoxedLayer;

/// RenderObject that clips its child to an oval shape
///
/// The oval fills the bounds of this RenderObject.
/// If the bounds are square, this creates a circle.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderClipOval;
/// use flui_types::painting::Clip;
///
/// let clip_oval = RenderClipOval::new(Clip::AntiAlias);
/// ```
#[derive(Debug)]
pub struct RenderClipOval {
    /// The clipping behavior (None, HardEdge, AntiAlias, etc.)
    pub clip_behavior: Clip,
}

impl RenderClipOval {
    /// Create new RenderClipOval with specified clip behavior
    pub fn new(clip_behavior: Clip) -> Self {
        Self { clip_behavior }
    }

    /// Create with hard edge clipping
    pub fn hard_edge() -> Self {
        Self {
            clip_behavior: Clip::HardEdge,
        }
    }

    /// Create with anti-aliased clipping (default for ovals)
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

impl Default for RenderClipOval {
    fn default() -> Self {
        Self::anti_alias()
    }
}

impl RenderObject for RenderClipOval {
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
        

        // TODO: Implement ClipOvalLayer when oval clipping is supported
        // For now, just return the child layer without clipping
        // In a real implementation, we would:
        // 1. Create a ClipOvalLayer
        // 2. Add the child layer to it
        // 3. Return the ClipOvalLayer
        //
        // Alternative approaches:
        // - Use ClipPathLayer with an oval path
        // - Render to offscreen buffer and mask it
        // - Use backend-specific oval clipping

        (cx.capture_child_layer(child)) as _
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_oval_new() {
        let clip = RenderClipOval::new(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_default() {
        let clip = RenderClipOval::default();
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_hard_edge() {
        let clip = RenderClipOval::hard_edge();
        assert_eq!(clip.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_oval_anti_alias() {
        let clip = RenderClipOval::anti_alias();
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_set_clip_behavior() {
        let mut clip = RenderClipOval::hard_edge();
        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior, Clip::AntiAlias);
    }
}
