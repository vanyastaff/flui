//! ClipRectLayer - Rectangular clipping layer
//!
//! This layer clips its children to a rectangular region.
//! Corresponds to Flutter's `ClipRectLayer`.

use flui_types::geometry::{Pixels, Rect};
use flui_types::painting::Clip;

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
/// use flui_layer::ClipRectLayer;
/// use flui_types::geometry::Rect;
/// use flui_types::painting::Clip;
///
/// let layer = ClipRectLayer::new(
///     Rect::from_xywh(10.0, 10.0, 100.0, 100.0),
///     Clip::HardEdge,
/// );
///
/// assert_eq!(layer.clip_rect().width(), 100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRectLayer {
    /// The clipping rectangle
    clip_rect: Rect<Pixels>,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipRectLayer {
    /// Creates a new clip rect layer.
    ///
    /// # Arguments
    ///
    /// * `clip_rect` - The rectangle to clip to
    /// * `clip_behavior` - How to apply the clip (HardEdge, AntiAlias, etc.)
    #[inline]
    pub fn new(clip_rect: Rect<Pixels>, clip_behavior: Clip) -> Self {
        Self {
            clip_rect,
            clip_behavior,
        }
    }

    /// Creates a clip rect layer with hard edge clipping.
    ///
    /// Hard edge clipping is faster but may show aliasing on edges.
    #[inline]
    pub fn hard_edge(clip_rect: Rect<Pixels>) -> Self {
        Self::new(clip_rect, Clip::HardEdge)
    }

    /// Creates a clip rect layer with anti-aliased clipping.
    ///
    /// Anti-aliased clipping is smoother but more expensive.
    #[inline]
    pub fn anti_alias(clip_rect: Rect<Pixels>) -> Self {
        Self::new(clip_rect, Clip::AntiAlias)
    }

    /// Returns the clipping rectangle.
    #[inline]
    pub fn clip_rect(&self) -> Rect<Pixels> {
        self.clip_rect
    }

    /// Sets the clipping rectangle.
    #[inline]
    pub fn set_clip_rect(&mut self, clip_rect: Rect<Pixels>) {
        self.clip_rect = clip_rect;
    }

    /// Returns the clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    #[inline]
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        self.clip_behavior = clip_behavior;
    }

    /// Returns the bounds of this layer (same as clip_rect).
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_rect
    }

    /// Returns true if this layer performs actual clipping.
    ///
    /// Returns false if clip behavior is `Clip::None`.
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }

    /// Returns true if this layer uses anti-aliased clipping.
    #[inline]
    pub fn is_anti_aliased(&self) -> bool {
        self.clip_behavior.is_anti_aliased()
    }
}

// Thread safety
unsafe impl Send for ClipRectLayer {}
unsafe impl Sync for ClipRectLayer {}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

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
        assert!(!layer.is_anti_aliased());
    }

    #[test]
    fn test_clip_rect_layer_anti_alias() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let layer = ClipRectLayer::anti_alias(rect);

        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
        assert!(layer.clips());
        assert!(layer.is_anti_aliased());
    }

    #[test]
    fn test_clip_rect_layer_no_clip() {
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let layer = ClipRectLayer::new(rect, Clip::None);

        assert!(!layer.clips());
    }

    #[test]
    fn test_clip_rect_layer_setters() {
        let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0));
        let rect2 = Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(100.0));
        let mut layer = ClipRectLayer::new(rect1, Clip::HardEdge);

        layer.set_clip_rect(rect2);
        assert_eq!(layer.clip_rect(), rect2);

        layer.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rect_layer_clone() {
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = ClipRectLayer::new(rect, Clip::AntiAlias);
        let cloned = layer.clone();

        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_clip_rect_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ClipRectLayer>();
        assert_sync::<ClipRectLayer>();
    }
}
