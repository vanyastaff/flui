//! `ClipRectLayer` — clips its subtree to a rectangle.

use flui_foundation::geometry::Rect;
use flui_painting::paint::Clip;

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
/// use flui_foundation::geometry::Rect;
/// use flui_painting::paint::Clip;
///
/// let layer = ClipRectLayer::new(Rect::from_xywh(10.0, 10.0, 100.0, 100.0), Clip::HardEdge);
///
/// assert_eq!(layer.clip_rect().width(), 100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRectLayer {
    /// The clipping rectangle
    clip_rect: Rect<f64>,

    /// Clip behavior (HardEdge, AntiAlias, etc.)
    clip_behavior: Clip,
}

impl ClipRectLayer {
    /// Clips the subtree to a rectangle; `Clip::None` is accepted and lowered as no clip.
    #[inline]
    pub fn new(clip_rect: Rect<f64>, clip_behavior: Clip) -> Self {
        Self {
            clip_rect,
            clip_behavior,
        }
    }

    /// Aliased clip: cheapest, jagged on curves and diagonals.
    #[inline]
    pub fn hard_edge(clip_rect: Rect<f64>) -> Self {
        Self::new(clip_rect, Clip::HardEdge)
    }

    /// Anti-aliased clip edge.
    #[inline]
    pub fn anti_alias(clip_rect: Rect<f64>) -> Self {
        Self::new(clip_rect, Clip::AntiAlias)
    }

    /// The clip shape.
    #[inline]
    pub fn clip_rect(&self) -> Rect<f64> {
        self.clip_rect
    }

    /// How the edge is rasterised; `Clip::None` means no clip.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The clip shape's bounding box.
    #[inline]
    pub fn bounds(&self) -> Rect<f64> {
        self.clip_rect
    }

    /// Whether the layer clips at all (`clip_behavior != Clip::None`).
    #[inline]
    pub fn clips(&self) -> bool {
        self.clip_behavior.clips()
    }
}
