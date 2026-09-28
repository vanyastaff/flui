//! `ClipSuperellipseLayer` — clips its subtree to an iOS-style squircle.

use flui_foundation::geometry::{RSuperellipse, Rect};
use flui_painting::paint::Clip;

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
/// use flui_foundation::geometry::{RSuperellipse, Radius, Rect};
/// use flui_painting::paint::Clip;
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
}

impl ClipSuperellipseLayer {
    /// Clips the subtree to a superellipse; `Clip::None` is accepted and lowered as no clip.
    #[inline]
    pub fn new(clip_superellipse: RSuperellipse, clip_behavior: Clip) -> Self {
        Self {
            clip_superellipse,
            clip_behavior,
        }
    }

    /// Anti-aliased clip edge.
    #[inline]
    pub fn anti_alias(clip_superellipse: RSuperellipse) -> Self {
        Self::new(clip_superellipse, Clip::AntiAlias)
    }

    /// Aliased clip: cheapest, jagged on curves and diagonals.
    #[inline]
    pub fn hard_edge(clip_superellipse: RSuperellipse) -> Self {
        Self::new(clip_superellipse, Clip::HardEdge)
    }

    /// The clip shape.
    #[inline]
    pub fn clip_superellipse(&self) -> &RSuperellipse {
        &self.clip_superellipse
    }

    /// How the edge is rasterised; `Clip::None` means no clip.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The clip shape's bounding box.
    #[inline]
    pub fn bounds(&self) -> Rect<f64> {
        self.clip_superellipse.outer_rect()
    }

    /// Whether the layer clips at all (`clip_behavior != Clip::None`).
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_clip_superellipse_layer_new() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), 20.0);
        let layer = ClipSuperellipseLayer::new(squircle, Clip::AntiAlias);

        assert_eq!(layer.clip_superellipse().width(), 100.0);
        assert_eq!(layer.clip_superellipse().height(), 100.0);
        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn test_clip_superellipse_layer_anti_alias() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), 10.0);
        let layer = ClipSuperellipseLayer::anti_alias(squircle);

        assert_eq!(layer.clip_behavior(), Clip::AntiAlias);
        assert!(layer.clips());
    }

    #[test]
    fn test_clip_superellipse_layer_hard_edge() {
        let squircle =
            RSuperellipse::from_rect_circular(Rect::from_xywh(0.0, 0.0, 50.0, 50.0), 10.0);
        let layer = ClipSuperellipseLayer::hard_edge(squircle);

        assert_eq!(layer.clip_behavior(), Clip::HardEdge);
        assert!(layer.clips());
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

        assert_eq!(layer.bounds(), bounds);
    }
}
