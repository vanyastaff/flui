//! [`NavigatorObserver`] and the two observation queues.
//!
//! ADR-0019 U2. Private; nothing here is exported.
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
//! Oracles: `test/widgets/navigator_test.dart` — `'initial route trigger observer
//! in the right order'`, `'Push and pop should trigger the observers'`.
//!
//! # Divergence: observers receive [`RouteId`], not route objects
//!
//! Flutter hands observers the `Route` itself. Handing out `&mut dyn ErasedRoute`
//! while the history holds it is not expressible, and this layer is pure data by
//! design (ADR-0019 U2). Ids preserve identity, ordering and arity — everything
//! the oracles assert. An observer that needs more resolves the id through the
//! [`NavigatorHandle`] it is handed at [`did_attach`] — which is what
//! `HeroController` will do (ADR-0021 U2, seam 2).
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
/// # The handle, and the one thing you must not do with it
///
/// [`did_attach`](Self::did_attach) hands over an owned [`NavigatorHandle`], the
/// FLUI shape of Flutter's `NavigatorObserver.navigator` getter
/// (`navigator.dart:779`, backed by an `Expando`). No `GlobalKey`, no element-tree
/// lookup: the navigator pushes the capability at the observer, in registration
/// order, from its own `init_state`.
///
/// **The stack is locked while `did_push` / `did_pop` / `did_remove` /
/// `did_replace` / `did_change_top` run.** They are called from inside
/// `RouteHistory::flush`, which holds the history's mutex, and
/// `parking_lot::Mutex` is not reentrant. Reading or mutating the stack through
/// the handle from one of those callbacks — `pop`, `current`, `route_ids`,
/// `can_pop` — **hangs**. Everything else on the handle is safe there, which is
/// exactly what `HeroController.didPush` needs (`heroes.dart:964-973`): flip a
/// route offstage, then schedule a post-frame callback and do the real work from
/// it, outside the flush.
#[allow(unused_variables)]
pub trait NavigatorObserver: Send + Sync {
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

/// The pair of queues, drained together by [`ObservationQueues::flush`].
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
    /// transcribed: additions LIFO, then deletions FIFO; both cleared without
    /// notification when there are no observers.
    pub(crate) fn flush(&mut self, observers: &[Arc<dyn NavigatorObserver>]) {
        if observers.is_empty() {
            self.additions.clear();
            self.deletions.clear();
            return;
        }

        while let Some(observation) = self.additions.pop_back() {
            for observer in observers {
                observation.notify(observer.as_ref());
            }
        }

        while let Some(observation) = self.deletions.pop_front() {
            for observer in observers {
                observation.notify(observer.as_ref());
            }
        }
    }
}
