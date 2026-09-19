//! `ClipRectLayer` — clips its subtree to a rectangle.

use flui_types::{
    geometry::{Pixels, Rect},
    painting::Clip,
};

/// Layer that clips children to a rectangle.
///
/// # Architecture
///
/// ```text
/// ClipRectLayer
///   │
///   │ Apply scissor/clip rect
///   ▼
/// Children rendered within clipped bounds
/// ```
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::px;
/// use flui_layer::ClipRectLayer;
/// use flui_types::{geometry::Rect, painting::Clip};
///
/// let layer = ClipRectLayer::new(Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(100.0)), Clip::HardEdge);
///
/// assert_eq!(layer.clip_rect().width(), px(100.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRectLayer {
    /// The clipping rectangle
    clip_rect: Rect<Pixels>,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipRectLayer {
    /// Clips the subtree to a rectangle; `Clip::None` is accepted and lowered as no clip.
    #[inline]
    pub fn new(clip_rect: Rect<Pixels>, clip_behavior: Clip) -> Self {
        Self {
            clip_rect,
            clip_behavior,
        }
    }

    /// Aliased clip: cheapest, jagged on curves and diagonals.
    #[inline]
    pub fn hard_edge(clip_rect: Rect<Pixels>) -> Self {
        Self::new(clip_rect, Clip::HardEdge)
    }

    /// Anti-aliased clip edge.
    #[inline]
    pub fn anti_alias(clip_rect: Rect<Pixels>) -> Self {
        Self::new(clip_rect, Clip::AntiAlias)
    }

    /// The clip shape.
    #[inline]
    pub fn clip_rect(&self) -> Rect<Pixels> {
        self.clip_rect
    }

    /// How the edge is rasterised; `Clip::None` means no clip.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The clip shape's bounding box.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_rect
    }

    /// Whether the layer clips at all (`clip_behavior != Clip::None`).
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_clip_rect_layer_new() {
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = ClipRectLayer::new(rect, Clip::HardEdge);

        assert_eq!(layer.clip_rect(), rect);
        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
        assert_eq!(layer.bounds(), rect);
    }

    #[test]
    fn test_clip_rect_layer_hard_edge() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let layer = ClipRectLayer::hard_edge(rect);

        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
        assert!(layer.clips());
        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_rect_layer_anti_alias() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let layer = ClipRectLayer::anti_alias(rect);

        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
        assert!(layer.clips());
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rect_layer_no_clip() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let layer = ClipRectLayer::new(rect, Clip::None);

        assert!(!layer.clips());
    }
}
