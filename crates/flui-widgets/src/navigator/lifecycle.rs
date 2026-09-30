//! [`RouteLifecycle`] — the state machine `flush_history_updates` walks.
//!
//! Private; nothing here is exported.
//!
//! # The declaration order is load-bearing
//!
//! The presence and announcement predicates are index comparisons against the
//! declaration order, so `#[derive(PartialOrd, Ord)]` over the variants — which
//! orders by declaration — expresses them as range checks. Reordering a variant
//! silently changes four predicates at once; `lifecycle_order_matches_flush_ranges`
//! pins every membership.
//!
//! # Two states are deliberately absent
//!
//! - **`staging`** would sit before `add`. It exists only for a transition delegate
//!   that decides whether a page-based route entering via `Navigator.pages` should
//!   animate. Page-based routing is deferred, so the state has no producer and no
//!   consumer. Re-adding it prepends a variant and shifts nothing, because the
//!   predicates are named ranges.
//! - **`disposing`** would sit before `disposed`. It exists in runtimes that
//!   cannot dispose a route until its overlay entries' elements have unmounted, on
//!   a later turn of the event loop. FLUI's unmount is synchronous, so `Dispose`
//!   is terminal and `disposing` is never observed inside the flush.

/// Where a route sits in its lifecycle.
///
/// Declaration order matters, minus `staging` and `disposing` — see the module
/// docs. `Ord` is derived so the predicates below are index comparisons, spelled
/// as ranges.
/// `PushReplace` gained its production producer when
/// `NavigatorHandle::push_replacement` was exported (2026-07-10). `Replace` still
/// has none: `replace` / `replace_route_below` are implemented and tested but not
/// exported, pending their own sign-off. The state is kept because the flush's
/// arms and the range predicates depend on the declaration order, and deleting a
/// variant would silently shift four predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RouteLifecycle {
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
    /// `add ..= idle`: the route is, or will be once its lifecycle settles, present.
    pub(crate) fn will_be_present(self) -> bool {
        (Self::Add..=Self::Idle).contains(&self)
    }

    /// `add ..= remove`: the route is present.
    pub(crate) fn is_present(self) -> bool {
        (Self::Add..=Self::Remove).contains(&self)
    }

    /// `push ..= removing`: the route is told about its neighbours.
    pub(crate) fn suitable_for_announcement(self) -> bool {
        (Self::Push..=Self::Removing).contains(&self)
    }

    /// `push ..= remove`: the route can drive a transition animation for its
    /// neighbours.
    pub(crate) fn suitable_for_transition_animation(self) -> bool {
        (Self::Push..=Self::Remove).contains(&self)
    }
}
