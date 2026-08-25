//! Per-frame `GlobalKey` reservations, and the end-of-frame verification
//! that turns a silent last-writer graft into a reported duplicate.
//!
//! # The problem this closes
//!
//! Resolving a `GlobalKey` is *optimistic*: when a parent declares a child
//! carrying a key that some other parent currently holds, the framework
//! grafts the existing element across rather than mounting a second one.
//! Flutter says so in `_retakeInactiveElement`'s own comment — the
//! "inactivity" is forward-looking, and "the only way that assumption could
//! be false is if the global key is being duplicated".
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
//! Flutter parity: the two ledgers are
//! `BuildOwner._debugGlobalKeyReservations` (`framework.dart:3180`,
//! populated by `_debugReserveGlobalKeyFor` from `Element.updateChild` at
//! `:4086`) and
//! `_debugElementsThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans`
//! (`:3148`, populated by `_retakeInactiveElement` at `:4539` and cleared by
//! `_debugElementWasRebuilt`). Both are verified inside `finalizeTree`.
//!
//! # Four deliberate divergences
//!
//! 1. **It is not debug-only.** Flutter's whole apparatus lives inside
//!    `assert(...)` and evaporates in release. FLUI records and verifies in
//!    every profile: the cost is two small maps per frame, and a duplicate
//!    `GlobalKey` corrupts a release tree exactly as badly as a debug one.
//! 2. **It reports, it does not throw.** Flutter raises a `FlutterError`
//!    out of `finalizeTree`. A duplicate key is caller-controlled input, so
//!    FLUI surfaces a typed [`DuplicateGlobalKey`] through the owner's
//!    diagnostic drain (`BuildOwner::take_global_key_diagnostics`) and a
//!    `tracing::error!`, and the frame completes. Same verdict, different
//!    channel — the same split the eager same-parent check already has.
//! 3. **Verification order is deterministic.** Flutter iterates a
//!    `HashMap`, so which of two conflicting parents is named "older"
//!    depends on hash order. Both ledgers here are held in declaration
//!    order, so the report is reproducible: the parent that declared the
//!    key first in the frame is always `first_parent`.
//! 4. **One parent declaring a key for two children is reported here.**
//!    Flutter skips that shape in `_debugVerifyGlobalKeyReservation`
//!    (`:3248`) and leaves it to a third mechanism,
//!    `_debugVerifyIllFatedPopulation`, which watches the key *registry*
//!    for a displaced-but-still-live element. FLUI has no third mechanism
//!    to leave it to: the eager check in
//!    `element_tree::retake_active_global_key` catches the shape in debug
//!    and is compiled out in release, where the second attachment therefore
//!    mounts a genuine second element under one key. Folding it in here is
//!    what keeps divergence 1 true.
//!
//! # Repair before reporting
//!
//! A duplicate leaves at most one parent actually holding the child; any
//! other parent still listing it has a dangling child edge that would make
//! teardown cascade secondary failures. [`verify`] therefore repairs first
//! — dropping the child from every parent that is not its real parent —
//! and only then records the report. Flutter does the same with
//! `forgetChild` (`framework.dart:3272`), and for the same stated reason.

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
///
/// Flutter tracks the same population separately, in
/// `_debugElementsThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans`
/// (`framework.dart:3148`), recorded by `_retakeInactiveElement` when it
/// takes an element from a live parent and cleared by `_debugElementWasRebuilt`.
struct Displacement {
    child: ElementId,
    key: Box<dyn ViewKey>,
    taken_by: ElementId,
}

/// The frame's reservations, in declaration order.
///
/// Declaration order is load-bearing, not incidental: it is what makes the
/// duplicate report reproducible across runs (see this module's divergence
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
    /// cannot make it look like a duplicate claimant. Flutter parity:
    /// `_debugRemoveGlobalKeyReservationFor` (`framework.dart:3188`).
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
    ///
    /// Flutter clears the same two populations at the same point —
    /// `_debugRemoveGlobalKeyReservationFor` from `updateChild`'s old-child
    /// branch and `_debugElementWasRebuilt` from `buildScope`'s loop.
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
/// Two populations are skipped, matching Flutter's
/// `_debugVerifyGlobalKeyReservation` (`framework.dart:3231`):
///
/// - a parent that is no longer in the tree — it was unmounted later in the
///   frame, so its declaration cannot conflict with anything live;
/// - a child that is no longer in the tree, or that ends the frame with no
///   parent — it was deactivated and never re-attached, so the reservation
///   describes a claim nobody kept.
///
/// Flutter states the first as *two* conditions (`_lifecycleState ==
/// defunct` **or** `renderObject?.attached == false`), because a deactivated
/// element is still reachable from its map there. One condition covers both
/// here because of where this runs: `finalize_tree` sweeps the inactive
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
            // is reported here too, unlike Flutter, which skips this shape
            // in `_debugVerifyGlobalKeyReservation` (`framework.dart:3248`)
            // and leaves it to `_debugVerifyIllFatedPopulation`. FLUI has no
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
///
/// Flutter parity: the `forgetChild` calls in
/// `_debugVerifyGlobalKeyReservation` (`framework.dart:3272`).
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

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::fmt;

    use super::*;

    /// A key with test-chosen identity and hash, so a collision between two
    /// distinct keys can be constructed on purpose.
    #[derive(Clone)]
    struct StubKey {
        identity: u32,
        hash: u64,
    }

    impl StubKey {
        fn new(identity: u32) -> Self {
            Self {
                identity,
                hash: u64::from(identity),
            }
        }

        /// A distinct key that deliberately hashes like `other`.
        fn colliding_with(identity: u32, other: &Self) -> Self {
            Self {
                identity,
                hash: other.hash,
            }
        }
    }

    impl ViewKey for StubKey {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn key_eq(&self, other: &dyn ViewKey) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|other| self.identity == other.identity)
        }

        fn key_hash(&self) -> u64 {
            self.hash
        }

        fn clone_key(&self) -> Box<dyn ViewKey> {
            Box::new(self.clone())
        }

        fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "StubKey({})", self.identity)
        }

        fn is_global_key(&self) -> bool {
            true
        }
    }

    fn eid(n: usize) -> ElementId {
        ElementId::new(n)
    }

    #[test]
    fn a_fresh_reservation_set_is_empty() {
        assert!(GlobalKeyReservations::new().is_empty());
    }

    #[test]
    fn reserving_the_same_parent_and_child_twice_records_one_claim() {
        let mut reservations = GlobalKeyReservations::new();
        let key = StubKey::new(1);
        reservations.reserve(eid(1), eid(2), &key);
        reservations.reserve(eid(1), eid(2), &key);

        assert_eq!(reservations.parents, vec![eid(1)]);
        assert_eq!(reservations.by_parent[&eid(1)].len(), 1);
    }

    #[test]
    fn forgetting_a_parents_last_reservation_drops_the_parent_from_the_order() {
        let mut reservations = GlobalKeyReservations::new();
        reservations.reserve(eid(1), eid(2), &StubKey::new(1));
        reservations.forget(eid(1), eid(2));

        assert!(reservations.is_empty());
        assert!(reservations.parents.is_empty());
    }

    #[test]
    fn seen_keys_distinguishes_two_colliding_identities() {
        let first = StubKey::new(1);
        let second = StubKey::colliding_with(2, &first);
        assert_eq!(first.key_hash(), second.key_hash());

        let mut seen = SeenKeys::default();
        seen.record(&first, eid(10), eid(11));

        assert_eq!(seen.claimant(&first), Some((eid(10), eid(11))));
        assert_eq!(
            seen.claimant(&second),
            None,
            "a colliding but distinct key must not read as already claimed",
        );
    }
}

/// Verification and repair driven against a real [`ElementTree`] through the
/// production frame boundary ([`BuildOwner::finalize_tree`]).
///
/// Separate from the container tests above because these need view fixtures
/// and a live tree; kept in this module rather than `build_owner`'s so the
/// whole `owner::global_key*` surface — containers and behaviour — sits under
/// one test-name prefix, which is what the miri job filters on.
#[cfg(test)]
mod tree_tests {
    use flui_foundation::{ElementId, ViewKey};

    use crate::owner::BuildOwner;
    use crate::tree::{ElementNode, ElementTree};
    use crate::view::{IntoView, View, ViewExt as _};
    use crate::{BuildContext, GlobalKey, StatelessView};

    /// A keyless filler used as a root and as a parent slot.
    #[derive(Clone)]
    struct Plain;

    impl StatelessView for Plain {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for Plain {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    /// A view carrying a `GlobalKey`, so attaching it drives the real
    /// register / retake / reserve path.
    #[derive(Clone)]
    struct Keyed {
        key: GlobalKey<()>,
    }

    impl StatelessView for Keyed {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            Plain.boxed()
        }
    }

    impl View for Keyed {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }

        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    /// A root with two parents beneath it.
    fn two_parents() -> (ElementTree, BuildOwner, ElementId, ElementId) {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = tree.mount_root(&Plain, &mut owner.element_owner_mut());
        let a = tree.insert(&Plain, root, 0, &mut owner.element_owner_mut());
        let b = tree.insert(&Plain, root, 1, &mut owner.element_owner_mut());
        (tree, owner, a, b)
    }

    /// Two parents declaring one key in one frame is reported, and the report
    /// names both of them in declaration order.
    #[test]
    fn two_parents_declaring_one_key_in_one_frame_are_reported() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        let child = tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        let grafted = tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());
        assert_eq!(grafted, child, "the second declaration grafts, not mounts");

        owner.finalize_tree(&mut tree);

        let reports = owner.take_global_key_diagnostics();
        assert_eq!(reports.len(), 1, "one key, two parents: {reports:?}");
        assert_eq!(reports[0].first_child, child);
        assert_eq!(reports[0].second_child, child);
        assert_eq!(
            (reports[0].first_parent, reports[0].second_parent),
            (parent_a, parent_b),
            "declaration order decides which parent is named first",
        );
        assert_eq!(reports[0].key_hash, keyed.key.id());
    }

    /// The legitimate cross-frame reparent, shape 1: the old parent gives
    /// the child up first (soft-removing it into the inactive queue), and
    /// the new parent takes it from there. Nothing is reported.
    ///
    /// The give-up is a real `ElementTree::remove` rather than a bare
    /// reservation withdrawal, because that is what the reconciler does and
    /// it is what makes the difference: the second attachment then takes the
    /// *inactive* retake path, which displaces nobody.
    #[test]
    fn a_reparent_the_old_parent_consented_to_is_not_a_duplicate() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        let child = tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        owner.finalize_tree(&mut tree);
        assert!(owner.take_global_key_diagnostics().is_empty());

        // Parent a lets the child go — soft-removed, still retakeable.
        tree.remove(child, &mut owner.element_owner_mut());
        owner
            .element_owner_mut()
            .forget_global_key_reservation(parent_a, child);

        let retaken = tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());
        assert_eq!(retaken, child, "the element is relocated, not duplicated");

        owner.finalize_tree(&mut tree);
        assert!(
            owner.take_global_key_diagnostics().is_empty(),
            "the old parent had already given the child up",
        );
    }

    /// The legitimate cross-frame reparent, shape 2: the new parent grafts
    /// the child straight out of the still-live old parent, and the old
    /// parent then rebuilds — which is how it consents to the loss. Nothing
    /// is reported.
    #[test]
    fn a_graft_the_old_parent_rebuilds_after_is_not_a_duplicate() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        let child = tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        owner.finalize_tree(&mut tree);
        assert!(owner.take_global_key_diagnostics().is_empty());

        let grafted = tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());
        assert_eq!(grafted, child);

        // `parent_a` rebuilds. The drain calls this for every element it
        // rebuilds; reaching for the ledger directly is how this test says
        // "a rebuilt" without standing up a full `build_scope` over fixture
        // views that never terminate.
        owner.global_key_reservations.note_parent_rebuild(parent_a);

        owner.finalize_tree(&mut tree);
        assert!(
            owner.take_global_key_diagnostics().is_empty(),
            "rebuilding is how the robbed parent consents to the loss",
        );
    }

    /// The illegal counterpart: the new parent grafts the child out of a
    /// live old parent that never rebuilds. The old parent ends the frame
    /// describing a child it no longer has, and the reservation ledger alone
    /// cannot see it — it never reserved, because it never ran.
    ///
    /// Flutter reports the same population from
    /// `_debugElementsThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans`.
    #[test]
    fn a_graft_out_of_a_parent_that_never_rebuilds_is_reported() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        let child = tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        owner.finalize_tree(&mut tree);
        assert!(owner.take_global_key_diagnostics().is_empty());

        tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());
        owner.finalize_tree(&mut tree);

        let reports = owner.take_global_key_diagnostics();
        assert_eq!(
            reports.len(),
            1,
            "the robbed parent is reported: {reports:?}"
        );
        assert_eq!(
            (reports[0].first_parent, reports[0].second_parent),
            (parent_a, parent_b),
            "the robbed parent is named first, the grafter second",
        );
        assert_eq!(
            (reports[0].first_child, reports[0].second_child),
            (child, child),
        );
    }

    /// The repair, driven against a tree that really carries the dangling
    /// edge it exists to clear.
    ///
    /// The production graft (`retake_active_global_key`) unlinks a child from
    /// its old parent before relinking it, so a losing parent normally has
    /// nothing left to forget and the repair is a no-op. That makes the
    /// repair untestable through the graft alone — so the stale edge is
    /// injected here directly, which is exactly the state Flutter's own
    /// `forgetChild` call in `_debugVerifyGlobalKeyReservation`
    /// (`framework.dart:3272`) is written to survive.
    #[test]
    fn verification_clears_a_losing_parents_dangling_child_edge() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };
        let child = tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());

        // `ElementTree::insert` mints the node; the parent-side `child_ids`
        // list is written by the reconciler's own pass, which a raw insert
        // never runs. Both lists are therefore set by hand — `b`'s to the
        // state a completed reconcile would leave, and `a`'s to that state
        // PLUS the stale edge the graft is supposed to have removed.
        tree.get_mut(parent_a)
            .expect("parent a is mounted")
            .child_ids
            .push(child);
        tree.get_mut(parent_b)
            .expect("parent b is mounted")
            .child_ids
            .push(child);

        owner.finalize_tree(&mut tree);

        assert_eq!(
            owner.take_global_key_diagnostics().len(),
            1,
            "the duplicate is still reported",
        );
        assert_eq!(
            tree.get(child).and_then(ElementNode::parent),
            Some(parent_b),
            "the child's own parent edge is untouched by the repair",
        );
        assert!(
            !tree
                .get(parent_a)
                .expect("parent a is still mounted")
                .child_ids()
                .contains(&child),
            "the losing parent's dangling edge is cleared before the report",
        );
        assert!(
            tree.get(parent_b)
                .expect("parent b is still mounted")
                .child_ids()
                .contains(&child),
            "the winning parent keeps the child it actually holds",
        );
    }

    /// A parent that gives its keyed child up mid-frame is no longer a
    /// claimant, so the parent that takes it is alone and nothing is
    /// reported — the whole sequence inside one frame.
    ///
    /// The give-up mirrors `id_reconcile::remove_child`: withdraw the
    /// reservation *and* soft-remove the child, which is what makes the
    /// second attachment an inactive retake rather than a graft out of a
    /// live parent.
    #[test]
    fn withdrawing_a_declaration_mid_frame_leaves_a_single_claimant() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        let child = tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        owner
            .element_owner_mut()
            .forget_global_key_reservation(parent_a, child);
        tree.remove(child, &mut owner.element_owner_mut());
        tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());

        owner.finalize_tree(&mut tree);
        assert!(
            owner.take_global_key_diagnostics().is_empty(),
            "a withdrawn declaration is not a competing claim",
        );
    }

    /// One parent declaring one key for two different children is reported
    /// too.
    ///
    /// In a debug build the eager check in
    /// `element_tree::retake_active_global_key` rejects the second
    /// attachment before a frame boundary is reached, so this shape only
    /// arrives here in release — where that check is compiled out and the
    /// second attachment mounts a genuine second element under one key. A
    /// release tree in that state must not end the frame with an empty
    /// diagnostic drain, which is why the frame boundary does not skip it.
    ///
    /// Flutter *does* skip it here (`framework.dart:3248`) and leaves it to
    /// `_debugVerifyIllFatedPopulation`; FLUI has no second mechanism to
    /// leave it to.
    ///
    /// The declarations are recorded directly rather than through two
    /// attachments so the test runs in both profiles — in debug the second
    /// attachment would panic before recording anything.
    #[test]
    fn one_parent_declaring_a_key_for_two_children_is_reported() {
        let (mut tree, mut owner, parent, _unused) = two_parents();
        let first = tree.insert(&Plain, parent, 0, &mut owner.element_owner_mut());
        let second = tree.insert(&Plain, parent, 1, &mut owner.element_owner_mut());
        let key = GlobalKey::<()>::new();

        {
            let mut handle = owner.element_owner_mut();
            handle.reserve_global_key(parent, first, &key);
            handle.reserve_global_key(parent, second, &key);
        }

        owner.finalize_tree(&mut tree);
        let reports = owner.take_global_key_diagnostics();
        assert_eq!(reports.len(), 1, "one key on two children: {reports:?}");
        assert_eq!(
            (reports[0].first_parent, reports[0].second_parent),
            (parent, parent),
            "both declarations came from the one parent",
        );
        assert_eq!(
            (reports[0].first_child, reports[0].second_child),
            (first, second),
            "the report names both children, in declaration order",
        );
    }

    /// The release-build path the test above stands in for, driven for real:
    /// with the eager same-parent check compiled out, the second attachment
    /// mounts a genuine second element under one key, and the frame boundary
    /// must still report it.
    ///
    /// Debug-only builds panic inside the second `insert`, so this can only
    /// run under `--release` (or any profile with `debug_assertions` off) —
    /// which is exactly the profile whose silence this closes.
    #[cfg(not(debug_assertions))]
    #[test]
    fn a_release_build_reports_the_second_element_mounted_under_one_key() {
        let (mut tree, mut owner, parent, _unused) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        let first = tree.insert(&keyed, parent, 0, &mut owner.element_owner_mut());
        let second = tree.insert(&keyed, parent, 1, &mut owner.element_owner_mut());
        assert_ne!(
            first, second,
            "with the eager check compiled out, a second element really is mounted",
        );

        owner.finalize_tree(&mut tree);
        let reports = owner.take_global_key_diagnostics();
        assert_eq!(
            reports.len(),
            1,
            "the release tree must not end the frame silent: {reports:?}",
        );
        assert_eq!(
            (reports[0].first_child, reports[0].second_child),
            (first, second),
            "the report names both live elements sharing the key",
        );
    }

    /// A parent unmounted later in the same frame is not a live claimant, so
    /// its earlier declaration cannot make the surviving parent look like a
    /// duplicate.
    #[test]
    fn a_declaration_from_a_parent_that_left_the_tree_is_ignored() {
        let (mut tree, mut owner, parent_a, parent_b) = two_parents();
        let keyed = Keyed {
            key: GlobalKey::<()>::new(),
        };

        tree.insert(&keyed, parent_a, 0, &mut owner.element_owner_mut());
        tree.insert(&keyed, parent_b, 0, &mut owner.element_owner_mut());
        tree.remove_finalized(parent_a, &mut owner.element_owner_mut());

        owner.finalize_tree(&mut tree);
        assert!(
            owner.take_global_key_diagnostics().is_empty(),
            "a parent that is gone by the frame boundary claims nothing",
        );
    }
}
