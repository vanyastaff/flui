//! [`NavigatorObserver`] and the two observation queues.
//!
//! Private; nothing here is exported.
//!
//! # Flutter parity
//!
//! `navigator.dart:777-839` (`class NavigatorObserver`) and `:4621-4636`
//! (`_flushObserverNotifications`).
//!
//! # The queues are asymmetric, and that is not an accident
//!
//! ```dart
//! while (_observedRouteAdditions.isNotEmpty) {
//!   final observation = _observedRouteAdditions.removeLast();   // LIFO
//!   _effectiveObservers.forEach(observation.notify);
//! }
//! while (_observedRouteDeletions.isNotEmpty) {
//!   final observation = _observedRouteDeletions.removeFirst();  // FIFO
//!   _effectiveObservers.forEach(observation.notify);
//! }
//! ```
//!
//! Additions drain **last-in-first-out**, deletions **first-in-first-out**, and
//! every addition precedes every deletion. Observations are enqueued during the
//! flush's reverse walk and never fire inline. With no observers registered both
//! queues are simply cleared (`:4623-4626`), so registering an observer never
//! changes route lifecycle — only whether anyone hears about it.
//!
//! # Nothing here is notified under a lock
//!
//! [`ObservationQueues::drain`] returns the delivery order as **owned data**.
//! `RouteHistory::flush` puts it into `FlushOutcome`, and `NavigatorShared::apply`
//! hands it to [`deliver`] *after* releasing the history mutex. That is what makes
//! the [`NavigatorHandle`] an observer holds usable from `did_push`: it can read the
//! stack, and it can mutate it.
//!
//! Oracles: `test/widgets/navigator_test.dart` — `'initial route trigger observer
//! in the right order'`, `'Push and pop should trigger the observers'`.
//!
//! # Divergence: observers receive [`RouteId`], not route objects
//!
//! Flutter hands observers the `Route` itself. Handing out `&mut dyn ErasedRoute`
//! while the history holds it is not expressible, and this layer is pure data by
//! design. Ids preserve identity, ordering and arity — everything
//! the oracles assert. An observer that needs more resolves the id through the
//! [`NavigatorHandle`] it is handed at [`did_attach`] — which is what
//! `HeroController` will do.
//!
//! [`did_attach`]: NavigatorObserver::did_attach

use std::collections::VecDeque;
use std::sync::Arc;

use super::navigator::NavigatorHandle;
use super::route::RouteId;

/// Observes route-stack mutations. Flutter's `NavigatorObserver`
/// (`navigator.dart:777`). Every method has a no-op default, as there.
///
/// `&self`, not `&mut self`: an observer is shared (`Arc`) and outlives any one
/// flush. Implementations that accumulate use interior mutability, exactly as the
/// rest of the framework does behind private fields.
///
/// # The handle
///
/// [`did_attach`](Self::did_attach) hands over an owned [`NavigatorHandle`], the
/// FLUI shape of Flutter's `NavigatorObserver.navigator` getter
/// (`navigator.dart:779`, backed by an `Expando`). No `GlobalKey`, no element-tree
/// lookup: the navigator pushes the capability at the observer, in registration
/// order, from its own `init_state`.
///
/// **Every callback here runs with no navigator lock held.** The flush computes its
/// notifications as owned data and `NavigatorShared::apply` delivers them once the
/// history mutex is released, so `current()`, `route_ids()`, `can_pop()` — and even
/// `push()` / `pop()` — are all safe from `did_push` and friends. A mutation raised
/// from a callback runs a *fresh* flush whose notifications are delivered after this
/// one finishes draining; Flutter would `assert(!_debugLocked)` on the same move
/// (`navigator.dart:4452`), so treat it as defined but unusual.
///
/// The one ordering divergence: `Route::did_change_next` / `did_change_previous`
/// (Flutter's `_flushRouteAnnouncement`) now run *before* these callbacks rather
/// than between them and `did_change_top`, because they need the history borrow.
/// They are route-internal — they drive secondary animations, which no observer
/// surface exposes — so an observer sees a strictly more settled stack, never a
/// different one.
#[allow(unused_variables)]
pub trait NavigatorObserver {
    /// This observer was registered on a navigator that is now mounted.
    ///
    /// Flutter's `NavigatorObserver._navigators[observer] = this`
    /// (`navigator.dart:3836`, `:4060`, `:4121`). Called once per attachment, in
    /// registration order, with no lock held.
    ///
    /// Store the handle if you need it; it is `'static` and cloneable. It is
    /// **not** valid after [`did_detach`](Self::did_detach) — a detached handle
    /// still resolves, but names a navigator that is no longer in the tree
    /// (`NavigatorHandle::is_mounted` is then `false`).
    fn did_attach(&self, navigator: NavigatorHandle) {}

    /// The navigator this observer was attached to left the tree, or the observer
    /// was removed from it.
    ///
    /// Flutter's `NavigatorObserver._navigators[observer] = null`
    /// (`navigator.dart:4034`, `:4056`, `:4108`). Drop the handle here.
    fn did_detach(&self) {}

    /// Whether this observer is a hero controller — i.e. whether it drives hero
    /// flights off `did_change_top`.
    ///
    /// The default is `false`. `HeroController` overrides it to `true`, and a
    /// `Navigator` reads it to decide whether to auto-create its own default
    /// controller: a hand-attached controller suppresses the
    /// auto-default, so `add_observer` and automatic attach never double up. This is a
    /// self-declaration, **not** a downcast — FR-033 is untouched.
    fn observes_hero_flights(&self) -> bool {
        false
    }

    /// A route was pushed (or an initial route added).
    fn did_push(&self, route: RouteId, previous: Option<RouteId>) {}
    /// A route was popped.
    fn did_pop(&self, route: RouteId, previous: Option<RouteId>) {}
    /// A route was removed without being popped.
    fn did_remove(&self, route: RouteId, previous: Option<RouteId>) {}
    /// A route replaced another.
    fn did_replace(&self, new_route: Option<RouteId>, old_route: Option<RouteId>) {}
    /// The topmost present route changed.
    fn did_change_top(&self, top: RouteId, previous_top: Option<RouteId>) {}
}

/// One queued notification. Flutter's `_NavigatorObservation` hierarchy
/// (`navigator.dart:3690+`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    /// `_NavigatorPushObservation` — from `handle_push` and `handle_add`.
    Push {
        route: RouteId,
        previous: Option<RouteId>,
    },
    /// `_NavigatorReplaceObservation` — from `handle_push` when the previous
    /// state was `PushReplace` or `Replace`.
    Replace {
        new_route: Option<RouteId>,
        old_route: Option<RouteId>,
    },
    /// `_NavigatorPopObservation` — from the flush's `Pop` arm.
    Pop {
        route: RouteId,
        previous: Option<RouteId>,
    },
    /// `_NavigatorRemoveObservation` — from `handle_removal`, unless the route
    /// was replaced.
    Remove {
        route: RouteId,
        previous: Option<RouteId>,
    },
}

impl Observation {
    /// Whether this belongs on the additions queue (LIFO) or the deletions queue
    /// (FIFO). Mirrors which queue Flutter's producer pushes onto.
    pub(crate) fn is_addition(self) -> bool {
        matches!(self, Self::Push { .. } | Self::Replace { .. })
    }

    fn notify(self, observer: &dyn NavigatorObserver) {
        match self {
            Self::Push { route, previous } => observer.did_push(route, previous),
            Self::Replace {
                new_route,
                old_route,
            } => observer.did_replace(new_route, old_route),
            Self::Pop { route, previous } => observer.did_pop(route, previous),
            Self::Remove { route, previous } => observer.did_remove(route, previous),
        }
    }
}

/// One thing a flush decided to tell the observers, in delivery order.
///
/// `didChangeTop` is not an [`Observation`]: it never enters either queue, and it
/// fires *after* both have drained (`navigator.dart:4590-4596`). Keeping it a
/// separate variant rather than a fifth `Observation` makes enqueueing it
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Notification {
    /// A queued observation, already ordered by [`ObservationQueues::drain`].
    Observed(Observation),
    /// Flutter's `didChangeTop` (`navigator.dart:4590-4596`).
    TopChanged {
        top: RouteId,
        previous_top: Option<RouteId>,
    },
}

impl Notification {
    fn notify(self, observer: &dyn NavigatorObserver) {
        match self {
            Self::Observed(observation) => observation.notify(observer),
            Self::TopChanged { top, previous_top } => observer.did_change_top(top, previous_top),
        }
    }
}

/// Fire `notifications` at every observer, in order.
///
/// **The caller must hold no navigator lock.** `NavigatorShared::apply` is the one
/// production call site, and it runs after `history.lock()` is released — which is
/// the whole point: an observer holds a `NavigatorHandle` and would otherwise
/// deadlock on `parking_lot::Mutex` the moment it read the stack it was told about.
pub(crate) fn deliver(notifications: &[Notification], observers: &[Arc<dyn NavigatorObserver>]) {
    for notification in notifications {
        for observer in observers {
            notification.notify(observer.as_ref());
        }
    }
}

/// The pair of queues, emptied together by [`ObservationQueues::drain`].
#[derive(Default)]
pub(crate) struct ObservationQueues {
    additions: VecDeque<Observation>,
    deletions: VecDeque<Observation>,
}

impl ObservationQueues {
    pub(crate) fn enqueue(&mut self, observation: Observation) {
        if observation.is_addition() {
            self.additions.push_back(observation);
        } else {
            self.deletions.push_back(observation);
        }
    }

    /// Flutter's `_flushObserverNotifications` (`navigator.dart:4621-4636`),
    /// transcribed — but as *ordering*, not as delivery: additions LIFO, then
    /// deletions FIFO, returned as owned data so the caller can notify with no
    /// lock held. Both queues are emptied either way, so registering an observer
    /// still never changes route lifecycle (`:4623-4626`).
    pub(crate) fn drain(&mut self) -> Vec<Observation> {
        let mut ordered = Vec::with_capacity(self.additions.len() + self.deletions.len());
        while let Some(observation) = self.additions.pop_back() {
            ordered.push(observation);
        }
        while let Some(observation) = self.deletions.pop_front() {
            ordered.push(observation);
        }
        ordered
    }
}
