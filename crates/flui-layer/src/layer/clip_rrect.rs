//! `ClipRRectLayer` — clips its subtree to a rounded rectangle.

use flui_foundation::geometry::{RRect, Rect};
use flui_painting::paint::Clip;

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
/// use flui_foundation::geometry::{RRect, Rect};
/// use flui_painting::paint::Clip;
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
    pub fn bounds(&self) -> Rect<f64> {
        self.clip_rrect.bounding_rect()
    }

    /// Whether the layer clips at all (`clip_behavior != Clip::None`).
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }
}
