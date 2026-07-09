//! [`RouteBinding`] — the owned capability a route uses to drive its own
//! lifecycle: the ADR-0020 U5.1 route-animation seam.
//!
//! Private. Nothing here is exported, and `Route` — which *is* public — never
//! mentions it. See *Correction 2* below.
//!
//! # What Flutter does
//!
//! A Flutter `Route` holds `_navigator` and calls back into it directly:
//!
//! ```dart
//! // routes.dart:87-94 — OverlayRoute.didPop, i.e. DURING _flushHistoryUpdates
//! if (finishedWhenPopped) { navigator!.finalizeRoute(this); }
//!
//! // navigator.dart:5825-5828 — finalizeRoute
//! entry.finalize();
//! if (!_flushingHistory) { _flushHistoryUpdates(rearrangeOverlay: false); }
//! ```
//!
//! So `finalizeRoute` mutates the entry **immediately** and merely declines to
//! start a *nested* flush while one is running. `handlePush`'s
//! `whenCompleteOrCancel` (`navigator.dart:3276-3290`) is the opposite: it
//! `assert(!navigator._debugLocked)`, because a `TickerFuture` completion always
//! arrives on a later microtask, never inside a flush.
//!
//! # Correction 1 to ADR-0020 Decision 2: a direct callback would **deadlock**
//!
//! ADR-0020 proposed a `RouteBinding` exposing `notify_push_completed()` and
//! `finalize()` as direct navigator callbacks. That cannot work.
//! `NavigatorShared::mutate` holds `history.lock()` for the whole flush, and
//! `parking_lot::Mutex` is not reentrant — a route calling back into
//! `RouteHistory` from `did_pop` would hang, not panic.
//!
//! So a binding **enqueues a [`RouteCommand`]** onto a queue guarded by its own
//! mutex, then calls a `wake` closure. `wake` uses `try_lock` on the history: if
//! it succeeds we are outside a flush and the commands are applied and flushed
//! at once; if it fails, a flush is in progress on this thread and *that* flush
//! drains the queue before it returns. The queue is Flutter's `_flushingHistory`
//! check, expressed as ownership rather than as a flag.
//!
//! This preserves both invariants: `RouteHistory` never learns about the
//! navigator (so `route_stack_flush_is_pure_data` stays green), and the
//! `BUG: flush_history_updates re-entered` assert stays reachable for a genuinely
//! recursive `flush()` — which is what it was always guarding.
//!
//! # Correction 2 to ADR-0020 Decision 2: `install(&mut self, binding)` cannot be
//!
//! `Route` is public (ADR-0019 U4). Threading a `&RouteBinding` through
//! `Route::install` would force `RouteBinding` into the public surface, which U5.1
//! is explicitly not authorized to do. Instead the binding reaches a route through
//! [`BoundRoute`], a private trait, delivered by a private
//! `NavigatorHandle::push_bound` **before** the route is boxed — which is why
//! `RouteId` is minted up front rather than inside `RouteRecord::erase`.
//!
//! U5.2's `TransitionRoute` is internal, so it can implement `BoundRoute`. If
//! U5.4 ever lets an app author write an animated route, *that* is where the
//! public shape gets decided — and signed off.

// U5.2's `TransitionRoute` is the intended consumer of this seam. Until it lands,
// the only caller of `NavigatorHandle::push_bound` is the test suite, so from
// rustc's reachability view every item here is dead. The attribute goes with U5.2.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use parking_lot::Mutex;

use super::route::{Route, RouteId};

/// A lifecycle transition a route asks its navigator to make.
///
/// Applied by [`RouteHistory`](super::history::RouteHistory) either at the head
/// of the next flush, or — when raised *during* a flush — immediately after that
/// flush's walk, which then re-runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteCommand {
    /// The entrance transition finished. Flutter's `whenCompleteOrCancel`
    /// callback (`navigator.dart:3276-3290`): `pushing` → `idle`, then re-flush.
    PushCompleted(RouteId),
    /// The route is finished and may be disposed. Flutter's `finalizeRoute`
    /// (`navigator.dart:5798-5834`): `entry.finalize()`, then flush unless one is
    /// already running.
    Finalize(RouteId),
}

/// The queue a [`RouteBinding`] writes to and a `RouteHistory` drains.
///
/// Its own mutex, deliberately: it must be lockable while the history's mutex is
/// held by an in-progress flush.
pub(crate) type RouteCommandQueue = Arc<Mutex<VecDeque<RouteCommand>>>;

/// An owned, `'static` capability, pre-bound to one [`RouteId`].
///
/// A route can only ever drive *itself*: the id is baked in at construction, so
/// no route can finalize another. Cloneable and `Send + Sync`, so a route may
/// hand it to an animation status listener (U5.2).
///
/// Inert once the navigator is gone: the `wake` closure holds a `Weak`, and a
/// queued command for a route that no longer exists is dropped on drain.
#[derive(Clone)]
pub(crate) struct RouteBinding {
    route: RouteId,
    queue: RouteCommandQueue,
    /// Applies the queue if the history is not currently locked. See *Correction 1*.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl RouteBinding {
    pub(crate) fn new(
        route: RouteId,
        queue: RouteCommandQueue,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self { route, queue, wake }
    }

    /// The route this binding drives.
    pub(crate) fn route_id(&self) -> RouteId {
        self.route
    }

    /// The entrance transition finished — Flutter's `whenCompleteOrCancel`.
    ///
    /// Safe to call from inside a flush (a zero-duration transition), from an
    /// animation status listener, or from any thread.
    pub(crate) fn notify_push_completed(&self) {
        self.raise(RouteCommand::PushCompleted(self.route));
    }

    /// The route is finished; dispose it — Flutter's `navigator.finalizeRoute`.
    pub(crate) fn finalize(&self) {
        self.raise(RouteCommand::Finalize(self.route));
    }

    fn raise(&self, command: RouteCommand) {
        self.queue.lock().push_back(command);
        // Outside a flush this applies and flushes now; inside one it is a no-op
        // and the running flush drains the queue before returning.
        (self.wake)();
    }
}

impl fmt::Debug for RouteBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteBinding")
            .field("route", &self.route.get())
            .field("pending", &self.queue.lock().len())
            .finish_non_exhaustive()
    }
}

/// A route that participates in the animation seam.
///
/// Private, and it must stay that way until the public shape is signed off — see
/// *Correction 2*. U5.2's `TransitionRoute` is the intended implementor; today
/// only tests implement it, and `NavigatorHandle::push_bound` is the only door.
pub(crate) trait BoundRoute: Route {
    /// Called once, before the route is pushed and therefore before `install()`.
    fn bind(&mut self, binding: RouteBinding);
}
