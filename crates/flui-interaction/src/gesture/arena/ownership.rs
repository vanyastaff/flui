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

use flui_foundation::geometry::{Axis, Matrix4, Offset};

use crate::gesture::constraints::DragAxis;

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
/// axis. A region enclosing the axial member too is not between them and does
/// not count: a carousel inside a map's `None` region still takes its axis
/// from the map's pan. Admit a subtree's recognizers through
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

/// How a member sees displacement: the linear part of the transform from the
/// root's space to the member's own, so an axial member measures a rotated or
/// scaled route along the axis it drags on, not along the root's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MotionFrame {
    /// Row-major 2 × 2: `local = [a b; c d] · root`.
    linear: [f64; 4],
}

impl MotionFrame {
    /// The root's own space.
    pub(crate) const IDENTITY: Self = Self {
        linear: [1.0, 0.0, 0.0, 1.0],
    };

    /// The frame of a Down's target: the transform its events were mapped
    /// by, differentiated at the root-space contact so a perspective route
    /// reports the motion it has there, not at the origin.
    pub(crate) fn of_down(dispatch: crate::pointer::PointerDispatch<'_>) -> Self {
        use crate::events::PointerEventExt as _;
        let (Some(transform), Some(at)) = (dispatch.root_to_local(), dispatch.global.position())
        else {
            return Self::IDENTITY;
        };
        Self::from_root_to_local(transform, at)
    }

    /// The frame of a target whose events the root-to-local `transform` maps,
    /// at the root-space point `at`.
    fn from_root_to_local(transform: &Matrix4, at: Offset<f64>) -> Self {
        let (ox, oy) = transform.transform_point(at.dx, at.dy);
        let (x_x, x_y) = transform.transform_point(at.dx + 1.0, at.dy);
        let (y_x, y_y) = transform.transform_point(at.dx, at.dy + 1.0);
        let linear = [x_x - ox, y_x - ox, x_y - oy, y_y - oy];
        if linear.iter().all(|value| value.is_finite()) {
            Self { linear }
        } else {
            Self::IDENTITY
        }
    }

    /// `root` displacement in the member's own space.
    ///
    /// The direction is the member's own; the length stays the root's, so a
    /// scaled route compares like a rotated one: slop and ownership are both
    /// in root logical pixels.
    pub(crate) fn localize(self, root: Offset<f64>) -> Offset<f64> {
        let [a, b, c, d] = self.linear;
        let local = Offset::new(a * root.dx + b * root.dy, c * root.dx + d * root.dy);
        let length = local.dx.hypot(local.dy);
        if length > 0.0 && length.is_finite() {
            let scale = root.dx.hypot(root.dy) / length;
            Offset::new(local.dx * scale, local.dy * scale)
        } else {
            local
        }
    }
}

impl Default for MotionFrame {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// One moving member competing for a contact, in route order (innermost
/// first).
pub(super) struct MotionCandidate<'a> {
    pub(super) role: MotionRole,
    pub(super) frame: MotionFrame,
    pub(super) pan_claim: Option<&'a Rc<PanClaimNode>>,
}

/// Whether the claims between a free member and an axial member admit `axis`.
///
/// The intersection runs from the free member's region outwards and stops at
/// the first region that also encloses the axial member: a region enclosing
/// both declares nothing between them, whether the axial member sits in it
/// directly or in a region of its own nested inside it.
fn admits(free: Option<&Rc<PanClaimNode>>, axial: Option<&Rc<PanClaimNode>>, axis: Axis) -> bool {
    let encloses_axial =
        |node: &Rc<PanClaimNode>| chain(axial).any(|outer| Rc::ptr_eq(outer, node));
    chain(free)
        .take_while(|node| !encloses_axial(node))
        .all(|node| node.claim.admits(axis))
}

/// `innermost` and every region enclosing it, outwards.
fn chain(innermost: Option<&Rc<PanClaimNode>>) -> impl Iterator<Item = &Rc<PanClaimNode>> {
    std::iter::successors(innermost, |node| node.parent.as_ref())
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
    // strictly larger component displaces an earlier member. Each axial
    // member measures the motion in its own space, along its own axis.
    let mut best: Option<(usize, f64, f64)> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let MotionRole::Axial(axis) = candidate.role else {
            continue;
        };
        if let Some(free) = free
            && !admits(candidates[free].pan_claim, candidate.pan_claim, axis)
        {
            continue;
        }
        let local = candidate.frame.localize(displacement);
        let projection = component(local, axis);
        if best.is_none_or(|(_, larger, _)| projection > larger) {
            best = Some((index, projection, local.distance_squared()));
        }
    }
    match (free, best) {
        // |d · axis| ≥ |d| cos 45° ⇔ 2 (d · axis)² ≥ |d|², exact at 45°, with
        // both sides in the axial member's space.
        (Some(free), Some((axial, projection, magnitude_squared))) => {
            if 2.0 * projection * projection >= magnitude_squared {
                Some(axial)
            } else {
                Some(free)
            }
        }
        (None, Some((axial, _, _))) => Some(axial),
        (Some(free), None) => Some(free),
        (None, None) => None,
    }
}
