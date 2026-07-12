//! [`RouteLifecycle`] — the state machine `flush_history_updates` walks.
//!
//! Private; nothing here is exported.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/navigator.dart:3139-3168`
//! (`enum _RouteLifecycle`) and `:3519-3539` (the range predicates).
//!
//! **The declaration order is load-bearing.** Flutter's predicates are index
//! comparisons against that order:
//!
//! ```dart
//! bool get willBePresent =>
//!     currentState.index <= _RouteLifecycle.idle.index &&
//!     currentState.index >= _RouteLifecycle.add.index;
//! ```
//!
//! So `#[derive(PartialOrd, Ord)]` over the variants — which orders by
//! declaration — reproduces them as range checks. Reordering a variant silently
//! changes four predicates at once; `lifecycle_order_matches_flush_ranges` pins
//! every membership.
//!
//! # Two of Flutter's sixteen states are deliberately absent
//!
//! - **`staging`** (index 0) exists only for `TransitionDelegate`, which decides
//!   whether a page-based route entering via `Navigator.pages` should animate.
//!   Page-based routing is deferred, so the state has no producer
//!   and no consumer. Re-checked against the reference: `staging` is
//!   written only by `_updatePages` / `RouteTransitionRecord`, read only by
//!   `isWaitingForEnteringDecision` (`navigator.dart:3559`), and
//!   `_flushHistoryUpdates` `assert(false)`s on it (`:4576`). Omitting it is
//!   sound; re-adding it prepends a variant and shifts nothing, because the
//!   predicates are named ranges.
//! - **`disposing`** (index 14) exists because Dart cannot dispose a route until
//!   its overlay entries' elements have unmounted, which happens on a later
//!   microtask (`_entryWaitingForSubTreeDisposal`, `navigator.dart:3464-3517`).
//!   FLUI's unmount is synchronous, so `dispose` is terminal. `_flushHistoryUpdates`
//!   `assert(false)`s on `disposing` too, so it is never observed inside the flush.

/// Where a route sits in its lifecycle.
///
/// Ordered by Flutter's declaration order (`navigator.dart:3139-3168`), minus
/// `staging` and `disposing` — see the module docs. `Ord` is derived so the
/// predicates below are the reference's index comparisons, spelled as ranges.
/// `PushReplace` gained its production producer when
/// `NavigatorHandle::push_replacement` was exported (2026-07-10). `Replace` still
/// has none: `replace` / `replaceRouteBelow` are ported and tested but not
/// exported, pending their own sign-off. The state is kept because the flush's
/// arms and the range predicates are transcribed from Flutter's declaration
/// order, and deleting a variant would silently shift four predicates.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum RouteLifecycle {
    /// Will call `install` + `did_add`. Entered from an initial-route seed.
    Add,
    /// Awaiting the top-most push to settle before it may quietly appear.
    Adding,
    /// Will call `install` + `did_push`. Entered from `push`.
    Push,
    /// Will call `install` + `did_push`, and reports `did_replace` to observers.
    PushReplace,
    /// Awaiting the push transition.
    Pushing,
    /// Will call `install` + `did_replace`. Entered from `replace`.
    Replace,
    /// Settled and present.
    Idle,
    /// Will call `did_pop`.
    Pop,
    /// Will call `did_complete`.
    Complete,
    /// Will report `did_replace` / `did_remove` to observers.
    Remove,
    /// Awaiting the pop transition (`finished_when_popped == false`).
    Popping,
    /// Awaiting the covering route's transition before it may quietly vanish.
    Removing,
    /// Will be disposed at the end of this flush.
    Dispose,
    /// Done.
    Disposed,
}

impl RouteLifecycle {
    /// `add ..= idle` — Flutter `_RouteEntry.willBePresent` (`navigator.dart:3519`).
    pub(crate) fn will_be_present(self) -> bool {
        (Self::Add..=Self::Idle).contains(&self)
    }

    /// `add ..= remove` — Flutter `_RouteEntry.isPresent` (`:3524`).
    pub(crate) fn is_present(self) -> bool {
        (Self::Add..=Self::Remove).contains(&self)
    }

    /// `push ..= removing` — Flutter `_RouteEntry.suitableForAnnouncement` (`:3531`).
    pub(crate) fn suitable_for_announcement(self) -> bool {
        (Self::Push..=Self::Removing).contains(&self)
    }

    /// `push ..= remove` — Flutter `_RouteEntry.suitableForTransitionAnimation`
    /// (`:3536`).
    pub(crate) fn suitable_for_transition_animation(self) -> bool {
        (Self::Push..=Self::Remove).contains(&self)
    }
}
