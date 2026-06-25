//! `FollowerLayer` — positions content relative to a `LeaderLayer`.
//!
//! Used for tooltips, dropdowns, and connected overlays. Multiple followers
//! can target the same leader; each picks an anchor on the leader plus an
//! anchor on itself, and the layer's runtime offset is derived from the two
//! anchors at composite time.
//!
//! Anchors use the workspace-canonical [`Alignment`] coordinate system —
//! `flui_types::painting::Alignment` re-exports the canonical
//! `flui_types::layout::Alignment` — `(-1, -1)` = top-left, `(0, 0)` =
//! center, `(+1, +1)` = bottom-right. The pre-cycle representation used
//! `Offset<Pixels>` in a 0..1 visual-fraction range; the U7 migration unifies
//! anchor expression with the rest of the painting API and matches Flutter
//! [`painting/alignment.dart`](https://api.flutter.dev/flutter/painting/Alignment-class.html).

use flui_types::{
    geometry::{Offset, Pixels, Size},
    painting::Alignment,
};

use super::leader::LayerLink;

/// Layer that positions content relative to a [`LeaderLayer`](super::leader::LeaderLayer).
///
/// A `FollowerLayer` links to a leader and positions its content relative to
/// the leader's coordinate space. Multiple followers can link to the same
/// leader.
///
/// # Anchors
///
/// `leader_anchor` picks a point on the leader; `follower_anchor` picks a
/// point on the follower. The follower's runtime origin is computed so the
/// two anchor points coincide (plus an optional [`target_offset`] in pixels
/// to introduce a gap).
///
/// Anchors are [`Alignment`] values. The 9-point canonical grid (`TOP_LEFT`,
/// `BOTTOM_CENTER`, etc.) covers the common cases; arbitrary `Alignment::new(x, y)`
/// supports fine-grained anchoring and off-rectangle anchors.
///
/// # Visibility
///
/// The follower can be hidden when the leader is not visible, preventing
/// orphaned overlays — see [`show_when_unlinked`](Self::show_when_unlinked).
///
/// # Example
///
/// ```rust
/// use flui_layer::{FollowerLayer, LayerLink};
///
/// let link = LayerLink::new();
///
/// // Tooltip below the leader with a 5 px gap.
/// let tooltip = FollowerLayer::below(link, 5.0);
/// ```
///
/// [`target_offset`]: Self::target_offset
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowerLayer {
    /// Link to the leader.
    link: LayerLink,

    /// Pixel offset added on top of the anchor-derived position (gap).
    target_offset: Offset<Pixels>,

    /// Whether to show when the leader is not in the tree.
    show_when_unlinked: bool,

    /// Alignment point on the leader rectangle.
    leader_anchor: Alignment,

    /// Alignment point on the follower rectangle.
    follower_anchor: Alignment,
}

impl FollowerLayer {
    /// Creates a new follower layer linked to a leader.
    ///
    /// Defaults: top-left anchors on both leader and follower, zero target
    /// offset, `show_when_unlinked = true`.
    #[inline]
    pub fn new(link: LayerLink) -> Self {
        Self {
            link,
            target_offset: Offset::ZERO,
            show_when_unlinked: true,
            leader_anchor: Alignment::TOP_LEFT,
            follower_anchor: Alignment::TOP_LEFT,
        }
    }

    /// Sets the pixel-offset gap added on top of the anchor-derived position.
    #[inline]
    pub fn with_target_offset(mut self, offset: Offset<Pixels>) -> Self {
        self.target_offset = offset;
        self
    }

    /// Sets whether the follower should remain visible when the leader is not
    /// in the tree.
    #[inline]
    pub fn with_show_when_unlinked(mut self, show: bool) -> Self {
        self.show_when_unlinked = show;
        self
    }

    /// Sets the anchor point on the leader.
    #[inline]
    pub fn with_leader_anchor(mut self, anchor: Alignment) -> Self {
        self.leader_anchor = anchor;
        self
    }

    /// Sets the anchor point on the follower.
    #[inline]
    pub fn with_follower_anchor(mut self, anchor: Alignment) -> Self {
        self.follower_anchor = anchor;
        self
    }

    /// Returns the layer link.
    #[inline]
    pub fn link(&self) -> LayerLink {
        self.link
    }

    /// Returns the target offset.
    #[inline]
    pub fn target_offset(&self) -> Offset<Pixels> {
        self.target_offset
    }

    /// Returns whether to show when unlinked.
    #[inline]
    pub fn show_when_unlinked(&self) -> bool {
        self.show_when_unlinked
    }

    /// Returns the leader anchor.
    #[inline]
    pub fn leader_anchor(&self) -> Alignment {
        self.leader_anchor
    }

    /// Returns the follower anchor.
    #[inline]
    pub fn follower_anchor(&self) -> Alignment {
        self.follower_anchor
    }

    /// Sets the target offset.
    #[inline]
    pub fn set_target_offset(&mut self, offset: Offset<Pixels>) {
        self.target_offset = offset;
    }

    /// Sets whether to show when unlinked.
    #[inline]
    pub fn set_show_when_unlinked(&mut self, show: bool) {
        self.show_when_unlinked = show;
    }

    /// Sets the leader anchor.
    #[inline]
    pub fn set_leader_anchor(&mut self, anchor: Alignment) {
        self.leader_anchor = anchor;
    }

    /// Sets the follower anchor.
    #[inline]
    pub fn set_follower_anchor(&mut self, anchor: Alignment) {
        self.follower_anchor = anchor;
    }

    /// Calculates the global pixel offset where the follower should be
    /// positioned for a given leader pose.
    ///
    /// The math:
    /// 1. Map `leader_anchor` to a pixel position inside the leader's
    ///    rectangle relative to the leader's origin.
    /// 2. Map `follower_anchor` to a pixel offset *within* a follower-sized
    ///    rectangle (origin at zero).
    /// 3. Final position = `leader_offset` + leader-anchor-px +
    ///    `target_offset` − follower-anchor-px.
    pub fn calculate_offset(
        &self,
        leader_offset: Offset<Pixels>,
        leader_size: Size<Pixels>,
        follower_size: Size<Pixels>,
    ) -> Offset<Pixels> {
        // Pixel position of the anchor inside a leader-sized rect at origin.
        let leader_half_width = leader_size.width.get() * 0.5;
        let leader_half_height = leader_size.height.get() * 0.5;
        let leader_anchor_px = Offset::new(
            Pixels::new(leader_half_width + self.leader_anchor.x * leader_half_width),
            Pixels::new(leader_half_height + self.leader_anchor.y * leader_half_height),
        );

        // Pixel position of the anchor inside a follower-sized rect at origin.
        let follower_half_width = follower_size.width.get() * 0.5;
        let follower_half_height = follower_size.height.get() * 0.5;
        let follower_anchor_px = Offset::new(
            Pixels::new(follower_half_width + self.follower_anchor.x * follower_half_width),
            Pixels::new(follower_half_height + self.follower_anchor.y * follower_half_height),
        );

        // Final position: leader_offset + leader anchor + target offset
        //                 − follower anchor offset.
        Offset::new(
            leader_offset.dx + leader_anchor_px.dx + self.target_offset.dx - follower_anchor_px.dx,
            leader_offset.dy + leader_anchor_px.dy + self.target_offset.dy - follower_anchor_px.dy,
        )
    }
}

// Convenience constructors for common follower-on-leader anchor pairs.
//
// Each maps to a pair of `Alignment` constants — the math goes through
// `align_within` at composite time, so these are pure construction sugar.
impl FollowerLayer {
    /// Creates a follower positioned below the leader (tooltip style).
    pub fn below(link: LayerLink, gap: f32) -> Self {
        use flui_types::geometry::px;
        Self::new(link)
            .with_leader_anchor(Alignment::BOTTOM_CENTER)
            .with_follower_anchor(Alignment::TOP_CENTER)
            .with_target_offset(Offset::new(px(0.0), px(gap)))
    }

    /// Creates a follower positioned above the leader.
    pub fn above(link: LayerLink, gap: f32) -> Self {
        use flui_types::geometry::px;
        Self::new(link)
            .with_leader_anchor(Alignment::TOP_CENTER)
            .with_follower_anchor(Alignment::BOTTOM_CENTER)
            .with_target_offset(Offset::new(px(0.0), px(-gap)))
    }

    /// Creates a follower positioned to the right of the leader.
    pub fn right_of(link: LayerLink, gap: f32) -> Self {
        use flui_types::geometry::px;
        Self::new(link)
            .with_leader_anchor(Alignment::CENTER_RIGHT)
            .with_follower_anchor(Alignment::CENTER_LEFT)
            .with_target_offset(Offset::new(px(gap), px(0.0)))
    }

    /// Creates a follower positioned to the left of the leader.
    pub fn left_of(link: LayerLink, gap: f32) -> Self {
        use flui_types::geometry::px;
        Self::new(link)
            .with_leader_anchor(Alignment::CENTER_LEFT)
            .with_follower_anchor(Alignment::CENTER_RIGHT)
            .with_target_offset(Offset::new(px(-gap), px(0.0)))
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn follower_layer_new() {
        let link = LayerLink::new();
        let layer = FollowerLayer::new(link);

        assert_eq!(layer.link(), link);
        assert_eq!(layer.target_offset(), Offset::ZERO);
        assert!(layer.show_when_unlinked());
        assert_eq!(layer.leader_anchor(), Alignment::TOP_LEFT);
        assert_eq!(layer.follower_anchor(), Alignment::TOP_LEFT);
    }

    #[test]
    fn follower_layer_builder() {
        let link = LayerLink::new();
        let layer = FollowerLayer::new(link)
            .with_target_offset(Offset::new(px(10.0), px(20.0)))
            .with_show_when_unlinked(false)
            .with_leader_anchor(Alignment::BOTTOM_CENTER)
            .with_follower_anchor(Alignment::TOP_CENTER);

        assert_eq!(layer.target_offset(), Offset::new(px(10.0), px(20.0)));
        assert!(!layer.show_when_unlinked());
        assert_eq!(layer.leader_anchor(), Alignment::BOTTOM_CENTER);
        assert_eq!(layer.follower_anchor(), Alignment::TOP_CENTER);
    }

    #[test]
    fn follower_calculate_offset_default_top_left_anchors() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link).with_target_offset(Offset::new(px(0.0), px(10.0)));

        let offset = follower.calculate_offset(
            Offset::new(px(100.0), px(100.0)),
            Size::new(px(50.0), px(30.0)),
            Size::new(px(80.0), px(40.0)),
        );

        // Default TOP_LEFT/TOP_LEFT anchors: follower's top-left aligns with
        // leader's top-left, then add target offset.
        assert_eq!(offset, Offset::new(px(100.0), px(110.0)));
    }

    #[test]
    fn follower_calculate_offset_below_anchor_pair() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link)
            .with_leader_anchor(Alignment::BOTTOM_CENTER)
            .with_follower_anchor(Alignment::TOP_CENTER)
            .with_target_offset(Offset::new(px(0.0), px(5.0)));

        let offset = follower.calculate_offset(
            Offset::new(px(100.0), px(100.0)),
            Size::new(px(50.0), px(30.0)),
            Size::new(px(80.0), px(40.0)),
        );

        // Leader bottom-center anchor in leader-local px: (25, 30).
        // Plus leader_offset: (125, 130).
        // Plus target_offset: (125, 135).
        // Minus follower top-center anchor (40, 0): (85, 135).
        assert_eq!(offset.dx, px(85.0));
        assert_eq!(offset.dy, px(135.0));
    }

    #[test]
    fn follower_below_constructor() {
        let link = LayerLink::new();
        let follower = FollowerLayer::below(link, 5.0);

        assert_eq!(follower.leader_anchor(), Alignment::BOTTOM_CENTER);
        assert_eq!(follower.follower_anchor(), Alignment::TOP_CENTER);
        assert_eq!(follower.target_offset(), Offset::new(px(0.0), px(5.0)));
    }

    #[test]
    fn follower_above_constructor() {
        let link = LayerLink::new();
        let follower = FollowerLayer::above(link, 5.0);

        assert_eq!(follower.leader_anchor(), Alignment::TOP_CENTER);
        assert_eq!(follower.follower_anchor(), Alignment::BOTTOM_CENTER);
        assert_eq!(follower.target_offset(), Offset::new(px(0.0), px(-5.0)));
    }

    #[test]
    fn follower_right_of_constructor() {
        let link = LayerLink::new();
        let follower = FollowerLayer::right_of(link, 10.0);

        assert_eq!(follower.leader_anchor(), Alignment::CENTER_RIGHT);
        assert_eq!(follower.follower_anchor(), Alignment::CENTER_LEFT);
        assert_eq!(follower.target_offset(), Offset::new(px(10.0), px(0.0)));
    }

    #[test]
    fn follower_left_of_constructor() {
        let link = LayerLink::new();
        let follower = FollowerLayer::left_of(link, 10.0);

        assert_eq!(follower.leader_anchor(), Alignment::CENTER_LEFT);
        assert_eq!(follower.follower_anchor(), Alignment::CENTER_RIGHT);
        assert_eq!(follower.target_offset(), Offset::new(px(-10.0), px(0.0)));
    }

    #[test]
    fn follower_setters_use_alignment() {
        let link = LayerLink::new();
        let mut layer = FollowerLayer::new(link);

        layer.set_target_offset(Offset::new(px(5.0), px(10.0)));
        layer.set_show_when_unlinked(false);
        layer.set_leader_anchor(Alignment::BOTTOM_RIGHT);
        layer.set_follower_anchor(Alignment::CENTER);

        assert_eq!(layer.target_offset(), Offset::new(px(5.0), px(10.0)));
        assert!(!layer.show_when_unlinked());
        assert_eq!(layer.leader_anchor(), Alignment::BOTTOM_RIGHT);
        assert_eq!(layer.follower_anchor(), Alignment::CENTER);
    }

    #[test]
    fn follower_off_rectangle_anchors_legal() {
        // FollowerLayer must accept anchors outside [-1, 1] for off-rectangle
        // pivots (e.g. an arrow that sticks out past the leader's edge).
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link)
            .with_leader_anchor(Alignment::new(2.0, 0.0))
            .with_follower_anchor(Alignment::CENTER);
        assert!((follower.leader_anchor().x - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn follower_send_sync() {
        const fn assert_send<T: Send>() {}
        const fn assert_sync<T: Sync>() {}

        assert_send::<FollowerLayer>();
        assert_sync::<FollowerLayer>();
    }
}
