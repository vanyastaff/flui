//! Arena composition: a tree of relations between recognizer branches
//! (ADR-0208 §3).
//!
//! Every node is one relation between two branches. A branch handle composes
//! again into a child node, which holds its parent strongly; the arena entry of
//! each admitted member records the innermost branch it joined through, so the
//! relation between two members is found on their common ancestors.

use std::{rc::Rc, time::Duration};

use super::GestureArena;
use crate::{
    gesture::observer::ObservedComposition,
    gesture::settings::{GestureSettings, SLOT_KINDS},
    ids::PointerId,
};

/// How the two branches of a composition relate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GestureComposition {
    /// Both branches compete for the contact's single owner.
    Exclusive,
    /// The second branch may win only after every first-branch member fails.
    RequireFailure,
}

/// What a composition node is, as observers and inspectors report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CompositionKind {
    /// [`GestureComposition::Exclusive`].
    Exclusive,
    /// [`GestureComposition::RequireFailure`].
    RequireFailure,
    /// A hold that hands its won contact to a successor
    /// ([`GestureArena::sequenced`]).
    Sequenced,
}

impl From<GestureComposition> for CompositionKind {
    fn from(composition: GestureComposition) -> Self {
        match composition {
            GestureComposition::Exclusive => Self::Exclusive,
            GestureComposition::RequireFailure => Self::RequireFailure,
        }
    }
}

impl CompositionKind {
    /// The longest a contact can wait on this node before its verdict, in
    /// `settings`' authored values (before accessibility floors, ADR-0212 §3).
    ///
    /// A `RequireFailure` fallback waits for its preferred branch to fail; the
    /// framework budgets that wait as the double-tap interval, the longest hold
    /// a built-in preferred branch keeps. A `Sequenced` hold waits for its
    /// long-press timeout. An `Exclusive` node adds no wait of its own.
    fn resolution_delay(self, settings: &GestureSettings) -> Duration {
        match self {
            Self::Exclusive => Duration::ZERO,
            Self::RequireFailure => settings.double_tap_timeout(),
            Self::Sequenced => settings.authored_long_press_timeout(),
        }
    }
}

/// Ordered handles for the two branches of a composition.
///
/// Build preferred recognizers with the first handle and fallback recognizers
/// with the second for [`GestureComposition::RequireFailure`].
#[must_use]
#[derive(Debug)]
pub struct GestureBranches {
    first: GestureArena,
    second: GestureArena,
}

impl GestureBranches {
    /// Consume the pair into its ordered recognizer-construction handles.
    #[must_use]
    pub fn into_branches(self) -> (GestureArena, GestureArena) {
        (self.first, self.second)
    }
}

/// A hold and its successor, composed as one arena member (ADR-0208 §3).
///
/// Build the hold recognizer (a long press) with [`Self::hold`] and the
/// successor (a drag) with [`Self::successor`]. The successor never wins the
/// arena itself: when the hold wins a contact, its successors stay admitted,
/// and [`Self::hand_off`] gives them the contact. When every hold member of the
/// contact fails, its successors fail with it.
///
/// Only this value carries the hand-off authority; keep it beside the
/// recognizers that were built from its handles.
#[must_use]
pub struct SequencedBranch {
    hold: GestureArena,
    successor: GestureArena,
    node: Rc<CompositionNode>,
}

impl SequencedBranch {
    /// The handle the hold recognizer is built with.
    pub fn hold(&self) -> GestureArena {
        self.hold.clone()
    }

    /// The handle the successor recognizer is built with.
    pub fn successor(&self) -> GestureArena {
        self.successor.clone()
    }

    /// Give `pointer`'s contact, already won by the hold, to its successor.
    ///
    /// The earliest-admitted live successor that asked for the contact wins,
    /// or else the earliest admitted; the others are rejected. Returns whether
    /// a successor received the contact: `false` when the hold has not won
    /// `pointer`, the contact already ended, or it was already handed off.
    pub fn hand_off(&self, pointer: PointerId) -> bool {
        self.hold.hand_off_carried(&self.node, pointer)
    }
}

impl std::fmt::Debug for SequencedBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SequencedBranch").finish_non_exhaustive()
    }
}

/// Why a composition was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CompositionError {
    /// The summed waits on one root-to-leaf path of the composition tree
    /// exceed the preset's `max_resolution_delay` for some pointer kind.
    #[error("composition delays sum to {sum:?}, above the {ceiling:?} resolution ceiling")]
    DelayCeilingExceeded {
        /// The summed waits on the offending path.
        sum: Duration,
        /// The ceiling they exceed.
        ceiling: Duration,
    },
    /// The arena's presentation has closed.
    #[error("the arena's presentation has closed")]
    Closed,
}

/// One relation in the composition tree.
pub(crate) struct CompositionNode {
    pub(crate) kind: CompositionKind,
    parent: Option<BranchLink>,
}

impl CompositionNode {
    pub(super) fn parent(&self) -> Option<&BranchLink> {
        self.parent.as_ref()
    }
}

/// The branch of one node an arena handle admits members into.
#[derive(Clone)]
pub(super) struct BranchLink {
    pub(super) node: Rc<CompositionNode>,
    pub(super) position: CompositionBranch,
}

/// Which branch of a composition a member joined through.
///
/// For [`GestureComposition::RequireFailure`] the first branch is the
/// preferred one and the second the fallback; for
/// [`GestureArena::sequenced`] the first is the hold and the second its
/// successor; [`GestureComposition::Exclusive`] ranks neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum CompositionBranch {
    /// The first handle of the pair.
    First,
    /// The second handle of the pair.
    Second,
}

impl BranchLink {
    /// This branch and every enclosing one, innermost first.
    pub(super) fn path(&self) -> impl Iterator<Item = &BranchLink> {
        std::iter::successors(Some(self), |link| link.node.parent())
    }

    /// The relation under which a member on `self` waits for `other`: both
    /// sit under one node, `self` in its second branch and `other` in its
    /// first.
    pub(super) fn waits_for(&self, other: &Self) -> Option<CompositionKind> {
        self.path()
            .filter(|link| link.position == CompositionBranch::Second)
            .find_map(|link| {
                other
                    .path()
                    .any(|theirs| {
                        theirs.position == CompositionBranch::First
                            && Rc::ptr_eq(&theirs.node, &link.node)
                    })
                    .then_some(link.node.kind)
            })
    }

    /// The innermost `Sequenced` node this branch is a successor of.
    pub(super) fn sequenced_successor_of(&self) -> Option<&Rc<CompositionNode>> {
        self.path()
            .find(|link| {
                link.position == CompositionBranch::Second
                    && link.node.kind == CompositionKind::Sequenced
            })
            .map(|link| &link.node)
    }

    /// Whether this branch sits under `node`'s first branch.
    pub(super) fn is_hold_of(&self, node: &Rc<CompositionNode>) -> bool {
        self.path()
            .any(|link| link.position == CompositionBranch::First && Rc::ptr_eq(&link.node, node))
    }
}

impl GestureArena {
    /// Compose two branches of this arena handle (ADR-0208 §3).
    ///
    /// A branch composes again into a nested node. Membership inherits the
    /// branch on every admission, including recognizers that create a new
    /// arena member for each contact. The handles share the clock, owner
    /// lifetime and exact-generation close/sweep lifecycle. A member can join
    /// at most one branch of a contact's competition. Unadmitted or destroyed
    /// first-branch members count as failure after membership closes. Sweep
    /// retains a still-blocked second branch instead of granting it the
    /// gesture. Cancellation abandons both branches.
    ///
    /// ```rust
    /// use flui_interaction::{GestureArena, DoubleTapGestureRecognizer, TapGestureRecognizer};
    /// use flui_interaction::gesture::GestureComposition;
    /// let arena = GestureArena::new();
    /// let (preferred, fallback) = arena
    ///     .compose(GestureComposition::RequireFailure)?
    ///     .into_branches();
    /// let double_tap = DoubleTapGestureRecognizer::builder(preferred).build();
    /// let tap = TapGestureRecognizer::builder(fallback).build();
    /// # Ok::<(), flui_interaction::gesture::CompositionError>(())
    /// ```
    ///
    /// # Errors
    /// [`CompositionError::DelayCeilingExceeded`] when the new node's path
    /// waits longer than the handle's settings allow for any pointer kind;
    /// [`CompositionError::Closed`] after the presentation closed.
    pub fn compose(
        &self,
        composition: GestureComposition,
    ) -> Result<GestureBranches, CompositionError> {
        let (first, second, _) = self.branch_pair(composition.into())?;
        Ok(GestureBranches { first, second })
    }

    /// Compose a hold and its successor as one arena member (ADR-0208 §3).
    ///
    /// # Errors
    /// As [`Self::compose`].
    pub fn sequenced(&self) -> Result<SequencedBranch, CompositionError> {
        let (hold, successor, node) = self.branch_pair(CompositionKind::Sequenced)?;
        Ok(SequencedBranch {
            hold,
            successor,
            node,
        })
    }

    /// The relations this handle admits members under, innermost first; empty
    /// for an uncomposed handle. Observers report a member's composition with it.
    pub fn composition_path(&self) -> impl Iterator<Item = CompositionKind> + '_ {
        self.branch
            .iter()
            .flat_map(BranchLink::path)
            .map(|link| link.node.kind)
    }

    /// [`Self::composition_path`] as observers report it, with each node's
    /// identity and the branch taken through it. Empty, and unallocated, for
    /// an uncomposed handle.
    pub(super) fn observed_composition(&self) -> Vec<ObservedComposition> {
        self.branch
            .iter()
            .flat_map(BranchLink::path)
            .map(|link| ObservedComposition::new(&link.node, link.position))
            .collect()
    }

    fn branch_pair(
        &self,
        kind: CompositionKind,
    ) -> Result<(Self, Self, Rc<CompositionNode>), CompositionError> {
        if self.shared.owner_closed.get() {
            return Err(CompositionError::Closed);
        }
        self.check_delay_ceiling(kind)?;
        let node = Rc::new(CompositionNode {
            kind,
            parent: self.branch.clone(),
        });
        let mut first = self.clone();
        first.branch = Some(BranchLink {
            node: Rc::clone(&node),
            position: CompositionBranch::First,
        });
        let mut second = self.clone();
        second.branch = Some(BranchLink {
            node: Rc::clone(&node),
            position: CompositionBranch::Second,
        });
        Ok((first, second, node))
    }

    /// Refuse a node whose root-to-leaf path would wait past the ceiling of
    /// any pointer kind's profile. Authored and preset values are summed, never
    /// floored ones (ADR-0212 §3).
    fn check_delay_ceiling(&self, kind: CompositionKind) -> Result<(), CompositionError> {
        for pointer_kind in SLOT_KINDS {
            let settings = self.settings.snapshot_for(pointer_kind);
            let ceiling = settings.max_resolution_delay();
            let sum = self
                .composition_path()
                .chain(std::iter::once(kind))
                .try_fold(Duration::ZERO, |sum, node| {
                    sum.checked_add(node.resolution_delay(&settings))
                })
                .unwrap_or(Duration::MAX);
            if sum > ceiling {
                return Err(CompositionError::DelayCeilingExceeded { sum, ceiling });
            }
        }
        Ok(())
    }
}
