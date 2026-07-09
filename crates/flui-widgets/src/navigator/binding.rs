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

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use flui_animation::{Animation, Vsync};
use parking_lot::Mutex;

use super::route::{Route, RouteId};
use crate::overlay::OverlayEntry;

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

/// What one transition route publishes about itself so the route **below** it can
/// drive its `secondary_animation`.
///
/// Flutter reads these straight off the next `Route` object
/// (`routes.dart:429-437`). FLUI's routes are named by [`RouteId`] and live behind
/// `Box<dyn ErasedRoute>` inside a `Mutex`, so a route cannot reach another —
/// ADR-0019 §7b flagged exactly this ("U5 will need a lookup handle"). The
/// registry is that handle.
#[derive(Clone)]
pub(crate) struct TransitionPeer {
    /// The route's **primary** animation, controller-backed.
    pub(crate) animation: Arc<dyn Animation<f32>>,
    /// `nextRoute.canTransitionFrom(this)` (`routes.dart:561`), asked of the
    /// route *above*.
    pub(crate) can_transition_from: bool,
    /// Fires when the route is disposed — Flutter's `Route.completed`
    /// (`routes.dart:115-122`), which `_setSecondaryAnimation` awaits to release
    /// its reference to a gone route's animation (`:503-509`).
    pub(crate) completed: Arc<CompletedSignal>,
}

/// A one-shot "this route is disposed" signal with callbacks.
///
/// Flutter uses a `Future`; FLUI's routes are driven synchronously from the flush,
/// so a plain callback list is both sufficient and observable. Private: this is
/// the `completed` channel ADR-0020 U5.2 said to add **only if** the disposal /
/// train-hopping contract needs it. It does — see `transition_route.rs`.
#[derive(Default)]
pub(crate) struct CompletedSignal {
    done: Mutex<bool>,
    listeners: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

impl CompletedSignal {
    /// Run `callback` when the route completes, or **now** if it already has.
    pub(crate) fn on_completed(&self, callback: Arc<dyn Fn() + Send + Sync>) {
        if *self.done.lock() {
            callback();
            return;
        }
        self.listeners.lock().push(callback);
    }

    /// Fire once. Later `on_completed` calls run immediately.
    pub(crate) fn complete(&self) {
        {
            let mut done = self.done.lock();
            if *done {
                return;
            }
            *done = true;
        }
        // Snapshot then fire: a callback may re-enter the route.
        let callbacks: Vec<_> = self.listeners.lock().drain(..).collect();
        for callback in callbacks {
            callback();
        }
    }
}

impl fmt::Debug for CompletedSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletedSignal")
            .field("done", &*self.done.lock())
            .finish_non_exhaustive()
    }
}

/// `RouteId -> TransitionPeer`, shared by every binding a navigator mints.
pub(crate) type TransitionRegistry = Arc<Mutex<HashMap<RouteId, TransitionPeer>>>;

/// The clock a route's `AnimationController` registers with.
///
/// **Correction to ADR-0020 Decision 1.** Flutter's `vsync: navigator!` works
/// because `NavigatorState` mixes in `TickerProviderStateMixin`. FLUI's
/// `AnimationController::new` takes an `Arc<Scheduler>` and builds its **own**
/// ticker; `flui_animation::Vsync` is not a `TickerProvider` at all but a
/// *registry* a binding drives with `tick_all`. So the seam is not "the navigator
/// is the ticker" but "the navigator owns the `Vsync` its routes register with" —
/// which preserves the property that matters: one clock per navigator, and
/// transitions freeze when the navigator's binding stops ticking.
///
/// `None` when no `VsyncScope` is above the navigator; the controller then falls
/// back to its own wall-clock ticker, exactly as `AnimatedSize` does.
pub(crate) type RouteVsync = Arc<Mutex<Option<Vsync>>>;

/// `RouteId -> OverlayEntry`, the navigator's map. A route reaches **its own**
/// entry through it — Flutter's `OverlayRoute.overlayEntries`, which FLUI keeps
/// on the navigator instead (`overlay_route.rs`).
pub(crate) type RouteEntries = Arc<Mutex<HashMap<RouteId, OverlayEntry>>>;

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
    /// The navigator's clock. `Mutex` because `NavigatorState::init_state`
    /// resolves it after the handle (and therefore any seeded binding) exists.
    vsync: RouteVsync,
    /// `RouteId -> TransitionPeer`. A **different** mutex from the history's, so a
    /// route may consult it from inside a flush.
    peers: TransitionRegistry,
    /// `RouteId -> OverlayEntry`. Likewise its own mutex (ADR-0020 U5.3).
    entries: RouteEntries,
}

impl RouteBinding {
    pub(crate) fn new(
        route: RouteId,
        queue: RouteCommandQueue,
        wake: Arc<dyn Fn() + Send + Sync>,
        vsync: RouteVsync,
        peers: TransitionRegistry,
        entries: RouteEntries,
    ) -> Self {
        Self {
            route,
            queue,
            wake,
            vsync,
            peers,
            entries,
        }
    }

    /// This route's overlay entry, or `None` before it is installed — Flutter's
    /// `overlayEntries.isNotEmpty` guard (`routes.dart:295`).
    ///
    /// Cloned **out** of the map, so the caller never holds the `entries` lock
    /// while touching the overlay.
    fn entry(&self) -> Option<OverlayEntry> {
        self.entries.lock().get(&self.route).cloned()
    }

    /// `overlayEntries.first.opaque = value` (`routes.dart:296`, `:304`).
    pub(crate) fn set_entry_opaque(&self, opaque: bool) {
        if let Some(entry) = self.entry() {
            entry.set_opaque(opaque);
        }
    }

    /// `_modalScope.maintainState = maintainState` (`routes.dart:2230`).
    pub(crate) fn set_entry_maintain_state(&self, maintain_state: bool) {
        if let Some(entry) = self.entry() {
            entry.set_maintain_state(maintain_state);
        }
    }

    /// `_modalBarrier.markNeedsBuild()` (`routes.dart:2228`) — rebuild **this
    /// route's** overlay entry, not the navigator.
    pub(crate) fn mark_entry_needs_build(&self) {
        if let Some(entry) = self.entry() {
            entry.mark_needs_build();
        }
    }

    /// The navigator's clock, if it has one.
    pub(crate) fn vsync(&self) -> Option<Vsync> {
        self.vsync.lock().clone()
    }

    /// Publish this route's primary animation so the route below can drive its
    /// `secondary_animation` from it.
    pub(crate) fn publish_peer(&self, peer: TransitionPeer) {
        self.peers.lock().insert(self.route, peer);
    }

    /// Withdraw it. Called from `dispose`; a peer that outlives its controller
    /// would hand out a disposed animation.
    pub(crate) fn withdraw_peer(&self) {
        self.peers.lock().remove(&self.route);
    }

    /// The peer for `route`, or `None` when it is not a transition route —
    /// Flutter's `nextRoute is TransitionRoute` test (`routes.dart:429`).
    pub(crate) fn peer(&self, route: RouteId) -> Option<TransitionPeer> {
        self.peers.lock().get(&route).cloned()
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
