//! Leader/follower links and the composite-time resolution of a follower's
//! position.
//!
//! A [`LayerLink`] is the identity a [`LeaderLayer`] publishes and a
//! [`FollowerLayer`] targets. The tree indexes leaders by link as they are
//! pushed ([`LayerTree::leader`]); [`resolve_follower_offset`] turns that
//! index plus the two ancestor chains into the offset the renderer applies at
//! the follower's tree position.
//!
//! [`LeaderLayer`]: crate::LeaderLayer
//! [`FollowerLayer`]: crate::FollowerLayer

use std::sync::atomic::{AtomicU64, Ordering};

use flui_foundation::LayerId;
use flui_tree::TreeNav;
use flui_types::geometry::{Offset, Pixels};

use crate::LayerTree;

/// The identity shared by one leader and any number of followers.
///
/// Minted from a process-wide counter: a link is a `Copy` token a widget in
/// one realm may hand to a widget in another, so a realm-scoped counter would
/// invite collisions where a global one cannot. `Ordering::Relaxed` is enough
/// because uniqueness is the only requirement — the read-modify-write is
/// atomic under every ordering, and no other memory is published through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerLink {
    id: u64,
}

impl LayerLink {
    /// Mints a link no other call has returned.
    #[expect(
        clippy::new_without_default,
        reason = "a Default that minted a fresh unique link per call would read as the zero value"
    )]
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// The raw identity, for diagnostics.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }
}

/// Resolves the offset the renderer applies at `follower_layer_id`'s tree
/// position so the follower's content lands on its leader.
///
/// Walk the leader's and the follower's ancestor chains to their lowest
/// common ancestor, accumulating each layer's own translation, and take the
/// difference (the oracle is `FollowerLayer._establishTransform` in the
/// reference `layer.dart`). FLUI's follower is offset-only, so a chain sums
/// [`Layer::local_translation`] — a scale or rotation between the two is a
/// documented limitation, and a follower that sits on another follower's
/// chain contributes zero (its own offset is resolved by the walk, not stored
/// on the layer).
///
/// Returns `None` when nothing is to be painted at `follower_layer_id`: the
/// id is not a `Follower`, or the follower is unlinked with
/// `show_when_unlinked == false`. Unlinked with `show_when_unlinked == true`
/// yields the follower's `target_offset` as a plain paint-origin-relative
/// position. A leader nested *inside* the follower's subtree violates the
/// leader-before-follower paint-order precondition; it takes the unlinked
/// path with a warning rather than fabricating a position.
///
/// The follower is read off the tree rather than passed in, so the id and
/// the layer cannot disagree.
///
/// [`Layer::local_translation`]: crate::Layer::local_translation
pub fn resolve_follower_offset(
    tree: &LayerTree,
    follower_layer_id: LayerId,
) -> Option<Offset<Pixels>> {
    let follower = tree.get_layer(follower_layer_id)?.as_follower()?;
    let unlinked_fallback = || {
        follower
            .show_when_unlinked()
            .then_some(follower.target_offset())
    };

    let Some(leader_id) = tree.leader(follower.link()) else {
        return unlinked_fallback();
    };
    let Some(Some(leader)) = tree.get_layer(leader_id).map(|layer| layer.as_leader()) else {
        return unlinked_fallback();
    };

    let Some(common_ancestor) = tree.lowest_common_ancestor(leader_id, follower_layer_id) else {
        // Unreachable for two ids of one tree (it has a single root); kept as
        // the contract for a foreign id rather than a panic.
        return unlinked_fallback();
    };
    if common_ancestor == follower_layer_id {
        tracing::warn!(
            ?leader_id,
            ?follower_layer_id,
            "resolve_follower_offset: leader is inside the follower's subtree \
             (leader must precede its follower in paint order); using the unlinked contract",
        );
        return unlinked_fallback();
    }

    // Both chains include the common ancestor's own translation, which cancels;
    // the leader's chain starts at the leader itself so its own offset — the
    // position it gives a hypothetical child — is on the forward side, and on the inverse side exactly when the
    // leader is an ancestor of the follower.
    let leader_from_ancestor = translation_through(tree, leader_id, common_ancestor);
    let follower_from_ancestor = tree
        .parent(follower_layer_id)
        .map_or(Offset::ZERO, |parent| {
            translation_through(tree, parent, common_ancestor)
        });
    let leader_offset_from_follower = leader_from_ancestor - follower_from_ancestor;

    Some(follower.calculate_offset(leader_offset_from_follower, leader.size()))
}

/// Sums [`Layer::local_translation`] from `start` up to and including
/// `ancestor`.
///
/// [`Layer::local_translation`]: crate::Layer::local_translation
fn translation_through(tree: &LayerTree, start: LayerId, ancestor: LayerId) -> Offset<Pixels> {
    let mut sum = Offset::ZERO;
    for id in tree.ancestors(start) {
        if let Some(layer) = tree.get_layer(id) {
            sum += layer.local_translation();
        }
        if id == ancestor {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::{Size, px};

    use super::*;
    use crate::{FollowerLayer, Layer, LeaderLayer, OffsetLayer, OpacityLayer, TransformLayer};

    fn offset(dx: f32, dy: f32) -> Layer {
        Layer::from(OffsetLayer::new(Offset::new(px(dx), px(dy))))
    }

    fn leader(link: LayerLink, dx: f32, dy: f32) -> Layer {
        Layer::from(LeaderLayer::with_offset(
            link,
            Size::new(px(20.0), px(20.0)),
            Offset::new(px(dx), px(dy)),
        ))
    }

    fn follower(link: LayerLink) -> FollowerLayer {
        FollowerLayer::new(link).with_size(Size::new(px(10.0), px(10.0)))
    }

    #[test]
    fn linked_under_a_shared_parent_resolves_to_the_leader_offset() {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let _ = tree.push_child(root, leader(link, 30.0, 40.0));
        let follower = follower(link);
        let follower_id = tree.push_child(root, Layer::from(follower));

        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::new(px(30.0), px(40.0)))
        );
    }

    /// Leader and follower under two different `Offset` branches: both chains
    /// are summed to the root, not assumed to share an immediate parent.
    #[test]
    fn linked_across_offset_branches_sums_both_chains() {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let branch_a = tree.push_child(root, offset(100.0, 0.0));
        let _ = tree.push_child(branch_a, leader(link, 5.0, 5.0));
        let branch_b = tree.push_child(root, offset(0.0, 200.0));
        let follower = follower(link);
        let follower_id = tree.push_child(branch_b, Layer::from(follower));

        // Leader absolute (105,5) minus the follower's pushed position (0,200).
        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::new(px(105.0), px(-195.0)))
        );
    }

    /// Same geometry as above with the leader's branch a translation
    /// `Transform` (what the composer pushes for `RenderTransform`); dropping
    /// its translation would give `(5, -195)`.
    #[test]
    fn linked_through_a_transform_layer_counts_its_translation() {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let branch_a = tree.push_child(root, Layer::from(TransformLayer::translation(100.0, 0.0)));
        let _ = tree.push_child(branch_a, leader(link, 5.0, 5.0));
        let branch_b = tree.push_child(root, offset(0.0, 200.0));
        let follower = follower(link);
        let follower_id = tree.push_child(branch_b, Layer::from(follower));

        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::new(px(105.0), px(-195.0)))
        );
    }

    /// Every layer kind that translates its children counts — here an
    /// `Opacity` with an offset on the follower's chain. The engine pushes that
    /// offset, so the resolver must subtract it or the follower lands
    /// double-translated.
    #[test]
    fn linked_through_an_opacity_offset_counts_it() {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let _ = tree.push_child(root, leader(link, 30.0, 40.0));
        let branch = tree.push_child(
            root,
            Layer::from(OpacityLayer::with_offset(
                0.5,
                Offset::new(px(10.0), px(20.0)),
            )),
        );
        let follower = follower(link);
        let follower_id = tree.push_child(branch, Layer::from(follower));

        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::new(px(20.0), px(20.0)))
        );
    }

    /// Oracle `_establishTransform`: a follower that is a child of
    /// its own leader has the leader's offset on both chains, so they cancel
    /// and the follower stays where the leader's push already put it.
    #[test]
    fn follower_nested_under_its_leader_resolves_to_zero() {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let leader_id = tree.push_child(root, leader(link, 100.0, 100.0));
        let follower = follower(link);
        let follower_id = tree.push_child(leader_id, Layer::from(follower));

        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::ZERO)
        );
    }

    #[test]
    fn unlinked_and_shown_uses_the_target_offset() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link)
            .with_show_when_unlinked(true)
            .with_target_offset(Offset::new(px(7.0), px(9.0)));
        let tree = LayerTree::new(Layer::from(follower));
        let follower_id = tree.root();

        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::new(px(7.0), px(9.0)))
        );
    }

    #[test]
    fn unlinked_and_hidden_returns_none() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link).with_show_when_unlinked(false);
        let tree = LayerTree::new(Layer::from(follower));
        let follower_id = tree.root();

        assert_eq!(resolve_follower_offset(&tree, follower_id), None);
    }

    /// A leader painted inside the follower's subtree cannot anchor it (it
    /// is composited after the follower); the unlinked contract applies.
    #[test]
    fn leader_inside_the_follower_subtree_falls_back_to_unlinked() {
        let link = LayerLink::new();
        let follower = FollowerLayer::new(link)
            .with_show_when_unlinked(true)
            .with_target_offset(Offset::new(px(3.0), px(4.0)));
        let mut tree = LayerTree::new(Layer::from(follower));
        let follower_id = tree.root();
        let _ = tree.push_child(follower_id, leader(link, 50.0, 50.0));

        assert_eq!(
            resolve_follower_offset(&tree, follower_id),
            Some(Offset::new(px(3.0), px(4.0)))
        );
    }
}
