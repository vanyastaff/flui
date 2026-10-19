//! The ownership table's motion rules (ADR-0208 §1, §2).
//!
//! One contact has one owner. When a moving member crosses its slop, the arena
//! decides between every competing moving member on the displacement
//! accumulated since Down, never on a single sample:
//!
//! - a channel leaf (scale, `InteractiveViewer`) does not yield to its
//!   ancestors, which are no candidates; the members inside it are inner
//!   claimants, and the leaf meets them as a free member does, so a list in a
//!   viewer owns its axis. A member whose role is dormant on the contact (a
//!   two-contact scale holding it alone) is no candidate at all;
//! - a free member (pan) yields to the axial member whose axis takes at least
//!   cos 45° of the displacement, unless the subtree's [`PanClaim`] keeps that
//!   axis from it; exactly 45° goes to the axial member;
//! - between axial members the larger component wins, and an exact tie, as two
//!   same-axis members always are, goes to the innermost member on the route.

use std::rc::Rc;

use flui_foundation::geometry::{Axis, Offset};

use crate::gesture::traits::DragAxis;

/// What an arena member does with a moving contact, for the ownership table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MotionRole {
    /// Moves along one axis: an axial drag, a `Scrollable`, an anchored drag
    /// row.
    Axial(Axis),
    /// Moves in any direction: a pan.
    Free,
    /// Takes every motion of the contact and passes what it does not consume
    /// to its ancestors through the scroll channel: scale, `InteractiveViewer`.
    ChannelLeaf,
}

impl From<DragAxis> for MotionRole {
    fn from(axis: DragAxis) -> Self {
        match axis {
            DragAxis::Horizontal => Self::Axial(Axis::Horizontal),
            DragAxis::Vertical => Self::Axial(Axis::Vertical),
            DragAxis::Free => Self::Free,
        }
    }
}

/// The axes axial members may claim from the free members of a subtree, as
/// CSS `touch-action` declares them.
///
/// Claims nest: the effective claim at a free member is the intersection, per
/// axis, of the claims on its route up to the nearest axial member on that
/// axis. Admit a subtree's recognizers through
/// [`GestureArena::with_pan_claim`](super::GestureArena::with_pan_claim).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PanClaim {
    /// Axial members on either axis may claim the contact.
    #[default]
    Auto,
    /// Only a horizontal axial member may claim the contact.
    PanX,
    /// Only a vertical axial member may claim the contact.
    PanY,
    /// The free member keeps the contact (a map, a signature pad).
    None,
}

impl PanClaim {
    const fn admits(self, axis: Axis) -> bool {
        matches!(
            (self, axis),
            (Self::Auto, _) | (Self::PanX, Axis::Horizontal) | (Self::PanY, Axis::Vertical)
        )
    }
}

/// One declared claim and the claim of the region enclosing it.
pub(super) struct PanClaimNode {
    claim: PanClaim,
    parent: Option<Rc<PanClaimNode>>,
}

impl PanClaimNode {
    pub(super) fn nested(claim: PanClaim, parent: Option<Rc<Self>>) -> Rc<Self> {
        Rc::new(Self { claim, parent })
    }
}

/// One moving member competing for a contact, in route order (innermost
/// first).
pub(super) struct MotionCandidate<'a> {
    pub(super) role: MotionRole,
    pub(super) pan_claim: Option<&'a Rc<PanClaimNode>>,
}

/// Whether the claims between a free member and an axial member admit `axis`.
///
/// The intersection runs from the free member's region outwards and stops at
/// the axial member's own region, which encloses the axial member too.
fn admits(free: Option<&Rc<PanClaimNode>>, axial: Option<&Rc<PanClaimNode>>, axis: Axis) -> bool {
    std::iter::successors(free, |node| node.parent.as_ref())
        .take_while(|node| axial.is_none_or(|stop| !Rc::ptr_eq(node, stop)))
        .all(|node| node.claim.admits(axis))
}

fn component(displacement: Offset<f64>, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => displacement.dx.abs(),
        Axis::Vertical => displacement.dy.abs(),
    }
}

/// The index of the candidate that owns a contact moved by `displacement`.
pub(super) fn motion_owner(
    candidates: &[MotionCandidate<'_>],
    displacement: Offset<f64>,
) -> Option<usize> {
    // The innermost channel leaf yields to no ancestor, so the members outside
    // it are no candidates; the members inside it are inner claimants, and
    // the leaf meets them as a free member does.
    let candidates = match candidates
        .iter()
        .position(|candidate| candidate.role == MotionRole::ChannelLeaf)
    {
        Some(leaf) => &candidates[..=leaf],
        None => candidates,
    };
    let free = candidates
        .iter()
        .position(|candidate| matches!(candidate.role, MotionRole::Free | MotionRole::ChannelLeaf));
    // The larger component wins; iteration is innermost first, so only a
    // strictly larger component displaces an earlier member.
    let mut best: Option<(usize, f64)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let MotionRole::Axial(axis) = candidate.role else {
            continue;
        };
        if let Some(free) = free
            && !admits(candidates[free].pan_claim, candidate.pan_claim, axis)
        {
            continue;
        }
        let projection = component(displacement, axis);
        if best.is_none_or(|(_, larger)| projection > larger) {
            best = Some((index, projection));
        }
    }
    match (free, best) {
        // |d · axis| ≥ |d| cos 45° ⇔ 2 (d · axis)² ≥ |d|², exact at 45°.
        (Some(free), Some((axial, projection))) => {
            if 2.0 * projection * projection >= displacement.distance_squared() {
                Some(axial)
            } else {
                Some(free)
            }
        }
        (Some(free), None) => Some(free),
        (None, Some((axial, _))) => Some(axial),
        (None, None) => None,
    }
}
