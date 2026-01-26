//! ClipSuperellipseLayer - Superellipse (squircle) clipping layer
//!
//! This layer clips its children to a superellipse shape, providing
//! the smooth corner transitions used in iOS/SwiftUI design.
//! Corresponds to Flutter's `ClipRSuperellipseLayer`.

use flui_types::geometry::{Pixels, RSuperellipse, Rect};
use flui_types::painting::Clip;

/// Layer that clips children to a superellipse (squircle) shape.
///
/// Superellipse clipping provides smoother corner transitions than
/// standard rounded rectangles, matching iOS/SwiftUI's `.continuous`
/// corner style.
///
/// # Architecture
///
/// ```text
/// ClipSuperellipseLayer
///   │
///   │ Apply superellipse clip (GPU stencil or shader)
///   ▼
/// Children rendered within clipped bounds
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::ClipSuperellipseLayer;
/// use flui_types::geometry::{RSuperellipse, Rect, Radius};
/// use flui_types::painting::Clip;
///
/// // Create superellipse with 20px corner radius
/// let squircle = RSuperellipse::from_rect_circular(
///     Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
///     20.0,
/// );
/// let layer = ClipSuperellipseLayer::new(squircle, Clip::AntiAlias);
///
/// assert_eq!(layer.clip_superellipse().width(), 100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipSuperellipseLayer {
    /// The superellipse to clip to
    clip_superellipse: RSuperellipse,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,

    /// Whether this layer needs to be re-added to scene
    needs_add_to_scene: bool,
}

impl ClipSuperellipseLayer {
    /// Creates a new clip superellipse layer.
    ///
    /// # Arguments
    ///
    /// * `clip_superellipse` - The superellipse to clip to
    /// * `clip_behavior` - How to apply the clip (must not be `Clip::None`)
    #[inline]
    pub fn new(clip_superellipse: RSuperellipse, clip_behavior: Clip) -> Self {
        debug_assert!(
            clip_behavior != Clip::None,
            "ClipSuperellipseLayer requires Clip::HardEdge, AntiAlias, or AntiAliasWithSaveLayer"
        );
        Self {
            clip_superellipse,
            clip_behavior,
            needs_add_to_scene: true,
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
        Self::new(
            RSuperellipse::from_rect_circular(rect, px(radius)),
            clip_behavior,
        )
    }

    /// Creates a clip layer with anti-aliased superellipse clipping.
    ///
    /// This is the most common use case for iOS-style rounded corners.
    #[inline]
    pub fn anti_alias(clip_superellipse: RSuperellipse) -> Self {
        Self::new(clip_superellipse, Clip::AntiAlias)
    }

    /// Creates a clip layer with hard edge clipping.
    ///
    /// Note: Hard edge clipping on curved shapes may show
    /// visible aliasing artifacts. Consider using `anti_alias` instead.
    #[inline]
    pub fn hard_edge(clip_superellipse: RSuperellipse) -> Self {
        Self::new(clip_superellipse, Clip::HardEdge)
    }

    /// Returns a reference to the clipping superellipse.
    #[inline]
    pub fn clip_superellipse(&self) -> &RSuperellipse {
        &self.clip_superellipse
    }

    /// Sets the clipping superellipse.
    ///
    /// This marks the layer as needing to be re-added to the scene.
    #[inline]
    pub fn set_clip_superellipse(&mut self, clip_superellipse: RSuperellipse) {
        if self.clip_superellipse != clip_superellipse {
            self.clip_superellipse = clip_superellipse;
            self.needs_add_to_scene = true;
        }
    }

    /// Returns the clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    ///
    /// This marks the layer as needing to be re-added to the scene.
    ///
    /// # Panics
    ///
    /// Debug-panics if `clip_behavior` is `Clip::None`.
    #[inline]
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) {
        debug_assert!(clip_behavior != Clip::None);
        if self.clip_behavior != clip_behavior {
            self.clip_behavior = clip_behavior;
            self.needs_add_to_scene = true;
        }
    }

    /// Returns the bounding rectangle of this layer.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.clip_superellipse.outer_rect()
    }

    /// Returns the clip bounds (same as bounds for this layer type).
    #[inline]
    pub fn describe_clip_bounds(&self) -> Rect<Pixels> {
        self.clip_superellipse.outer_rect()
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

    /// Returns true if the superellipse has any corner rounding.
    ///
    /// If false, this layer could be optimized to a simple rect clip.
    #[inline]
    pub fn has_rounding(&self) -> bool {
        !self.clip_superellipse.is_rect()
    }

    /// Returns true if all corners have the same radius.
    #[inline]
    pub fn has_uniform_corners(&self) -> bool {
        self.clip_superellipse.has_uniform_corners()
    }

    /// Returns true if this layer needs to be re-added to the scene.
    #[inline]
    pub fn needs_add_to_scene(&self) -> bool {
        self.needs_add_to_scene
    }

    /// Marks this layer as needing to be re-added to the scene.
    #[inline]
    pub fn mark_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = true;
    }

    /// Clears the needs_add_to_scene flag.
    #[inline]
    pub fn clear_needs_add_to_scene(&mut self) {
        self.needs_add_to_scene = false;
    }
}

// Thread safety
unsafe impl Send for ClipSuperellipseLayer {}
unsafe impl Sync for ClipSuperellipseLayer {}

impl Default for ClipSuperellipseLayer {
    fn default() -> Self {
        Self::new(RSuperellipse::ZERO, Clip::AntiAlias)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::Radius;

    #[test]
    fn test_clip_superellipse_layer_new() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let layer = ClipSuperellipseLayer::new(squircle, Clip::AntiAlias);

        assert_eq!(layer.clip_superellipse().width(), 100.0);
        assert_eq!(layer.clip_superellipse().height(), 100.0);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
        assert!(layer.needs_add_to_scene());
    }

    #[test]
    fn test_clip_superellipse_layer_circular() {
        let rect = Rect::from_xywh(10.0, 20.0, 80.0, 60.0);
        let layer = ClipSuperellipseLayer::circular(rect, 15.0, Clip::HardEdge);

        assert_eq!(layer.bounds(), rect);
        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
        assert!(layer.has_uniform_corners());
    }

    #[test]
    fn test_clip_superellipse_layer_anti_alias() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), 10.0);
        let layer = ClipSuperellipseLayer::anti_alias(squircle);

        assert!(layer.is_anti_aliased());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_superellipse_layer_hard_edge() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), 10.0);
        let layer = ClipSuperellipseLayer::hard_edge(squircle);

        assert!(!layer.is_anti_aliased());
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_superellipse_layer_setters() {
        let squircle1 =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let squircle2 =
            RSuperellipse::from_rect_circular(Rect::from_xywh(10.0, 10.0, 80.0, 80.0), 15.0);

        let mut layer = ClipSuperellipseLayer::new(squircle1, Clip::AntiAlias);
        layer.clear_needs_add_to_scene();
        assert!(!layer.needs_add_to_scene());

        layer.set_clip_superellipse(squircle2);
        assert!(layer.needs_add_to_scene());
        assert_eq!(layer.bounds().width(), 80.0);

        layer.clear_needs_add_to_scene();
        layer.set_clip_behavior(Clip::HardEdge);
        assert!(layer.needs_add_to_scene());
        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_superellipse_layer_bounds() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(10.0, 20.0, 100.0, 50.0), 15.0);
        let layer = ClipSuperellipseLayer::new(squircle, Clip::AntiAlias);

        let bounds = layer.bounds();
        assert_eq!(bounds.left(), 10.0);
        assert_eq!(bounds.top(), 20.0);
        assert_eq!(bounds.width(), 100.0);
        assert_eq!(bounds.height(), 50.0);

        assert_eq!(layer.describe_clip_bounds(), bounds);
    }

    #[test]
    fn test_clip_superellipse_layer_has_rounding() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let rounded = ClipSuperellipseLayer::circular(rect, 20.0, Clip::AntiAlias);
        assert!(rounded.has_rounding());

        let sharp = ClipSuperellipseLayer::new(
            RSuperellipse::from_rect_and_radius(rect, Radius::ZERO),
            Clip::AntiAlias,
        );
        assert!(!sharp.has_rounding());
    }

    #[test]
    fn test_clip_superellipse_layer_different_corners() {
        let squircle = RSuperellipse::from_rect_and_corners(
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            Radius::circular(10.0),
            Radius::circular(20.0),
            Radius::circular(15.0),
            Radius::circular(5.0),
        );
        let layer = ClipSuperellipseLayer::new(squircle, Clip::AntiAlias);

        assert!(!layer.has_uniform_corners());
        assert!(layer.has_rounding());
    }

    #[test]
    fn test_clip_superellipse_layer_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ClipSuperellipseLayer>();
    }

    #[test]
    fn test_clip_superellipse_layer_clone() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let layer = ClipSuperellipseLayer::new(squircle, Clip::AntiAlias);
        let cloned = layer.clone();

        assert_eq!(layer.clip_superellipse(), cloned.clip_superellipse());
        assert_eq!(layer.clip_behavior(), cloned.clip_behavior());
    }
}
