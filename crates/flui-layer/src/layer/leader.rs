//! `LeaderLayer` — the anchor a [`FollowerLayer`](super::FollowerLayer)
//! positions itself against (tooltips, dropdowns, connected overlays).

use flui_foundation::geometry::{Offset, Rect, Size};

use crate::LayerLink;

/// Publishes a [`LayerLink`] at a position in the layer tree.
///
/// `offset` is the layer's own translation — the position it gives its
/// children and the point a follower resolves to — and `size` is what a
/// follower's anchor aligns within. The
/// tree indexes every leader by link as it is pushed
/// ([`LayerTree::leader`](crate::LayerTree::leader)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaderLayer {
    link: LayerLink,
    size: Size<f64>,
    offset: Offset<f64>,
}

impl LeaderLayer {
    /// A leader at the paint origin.
    #[inline]
    pub fn new(link: LayerLink, size: Size<f64>) -> Self {
        Self::with_offset(link, size, Offset::ZERO)
    }

    /// A leader translated by `offset` within its parent.
    #[inline]
    pub fn with_offset(link: LayerLink, size: Size<f64>, offset: Offset<f64>) -> Self {
        Self { link, size, offset }
    }

    /// The link followers target.
    #[inline]
    pub fn link(&self) -> LayerLink {
        self.link
    }

    /// The extent a follower's leader anchor aligns within.
    #[inline]
    pub fn size(&self) -> Size<f64> {
        self.size
    }

    /// The translation this leader applies to its children.
    #[inline]
    pub fn offset(&self) -> Offset<f64> {
        self.offset
    }

    /// The leader's rectangle in its parent's coordinates.
    #[inline]
    pub fn bounds(&self) -> Rect<f64> {
        Rect::from_xywh(
            self.offset.dx,
            self.offset.dy,
            self.size.width,
            self.size.height,
        )
    }
}
