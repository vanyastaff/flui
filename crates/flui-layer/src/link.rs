//! Leader/follower links and the composite-time resolution of a follower's
//! position.
//!
//! A [`LayerLink`] is the identity a [`LeaderLayer`] publishes and a
//! [`FollowerLayer`] targets. The tree indexes leaders by link as they are
//! pushed ([`LayerTree::leader`]); [`resolve_follower_transform`] turns that
//! index plus the two ancestor chains into the transform the renderer applies at
//! the follower's tree position.
//!
//! [`LeaderLayer`]: crate::LeaderLayer
//! [`FollowerLayer`]: crate::FollowerLayer

use std::sync::atomic::{AtomicU64, Ordering};

use flui_foundation::LayerId;
use flui_foundation::geometry::{Matrix4, Point};

use crate::tree::annotations::clip_admits;
use crate::{Layer, LayerTree, LeaderClip};

/// The identity shared by one leader and any number of followers.
///
/// Minted from a process-wide counter: a link is a `Copy` token a widget in
/// one UI runtime may hand to a widget in another, so a UI runtime-scoped counter would
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

/// How many followers may stand on one another's chains before resolution
/// gives up: a menu in a menu in a menu is a handful, and a cycle (two
/// followers each on the other's leader chain) would never end.
const MAX_FOLLOWER_NESTING: usize = 16;

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
///   [`MAX_FOLLOWER_NESTING`] followers stand on the chains (a debug build
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

fn resolve_at_depth(
    tree: &LayerTree,
    follower_layer_id: LayerId,
    depth: usize,
) -> Result<Matrix4, Unresolved> {
    if depth > MAX_FOLLOWER_NESTING {
        return Err(Unresolved::TooDeep);
    }
    let follower = tree
        .get_layer(follower_layer_id)
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
    let Some(leader) = tree.get_layer(leader_id).and_then(Layer::as_leader) else {
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
        let layer = tree.get_layer(id).ok_or(Unresolved::Hidden)?;
        if hide_when_clipped && id != leader_id {
            let (x, y) = leader_chain
                .project_plane_point(anchor.dx, anchor.dy)
                .ok_or(Unresolved::Hidden)?;
            if !clip_admits(layer, Point::new(x, y)) {
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
            let layer = tree.get_layer(id).ok_or(Unresolved::Hidden)?;
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

/// The inverse of `matrix`, kept in the 2D affine shape when `matrix` has
/// it.
///
/// The general 4x4 inverse divides by the determinant everywhere, so the
/// inverse of a scale of 7 holds `49 * (1/49)` where the affine shape needs
/// an exact `1` and `0`, and a consumer that admits only affine transforms
/// (the GPU painter) would refuse an ordinary scaled chain. Products of
/// affine matrices keep the shape exactly, so only the inverse needs this.
fn invert(matrix: &Matrix4) -> Option<Matrix4> {
    let coefficients = &matrix.m;
    let affine = [2, 3, 6, 7, 8, 9, 11, 14]
        .iter()
        .all(|&index| coefficients[index] == 0.0)
        && coefficients[10] == 1.0
        && coefficients[15] == 1.0;
    if !affine {
        return matrix.try_inverse();
    }
    // Column-major: x' = xx·x + yx·y + tx, y' = xy·x + yy·y + ty.
    let [xx, xy, yx, yy, tx, ty] = [0, 1, 4, 5, 12, 13].map(|index| coefficients[index]);
    let determinant = xx * yy - xy * yx;
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let [inv_xx, inv_xy, inv_yx, inv_yy] = [yy, -xy, -yx, xx].map(|value| value / determinant);
    let mut inverse = Matrix4::IDENTITY;
    inverse.m[0] = inv_xx;
    inverse.m[1] = inv_xy;
    inverse.m[4] = inv_yx;
    inverse.m[5] = inv_yy;
    inverse.m[12] = -(inv_xx * tx + inv_yx * ty);
    inverse.m[13] = -(inv_xy * tx + inv_yy * ty);
    finite(inverse).ok()
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
