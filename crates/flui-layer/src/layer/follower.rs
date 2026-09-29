//! `FollowerLayer` — content positioned relative to a
//! [`LeaderLayer`](super::LeaderLayer) elsewhere in the tree.

use flui_foundation::geometry::{Offset, Size};
use flui_painting::paint::Alignment;

use crate::LayerLink;

/// Positions its subtree relative to the leader that shares its link.
///
/// Both anchors and the follower's own `size` live on the layer so
/// [`calculate_offset`] can run at composite time from the layer tree alone.
/// Anchors outside `[-1, 1]` are legal off-rectangle pivots.
///
/// The resolved position is computed by
/// [`resolve_follower_offset`](crate::resolve_follower_offset), not stored
/// here.
///
/// [`calculate_offset`]: Self::calculate_offset
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowerLayer {
    link: LayerLink,
    target_offset: Offset<f64>,
    show_when_unlinked: bool,
    leader_anchor: Alignment,
    follower_anchor: Alignment,
    size: Size<f64>,
}

impl FollowerLayer {
    /// A follower aligned top-left to top-left with its leader, shown at its
    /// `target_offset` when unlinked.
    #[inline]
    pub fn new(link: LayerLink) -> Self {
        Self {
            link,
            target_offset: Offset::ZERO,
            show_when_unlinked: true,
            leader_anchor: Alignment::TOP_LEFT,
            follower_anchor: Alignment::TOP_LEFT,
            size: Size::ZERO,
        }
    }

    /// Extra translation applied after the anchors align;
    /// `linkedOffset`); also the paint-origin-relative position when unlinked.
    #[inline]
    #[must_use]
    pub fn with_target_offset(mut self, offset: Offset<f64>) -> Self {
        self.target_offset = offset;
        self
    }

    /// Whether the subtree paints at all when no leader is registered.
    #[inline]
    #[must_use]
    pub fn with_show_when_unlinked(mut self, show: bool) -> Self {
        self.show_when_unlinked = show;
        self
    }

    /// The point on the leader's rectangle the follower anchors to.
    #[inline]
    #[must_use]
    pub fn with_leader_anchor(mut self, anchor: Alignment) -> Self {
        self.leader_anchor = anchor;
        self
    }

    /// The point on the follower's own rectangle placed at the leader anchor.
    #[inline]
    #[must_use]
    pub fn with_follower_anchor(mut self, anchor: Alignment) -> Self {
        self.follower_anchor = anchor;
        self
    }

    /// The follower's extent, which its own anchor aligns within.
    #[inline]
    #[must_use]
    pub fn with_size(mut self, size: Size<f64>) -> Self {
        self.size = size;
        self
    }

    /// The link this follower targets.
    #[inline]
    pub fn link(&self) -> LayerLink {
        self.link
    }

    /// See [`Self::with_target_offset`].
    #[inline]
    pub fn target_offset(&self) -> Offset<f64> {
        self.target_offset
    }

    /// See [`Self::with_show_when_unlinked`].
    #[inline]
    pub fn show_when_unlinked(&self) -> bool {
        self.show_when_unlinked
    }

    /// See [`Self::with_leader_anchor`].
    #[inline]
    pub fn leader_anchor(&self) -> Alignment {
        self.leader_anchor
    }

    /// See [`Self::with_follower_anchor`].
    #[inline]
    pub fn follower_anchor(&self) -> Alignment {
        self.follower_anchor
    }

    /// See [`Self::with_size`].
    #[inline]
    pub fn size(&self) -> Size<f64> {
        self.size
    }

    /// The offset to apply at this follower's tree position, given the
    /// leader's origin relative to that position and the leader's size.
    ///
    /// `leader_anchor` within `leader_size`, plus `target_offset`, minus
    /// `follower_anchor` within this layer's own `size`.
    pub fn calculate_offset(
        &self,
        leader_offset: Offset<f64>,
        leader_size: Size<f64>,
    ) -> Offset<f64> {
        leader_offset + self.leader_anchor.along_size(leader_size) + self.target_offset
            - self.follower_anchor.along_size(self.size)
    }
}
