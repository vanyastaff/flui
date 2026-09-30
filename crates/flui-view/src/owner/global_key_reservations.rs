//! Per-frame `GlobalKey` reservations, and the end-of-frame verification
//! that turns a silent last-writer graft into a reported duplicate.
//!
//! # The problem this closes
//!
//! Resolving a `GlobalKey` is *optimistic*: when a parent declares a child
//! carrying a key that some other parent currently holds, the framework
//! grafts the existing element across rather than mounting a second one.
//! The graft assumes the holder is about to be deactivated, and the only way
//! that assumption could be false is if the global key is being duplicated.
//!
//! The graft alone therefore cannot tell a legal reparent from an illegal
//! duplicate: both look identical at the moment they happen. What separates
//! them is what the *frame* looks like once every parent has built. If only
//! one parent ends up declaring the key, the graft was a reparent. If two
//! do, the element ping-pongs and the tree is illegal — and before this
//! module existed FLUI never noticed, leaving whichever parent asked last
//! holding the child and the other silently empty.
//!
//! # The mechanism
//!
//! Two ledgers, both per-frame, both cleared by [`verify`].
//!
//! **Reservations.** Every time a parent declares a child carrying a
//! `GlobalKey` — a fresh mount, a graft, or an in-place update of a child it
//! already had — that declaration is recorded: `parent -> (child -> key)`.
//! A key reserved twice is a duplicate.
//!
//! **Displacements.** A graft can also take a keyed child out of a parent
//! that never runs at all this frame, and a parent that never runs never
//! reserves — so the reservation ledger alone is blind to the most ordinary
//! cross-parent duplicate there is. Each such robbery is recorded against
//! the parent that lost the child, and dropped again the moment that parent
//! rebuilds (which is how it consents to the loss). Whatever is left at the
//! frame boundary is a parent still describing a child it no longer has.
//!
//! A parent rebuilding clears **both** of its ledgers before the rebuild
//! re-states them, so a parent's newest build is the whole truth about what
//! it declares.
//!
//! Reservations and displacements are per-frame, so a key legally moving
//! from parent A in one frame to parent B in the next is never reported.
//!
//! Both ledgers are verified in `finalize_tree`.
//!
//! # Four deliberate properties
//!
//! 1. **It is not debug-only.** FLUI records and verifies in every profile:
//!    the cost is two small maps per frame, and a duplicate `GlobalKey`
//!    corrupts a release tree exactly as badly as a debug one.
//! 2. **It reports, it does not throw.** A duplicate key is caller-controlled
//!    input, so FLUI surfaces a typed [`DuplicateGlobalKey`] through the
//!    owner's diagnostic drain (`BuildOwner::take_global_key_diagnostics`)
//!    and a `tracing::error!`, and the frame completes — the same split the
//!    eager same-parent check already has.
//! 3. **Verification order is deterministic.** Both ledgers are held in
//!    declaration order rather than hash order, so the report is
//!    reproducible: the parent that declared the key first in the frame is
//!    always `first_parent`.
//! 4. **One parent declaring a key for two children is reported here.** There
//!    is no separate mechanism to leave it to: the eager check in
//!    `element_tree::retake_active_global_key` catches the shape in debug
//!    and is compiled out in release, where the second attachment therefore
//!    mounts a genuine second element under one key. Folding it in here is
//!    what keeps property 1 true.
//!
//! # Repair before reporting
//!
//! A duplicate leaves at most one parent actually holding the child; any
//! other parent still listing it has a dangling child edge that would make
//! teardown cascade secondary failures. [`verify`] therefore repairs first
//! — dropping the child from every parent that is not its real parent —
//! and only then records the report.

use std::collections::HashMap;

use flui_foundation::{ElementId, ViewKey};

use crate::tree::ElementTree;

/// One reported duplicate: a single `GlobalKey` declared by two different
/// parents within one frame.
///
/// Caller-controlled input, so this is a typed diagnostic rather than a
/// panic. Drain it with
/// [`BuildOwner::take_global_key_diagnostics`](super::BuildOwner::take_global_key_diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "multiple views used the same GlobalKey: {key} was declared twice in one frame — by \
     {first_parent:?} for {first_child:?}, and by {second_parent:?} for {second_child:?}. A \
     GlobalKey can only be specified on one view at a time in the view tree"
)]
pub struct DuplicateGlobalKey {
    /// `Debug` rendering of the offending key, captured at report time —
    /// the key itself is not carried because the diagnostic outlives the
    /// element that declared it.
    pub key: String,
    /// The key's hash, for correlating with tracing output.
    pub key_hash: u64,
    /// The parent that declared the key **first**, and the element it named.
    pub first_parent: ElementId,
    /// The element the first declaration named.
    pub first_child: ElementId,
    /// The parent that declared the key again.
    ///
    /// Equal to [`Self::first_parent`] when one parent declared the same key
    /// for two different children — the shape that reaches a release build
    /// after the eager same-parent check has been compiled out.
    pub second_parent: ElementId,
    /// The element the second declaration named.
    ///
    /// Equal to [`Self::first_child`] when both parents fought over one
    /// element (the graft case); different when a second element was
    /// actually mounted under the same key.
    pub second_child: ElementId,
}

/// One parent's declaration of one keyed child.
struct Reservation {
    child: ElementId,
    key: Box<dyn ViewKey>,
}

/// A keyed child taken out of a live parent by another parent's graft,
/// while that parent had made no declaration of its own this frame.
///
/// A graft is only legitimate if the parent losing the child agrees — which
/// it expresses by rebuilding without that child. A parent that never
/// rebuilds has not agreed to anything: its own configuration still
/// describes the child that was just removed from underneath it, so it ends
/// the frame inconsistent with its own build output. That is the one
/// cross-parent duplicate the reservation ledger alone cannot see, because
/// the losing parent never ran and therefore never reserved.
struct Displacement {
    child: ElementId,
    key: Box<dyn ViewKey>,
    taken_by: ElementId,
}

/// The frame's reservations, in declaration order.
///
/// Declaration order is load-bearing, not incidental: it is what makes the
/// duplicate report reproducible across runs (see this module's property
/// 3). `parents` is the ordered parent list and `by_parent` holds each
/// parent's own ordered declarations.
#[derive(Default)]
pub(crate) struct GlobalKeyReservations {
    parents: Vec<ElementId>,
    by_parent: HashMap<ElementId, Vec<Reservation>>,
    /// Parents robbed of a keyed child by someone else's graft, in the order
    /// they were robbed. Cleared for a parent the moment it rebuilds — see
    /// [`Displacement`].
    displaced_parents: Vec<ElementId>,
    displaced: HashMap<ElementId, Vec<Displacement>>,
}

impl GlobalKeyReservations {
    /// An empty reservation set.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record that `parent` declared `child` carrying `key` in this frame.
    ///
    /// Re-declaring the same `(parent, child)` pair overwrites rather than
    /// accumulating — a parent that rebuilds twice in one frame has still
    /// only made one claim on the key.
    pub(crate) fn reserve(&mut self, parent: ElementId, child: ElementId, key: &dyn ViewKey) {
        let entries = match self.by_parent.entry(parent) {
            std::collections::hash_map::Entry::Occupied(occupied) => occupied.into_mut(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                self.parents.push(parent);
                vacant.insert(Vec::new())
            }
        };
        if let Some(existing) = entries.iter_mut().find(|entry| entry.child == child) {
            existing.key = key.clone_key();
            return;
        }
        entries.push(Reservation {
            child,
            key: key.clone_key(),
        });
    }

    /// Drop `parent`'s reservation on `child`.
    ///
    /// Called when a parent gives a child up mid-frame (the reconciler
    /// replacing or removing it), so a child the parent no longer declares
    /// cannot make it look like a duplicate claimant.
    pub(crate) fn forget(&mut self, parent: ElementId, child: ElementId) {
        let Some(entries) = self.by_parent.get_mut(&parent) else {
            return;
        };
        entries.retain(|entry| entry.child != child);
        if entries.is_empty() {
            self.by_parent.remove(&parent);
            self.parents.retain(|&id| id != parent);
        }
    }

    /// Record that `taken_by` grafted `child` — which carries `key` — out
    /// of `losing_parent`.
    ///
    /// A no-op when `losing_parent` has already declared that child itself
    /// this frame: the ordinary reservation walk will report that conflict,
    /// and recording it twice would report one defect twice.
    pub(crate) fn displace(
        &mut self,
        losing_parent: ElementId,
        child: ElementId,
        key: &dyn ViewKey,
        taken_by: ElementId,
    ) {
        let already_declared = self
            .by_parent
            .get(&losing_parent)
            .is_some_and(|entries| entries.iter().any(|entry| entry.child == child));
        if already_declared {
            return;
        }
        let entries = match self.displaced.entry(losing_parent) {
            std::collections::hash_map::Entry::Occupied(occupied) => occupied.into_mut(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                self.displaced_parents.push(losing_parent);
                vacant.insert(Vec::new())
            }
        };
        if let Some(existing) = entries.iter_mut().find(|entry| entry.child == child) {
            existing.key = key.clone_key();
            existing.taken_by = taken_by;
            return;
        }
        entries.push(Displacement {
            child,
            key: key.clone_key(),
            taken_by,
        });
    }

    /// Drop everything this frame recorded *about* `parent`, because
    /// `parent` is about to rebuild and will re-state its declarations from
    /// scratch.
    ///
    /// Both halves matter. Its reservations go because a parent's newest
    /// build is the whole truth about what it declares — an earlier build in
    /// the same frame that named a keyed child it has since dropped must not
    /// linger as a competing claim. Its displacements go because rebuilding
    /// is exactly how a parent consents to having lost a child: the tree it
    /// is about to produce is the one that will be checked.
    pub(crate) fn note_parent_rebuild(&mut self, parent: ElementId) {
        if self.by_parent.remove(&parent).is_some() {
            self.parents.retain(|&id| id != parent);
        }
        if self.displaced.remove(&parent).is_some() {
            self.displaced_parents.retain(|&id| id != parent);
        }
    }

    /// Whether anything was recorded this frame.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_parent.is_empty() && self.displaced.is_empty()
    }

    /// Forget everything recorded without verifying.
    pub(crate) fn clear(&mut self) {
        self.parents.clear();
        self.by_parent.clear();
        self.displaced_parents.clear();
        self.displaced.clear();
    }
}

/// A key seen during verification, remembered by identity.
///
/// Same shape as the registry's: hash picks the bucket, [`ViewKey::key_eq`]
/// decides membership, so two colliding-but-distinct keys are never
/// conflated into a false duplicate report.
#[derive(Default)]
struct SeenKeys {
    buckets: HashMap<u64, Vec<SeenKey>>,
}

/// One key already claimed during this verification pass, and by whom for
/// which child.
struct SeenKey {
    key: Box<dyn ViewKey>,
    parent: ElementId,
    child: ElementId,
}

impl SeenKeys {
    /// The `(parent, child)` that already claimed `key`, if any.
    fn claimant(&self, key: &dyn ViewKey) -> Option<(ElementId, ElementId)> {
        self.buckets
            .get(&key.key_hash())?
            .iter()
            .find(|seen| seen.key.key_eq(key))
            .map(|seen| (seen.parent, seen.child))
    }

    fn record(&mut self, key: &dyn ViewKey, parent: ElementId, child: ElementId) {
        self.buckets
            .entry(key.key_hash())
            .or_default()
            .push(SeenKey {
                key: key.clone_key(),
                parent,
                child,
            });
    }
}

/// Verify the frame's reservations, repairing and reporting any duplicate,
/// then clear them.
///
/// Returns one [`DuplicateGlobalKey`] per conflicting *declaration* — a key
/// claimed by three parents yields two reports, each naming the first
/// claimant and the newcomer, so no conflict is collapsed away.
///
/// Two populations are skipped:
///
/// - a parent that is no longer in the tree — it was unmounted later in the
///   frame, so its declaration cannot conflict with anything live;
/// - a child that is no longer in the tree, or that ends the frame with no
///   parent — it was deactivated and never re-attached, so the reservation
///   describes a claim nobody kept.
///
/// One condition covers the first because of where this runs: `finalize_tree` sweeps the inactive
/// queue immediately before calling it, so a parent that was deactivated
/// this frame and not re-taken is already out of the tree by the time the
/// walk starts, and one that *was* re-taken is active again.
pub(crate) fn verify(
    reservations: &mut GlobalKeyReservations,
    tree: &mut ElementTree,
) -> Vec<DuplicateGlobalKey> {
    let mut reports = Vec::new();
    let mut seen = SeenKeys::default();

    for parent in std::mem::take(&mut reservations.parents) {
        let Some(entries) = reservations.by_parent.remove(&parent) else {
            continue;
        };
        if !tree.contains(parent) {
            continue;
        }
        for entry in entries {
            let Some(child_node) = tree.get(entry.child) else {
                continue;
            };
            if child_node.parent().is_none() {
                continue;
            }
            let Some((first_parent, first_child)) = seen.claimant(entry.key.as_ref()) else {
                seen.record(entry.key.as_ref(), parent, entry.child);
                continue;
            };
            if first_parent == parent && first_child == entry.child {
                // Not a second claim at all — `reserve` keeps one entry per
                // `(parent, child)`, so this can only be reached if a caller
                // hand-built the ledger. Nothing to report.
                continue;
            }

            // A key claimed twice by ONE parent for two different children
            // is reported here too. There is no
            // second mechanism to leave it to: the eager check in
            // `element_tree::retake_active_global_key` catches it in debug
            // and is compiled out in release, where the second attachment
            // therefore mounts a genuine second element under one key. That
            // release tree must not end the frame with an empty diagnostic
            // drain.
            reports.push(report_duplicate(
                tree,
                entry.key.as_ref(),
                (first_parent, first_child),
                (parent, entry.child),
            ));
        }
    }

    reports.extend(verify_displacements(reservations, tree));
    reservations.clear();
    reports
}

/// Report every parent that was robbed of a keyed child and never rebuilt to
/// consent to it — see [`Displacement`].
fn verify_displacements(
    reservations: &mut GlobalKeyReservations,
    tree: &mut ElementTree,
) -> Vec<DuplicateGlobalKey> {
    let mut reports = Vec::new();
    for losing_parent in std::mem::take(&mut reservations.displaced_parents) {
        let Some(entries) = reservations.displaced.remove(&losing_parent) else {
            continue;
        };
        if !tree.contains(losing_parent) {
            continue;
        }
        for entry in entries {
            let Some(child_node) = tree.get(entry.child) else {
                continue;
            };
            // The child came home — whatever moved it moved it back, so the
            // parent it was taken from is the parent that has it.
            if child_node.parent() == Some(losing_parent) {
                continue;
            }
            if child_node.parent().is_none() {
                continue;
            }
            reports.push(report_duplicate(
                tree,
                entry.key.as_ref(),
                (losing_parent, entry.child),
                (entry.taken_by, entry.child),
            ));
        }
    }
    reports
}

/// Repair both sides, then build and trace one report.
///
/// Repair comes BEFORE the report: any parent still listing the child that
/// is not its real parent has a dangling edge, and tearing that tree down
/// would cascade secondary failures on top of the real one.
fn report_duplicate(
    tree: &mut ElementTree,
    key: &dyn ViewKey,
    first: (ElementId, ElementId),
    second: (ElementId, ElementId),
) -> DuplicateGlobalKey {
    let (first_parent, first_child) = first;
    let (second_parent, second_child) = second;
    repair_losing_parent(tree, first_parent, first_child);
    repair_losing_parent(tree, second_parent, second_child);

    let key_hash = key.key_hash();
    let report = DuplicateGlobalKey {
        key: format!("{key:?}"),
        key_hash,
        first_parent,
        first_child,
        second_parent,
        second_child,
    };
    tracing::error!(
        key = %report.key,
        key_hash,
        ?first_parent,
        ?first_child,
        ?second_parent,
        ?second_child,
        "duplicate GlobalKey: one key was declared twice in one frame"
    );
    report
}

/// Drop `child` from `parent`'s child list when `parent` is not actually
/// the child's parent any more.
///
/// A no-op in the common case — the graft already unlinked the child when
/// it moved it — but the check costs one lookup and closes the window in
/// which a reservation outlives an edge the graft did not clean up.
fn repair_losing_parent(tree: &mut ElementTree, parent: ElementId, child: ElementId) {
    if tree
        .get(child)
        .and_then(super::super::tree::ElementNode::parent)
        == Some(parent)
    {
        return;
    }
    let Some(parent_node) = tree.get_mut(parent) else {
        return;
    };
    let Some(position) = parent_node
        .child_ids
        .iter()
        .position(|&existing| existing == child)
    else {
        return;
    };
    parent_node.child_ids.remove(position);
    tracing::warn!(
        ?parent,
        ?child,
        "duplicate GlobalKey repair: dropped a child edge from a parent that no longer holds it"
    );
}
