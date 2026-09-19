//! `ClipRRectLayer` — clips its subtree to a rounded rectangle.

use flui_types::{
    geometry::{Pixels, RRect, Rect},
    painting::Clip,
};

/// Layer that clips children to a rounded rectangle.
///
/// Rounded rectangle clipping is commonly used for cards, buttons,
/// and other UI elements with rounded corners.
///
/// # Architecture
///
/// ```text
/// ClipRRectLayer
///   │
///   │ Apply rounded rect clip (GPU stencil or shader)
///   ▼
/// Children rendered within clipped bounds
/// ```
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::px;
/// use flui_layer::ClipRRectLayer;
/// use flui_types::{
///     geometry::{RRect, Rect},
///     painting::Clip,
/// };
///
/// // Create rounded rectangle with 10px corner radius
/// let rrect = RRect::from_rect_circular(
///     Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
///     px(10.0),
/// );
/// let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);
///
/// assert_eq!(layer.clip_rrect().width(), px(100.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRRectLayer {
    /// The rounded rectangle to clip to
    clip_rrect: RRect,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipRRectLayer {
    /// Clips the subtree to a rounded rectangle; `Clip::None` is accepted and lowered as no clip.
    #[inline]
    pub fn new(clip_rrect: RRect, clip_behavior: Clip) -> Self {
        Self {
            clip_rrect,
            clip_behavior,
        }
    }

    /// Anti-aliased clip edge.
    #[inline]
    pub fn anti_alias(clip_rrect: RRect) -> Self {
        Self::new(clip_rrect, Clip::AntiAlias)
    }

    /// Aliased clip: cheapest, jagged on curves and diagonals.
    #[inline]
    pub fn hard_edge(clip_rrect: RRect) -> Self {
        Self::new(clip_rrect, Clip::HardEdge)
    }

    /// The clip shape.
    #[inline]
    pub fn clip_rrect(&self) -> &RRect {
        &self.clip_rrect
    }

    /// How the edge is rasterised; `Clip::None` means no clip.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The clip shape's bounding box.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_rrect.bounding_rect()
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
    fn test_clip_rrect_layer_new() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            px(10.0),
        );
        let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);

        assert_eq!(layer.clip_rrect(), &rrect);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rrect_layer_anti_alias() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            px(5.0),
        );
        let layer = ClipRRectLayer::anti_alias(rrect);

        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_rrect_layer_hard_edge() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            px(5.0),
        );
        let layer = ClipRRectLayer::hard_edge(rrect);

        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_rrect_layer_bounds() {
        let bounds_rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let rrect = RRect::from_rect_circular(bounds_rect, px(10.0));
        let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);

        assert_eq!(layer.bounds(), bounds_rect);
        assert_eq!(layer.bounds().width(), px(100.0));
        assert_eq!(layer.bounds().height(), px(50.0));
    }
}
