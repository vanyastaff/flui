//! RenderClipRect - clips child to a rectangle

use flui_painting::Canvas;
use flui_types::{painting::Clip, Rect, Size};

use super::clip_base::{ClipShape, RenderClip};

/// Shape implementation for rectangular clipping
#[derive(Debug, Clone, Copy)]
pub struct RectShape;

impl ClipShape for RectShape {
    fn create_clip_layer(&self, child_layer: BoxedLayer, size: Size) -> BoxedLayer {
        let clip_rect = Rect::from_xywh(0.0, 0.0, size.width, size.height);
        // Use pool for allocation efficiency
        let mut clip_layer = flui_engine::layer::pool::acquire_clip_rect();
        clip_layer.set_clip_shape(clip_rect);
        clip_layer.add_child(child_layer);
        Box::new(clip_layer)
    }
}

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
pub type RenderClipRect = RenderClip<RectShape>;

impl RenderClipRect {
    /// Create with specified clip behavior
    pub fn with_clip(clip_behavior: Clip) -> Self {
        RenderClip::new(RectShape, clip_behavior)
    }

    /// Create with hard edge clipping (default)
    pub fn hard_edge() -> Self {
        Self::with_clip(Clip::HardEdge)
    }

    /// Create with anti-aliased clipping
    pub fn anti_alias() -> Self {
        Self::with_clip(Clip::AntiAlias)
    }
}

impl Default for RenderClipRect {
    fn default() -> Self {
        Self::hard_edge()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_clip_rect_with_clip() {
        let clip = RenderClipRect::with_clip(Clip::AntiAlias);
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
