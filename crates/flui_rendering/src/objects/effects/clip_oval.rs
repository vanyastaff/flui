//! RenderClipOval - clips child to an oval shape

use flui_painting::Canvas;
use flui_types::{painting::Clip, Size};

use super::clip_base::{ClipShape, RenderClip};

/// Shape implementation for oval clipping
#[derive(Debug, Clone, Copy)]
pub struct OvalShape;

impl ClipShape for OvalShape {
    fn create_clip_layer(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer {
        use flui_engine::ClipOvalLayer;
        use flui_types::Rect;

        // Create oval clip layer with bounding rect
        let clip_rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
        let mut clip_layer = ClipOvalLayer::new(clip_rect);
        clip_layer.add_child(child_layer);
        Box::new(clip_layer)
    }
}

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
pub type RenderClipOval = RenderClip<OvalShape>;

impl RenderClipOval {
    /// Create with specified clip behavior
    pub fn with_clip(clip_behavior: Clip) -> Self {
        RenderClip::new(OvalShape, clip_behavior)
    }

    /// Create with hard edge clipping
    pub fn hard_edge() -> Self {
        Self::with_clip(Clip::HardEdge)
    }

    /// Create with anti-aliased clipping (default for ovals)
    pub fn anti_alias() -> Self {
        Self::with_clip(Clip::AntiAlias)
    }
}

impl Default for RenderClipOval {
    fn default() -> Self {
        Self::anti_alias()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_oval_with_clip() {
        let clip = RenderClipOval::with_clip(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_default() {
        let clip = RenderClipOval::default();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_hard_edge() {
        let clip = RenderClipOval::hard_edge();
        assert_eq!(clip.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_render_clip_oval_anti_alias() {
        let clip = RenderClipOval::anti_alias();
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_render_clip_oval_set_clip_behavior() {
        let mut clip = RenderClipOval::hard_edge();
        clip.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(clip.clip_behavior(), Clip::AntiAlias);
    }
}
