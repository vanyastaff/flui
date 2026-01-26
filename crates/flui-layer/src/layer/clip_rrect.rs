//! ClipRRectLayer - Rounded rectangle clipping layer
//!
//! This layer clips its children to a rounded rectangle region.
//! Corresponds to Flutter's `ClipRRectLayer`.

use flui_types::geometry::{Pixels, RRect, Rect};
use flui_types::painting::Clip;

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
/// use flui_layer::ClipRRectLayer;
/// use flui_types::geometry::{RRect, Rect};
/// use flui_types::painting::Clip;
///
/// // Create rounded rectangle with 10px corner radius
/// let rrect = RRect::from_rect_circular(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     10.0,
/// );
/// let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);
///
/// assert_eq!(layer.clip_rrect().width(), 100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRRectLayer {
    /// The rounded rectangle to clip to
    clip_rrect: RRect,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipRRectLayer {
    /// Creates a new clip rounded rect layer.
    ///
    /// # Arguments
    ///
    /// * `clip_rrect` - The rounded rectangle to clip to
    /// * `clip_behavior` - How to apply the clip
    #[inline]
    pub fn new(clip_rrect: RRect, clip_behavior: Clip) -> Self {
        Self {
            clip_rrect,
            clip_behavior,
        }
    }

    /// Creates a clip layer with uniform circular corner radius.
    ///
    /// # Arguments
    ///
    /// * `rect` - The base rectangle
    /// * `radius` - Corner radius for all corners
    /// * `clip_behavior` - How to apply the clip
    #[inline]
    pub fn circular(rect: Rect<Pixels>, radius: f32, clip_behavior: Clip) -> Self {
        use flui_types::geometry::px;
        Self::new(RRect::from_rect_circular(rect, px(radius)), clip_behavior)
    }

    /// Creates a clip layer with anti-aliased rounded corners.
    ///
    /// This is the most common use case for rounded clipping.
    #[inline]
    pub fn anti_alias(clip_rrect: RRect) -> Self {
        Self::new(clip_rrect, Clip::AntiAlias)
    }

    /// Creates a clip layer with hard edge clipping.
    ///
    /// Note: Hard edge clipping on rounded corners may show
    /// visible aliasing artifacts. Consider using `anti_alias` instead.
    #[inline]
    pub fn hard_edge(clip_rrect: RRect) -> Self {
        Self::new(clip_rrect, Clip::HardEdge)
    }

    /// Returns a reference to the clipping rounded rectangle.
    #[inline]
    pub fn clip_rrect(&self) -> &RRect {
        &self.clip_rrect
    }

    /// Sets the clipping rounded rectangle.
    #[inline]
    pub fn set_clip_rrect(&mut self, clip_rrect: RRect) {
        self.clip_rrect = clip_rrect;
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

    /// Returns the bounding rectangle of this layer.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_rrect.bounding_rect()
    }

    /// Returns true if this layer performs actual clipping.
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }

    /// Returns true if this layer uses anti-aliased clipping.
    #[inline]
    pub fn is_anti_aliased(&self) -> bool {
        self.clip_behavior.is_anti_aliased()
    }

    /// Returns true if the rounded rectangle has any corner rounding.
    ///
    /// If false, this layer could be optimized to a simple rect clip.
    #[inline]
    pub fn has_rounding(&self) -> bool {
        self.clip_rrect.has_rounding()
    }

    /// Returns true if all corners have uniform circular radius.
    #[inline]
    pub fn is_uniform(&self) -> bool {
        self.clip_rrect.is_uniform()
    }
}

// Thread safety
unsafe impl Send for ClipRRectLayer {}
unsafe impl Sync for ClipRRectLayer {}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

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
    fn test_clip_rrect_layer_circular() {
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = ClipRRectLayer::circular(rect, 8.0, Clip::AntiAlias);

        assert_eq!(layer.bounds(), rect);
        assert!(layer.has_rounding());
        assert!(layer.is_uniform());
    }

    #[test]
    fn test_clip_rrect_layer_anti_alias() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            px(5.0),
        );
        let layer = ClipRRectLayer::anti_alias(rrect);

        assert!(layer.is_anti_aliased());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_rrect_layer_hard_edge() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            px(5.0),
        );
        let layer = ClipRRectLayer::hard_edge(rrect);

        assert!(!layer.is_anti_aliased());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_rrect_layer_no_rounding() {
        let rrect = RRect::from_rect(Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)));
        let layer = ClipRRectLayer::new(rrect, Clip::HardEdge);

        assert!(!layer.has_rounding());
    }

    #[test]
    fn test_clip_rrect_layer_bounds() {
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let rrect = RRect::from_rect_circular(rect, px(10.0));
        let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);

        assert_eq!(layer.bounds(), rect);
        assert_eq!(layer.bounds().width(), px(100.0));
        assert_eq!(layer.bounds().height(), px(50.0));
    }

    #[test]
    fn test_clip_rrect_layer_setters() {
        let rrect1 = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            px(5.0),
        );
        let rrect2 = RRect::from_rect_circular(
            Rect::from_xywh(px(10.0), px(10.0), px(100.0), px(100.0)),
            px(15.0),
        );
        let mut layer = ClipRRectLayer::new(rrect1, Clip::HardEdge);

        layer.set_clip_rrect(rrect2);
        assert_eq!(layer.clip_rrect(), &rrect2);

        layer.set_clip_behavior(Clip::AntiAlias);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rrect_layer_clone() {
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(0.0), px(0.0), px(50.0), px(50.0)),
            px(5.0),
        );
        let layer = ClipRRectLayer::new(rrect, Clip::AntiAlias);
        let cloned = layer.clone();

        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_clip_rrect_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ClipRRectLayer>();
        assert_sync::<ClipRRectLayer>();
    }
}
