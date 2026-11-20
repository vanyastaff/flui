//! The leader/follower resolution over a hand-built [`LayerTree`]: the
//! oracle the retained scene's own resolution
//! ([`SceneVersion::follower_transform`]) is compared against, behind the
//! `testing` feature. Both keep ADR-0276's rule.
//!
//! [`SceneVersion::follower_transform`]: crate::SceneVersion::follower_transform

use flui_foundation::LayerId;
use flui_foundation::geometry::{Matrix4, Point};

use crate::link::{MAX_FOLLOWER_NESTING, invert};
use crate::{Layer, LayerLink, LayerTree, LeaderClip};

/// Why a follower resolves to nothing.
enum Unresolved {
    /// Nothing is painted there: unlinked and hidden, clipped under
    /// [`LeaderClip::Hide`], a singular or non-finite chain, or a nested
    /// follower on a chain that is itself hidden.
    Hidden,
    /// More than [`MAX_FOLLOWER_NESTING`] followers stand on the chains.
    TooDeep,
}

/// Resolves the transform the renderer applies at `follower_layer_id`'s tree
/// position so the follower's content lands on its leader.
///
/// Both chains run from the lowest common ancestor (exclusive) down to the
/// leader and to the follower's parent; each layer on them contributes its
/// matrix (`Transform`), its own resolved transform (a nested `Follower`) or
/// its [`Layer::local_translation`]. With `L` the leader's chain and `F` the
/// follower's, the result is `F⁻¹ · L · translate(linked_offset)` (the oracle
/// is `FollowerLayer._establishTransform` in the reference `layer.dart`): the
/// follower's content takes the leader's scale and rotation.
///
/// Returns `None` when nothing is to be painted or hit at
/// `follower_layer_id`:
///
/// - the id is not a `Follower`, or it is unlinked with
///   `show_when_unlinked == false` (unlinked with `true` yields a translation
///   by the follower's `target_offset` from its paint origin);
/// - a transform on either chain is singular or not finite, so no geometry
///   that is not finite is published;
/// - under [`LeaderClip::Hide`], the leader's anchor point lies outside a clip
///   layer on the leader's chain;
/// - a nested follower on a chain resolves to `None`, or more than
///   16 followers stand on the chains (a debug build
///   warns once per resolution).
///
/// A leader nested *inside* the follower's subtree violates the
/// leader-before-follower paint-order precondition; it takes the unlinked
/// path with a warning rather than fabricating a position.
///
/// The follower is read off the tree rather than passed in, so the id and
/// the layer cannot disagree.
///
/// [`Layer::local_translation`]: crate::Layer::local_translation
pub fn resolve_follower_transform(tree: &LayerTree, follower_layer_id: LayerId) -> Option<Matrix4> {
    match resolve_at_depth(tree, follower_layer_id, 0) {
        Ok(transform) => Some(transform),
        Err(Unresolved::Hidden) => None,
        Err(Unresolved::TooDeep) => {
            #[cfg(debug_assertions)]
            tracing::warn!(
                ?follower_layer_id,
                limit = MAX_FOLLOWER_NESTING,
                "resolve_follower_transform: followers nest deeper than the limit \
                 (or form a cycle through their leaders); painting nothing",
            );
            None
        }
    }
}

/// Where the leader publishing under `link` has its top-left corner, in the
/// coordinates of `tree`'s root: what moves when a scroll or a transform
/// above the leader moves it while its size stays.
///
/// Returns `None` when no leader publishes under `link`, or when a transform
/// on its chain is singular or not finite, or a follower on it resolves to
/// nothing.
pub fn leader_origin(tree: &LayerTree, link: LayerLink) -> Option<Point<f64>> {
    let leader_id = tree.leader(link)?;
    let mut chain = Matrix4::IDENTITY;
    for id in tree.ancestors(leader_id) {
        let layer = tree.layer(id)?;
        chain = chain_transform(tree, id, layer, 0).ok()? * chain;
    }
    let (x, y) = chain.project_plane_point(0.0, 0.0)?;
    (x.is_finite() && y.is_finite()).then(|| Point::new(x, y))
}

fn resolve_at_depth(
    tree: &LayerTree,
    follower_layer_id: LayerId,
    depth: usize,
) -> Result<Matrix4, Unresolved> {
    if depth > MAX_FOLLOWER_NESTING {
        return Err(Unresolved::TooDeep);
    }
    let follower = tree
        .layer(follower_layer_id)
        .and_then(Layer::as_follower)
        .ok_or(Unresolved::Hidden)?;
    let unlinked_fallback = || {
        if follower.show_when_unlinked() {
            let offset = follower.target_offset();
            finite(Matrix4::translation(offset.dx, offset.dy, 0.0))
        } else {
            Err(Unresolved::Hidden)
        }
    };

    let Some(leader_id) = tree.leader(follower.link()) else {
        return unlinked_fallback();
    };
    let Some(leader) = tree.layer(leader_id).and_then(Layer::as_leader) else {
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
            "resolve_follower_transform: leader is inside the follower's subtree \
             (leader must precede its follower in paint order); using the unlinked contract",
        );
        return unlinked_fallback();
    }

    // The leader's chain, walked up from the leader itself so its own offset
    // (the position it gives a hypothetical child) is on the forward side.
    // For the clip test, the anchor point (in the leader's own coordinates)
    // is projected through the chain accumulated so far at each clip, never
    // through points already flattened at a lower level: depth a 3D rotation
    // gives the point must survive until a later one takes it back.
    let hide_when_clipped = follower.leader_clip() == LeaderClip::Hide;
    let anchor = follower.leader_anchor().along_size(leader.size());
    let mut leader_chain = Matrix4::IDENTITY;
    for id in tree.ancestors(leader_id) {
        if id == common_ancestor {
            break;
        }
        let layer = tree.layer(id).ok_or(Unresolved::Hidden)?;
        if hide_when_clipped && id != leader_id {
            let (x, y) = leader_chain
                .project_plane_point(anchor.dx, anchor.dy)
                .ok_or(Unresolved::Hidden)?;
            if !layer.clip_admits(Point::new(x, y)) {
                return Err(Unresolved::Hidden);
            }
        }
        leader_chain = chain_transform(tree, id, layer, depth)? * leader_chain;
    }

    let mut follower_chain = Matrix4::IDENTITY;
    if let Some(parent) = tree.parent(follower_layer_id) {
        for id in tree.ancestors(parent) {
            if id == common_ancestor {
                break;
            }
            let layer = tree.layer(id).ok_or(Unresolved::Hidden)?;
            follower_chain = chain_transform(tree, id, layer, depth)? * follower_chain;
        }
    }

    let follower_from_ancestor = invert(&follower_chain).ok_or(Unresolved::Hidden)?;
    let linked = follower.linked_offset(leader.size());
    let transform =
        follower_from_ancestor * leader_chain * Matrix4::translation(linked.dx, linked.dy, 0.0);
    // A singular leader chain (a zero scale) collapses the follower: nothing
    // of it can be seen or hit, and hit-testing needs the inverse.
    invert(&transform).ok_or(Unresolved::Hidden)?;
    Ok(transform)
}

/// The transform `layer` (at `id`) applies to its children, as a chain sees
/// it: a nested follower's is resolved against its own leader.
fn chain_transform(
    tree: &LayerTree,
    id: LayerId,
    layer: &Layer,
    depth: usize,
) -> Result<Matrix4, Unresolved> {
    match layer {
        Layer::Transform(transform) => finite(*transform.transform()),
        Layer::Follower(_) => resolve_at_depth(tree, id, depth + 1),
        _ => {
            let offset = layer.local_translation();
            finite(Matrix4::translation(offset.dx, offset.dy, 0.0))
        }
    }
}

fn finite(transform: Matrix4) -> Result<Matrix4, Unresolved> {
    if transform.m.iter().all(|value| value.is_finite()) {
        Ok(transform)
    } else {
        Err(Unresolved::Hidden)
    }
}
