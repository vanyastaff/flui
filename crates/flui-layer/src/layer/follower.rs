//! FollowerLayer - Linked positioning follower
//!
//! This layer positions its content relative to a LeaderLayer.
//! Used for tooltips, dropdowns, and connected overlays.

use super::leader::LayerLink;
use flui_types::geometry::{Offset, Size};

/// Layer that positions content relative to a LeaderLayer.
///
/// A FollowerLayer links to a LeaderLayer and positions its content
/// relative to the leader's coordinate space. Multiple followers
/// can link to the same leader.
///
/// # Alignment
///
/// The follower can align to various positions on the leader:
/// - Top-left, top-center, top-right
/// - Center-left, center, center-right
/// - Bottom-left, bottom-center, bottom-right
///
/// # Visibility
///
/// The follower can be hidden when the leader is not visible,
/// preventing orphaned overlays.
///
/// # Example
///
/// ```rust
/// use flui_layer::{LeaderLayer, FollowerLayer, LayerLink};
/// use flui_types::geometry::Offset;
///
/// let link = LayerLink::new();
///
/// // Create follower that appears below the leader
/// let tooltip = FollowerLayer::new(link)
///     .with_target_offset(Offset::new(0.0, 5.0))
///     .with_show_when_unlinked(false);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowerLayer {
    /// Link to the leader
    link: LayerLink,

    /// Offset from the leader's anchor point
    target_offset: Offset,

    /// Whether to show when the leader is not in the tree
    show_when_unlinked: bool,

    /// Alignment on the leader (0,0 = top-left, 1,1 = bottom-right)
    leader_anchor: Offset,

    /// Alignment on the follower (0,0 = top-left, 1,1 = bottom-right)
    follower_anchor: Offset,
}

impl FollowerLayer {
    /// Creates a new follower layer linked to a leader.
    #[inline]
    pub fn new(link: LayerLink) -> Self {
        Self {
            link,
            target_offset: Offset::ZERO,
            show_when_unlinked: true,
            leader_anchor: Offset::ZERO,
            follower_anchor: Offset::ZERO,
        }
    }

    /// Sets the offset from the leader's anchor point.
    #[inline]
    pub fn with_target_offset(mut self, offset: Offset) -> Self {
        self.target_offset = offset;
        self
    }

    /// Sets whether to show when the leader is not in the tree.
    #[inline]
    pub fn with_show_when_unlinked(mut self, show: bool) -> Self {
        self.show_when_unlinked = show;
        self
    }

    /// Sets the alignment point on the leader.
    ///
    /// Values are normalized: (0,0) = top-left, (1,1) = bottom-right.
    #[inline]
    pub fn with_leader_anchor(mut self, anchor: Offset) -> Self {
        self.leader_anchor = anchor;
        self
    }

    /// Sets the alignment point on the follower.
    ///
    /// Values are normalized: (0,0) = top-left, (1,1) = bottom-right.
    #[inline]
    pub fn with_follower_anchor(mut self, anchor: Offset) -> Self {
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
    pub fn target_offset(&self) -> Offset {
        self.target_offset
    }

    /// Returns whether to show when unlinked.
    #[inline]
    pub fn show_when_unlinked(&self) -> bool {
        self.show_when_unlinked
    }

    /// Returns the leader anchor.
    #[inline]
    pub fn leader_anchor(&self) -> Offset {
        self.leader_anchor
    }

    /// Returns the follower anchor.
    #[inline]
    pub fn follower_anchor(&self) -> Offset {
        self.follower_anchor
    }

    /// Sets the target offset.
    #[inline]
    pub fn set_target_offset(&mut self, offset: Offset) {
        self.target_offset = offset;
    }

    /// Sets whether to show when unlinked.
    #[inline]
    pub fn set_show_when_unlinked(&mut self, show: bool) {
        self.show_when_unlinked = show;
    }

    /// Sets the leader anchor.
    #[inline]
    pub fn set_leader_anchor(&mut self, anchor: Offset) {
        self.leader_anchor = anchor;
    }

    /// Sets the follower anchor.
    #[inline]
    pub fn set_follower_anchor(&mut self, anchor: Offset) {
        self.follower_anchor = anchor;
    }

    /// Calculates the offset to position the follower relative to the leader.
    ///
    /// # Arguments
    ///
    /// * `leader_offset` - The leader's global offset
    /// * `leader_size` - The leader's size
    /// * `follower_size` - The follower's size
    ///
    /// # Returns
    ///
    /// The global offset where the follower should be positioned.
    pub fn calculate_offset(
        &self,
        leader_offset: Offset,
        leader_size: Size,
        follower_size: Size,
    ) -> Offset {
        // Calculate the anchor point on the leader
        let leader_anchor_point = Offset::new(
            leader_offset.dx + leader_size.width * self.leader_anchor.dx,
            leader_offset.dy + leader_size.height * self.leader_anchor.dy,
        );

        // Calculate the anchor point on the follower (relative to follower origin)
        let follower_anchor_point = Offset::new(
            follower_size.width * self.follower_anchor.dx,
            follower_size.height * self.follower_anchor.dy,
        );

        // Final position: leader anchor + target offset - follower anchor offset
        Offset::new(
            leader_anchor_point.dx + self.target_offset.dx - follower_anchor_point.dx,
            leader_anchor_point.dy + self.target_offset.dy - follower_anchor_point.dy,
        )
    }
}

// Convenience constructors for common alignments
impl FollowerLayer {
    /// Creates a follower positioned below the leader (tooltip style).
    pub fn below(link: LayerLink, gap: f32) -> Self {
        Self::new(link)
            .with_leader_anchor(Offset::new(0.5, 1.0)) // Bottom-center of leader
            .with_follower_anchor(Offset::new(0.5, 0.0)) // Top-center of follower
            .with_target_offset(Offset::new(0.0, gap))
    }

    /// Creates a follower positioned above the leader.
    pub fn above(link: LayerLink, gap: f32) -> Self {
        Self::new(link)
            .with_leader_anchor(Offset::new(0.5, 0.0)) // Top-center of leader
            .with_follower_anchor(Offset::new(0.5, 1.0)) // Bottom-center of follower
            .with_target_offset(Offset::new(0.0, -gap))
    }

    /// Creates a follower positioned to the right of the leader.
    pub fn right_of(link: LayerLink, gap: f32) -> Self {
        Self::new(link)
            .with_leader_anchor(Offset::new(1.0, 0.5)) // Right-center of leader
            .with_follower_anchor(Offset::new(0.0, 0.5)) // Left-center of follower
            .with_target_offset(Offset::new(gap, 0.0))
    }

    /// Creates a follower positioned to the left of the leader.
    pub fn left_of(link: LayerLink, gap: f32) -> Self {
        Self::new(link)
            .with_leader_anchor(Offset::new(0.0, 0.5)) // Left-center of leader
            .with_follower_anchor(Offset::new(1.0, 0.5)) // Right-center of follower
            .with_target_offset(Offset::new(-gap, 0.0))
    }
}

// Thread safety
unsafe impl Send for FollowerLayer {}
unsafe impl Sync for FollowerLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_follower_layer_new() {
        let link = LayerLink::new();
        let layer = FollowerLayer::new(link);

        assert_eq!(layer.link(), link);
        assert_eq!(layer.target_offset(), Offset::ZERO);
        assert!(layer.show_when_unlinked());
    }

    #[test]
    fn test_follower_layer_builder() {
        let link = LayerLink::new();
        let layer = FollowerLayer::new(link)
            .with_target_offset(Offset::new(10.0, 20.0))
            .with_show_when_unlinked(false)
            .with_leader_anchor(Offset::new(0.5, 1.0))
            .with_follower_anchor(Offset::new(0.5, 0.0));

        assert_eq!(layer.target_offset(), Offset::new(10.0, 20.0));
        assert!(!layer.show_when_unlinked());
        assert_eq!(layer.leader_anchor(), Offset::new(0.5, 1.0));
        assert_eq!(layer.follower_anchor(), Offset::new(0.5, 0.0));
    }

    #[test]
    fn test_follower_calculate_offset_simple() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link).with_target_offset(Offset::new(0.0, 10.0));

        let offset = follower.calculate_offset(
            Offset::new(100.0, 100.0), // Leader at (100, 100)
            Size::new(50.0, 30.0),     // Leader size
            Size::new(80.0, 40.0),     // Follower size
        );

        // Default anchors are (0,0), so follower goes to leader's top-left + target offset
        assert_eq!(offset, Offset::new(100.0, 110.0));
    }

    #[test]
    fn test_follower_calculate_offset_centered() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link)
            .with_leader_anchor(Offset::new(0.5, 1.0)) // Bottom-center of leader
            .with_follower_anchor(Offset::new(0.5, 0.0)) // Top-center of follower
            .with_target_offset(Offset::new(0.0, 5.0)); // 5px gap

        let offset = follower.calculate_offset(
            Offset::new(100.0, 100.0), // Leader at (100, 100)
            Size::new(50.0, 30.0),     // Leader 50x30
            Size::new(80.0, 40.0),     // Follower 80x40
        );

        // Leader bottom-center: (100 + 25, 100 + 30) = (125, 130)
        // Add target offset: (125, 135)
        // Subtract follower anchor offset: (125 - 40, 135 - 0) = (85, 135)
        assert_eq!(offset.dx, 85.0);
        assert_eq!(offset.dy, 135.0);
    }

    #[test]
    fn test_follower_below() {
        let link = LayerLink::new();
        let follower = FollowerLayer::below(link, 5.0);

        assert_eq!(follower.leader_anchor(), Offset::new(0.5, 1.0));
        assert_eq!(follower.follower_anchor(), Offset::new(0.5, 0.0));
        assert_eq!(follower.target_offset(), Offset::new(0.0, 5.0));
    }

    #[test]
    fn test_follower_above() {
        let link = LayerLink::new();
        let follower = FollowerLayer::above(link, 5.0);

        assert_eq!(follower.leader_anchor(), Offset::new(0.5, 0.0));
        assert_eq!(follower.follower_anchor(), Offset::new(0.5, 1.0));
        assert_eq!(follower.target_offset(), Offset::new(0.0, -5.0));
    }

    #[test]
    fn test_follower_right_of() {
        let link = LayerLink::new();
        let follower = FollowerLayer::right_of(link, 10.0);

        assert_eq!(follower.leader_anchor(), Offset::new(1.0, 0.5));
        assert_eq!(follower.follower_anchor(), Offset::new(0.0, 0.5));
        assert_eq!(follower.target_offset(), Offset::new(10.0, 0.0));
    }

    #[test]
    fn test_follower_left_of() {
        let link = LayerLink::new();
        let follower = FollowerLayer::left_of(link, 10.0);

        assert_eq!(follower.leader_anchor(), Offset::new(0.0, 0.5));
        assert_eq!(follower.follower_anchor(), Offset::new(1.0, 0.5));
        assert_eq!(follower.target_offset(), Offset::new(-10.0, 0.0));
    }

    #[test]
    fn test_follower_setters() {
        let link = LayerLink::new();
        let mut layer = FollowerLayer::new(link);

        layer.set_target_offset(Offset::new(5.0, 10.0));
        layer.set_show_when_unlinked(false);
        layer.set_leader_anchor(Offset::new(1.0, 1.0));
        layer.set_follower_anchor(Offset::new(0.0, 0.0));

        assert_eq!(layer.target_offset(), Offset::new(5.0, 10.0));
        assert!(!layer.show_when_unlinked());
        assert_eq!(layer.leader_anchor(), Offset::new(1.0, 1.0));
        assert_eq!(layer.follower_anchor(), Offset::ZERO);
    }

    #[test]
    fn test_follower_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<FollowerLayer>();
        assert_sync::<FollowerLayer>();
    }
}
