//! Bounded-retry poison for layout failures.
//!
//! A render object whose `perform_layout` keeps failing must not be retried
//! forever: each retry re-runs the whole subtree walk, re-logs the error,
//! and re-marks paint, so any per-frame invalidation source (an animation
//! tick, a stream, a timer) turns one broken node into perpetual full-frame
//! layout+paint work with zero progress. This module tracks consecutive
//! layout failures per [`RenderId`] and, once a node crosses the retry
//! budget, marks it **layout-poisoned**: the layout walk skips it (its last
//! committed geometry, or `Size::ZERO` / `SliverGeometry::ZERO` when it never
//! succeeded, stands in) until it is freshly invalidated from outside.
//!
//! Recovery: `PipelineOwner::mark_needs_layout` lifts the poison on every
//! node its invalidation walk visits, so a real property / child / tree
//! change gives the node another chance. The failure counter itself is only
//! reset by an observed *successful* layout — a node that is un-poisoned and
//! fails again re-poisons immediately (one bounded attempt per external
//! invalidation) instead of earning a fresh budget it has not shown to
//! deserve.

use flui_foundation::RenderId;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::RenderError;
use crate::storage::ErasedConstraints;

/// Consecutive layout failures a node may accrue before it is poisoned.
///
/// Structural failures ([`LayoutFailureKind::Structural`]) poison on the
/// first occurrence; this budget applies only to failures that could
/// plausibly self-heal on a later frame.
pub(super) const MAX_CONSECUTIVE_LAYOUT_FAILURES: u8 = 3;

/// Whether a layout failure is permanent for the current tree state or
/// could plausibly self-heal on a retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LayoutFailureKind {
    /// Permanent for the current tree state: retrying with unchanged inputs
    /// deterministically fails again (layout cycles, contract violations,
    /// invalid geometry, unbounded input, depth overflow, protocol
    /// mismatches, child-index corruption, and panics). Poisons immediately.
    Structural,
    /// Could plausibly self-heal on a later frame (stale ids mid-removal,
    /// pipeline-state races). Retried up to [`MAX_CONSECUTIVE_LAYOUT_FAILURES`]
    /// consecutive times before poisoning.
    Retriable,
}

impl LayoutFailureKind {
    /// Classifies a layout-phase error by whether a retry with unchanged
    /// tree state could succeed.
    pub(super) fn of(err: &RenderError) -> Self {
        match err {
            RenderError::LayoutCycle(..)
            | RenderError::ContractViolation { .. }
            | RenderError::InvalidGeometry { .. }
            | RenderError::UnboundedConstraint { .. }
            | RenderError::LayoutDepthExceeded { .. }
            | RenderError::ProtocolMismatch { .. }
            | RenderError::ChildIndexOutOfBounds { .. }
            | RenderError::Poisoned { .. } => Self::Structural,
            _ => Self::Retriable,
        }
    }
}

/// What a node's failure record says about its last layout attempt.
///
/// The walk's poison skip keys on this:
/// - [`FailedAttempt::Layout`] poison under the *same* constraints is
///   guaranteed to fail again — serve the stand-in geometry;
/// - [`FailedAttempt::Layout`] poison under *different* constraints
///   earns one fresh attempt (an ancestor may now offer valid inputs);
/// - [`FailedAttempt::IntrinsicOnly`] poison has no constraints to
///   compare against, so the layout channel cannot vouch for recovery —
///   the stand-in stands until an external invalidation lifts the poison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FailedAttempt {
    /// The last failure came from a layout walk under these constraints.
    Layout(ErasedConstraints),
    /// The last failure came only from intrinsic queries, which carry no
    /// layout constraints.
    IntrinsicOnly,
}

/// Per-node failure record.
#[derive(Debug, Clone, Copy, Default)]
struct PoisonEntry {
    /// Failures since the last observed success. Saturates at `u8::MAX`;
    /// only compared against the small retry budget.
    consecutive_failures: u8,
    /// True while the layout walk skips this node.
    poisoned: bool,
    /// True once the first poison transition has been logged at error
    /// level; re-poisons after an un-poison log at debug instead, so a
    /// node that keeps being invalidated and keeps failing does not spam
    /// an error per frame. Cleared by [`LayoutPoison::note_success`].
    poison_reported: bool,
    /// Constraints of the last failed layout attempt. Updated on every
    /// failure record that carries constraints (intrinsic-query records
    /// carry none and leave the previous value intact). Compared against
    /// the incoming constraints at the walk's poison skip: a difference
    /// means the node is being laid out under different inputs than the
    /// ones it failed under — for example an ancestor now hands valid
    /// constraints to a child poisoned by an unbounded/invalid one —
    /// which earns one fresh attempt instead of a stand-in geometry
    /// pinned forever. Committed (last-good) constraints cannot serve
    /// this role: a failed layout never commits, so the committed value
    /// is always older than the failing attempt and would misread a
    /// retry under the *same* failing constraints as "changed",
    /// reopening the unbounded-retry loop the poison exists to break.
    last_attempt: Option<ErasedConstraints>,
}

/// Failure counters and poison flags for every node that failed layout
/// since its last success. Entries are created on the first failure and
/// removed on the first subsequent success, so the map is empty in the
/// common all-healthy case and lookups on the layout hot path stay cheap.
#[derive(Debug, Default)]
pub(super) struct LayoutPoison {
    entries: FxHashMap<RenderId, PoisonEntry>,
}

impl LayoutPoison {
    /// True while `id` is layout-poisoned (the walk must skip it).
    #[inline]
    pub(super) fn is_poisoned(&self, id: RenderId) -> bool {
        self.entries.get(&id).is_some_and(|e| e.poisoned)
    }

    /// What the node's last failure says about its layout attempt —
    /// see [`FailedAttempt`]. `None` when the node has no record at all.
    #[inline]
    pub(super) fn failed_attempt(&self, id: RenderId) -> Option<FailedAttempt> {
        self.entries.get(&id).map(|e| match e.last_attempt {
            Some(constraints) => FailedAttempt::Layout(constraints),
            None => FailedAttempt::IntrinsicOnly,
        })
    }

    /// True when `id` has open (not yet poisoned) failures on record. Used
    /// by the walk's success bookkeeping: a poisoned node is skipped rather
    /// than laid out, so an `Ok` for it must not be read as a recovery.
    #[inline]
    pub(super) fn has_open_failures(&self, id: RenderId) -> bool {
        self.entries.get(&id).is_some_and(|e| !e.poisoned)
    }

    /// Records one layout failure for `id`.
    ///
    /// Returns `Some(first_report)` exactly when this failure tips the node
    /// into the poisoned state (the 0 → 1 transition), so the caller can
    /// clear the node's `NEEDS_LAYOUT` flag and log the event once.
    /// `first_report` is `true` only for the node's first-ever poison
    /// transition since its last success; re-poisons after an un-poison
    /// yield `false` so the caller logs them quietly instead of emitting
    /// one error per external invalidation.
    pub(super) fn note_failure(
        &mut self,
        id: RenderId,
        kind: LayoutFailureKind,
        attempt: Option<ErasedConstraints>,
    ) -> Option<bool> {
        let entry = self.entries.entry(id).or_default();
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        // Track the newest failed constraints even when the node is
        // already poisoned: the skip comparison must follow the most
        // recent attempt, or a retry under different constraints would
        // be misread as a repeat of the older failure.
        if attempt.is_some() {
            entry.last_attempt = attempt;
        }
        if entry.poisoned
            || (kind == LayoutFailureKind::Retriable
                && entry.consecutive_failures < MAX_CONSECUTIVE_LAYOUT_FAILURES)
        {
            return None;
        }
        entry.poisoned = true;
        let first_report = !entry.poison_reported;
        entry.poison_reported = true;
        Some(first_report)
    }

    /// Records a whole walk's failure records, deduplicated to **one
    /// failure count per failed node per walk**: a node that fails both
    /// `perform_layout` and an intrinsic query in the same pass burns its
    /// budget once, not twice. On a kind conflict for the same node,
    /// Structural wins (the more permanent signal); the newest
    /// constraints-carrying attempt wins. `records` items are
    /// `(parent, failed, kind, attempt)`.
    ///
    /// Returns one `(parent, failed, kind, first_report)` per NEWLY
    /// poisoned node so the caller can apply node state (clear
    /// `NEEDS_LAYOUT` where appropriate) and log the transition once.
    pub(super) fn note_failures(
        &mut self,
        records: Vec<(
            RenderId,
            RenderId,
            LayoutFailureKind,
            Option<ErasedConstraints>,
        )>,
    ) -> Vec<(RenderId, RenderId, LayoutFailureKind, bool)> {
        let mut merged: FxHashMap<
            RenderId,
            (RenderId, LayoutFailureKind, Option<ErasedConstraints>),
        > = FxHashMap::default();
        for (parent, failed, kind, attempt) in records {
            merged
                .entry(failed)
                .and_modify(|(_, existing_kind, existing_attempt)| {
                    if kind == LayoutFailureKind::Structural {
                        *existing_kind = LayoutFailureKind::Structural;
                    }
                    if attempt.is_some() {
                        *existing_attempt = attempt;
                    }
                })
                .or_insert((parent, kind, attempt));
        }
        merged
            .into_iter()
            .filter_map(|(failed, (parent, kind, attempt))| {
                self.note_failure(failed, kind, attempt)
                    .map(|first_report| (parent, failed, kind, first_report))
            })
            .collect()
    }

    /// Records a successful layout for `id`, clearing its failure record.
    /// No-op when the node has no record (the common case).
    #[inline]
    pub(super) fn note_success(&mut self, id: RenderId) {
        self.entries.remove(&id);
    }

    /// Lifts the poison on `id` without touching its failure counter: the
    /// next layout attempt runs again, and if it fails the node re-poisons
    /// immediately (its counter is already at budget). Called by the
    /// `mark_needs_layout` invalidation walk for every node it visits, so
    /// an actual property / child / tree change is always what re-arms a
    /// poisoned node.
    #[inline]
    pub(super) fn unpoison(&mut self, id: RenderId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.poisoned = false;
        }
    }

    /// Drops records for removed nodes. Ids are generational, so a stale
    /// record could never alias a new node; this keeps the map from
    /// accumulating dead entries across tree churn.
    pub(super) fn evict(&mut self, removed: &FxHashSet<RenderId>) {
        self.entries.retain(|id, _| !removed.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: usize) -> RenderId {
        RenderId::new(n)
    }

    #[test]
    fn structural_failure_poisons_on_first_occurrence() {
        let mut poison = LayoutPoison::default();
        assert_eq!(
            poison.note_failure(id(1), LayoutFailureKind::Structural, None),
            Some(true),
        );
        assert!(poison.is_poisoned(id(1)));
        // A second failure while poisoned is not a new transition.
        assert_eq!(
            poison.note_failure(id(1), LayoutFailureKind::Structural, None),
            None
        );
    }

    #[test]
    fn retriable_failure_poisons_at_budget() {
        let mut poison = LayoutPoison::default();
        for _ in 0..MAX_CONSECUTIVE_LAYOUT_FAILURES - 1 {
            assert_eq!(
                poison.note_failure(id(1), LayoutFailureKind::Retriable, None),
                None
            );
            assert!(!poison.is_poisoned(id(1)));
        }
        assert_eq!(
            poison.note_failure(id(1), LayoutFailureKind::Retriable, None),
            Some(true),
        );
        assert!(poison.is_poisoned(id(1)));
    }

    #[test]
    fn success_clears_the_record() {
        let mut poison = LayoutPoison::default();
        poison.note_failure(id(1), LayoutFailureKind::Retriable, None);
        assert!(poison.has_open_failures(id(1)));
        poison.note_success(id(1));
        assert!(!poison.has_open_failures(id(1)));
        // Budget restarts from zero after a success.
        assert_eq!(
            poison.note_failure(id(1), LayoutFailureKind::Retriable, None),
            None
        );
    }

    #[test]
    fn unpoison_keeps_the_counter_so_a_new_failure_repoisons_immediately() {
        let mut poison = LayoutPoison::default();
        assert_eq!(
            poison.note_failure(id(1), LayoutFailureKind::Structural, None),
            Some(true),
        );
        poison.unpoison(id(1));
        assert!(!poison.is_poisoned(id(1)));
        // The counter is still at budget: one more failure re-poisons, and
        // it is not reported as a first-time transition.
        assert_eq!(
            poison.note_failure(id(1), LayoutFailureKind::Structural, None),
            Some(false),
        );
        assert!(poison.is_poisoned(id(1)));
    }

    #[test]
    fn structural_classification() {
        let structural = RenderError::contract_violation("X", "contract");
        assert_eq!(
            LayoutFailureKind::of(&structural),
            LayoutFailureKind::Structural
        );
        let cycle = RenderError::layout_cycle(id(7));
        assert_eq!(LayoutFailureKind::of(&cycle), LayoutFailureKind::Structural);
        let retriable = RenderError::NodeNotFound(id(9));
        assert_eq!(
            LayoutFailureKind::of(&retriable),
            LayoutFailureKind::Retriable
        );
    }

    #[test]
    fn note_failures_counts_each_node_once_per_walk() {
        let mut poison = LayoutPoison::default();
        // The same node fails twice in one walk (e.g. layout + intrinsic):
        // one budget tick, not two.
        let transitions = poison.note_failures(vec![
            (id(1), id(2), LayoutFailureKind::Retriable, None),
            (id(1), id(2), LayoutFailureKind::Retriable, None),
            (id(1), id(3), LayoutFailureKind::Retriable, None),
        ]);
        assert!(transitions.is_empty(), "nothing poisons below budget");
        // Second walk: id(2) reaches 2 of 3 — still below budget — proving
        // the duplicate in walk one counted once (twice would be 3 already).
        let transitions =
            poison.note_failures(vec![(id(1), id(2), LayoutFailureKind::Retriable, None)]);
        assert!(transitions.is_empty(), "id(2) must be at 2, not 3");
        // Third walk: id(2) hits the budget of 3, while id(3) is at 2.
        let transitions =
            poison.note_failures(vec![(id(1), id(2), LayoutFailureKind::Retriable, None)]);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].1, id(2));
        assert!(poison.is_poisoned(id(2)));
        assert!(!poison.is_poisoned(id(3)));
    }

    #[test]
    fn note_failures_structural_wins_kind_conflicts() {
        let mut poison = LayoutPoison::default();
        // Same node, mixed kinds in one walk: the structural record must
        // dominate, poisoning immediately.
        let transitions = poison.note_failures(vec![
            (id(1), id(2), LayoutFailureKind::Retriable, None),
            (id(1), id(2), LayoutFailureKind::Structural, None),
        ]);
        assert_eq!(transitions.len(), 1);
        assert!(poison.is_poisoned(id(2)));
    }

    #[test]
    fn poison_tracks_the_failed_constraints_per_attempt() {
        use crate::constraints::BoxConstraints;
        use flui_types::geometry::px;

        let c1 =
            ErasedConstraints::from(BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0)));
        let c2 = ErasedConstraints::from(BoxConstraints::new(px(0.0), px(50.0), px(0.0), px(50.0)));
        let mut poison = LayoutPoison::default();

        // A layout-earned failure records its constraints; an
        // intrinsic-earned one is marked `IntrinsicOnly`.
        poison.note_failure(id(1), LayoutFailureKind::Structural, Some(c1));
        assert_eq!(
            poison.failed_attempt(id(1)),
            Some(FailedAttempt::Layout(c1))
        );
        poison.note_failure(id(2), LayoutFailureKind::Structural, None);
        assert_eq!(
            poison.failed_attempt(id(2)),
            Some(FailedAttempt::IntrinsicOnly)
        );

        // A new layout failure under c2 moves the tracked attempt to c2.
        poison.note_failure(id(1), LayoutFailureKind::Structural, Some(c2));
        assert_eq!(
            poison.failed_attempt(id(1)),
            Some(FailedAttempt::Layout(c2))
        );

        // Success clears the record entirely.
        poison.note_success(id(1));
        assert_eq!(poison.failed_attempt(id(1)), None);
    }
}
