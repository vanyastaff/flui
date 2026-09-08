//! [`Navigator`], [`NavigatorState`] and [`NavigatorHandle`].
//!
//! The private widget came first, then the signed-off
//! `Navigator` baseline was exported; public page/popup routes were added, then
//! the public Hero baseline that rides on this navigator.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/navigator.dart` (master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`): `NavigatorState`, `Navigator.of` /
//! `maybeOf` (`:2947-3001`), `canPop` (`:5551`), `maybePop` (`:5582`),
//! `_allRouteOverlayEntries` (`:4151`), and `build` returning an `Overlay`
//! (`:5984`).
//!
//! # How this avoids Flutter's two `GlobalKey`s, and the lock hazard behind them
//!
//! `BuildContext::find_ancestor_state` yields `&dyn Any` —
//! *immutable* — while the element tree is borrowed. So `Navigator::of` can never
//! return `&mut NavigatorState`, and it must not perform a second lookup inside
//! that callback: Flutter's `_overlayKey.currentState` would take the GlobalKey
//! registry's `WidgetsBinding::inner.read()` while the tree borrow is held, and
//! `parking_lot::RwLock` is not reentrant.
//!
//! Both problems dissolve the same way. `Navigator::of` clones an owned,
//! `'static` [`NavigatorHandle`] out of the state *inside* the callback and does
//! nothing else there; every mutation runs after the borrow is released. Navigator
//! and Overlay couple through an `Arc`, not through the tree — so
//! `GlobalKey<OverlayState>` is not ported either, and `navigator_uses_no_global_key`
//! keeps it that way.
//!
//! # Not implemented, and not claimed
//!
//! No Navigator 2.0/page-list API, restoration, `PopScope`,
//! `LocalHistoryRoute`, `HeroControllerScope`, `NavigationNotification`,
//! pointer-cancelling wrapper, or per-route focus scope that Flutter's `build` adds
//! (`:5946-5998`). `TransitionRoute` / `ModalRoute` stay private implementation
//! details behind public `PageRoute` / `PopupRoute`.
//!
//! Named-route generation *is* here — see the `Named routes` impl block below
//! and `named_route.rs`. What that feature deliberately leaves out is
//! `Navigator.initialRoute` / `defaultGenerateInitialRoutes` hierarchy
//! synthesis; `navigator/mod.rs` records why.

use std::any::{TypeId, type_name};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::thread::{self, ThreadId};
use std::time::Duration;

use flui_animation::Curve;
use flui_foundation::ChangeNotifier;
use flui_view::BuildContextExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::binding::{
    PopPacing, RouteBinding, RouteRegistries, RouteVsync, TransitionGroup, TransitionPeer,
};
use super::hero::{HeroRegistry, HeroScope, NestedHeroSource};
use super::hero_controller::HeroController;
use super::hero_controller_scope::HeroControllerScope;
use super::history::{DeferredEffect, FlushOutcome, ReplaceTarget, RouteHistory};
use super::modal_route::ModalHandle;
use super::named_route::{
    GeneratedRoute, KeyedSettings, NamedRouteError, PushMode, RouteKey, RouteRegistry,
    RouteRequest, requested_name,
};
use super::observer::{NavigatorObserver, Notification, deliver};
use super::overlay_route::NavigatorRoute;
use super::result::RouteResult;
use super::route::{
    AnyResult, Route, RouteId, RoutePopDisposition, RouteSettings, UndeliveredResult,
};
use super::subtree::RouteSubtree;
use crate::animated::VsyncScope;
use crate::overlay::{Overlay, OverlayEntry, OverlayHandle};

static NEXT_NAVIGATOR_COMMAND_TARGET_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static NAVIGATOR_COMMAND_TARGETS: RefCell<HashMap<NavigatorCommandTargetId, Weak<NavigatorShared>>> =
        RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct NavigatorCommandTargetId(NonZeroU64);

impl NavigatorCommandTargetId {
    fn next() -> Self {
        let raw = NEXT_NAVIGATOR_COMMAND_TARGET_ID.fetch_add(1, Ordering::Relaxed);
        let id =
            NonZeroU64::new(raw).expect("BUG: navigator command target id counter started at zero");
        Self(id)
    }
}

fn register_command_target(shared: &Arc<NavigatorShared>) -> NavigatorCommandTarget {
    let target = NavigatorCommandTarget {
        id: NavigatorCommandTargetId::next(),
        owner: thread::current().id(),
    };
    NAVIGATOR_COMMAND_TARGETS.with(|targets| {
        targets
            .borrow_mut()
            .insert(target.id, Arc::downgrade(shared));
    });
    target
}

/// Everything a [`NavigatorHandle`] and the mounted [`NavigatorState`] share.
///
/// The route stack lives behind a private `Mutex` because `ViewState::build` takes
/// `&self` and nothing can obtain `&mut NavigatorState`. That is
/// not a workaround: Flutter's `NavigatorState` mutates `_history` from `this` too.
struct NavigatorShared {
    history: Mutex<RouteHistory>,

    /// The overlay this navigator presents its routes in. Flutter reaches it
    /// through `GlobalKey<OverlayState>` (`navigator.dart:3746`); we hold the
    /// capability directly.
    overlay: OverlayHandle,

    /// The `RouteId`-keyed maps every binding shares: overlay entries, transition
    /// peers, page subtrees and modal (`offstage`) controls. Flutter reads all four
    /// straight off the `Route` object; FLUI's routes live behind
    /// `Box<dyn ErasedRoute>` inside the history's mutex, so they publish here
    /// instead (ADR-0019).
    registries: RouteRegistries,

    /// This navigator's name → route table and its two generator hooks — the
    /// three sources Flutter's `WidgetsApp._onGenerateRoute` folds into the
    /// single `Navigator.onGenerateRoute` hook (`app.dart`). Owned per
    /// navigator, so two navigators resolve the same name independently.
    named_routes: RouteRegistry,

    /// The clock this navigator's route transitions register with.
    /// Resolved from an ambient `VsyncScope` in `init_state`; `None` when there is
    /// none, in which case each controller falls back to its own wall-clock ticker.
    vsync: RouteVsync,

    /// The binding's post-frame capability and render tree, both read **once** from
    /// `NavigatorState::init_state` — a lifecycle hook, as port-check trigger #22
    /// requires. `HeroController` reaches them through its `NavigatorHandle`, which
    /// is how it schedules its measurement (`heroes.dart:968`) and then resolves the
    /// geometry that measurement was waiting for.
    ///
    /// `None` when the navigator is unmounted, or when the binding installed no
    /// post-frame handle — a `HeroController` then simply never measures, which is
    /// Flutter's `if (navigator == null) return;` (`heroes.dart:970`).
    post_frame: Mutex<Option<flui_scheduler::LocalPostFrameHandle>>,
    render_tree: Mutex<Option<flui_rendering::pipeline::PipelineCell>>,

    /// Whether the mounted `NavigatorState` currently holds the observers
    /// attached. Flutter's `NavigatorObserver._navigators[observer] != null`
    /// (`navigator.dart:779`, `:3836`), which is per-observer only because Dart
    /// has no way to ask the navigator; here it is one flag, because every
    /// observer of one navigator attaches and detaches together.
    observers_attached: AtomicBool,

    /// Flutter's `_effectiveObservers` (`navigator.dart:3769`).
    ///
    /// **Not on `RouteHistory`.** An observer holds a [`NavigatorHandle`], so
    /// notifying one is re-entrant by construction; the route stack must therefore
    /// neither own observers nor call them.
    observers: Mutex<Vec<Arc<dyn NavigatorObserver>>>,
    /// The controller the navigator auto-created when no `HeroControllerScope` was
    /// present. Kept separate so a later hand-attached `HeroController` can replace
    /// it instead of doubling the flight observer count.
    auto_hero_observer: Mutex<Option<Arc<dyn NavigatorObserver>>>,

    /// This navigator's cross-flight visibility hook, published to the nearest
    /// enclosing route's `HeroScope` from every `build` (see
    /// `NavigatorState::sync_nested_hero_registration`) — Flutter's
    /// nested-`Navigator` branch of `Hero._allHeroesFor` (`heroes.dart:317-333`).
    /// `None` for a top-level navigator, which has no enclosing route to publish
    /// to. Cleared in `dispose`, together with the registry it was registered
    /// on, so a disposed navigator's heroes are never visited again.
    nested_hero_registration: Mutex<Option<(HeroRegistry, NestedHeroSource)>>,

    /// How many overlapping user gestures (e.g. edge swipe-backs) are
    /// currently manipulating this navigator. Flutter's
    /// `NavigatorState._userGesturesInProgress` (`navigator.dart:5803`) — only
    /// the 0→1 and 1→0 transitions notify observers, so nested gestures on
    /// the same navigator collapse to one notification pair.
    ///
    /// `Arc`-wrapped so [`UserGestureSignal`] can share this exact counter
    /// with a Send+Sync context (a hero flight's data-plane animation
    /// listener) without holding the owner-affine [`NavigatorHandle`] itself.
    user_gestures_in_progress: Arc<AtomicU32>,

    /// Flutter's `userGestureInProgressNotifier` (`ValueNotifier<bool>`,
    /// `navigator.dart:5819`): fires exactly on the 0→1 and 1→0 transitions
    /// of [`user_gestures_in_progress`](Self::user_gestures_in_progress) —
    /// the same edges that notify [`NavigatorObserver::did_start_user_gesture`]
    /// / [`did_stop_user_gesture`](NavigatorObserver::did_stop_user_gesture).
    ///
    /// A pure notification relay rather than a `ValueNotifier<bool>` clone:
    /// this codebase's `ValueNotifier::clone` shares only the listener
    /// registry, not the value (each clone owns an independent `value`
    /// field), which would desync the instant it was cloned into a flight —
    /// the same [`ChangeNotifier`] idiom `ModalInner::relay` and
    /// `TransitionRouteInner::status_wake` already use to bridge a Send+Sync
    /// animation callback back to owner-local code. `HeroFlight` subscribes
    /// once, for its whole life, to replay a terminal status update parked
    /// mid-gesture (`_handleAnimationUpdate`, `heroes.dart:622-650`).
    user_gesture_in_progress_notifier: ChangeNotifier,
}

impl NavigatorShared {
    /// Apply what a flush left behind — Flutter's tail of `_flushHistoryUpdates`
    /// (`navigator.dart:4609-4613`), in that order:
    ///
    /// 1. remove each disposed route's overlay entries (`_disposeRouteEntry`);
    /// 2. `overlay.rearrange(_allRouteOverlayEntries)`, but **only** when the
    ///    flush asked for it. `pop` and `remove_route` pass `rearrangeOverlay:
    ///    false` (`:5671`, `:5747`) precisely because step 1 already updated the
    ///    overlay's list.
    fn apply(&self, mut outcome: FlushOutcome) {
        // 0. Everything the flush owed to user code, **in the order it was
        //    produced** — a multi-pass flush must not deliver a later pass's
        //    refusal ahead of an earlier pass's pop — and before the observers
        //    hear `didPop`, which is Flutter's relative order
        //    (`onPopInvokedWithResult` fires inside `handlePop`,
        //    `navigator.dart:3372`, before the observation at `:4527`).
        //
        //    With **no lock held**: these are user callbacks, they may call
        //    straight back into this navigator, and even a `can_pop()` read
        //    would deadlock on the non-reentrant history mutex.
        if !outcome.deferred.is_empty() {
            let modals = self.registries.modals.lock().clone();
            for effect in &outcome.deferred {
                let (DeferredEffect::PopInvoked(route, _)
                | DeferredEffect::LocalHistoryPopped(route)) = effect;
                let Some(modal) = modals.get(route) else {
                    continue;
                };
                match effect {
                    DeferredEffect::PopInvoked(_, did_pop) => modal.notify_pop_invoked(*did_pop),
                    DeferredEffect::LocalHistoryPopped(_) => modal.drain_local_history(),
                }
            }
        }

        // 0b. The new top route takes the keyboard: its scope becomes active and
        //     the focus it remembers is restored (`routes.dart:1692`, `:1137`).
        //     Also outside the lock — moving the focus fires user focus-change
        //     listeners, and one that calls back into this navigator would
        //     deadlock the same thread if this ran inside the flush.
        if let Some(top) =
            outcome
                .notifications
                .iter()
                .find_map(|notification| match notification {
                    Notification::TopChanged { top, .. } => Some(*top),
                    Notification::Observed(_) => None,
                })
        {
            let modal = self.registries.modals.lock().get(&top).cloned();
            if let Some(modal) = modal {
                modal.activate_focus_scope();
            }
        }

        // 1. Observers, with **no lock held** — this is the whole reason the flush
        //    returns data instead of calling them. An observer may read the stack
        //    through its `NavigatorHandle`, and may mutate it (which runs a fresh
        //    flush whose notifications land after this loop drains).
        deliver(&outcome.notifications, &self.observers());

        // 2. Each disposed route's overlay entries, then the route itself —
        //    Flutter's `_disposeRouteEntry` order (`navigator.dart:3978-3987`).
        {
            let mut entries = self.registries.entries.lock();
            for id in &outcome.disposed {
                if let Some(entry) = entries.remove(id)
                    && entry.is_attached()
                {
                    entry.remove();
                }
            }
        }
        outcome.dispose_routes();

        if !outcome.rearrange_overlay {
            return;
        }

        // `_allRouteOverlayEntries`: the entries of every route in `_history`
        // order, bottom → top (`navigator.dart:4151-4153`).
        let ordered: Vec<OverlayEntry> = {
            let ids = self.history.lock().ids();
            let entries = self.registries.entries.lock();
            ids.iter()
                .filter_map(|id| entries.get(id).cloned())
                .collect()
        };
        self.overlay.rearrange(&ordered);
    }

    /// Apply any [`RouteCommand`](super::binding::RouteCommand)s a route raised,
    /// and settle the history — the `wake` half of the route-binding seam.
    ///
    /// **`try_lock`, deliberately.** If the history mutex is held we are inside a
    /// flush on this thread (`mutate` holds it for the whole walk), and that flush
    /// drains the queue itself before returning — so there is nothing to do, and
    /// `lock()` here would deadlock rather than panic. If it is free we are
    /// between frames (an animation status listener), and the commands take
    /// effect now. See `binding.rs`, *Correction 1*.
    ///
    fn pump_route_commands(&self) {
        let outcome = {
            let Some(mut history) = self.history.try_lock() else {
                return; // A flush is running; it will drain the queue.
            };
            if !history.has_pending_commands() {
                return;
            }
            history.flush(false);
            history.take_outcome()
        };
        if let Some(outcome) = outcome {
            self.apply(outcome);
        }
    }

    /// The observers, cloned out in registration order.
    ///
    /// Cloned, not borrowed: every notification runs with this lock released too,
    /// so an observer may register another one — or drop its own `Arc` — from a
    /// callback without deadlocking on `observers`.
    fn observers(&self) -> Vec<Arc<dyn NavigatorObserver>> {
        self.observers.lock().clone()
    }

    /// Remove the auto-created hero controller, if one exists.
    ///
    /// Returns the removed observer so the caller can run `did_detach` with no
    /// `observers` lock held. Flutter's observer callbacks are user code; holding the
    /// lock would reintroduce a deadlock class this design removed.
    fn take_auto_hero_observer(&self) -> Option<Arc<dyn NavigatorObserver>> {
        let mut auto = self.auto_hero_observer.lock();
        let removed = auto.take()?;
        self.observers
            .lock()
            .retain(|observer| !Arc::ptr_eq(observer, &removed));
        Some(removed)
    }

    /// Flutter's `initState` / `activate` loop (`navigator.dart:3834-3837`,
    /// `:4118-4122`): hand every observer, in registration order, the capability
    /// it observes.
    fn attach_observers(&self, handle: &NavigatorHandle) {
        if self.observers_attached.swap(true, Ordering::Relaxed) {
            return;
        }
        for observer in self.observers() {
            observer.did_attach(handle.clone());
        }
    }

    /// Flutter's `deactivate` loop (`navigator.dart:4106-4110`), which nulls the
    /// Expando entry so `observer.navigator` reads `null` again.
    fn detach_observers(&self) {
        if !self.observers_attached.swap(false, Ordering::Relaxed) {
            return;
        }
        for observer in self.observers() {
            observer.did_detach();
        }
    }

    /// Flutter's `NavigatorState.didStartUserGesture` (`navigator.dart:5826-5841`).
    ///
    /// Only the 0→1 transition resolves the current route and notifies
    /// observers — a nested/overlapping gesture on the same navigator just
    /// bumps the count. The history lock is **released before** observers run:
    /// `did_start_user_gesture` hands out a `NavigatorHandle`, and an observer
    /// reading the stack back through it would deadlock on the same thread.
    fn did_start_user_gesture(&self) {
        let count_before = self
            .user_gestures_in_progress
            .fetch_add(1, Ordering::AcqRel);
        if count_before != 0 {
            return;
        }
        // Flutter's `ValueNotifier` setter fires before the observer loop
        // (`navigator.dart:5806`, then `:5826-5841`) — no navigator lock is
        // held at this call, so a listener that reads back through a
        // `NavigatorHandle` cannot deadlock on it.
        self.user_gesture_in_progress_notifier.notify_listeners();
        let Some((route, previous)) = self.history.lock().top_and_previous_for_gesture() else {
            return;
        };
        for observer in self.observers() {
            observer.did_start_user_gesture(route, previous);
        }
    }

    /// Flutter's `NavigatorState.didStopUserGesture` (`navigator.dart:5847-5855`).
    /// Only the 1→0 transition notifies observers.
    fn did_stop_user_gesture(&self) {
        // Saturating, not `fetch_sub`: `fetch_sub` on an unmatched call at 0
        // wraps to `u32::MAX` in release (the debug_assert below is compiled
        // out there), which would make every later `user_gesture_in_progress()`
        // read `true` forever. `try_update` with `saturating_sub` makes an
        // unmatched call a true no-op in release, exactly as the debug-only
        // assert already documents it should be.
        let count_before = self
            .user_gestures_in_progress
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                Some(count.saturating_sub(1))
            })
            .expect("BUG: the update closure always returns Some, try_update cannot fail");
        debug_assert!(
            count_before > 0,
            "BUG: did_stop_user_gesture called without a matching did_start_user_gesture"
        );
        if count_before != 1 {
            return;
        }
        // Same ordering as `did_start_user_gesture`: the notifier fires
        // before the observer loop (`navigator.dart:5806`, `:5848-5855`),
        // with no navigator lock held. A `HeroFlight` parked on this notifier
        // fires here, *before* `HeroController::did_stop_user_gesture` below
        // sweeps whatever flights are still airborne — but that reply only
        // writes the flight's terminal-status flag and wakes its shuttle
        // (`FlightInner::wake`); the actual `HeroFlight::finish` call still
        // waits for that shuttle's next `build`. So the same flight can still
        // be a live candidate for the sweep below, and may have `finish`
        // called on it twice in one turn — safe only because
        // `HeroFlight::finish` is idempotent (guarded by its own `ended`
        // flag), not because the two paths are mutually exclusive.
        self.user_gesture_in_progress_notifier.notify_listeners();
        for observer in self.observers() {
            observer.did_stop_user_gesture();
        }
    }

    /// Whether at least one user gesture is currently in progress on this
    /// navigator. Flutter's `NavigatorState.userGestureInProgress`
    /// (`navigator.dart:5816`).
    fn user_gesture_in_progress(&self) -> bool {
        self.user_gestures_in_progress.load(Ordering::Acquire) > 0
    }

    /// Run `mutate` against the stack, then apply whatever it flushed.
    ///
    /// The history lock is **released before** the overlay work, so no lock is
    /// held across `RebuildHandle::schedule`.
    fn mutate<R>(&self, mutate: impl FnOnce(&mut RouteHistory) -> R) -> R {
        let (value, outcome, undelivered) = {
            let mut history = self.history.lock();
            let value = mutate(&mut history);
            (value, history.take_outcome(), history.take_undelivered())
        };
        // Guard released. Both of the following may re-enter this navigator: a
        // `Route` lifecycle hook, and the `Drop` of a caller-supplied result.
        report_undelivered(undelivered);
        if let Some(outcome) = outcome {
            self.apply(outcome);
        }
        value
    }
}

/// Drop caller-supplied results the history could not deliver, and say so.
///
/// Two obligations, both easy to miss. **Dropped with the history guard
/// released**: an `AnyResult` wraps a value the caller supplied, so its `Drop` is
/// user code and may reach back into this navigator, which the non-reentrant
/// history mutex would deadlock on — the same hazard as dropping a registry
/// closure under its guard. And **logged**: silently evaporating is the failure
/// mode `pop_with`'s own contract exists to prevent, since the whole point of
/// that contract is that an undeliverable result is *reported*.
///
/// Reachable with no factory involved at all: `pop_with(v)` on an empty stack, or
/// on one whose top is mid-exit-transition and therefore not `is_present`.
///
/// `warn`, not `error`: unlike a type mismatch — where the caller and the route
/// disagree — this means there was no route to deliver to, which the operation's
/// own return value usually already says.
fn report_undelivered(undelivered: Vec<UndeliveredResult>) {
    for result in undelivered {
        match result {
            UndeliveredResult::NoTarget(value) => {
                tracing::warn!(
                    "a result supplied to a navigator operation reached no route and was \
                     discarded: the stack had no present route to deliver it to, or the \
                     target had already completed. Dropped outside the history lock."
                );
                drop(value);
            }
            UndeliveredResult::TypeMismatch {
                route,
                expected,
                value,
            } => {
                // The message and fields `RouteRecord::did_complete` used to emit
                // inline, moved here so neither the subscriber nor the value's
                // `Drop` runs under the history mutex.
                tracing::error!(
                    route = route.get(),
                    expected,
                    "pop result has the wrong type for this route; completed with None. \
                     Flutter throws a cast error here"
                );
                drop(value);
            }
        }
    }
}

/// A Send/Sync token naming one navigator for typed cross-thread commands.
///
/// This is deliberately not a [`NavigatorHandle`]. It carries only an opaque
/// registry id and the owner thread id; the non-`Send` route stack is resolved
/// from thread-local owner storage when a [`NavigatorCommand`] is drained by the
/// UI runtime.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NavigatorCommandTarget {
    id: NavigatorCommandTargetId,
    owner: ThreadId,
}

impl fmt::Debug for NavigatorCommandTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorCommandTarget")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .finish()
    }
}

/// A typed navigation command that may cross a thread boundary.
///
/// The command vocabulary is intentionally closed. It covers owner-thread
/// mutations that need no view/builder payload. Pushing a route remains a
/// [`NavigatorHandle`] operation because a route owns owner-local view builders
/// and cannot be made `Send` without changing the widget model.
///
/// `#[non_exhaustive]`: a route *name* is `Send`, so named-route generation is
/// what first makes this vocabulary growable — a `PushNamed { target, name }`
/// arm is now expressible where a `Push { route }` arm never was. Adding the
/// attribute is a breaking change that is free today and is not free later
/// (ADR-0024 §7.5).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigatorCommand {
    /// Pop the target navigator's top route.
    Pop {
        /// Navigator to mutate.
        target: NavigatorCommandTarget,
    },
    /// Run [`NavigatorHandle::maybe_pop`] on the target navigator.
    MaybePop {
        /// Navigator to mutate.
        target: NavigatorCommandTarget,
    },
    /// Remove a specific route from the target navigator.
    RemoveRoute {
        /// Navigator to mutate.
        target: NavigatorCommandTarget,
        /// Route to remove.
        route: RouteId,
    },
}

impl NavigatorCommand {
    /// Build a [`NavigatorHandle::pop`] command.
    #[must_use]
    pub fn pop(target: NavigatorCommandTarget) -> Self {
        Self::Pop { target }
    }

    /// Build a [`NavigatorHandle::maybe_pop`] command.
    #[must_use]
    pub fn maybe_pop(target: NavigatorCommandTarget) -> Self {
        Self::MaybePop { target }
    }

    /// Build a [`NavigatorHandle::remove_route`] command.
    #[must_use]
    pub fn remove_route(target: NavigatorCommandTarget, route: RouteId) -> Self {
        Self::RemoveRoute { target, route }
    }

    /// Apply this command on the navigator owner thread.
    ///
    /// # Errors
    ///
    /// Returns [`NavigatorCommandError::WrongOwnerThread`] when called from any
    /// other thread, and [`NavigatorCommandError::OwnerGone`] when the target was
    /// dropped or belongs to a dead owner registry.
    pub fn apply_on_owner(self) -> Result<NavigatorCommandOutcome, NavigatorCommandError> {
        let target = self.target();
        let shared = resolve_command_target(target)?;
        let handle = NavigatorHandle {
            shared,
            command_target: target,
            _owner_affine: PhantomData,
        };
        match self {
            Self::Pop { .. } => Ok(NavigatorCommandOutcome::Popped(handle.pop())),
            Self::MaybePop { .. } => Ok(NavigatorCommandOutcome::MaybePopped(handle.maybe_pop())),
            Self::RemoveRoute { route, .. } => {
                Ok(NavigatorCommandOutcome::Removed(handle.remove_route(route)))
            }
        }
    }

    fn target(&self) -> NavigatorCommandTarget {
        match self {
            Self::Pop { target } | Self::MaybePop { target } | Self::RemoveRoute { target, .. } => {
                *target
            }
        }
    }
}

/// Result of applying a typed [`NavigatorCommand`].
///
/// `#[non_exhaustive]` for the same reason as [`NavigatorCommand`]: the two
/// grow together, one outcome per command.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigatorCommandOutcome {
    /// Outcome of [`NavigatorHandle::pop`].
    Popped(bool),
    /// Outcome of [`NavigatorHandle::maybe_pop`].
    MaybePopped(bool),
    /// Outcome of [`NavigatorHandle::remove_route`].
    Removed(bool),
}

/// Why a typed navigation command could not reach its owner-local navigator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NavigatorCommandError {
    /// The command was drained outside the thread that owns the navigator.
    #[error("navigation command applied on the wrong owner thread")]
    WrongOwnerThread,
    /// The navigator or its owner registry no longer exists.
    #[error("navigation command target owner is gone")]
    OwnerGone,
}

fn resolve_command_target(
    target: NavigatorCommandTarget,
) -> Result<Arc<NavigatorShared>, NavigatorCommandError> {
    if thread::current().id() != target.owner {
        return Err(NavigatorCommandError::WrongOwnerThread);
    }

    NAVIGATOR_COMMAND_TARGETS.with(|targets| {
        let mut targets = targets.borrow_mut();
        let Some(shared) = targets.get(&target.id).and_then(Weak::upgrade) else {
            targets.remove(&target.id);
            return Err(NavigatorCommandError::OwnerGone);
        };
        Ok(shared)
    })
}

/// A Send+Sync-safe view onto a navigator's user-gesture state: the live
/// in-progress count and its change notifier, bundled.
///
/// What a data-plane animation status listener needs to defer a terminal
/// status update until a gesture ends (`_HeroFlight._handleAnimationUpdate`,
/// `heroes.dart:622-650`) without capturing the owner-affine
/// [`NavigatorHandle`] itself — `ProxyAnimation::add_status_listener` requires
/// `Send + Sync`, which `NavigatorHandle` deliberately is not (see its own
/// doc). Cloning this is cheap: both fields are `Arc`-backed.
#[derive(Clone)]
pub(crate) struct UserGestureSignal {
    in_progress: Arc<AtomicU32>,
    notifier: ChangeNotifier,
}

impl UserGestureSignal {
    /// Whether a user gesture is in progress right now — Flutter's
    /// `NavigatorState.userGestureInProgress` (`navigator.dart:5816`), read
    /// from a Send+Sync context.
    pub(crate) fn in_progress(&self) -> bool {
        self.in_progress.load(Ordering::Acquire) > 0
    }

    /// Fires on the 0→1 and 1→0 transitions of
    /// [`in_progress`](Self::in_progress). Register a listener once; it is
    /// cheap to keep for a flight's whole life.
    pub(crate) fn notifier(&self) -> &ChangeNotifier {
        &self.notifier
    }
}

/// An owned, `'static` capability to drive a [`Navigator`].
///
/// This is what `Navigator::of` returns — never `&mut NavigatorState`, which no
/// caller can obtain. Cloneable and inert once the navigator unmounts, but
/// deliberately owner-affine (`!Send + !Sync`): route storage contains view
/// builders and observer callbacks that belong to the UI owner thread. To send
/// navigation intent across threads, extract [`Self::command_target`] and enqueue
/// a [`NavigatorCommand`] through the runtime's typed command sender.
#[derive(Clone)]
pub struct NavigatorHandle {
    shared: Arc<NavigatorShared>,
    command_target: NavigatorCommandTarget,
    _owner_affine: PhantomData<Rc<()>>,
}

impl NavigatorHandle {
    /// A handle to an empty, unmounted navigator. Seed it, hand it to
    /// [`Navigator::new`], and keep a clone.
    #[must_use]
    pub fn new() -> Self {
        let shared = Arc::new(NavigatorShared {
            history: Mutex::new(RouteHistory::new()),
            overlay: OverlayHandle::new(),
            vsync: Arc::new(Mutex::new(None)),
            registries: RouteRegistries {
                peers: Arc::new(Mutex::new(HashMap::new())),
                entries: Arc::new(Mutex::new(HashMap::new())),
                subtrees: Arc::new(Mutex::new(HashMap::new())),
                modals: Arc::new(Mutex::new(HashMap::new())),
                pop_pacing: Arc::new(Mutex::new(HashMap::new())),
            },
            named_routes: RouteRegistry::default(),
            post_frame: Mutex::new(None),
            render_tree: Mutex::new(None),
            observers: Mutex::new(Vec::new()),
            auto_hero_observer: Mutex::new(None),
            observers_attached: AtomicBool::new(false),
            nested_hero_registration: Mutex::new(None),
            user_gestures_in_progress: Arc::new(AtomicU32::new(0)),
            user_gesture_in_progress_notifier: ChangeNotifier::new(),
        });
        let command_target = register_command_target(&shared);
        Self {
            shared,
            command_target,
            _owner_affine: PhantomData,
        }
    }

    /// Re-registrations that changed a name's `Output` type, and the conflict
    /// warnings actually emitted. Test-facing: `warns_emitted` is incremented in
    /// the same block that calls `tracing::warn!`, so asserting on it asserts on
    /// the warn rather than on a parallel predicate.
    #[cfg(test)]
    pub(crate) fn route_conflicts_seen(&self) -> usize {
        self.shared.named_routes.conflicts_seen()
    }

    #[cfg(test)]
    pub(crate) fn route_conflict_warns(&self) -> usize {
        self.shared.named_routes.warns_emitted()
    }

    /// How many attached observers drive hero flights — the auto-default plus any
    /// hand-attached `HeroController`s. Test-facing: pins that automatic attach adds
    /// exactly one, and that a manual controller suppresses it.
    #[cfg(test)]
    pub(crate) fn hero_observer_count(&self) -> usize {
        self.shared
            .observers
            .lock()
            .iter()
            .filter(|observer| observer.observes_hero_flights())
            .count()
    }

    /// Register an observer. Flutter's `Navigator.observers`.
    ///
    /// If the navigator is already mounted the observer is attached at once —
    /// Flutter's `didUpdateWidget` path (`navigator.dart:4058-4061`). Registered
    /// before mount, it is attached by `init_state` instead. Either way it holds a
    /// handle exactly while the navigator is mounted.
    pub fn add_observer(&self, observer: Arc<dyn NavigatorObserver>) {
        let replaced_auto = if observer.observes_hero_flights() {
            self.shared.take_auto_hero_observer()
        } else {
            None
        };
        if let Some(auto) = replaced_auto
            && self.shared.observers_attached.load(Ordering::Relaxed)
        {
            auto.did_detach();
        }

        self.shared.observers.lock().push(Arc::clone(&observer));
        if self.shared.observers_attached.load(Ordering::Relaxed) {
            observer.did_attach(self.clone());
        }
    }

    /// Deregister one previous [`add_observer`](Self::add_observer)
    /// registration of `observer` (matched by `Arc` identity), notifying it
    /// with [`NavigatorObserver::did_detach`] if the navigator is currently
    /// mounted — the deregistration half of the oracle's observer
    /// reconciliation (`NavigatorState.didUpdateWidget` /
    /// `NavigatorState.dispose` clear `NavigatorObserver._navigators`,
    /// `navigator.dart:4034`, `:4056`, `:4108`, oracle tag `3.44.0`).
    ///
    /// Crate-internal: `WidgetsApp` uses it to keep a caller-retained handle
    /// free of stale shell registrations across unmount/remount and
    /// configuration updates. A no-op when `observer` is not registered.
    ///
    /// Removing a hero-flight observer does **not** resurrect the automatic
    /// default hero controller its registration suppressed — re-add one
    /// explicitly instead (the auto-default is created only at navigator
    /// mount).
    pub(crate) fn remove_observer(&self, observer: &Arc<dyn NavigatorObserver>) {
        let removed = {
            let mut observers = self.shared.observers.lock();
            observers
                .iter()
                .position(|registered| Arc::ptr_eq(registered, observer))
                .map(|position| observers.remove(position))
        };
        // Notify outside the lock, mirroring add_observer's no-lock-held
        // attach notification.
        if let Some(removed) = removed
            && self.shared.observers_attached.load(Ordering::Relaxed)
        {
            removed.did_detach();
        }
    }

    /// Rebuild `route`'s content subtree on the next frame — the rebuild
    /// half of the oracle's `Route.changedExternalState` sweep, which
    /// `NavigatorState.didUpdateWidget` / `didChangeDependencies` run over
    /// every live route so a route whose builder reads the navigator
    /// widget's configuration re-reads it (`navigator.dart:3931-3937`,
    /// `:4055-4059`, oracle tag `3.44.0`; `ModalRoute.changedExternalState`
    /// marks the route's scope needs-build).
    ///
    /// Crate-internal: `WidgetsApp` marks its seeded home route after
    /// writing an updated `home` into the shared cell that route's builder
    /// reads. Inert before mount, after unmount, and for an unknown id.
    pub(crate) fn mark_route_needs_build(&self, route: RouteId) {
        // Cloned out, so the guard is released before `mark_needs_build` reaches
        // `RebuildHandle::schedule`. Binding a reference *into* the guard held it
        // across that call, which contradicts `NavigatorShared::mutate`'s own
        // documented promise that no lock is held across `schedule`. Nothing
        // user-supplied runs there today — this keeps the promise true rather than
        // fixing an observed failure.
        let entry = { self.shared.registries.entries.lock().get(&route).cloned() };
        if let Some(entry) = entry {
            entry.mark_needs_build();
        }
    }

    /// Whether the navigator is mounted. Flutter's `State.mounted`, consulted by
    /// `maybePop` (`navigator.dart:5595`).
    ///
    /// Derived from the overlay rather than a separate flag: the overlay is this
    /// navigator's only child, so it is mounted exactly when the navigator is.
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        self.shared.overlay.is_mounted()
    }

    /// Report that a user gesture (e.g. an edge swipe-back) started
    /// manipulating this navigator. Flutter's
    /// `NavigatorState.didStartUserGesture` (`navigator.dart:5826-5841`).
    ///
    /// Pair with a matching [`did_stop_user_gesture`](Self::did_stop_user_gesture)
    /// once the gesture settles. Calls nest: only the first call (0→1) resolves
    /// the current route and notifies [`NavigatorObserver::did_start_user_gesture`];
    /// an overlapping second call just bumps the count.
    pub fn did_start_user_gesture(&self) {
        self.shared.did_start_user_gesture();
    }

    /// Report that the gesture reported by a matching
    /// [`did_start_user_gesture`](Self::did_start_user_gesture) has finished.
    /// Flutter's `NavigatorState.didStopUserGesture` (`navigator.dart:5847-5855`).
    ///
    /// # Panics (debug only)
    ///
    /// Debug-asserts it is never called more often than
    /// [`did_start_user_gesture`](Self::did_start_user_gesture) — an unmatched
    /// call is a caller bug (mirrors Flutter's `assert(_userGesturesInProgress
    /// > 0)`).
    pub fn did_stop_user_gesture(&self) {
        self.shared.did_stop_user_gesture();
    }

    /// Whether at least one user gesture is currently in progress on this
    /// navigator. Flutter's `NavigatorState.userGestureInProgress`
    /// (`navigator.dart:5816`).
    #[must_use]
    pub fn user_gesture_in_progress(&self) -> bool {
        self.shared.user_gesture_in_progress()
    }

    /// Flutter's `userGestureInProgressNotifier` (`ValueNotifier<bool>`,
    /// `navigator.dart:5819`): fires on the 0→1 and 1→0 transitions of
    /// [`user_gesture_in_progress`](Self::user_gesture_in_progress), with no
    /// navigator lock held. Test-facing: production code reaches the same
    /// notifier only through [`user_gesture_signal`](Self::user_gesture_signal)'s
    /// Send+Sync-safe bundle.
    #[cfg(test)]
    pub(crate) fn user_gesture_in_progress_notifier(&self) -> ChangeNotifier {
        self.shared.user_gesture_in_progress_notifier.clone()
    }

    /// A Send+Sync-safe snapshot of this navigator's user-gesture state — the
    /// live count and its notifier, bundled — for a data-plane animation
    /// listener that cannot hold this owner-affine handle. `HeroFlight` uses
    /// it to defer a terminal status update mid-gesture
    /// (`_handleAnimationUpdate`, `heroes.dart:622-650`).
    pub(crate) fn user_gesture_signal(&self) -> UserGestureSignal {
        UserGestureSignal {
            in_progress: Arc::clone(&self.shared.user_gestures_in_progress),
            notifier: self.shared.user_gesture_in_progress_notifier.clone(),
        }
    }

    /// Whether both handles name the same navigator — an `Arc` identity check, for the
    /// shared-`HeroController` guard.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// A Send/Sync token for typed cross-thread navigation commands.
    ///
    /// This token cannot read or mutate the navigator by itself. It only names
    /// the navigator for a [`NavigatorCommand`] later drained on the owner thread
    /// by the UI runtime's command sender.
    #[must_use]
    pub fn command_target(&self) -> NavigatorCommandTarget {
        self.command_target
    }

    /// Seed an initial route **without flushing** — Flutter's `restoreState`
    /// (`navigator.dart:3900-3934`), which appends every route
    /// `onGenerateInitialRoutes` produced and flushes exactly once, on mount.
    ///
    /// Seed before handing the handle to [`Navigator::new`]. A deep link's
    /// synthesized back-stack is several `seed_initial` calls.
    pub fn seed_initial<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        let id = RouteId::next();
        self.bind(&route, id);
        let builder = route.content_builder();
        self.shared
            .registries
            .entries
            .lock()
            .insert(id, OverlayEntry::new(move |ctx| builder(ctx)));
        self.shared.history.lock().seed_initial_with_id(id, route)
    }

    /// Fill the route's [`RouteBindingSlot`], if it has one, before `install()`.
    ///
    /// [`RouteBindingSlot`]: super::binding::RouteBindingSlot
    fn bind<R: NavigatorRoute>(&self, route: &R, id: RouteId) {
        if let Some(slot) = route.binding_slot() {
            slot.fill(self.binding_for(id));
        }
    }

    /// Mint a [`RouteBinding`] for `route`, pre-bound to that id.
    ///
    /// The `wake` closure holds a `Weak`, so a binding that outlives its navigator
    /// is inert rather than a leak.
    ///
    fn binding_for(&self, route: RouteId) -> RouteBinding {
        let queue = self.shared.history.lock().command_queue();
        let weak: Weak<NavigatorShared> = Arc::downgrade(&self.shared);
        RouteBinding::new(
            route,
            queue,
            Rc::new(move || {
                if let Some(shared) = weak.upgrade() {
                    shared.pump_route_commands();
                }
            }),
            Arc::clone(&self.shared.vsync),
            self.shared.registries.clone(),
        )
    }

    /// Flutter's `NavigatorState.push` (`navigator.dart:5060-5063`). The future is
    /// created before any lifecycle runs.
    ///
    /// The route is bound and its overlay entry inserted **before** the flush.
    /// `install()` and a zero-duration route's first animation status change both
    /// run inside `push_with_id`, and both reach for that entry — Flutter has the
    /// same order, since `OverlayRoute.install` creates the entries and *then*
    /// calls `super.install()` (`routes.dart:69-71`).
    pub fn push<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        self.push_reporting_id(route).1
    }

    /// [`push`](Self::push), also handing back the [`RouteId`] it minted.
    ///
    /// `pub(super)` because the named-route path returns that id to its caller
    /// (`push_named` and friends answer `Result<RouteId, _>`), and the id is
    /// minted inside `push_prepared` — reading it back off
    /// [`current`](Self::current) afterwards would be inferring a fact the push
    /// already knows.
    pub(super) fn push_reporting_id<R: NavigatorRoute>(
        &self,
        route: R,
    ) -> (RouteId, RouteResult<R::Output>) {
        self.push_prepared(route, |history, id, route| {
            history.push_with_id(id, route).1
        })
    }

    /// Flutter's `NavigatorState.pushReplacement` (`navigator.dart:5245-5268`):
    /// push `route` and complete the current top **as replaced** — observers see
    /// `did_replace`, never `did_remove`, and the replaced route's future resolves
    /// with `None`.
    pub fn push_replacement<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        self.push_replacement_erased(route, None)
    }

    /// [`push_replacement`](Self::push_replacement), delivering `result` to
    /// whoever awaits the **replaced** route's [`RouteResult`] — Flutter's
    /// `pushReplacement(newRoute, result: …)`. Same delivery-time type contract as
    /// [`pop_with`](Self::pop_with).
    pub fn push_replacement_with<R: NavigatorRoute, T: Send + 'static>(
        &self,
        route: R,
        result: T,
    ) -> RouteResult<R::Output> {
        self.push_replacement_erased(route, Some(Box::new(result)))
    }

    fn push_replacement_erased<R: NavigatorRoute>(
        &self,
        route: R,
        result: Option<AnyResult>,
    ) -> RouteResult<R::Output> {
        // `CurrentTop`: nothing can run between this call and the flush, so "the
        // top now" is still the caller's route.
        self.push_replacement_erased_reporting_id(route, Some(ReplaceTarget::CurrentTop), result)
            .1
    }

    /// [`push_replacement`](Self::push_replacement) with an already-erased
    /// `result`, also handing back the new route's [`RouteId`].
    ///
    /// `pub(super)` for the named-route path, which needs both halves:
    /// `PushMode::Replace` carries a result that was erased before the route
    /// type was known, so it cannot go through the typed
    /// [`push_replacement_with`](Self::push_replacement_with) front door.
    pub(super) fn push_replacement_erased_reporting_id<R: NavigatorRoute>(
        &self,
        route: R,
        target: Option<ReplaceTarget>,
        result: Option<AnyResult>,
    ) -> (RouteId, RouteResult<R::Output>) {
        self.push_prepared(route, |history, id, route| {
            history
                .push_replacement_with_id(id, target, route, result)
                .1
        })
    }

    /// Flutter's `NavigatorState.pushAndRemoveUntil` (`navigator.dart:5347-5371`):
    /// push `route`, then walk downward from the old top completing every present
    /// route until `keep` answers `true` — the addition and all removals share one
    /// flush. Removed routes complete their futures with `None` (`:5360`) and
    /// observers see `did_remove` for each.
    ///
    /// `keep` receives each route's [`RouteId`]; `|_| false` clears everything
    /// beneath the new route. Flutter's `RoutePredicate` is handed the `Route`
    /// object itself — FLUI routes name each other by id (ADR-0019).
    ///
    /// `keep` runs with the history lock **released**: a
    /// predicate that queries the handle back (`|id| handle.route_ids().first()
    /// == Some(&id)`, the Rust shape of Flutter's `route.isFirst` /
    /// `ModalRoute.withName`) would otherwise re-enter `NavigatorShared`'s
    /// non-reentrant `parking_lot::Mutex` and deadlock the owner thread
    /// against itself. The push and the removal-completion are two separate
    /// locked sections around that unlocked evaluation; `NavigatorHandle` is
    /// owner-thread-bound, so nothing else can interleave between them.
    pub fn push_and_remove_until<R: NavigatorRoute>(
        &self,
        route: R,
        keep: impl FnMut(RouteId) -> bool,
    ) -> RouteResult<R::Output> {
        self.push_and_remove_until_reporting_id(route, keep).1
    }

    /// [`push_and_remove_until`](Self::push_and_remove_until), also handing back
    /// the new route's [`RouteId`]. `pub(super)` for the named-route path.
    pub(super) fn push_and_remove_until_reporting_id<R: NavigatorRoute>(
        &self,
        route: R,
        mut keep: impl FnMut(RouteId) -> bool,
    ) -> (RouteId, RouteResult<R::Output>) {
        let (id, (result, below_top_to_bottom)) = self
            .push_prepared(route, |history, id, route| {
                history.push_for_remove_until_with_id(id, route)
            });

        let mut remove_ids = Vec::new();
        for candidate in below_top_to_bottom {
            if keep(candidate) {
                break;
            }
            remove_ids.push(candidate);
        }

        self.shared
            .mutate(|history| history.complete_removed_and_flush(&remove_ids));

        (id, result)
    }

    /// The shared push shape: mint the id, fill the route's binding slot, insert
    /// its overlay entry, then `commit` against the locked history and apply the
    /// flush outcome. The route is bound and its entry inserted **before** the
    /// flush — `install()` and a zero-duration route's first status change both
    /// reach for the entry (`routes.dart:69-71`).
    fn push_prepared<R: NavigatorRoute, O>(
        &self,
        route: R,
        commit: impl FnOnce(&mut RouteHistory, RouteId, R) -> O,
    ) -> (RouteId, O) {
        let id = RouteId::next();
        self.bind(&route, id);

        let builder = route.content_builder();
        self.shared
            .registries
            .entries
            .lock()
            .insert(id, OverlayEntry::new(move |ctx| builder(ctx)));

        let (result, outcome, undelivered) = {
            let mut history = self.shared.history.lock();
            let result = commit(&mut history, id, route);
            (result, history.take_outcome(), history.take_undelivered())
        };

        // Guard released — the same drain `NavigatorShared::mutate` performs, and
        // needed here too because `push_replacement_with_id` runs under this lock
        // rather than that one.
        report_undelivered(undelivered);
        if let Some(outcome) = outcome {
            self.shared.apply(outcome);
        }
        (id, result)
    }

    fn pop_erased(&self, result: Option<AnyResult>) -> bool {
        self.shared.mutate(|history| history.pop(result))
    }

    fn remove_route_erased(&self, id: RouteId, result: Option<AnyResult>) -> bool {
        self.shared
            .mutate(|history| history.remove_route(id, result))
    }

    /// Pop the top route with no result — Flutter's `Navigator.pop()`
    /// (`navigator.dart:5642-5675`).
    ///
    /// The popped route's future resolves with its `current_result()` fallback, or
    /// `None`. Returns whether a present route was found. A route that refuses
    /// (`Route::did_pop` → `false`) stays, and this still returns `true`.
    pub fn pop(&self) -> bool {
        self.pop_erased(None)
    }

    /// Pop the top route, delivering `result` to whoever awaits its
    /// [`RouteResult`] — Flutter's `Navigator.pop(result)`.
    ///
    /// `T` is checked at **delivery**, not at the call site: the navigator holds a
    /// heterogeneous stack and cannot know the top route's `Output`. Passing the
    /// wrong type logs an error and completes the future with `None` rather than
    /// panicking; Flutter throws a cast error here.
    pub fn pop_with<T: Send + 'static>(&self, result: T) -> bool {
        self.pop_erased(Some(Box::new(result)))
    }

    /// Pop `route`, but drive its exit transition with `duration`/`curve`
    /// instead of its own default reverse pacing — for a gesture-driven pop
    /// whose pacing comes from the drag itself (fling velocity, or the flat
    /// "stay" pacing), not the route's static configuration. Flutter's
    /// `_CupertinoBackGestureController.dragEnd` calling
    /// `route.navigator!.pop()` while overriding `_controller.animateBack`'s
    /// duration/curve (`cupertino/route.dart`, 3.44.0).
    ///
    /// Returns `false` and pops nothing if `route` is no longer the current
    /// top route — Flutter's `!route.isCurrent` branch: a route swept away by
    /// something else between the gesture's last frame and this call must not
    /// have a stale drag finish it. Only a [`TransitionRoute`](super::transition_route::TransitionRoute)
    /// (directly, or via `ModalRoute`/`PageRoute`) consumes the pacing; a plain
    /// route's `did_pop` never asks for it.
    pub(crate) fn pop_paced(
        &self,
        route: RouteId,
        duration: Duration,
        curve: Arc<dyn Curve + Send + Sync>, // PORT-CHECK-OK-DYN: see `PopPacing`'s marker (binding.rs) — same erased-easing-curve boundary
    ) -> bool {
        if self.current() != Some(route) {
            return false;
        }
        // Both `insert` and `remove` hand back the displaced `PopPacing`, which
        // owns an `Arc<dyn Curve + Send + Sync>` — a caller-supplied value whose
        // `Drop` is user code. The guard is the first temporary in the statement,
        // so it would otherwise still be alive when that value drops. Same rule as
        // the registry's displaced closures and the history's undelivered results.
        let displaced = {
            self.shared
                .registries
                .pop_pacing
                .lock()
                .insert(route, PopPacing { duration, curve })
        };
        drop(displaced);

        let popped = self.pop_erased(None);

        // `did_pop` consumes this on a successful pop; a refused pop (or one
        // that somehow never reached `route`) must not leave a stale override
        // for a later, unrelated pop of the same route.
        let consumed = { self.shared.registries.pop_pacing.lock().remove(&route) };
        drop(consumed);
        popped
    }

    /// Remove `id` without popping it — Flutter's `Navigator.removeRoute`
    /// (`:5733-5751`).
    ///
    /// **The removed route still completes its future**, with its
    /// `current_result()` fallback or `None`. A port that completed only on `pop`
    /// would hang every awaiter.
    pub fn remove_route(&self, id: RouteId) -> bool {
        self.remove_route_erased(id, None)
    }

    /// Remove `id`, delivering `result`. Same type contract as
    /// [`pop_with`](NavigatorHandle::pop_with).
    pub fn remove_route_with<T: Send + 'static>(&self, id: RouteId, result: T) -> bool {
        self.remove_route_erased(id, Some(Box::new(result)))
    }

    /// Pop routes one at a time until `keep` accepts the route now on top —
    /// Flutter's `NavigatorState.popUntil` (`navigator.dart:5651-5660`).
    ///
    /// The route `keep` accepts is **not** popped. Unlike
    /// [`push_and_remove_until`](Self::push_and_remove_until), which removes
    /// its whole sweep in one flush, this is a bare loop over
    /// [`pop`](Self::pop): every popped route runs its full lifecycle
    /// (`did_pop`/`did_complete`/`did_pop_next`/`dispose`) and gets its own
    /// flush before the next candidate is read, exactly as repeated calls to
    /// `pop()` would. A predicate that never accepts empties the stack with
    /// no error — Flutter's `'Able to pop all routes'` regression.
    ///
    /// `keep` receives each candidate's [`RouteId`]; same shape as
    /// [`push_and_remove_until`](Self::push_and_remove_until)'s `keep`.
    ///
    /// `keep` runs with the history lock **released** — see
    /// [`push_and_remove_until`](Self::push_and_remove_until)'s doc for why a
    /// predicate that queries the handle back needs this. The candidate is
    /// read in one lock acquisition ([`current`](Self::current)), `keep`
    /// evaluates it unlocked, and a genuine pop is a second, independent
    /// acquisition ([`pop`](Self::pop)) — never the same critical section.
    pub fn pop_until(&self, mut keep: impl FnMut(RouteId) -> bool) {
        while let Some(candidate) = self.current() {
            if keep(candidate) {
                break;
            }
            self.pop();
        }
    }

    /// Flutter's `NavigatorState.canPop` (`:5551-5566`).
    #[must_use]
    pub fn can_pop(&self) -> bool {
        self.shared.history.lock().can_pop()
    }

    /// Flutter's `NavigatorState.maybePop` (`:5582-5615`), minus the deprecated
    /// `willPop` await — which is the only reason Flutter's is `async`. The
    /// remaining logic is a synchronous `switch` on `popDisposition`, and porting
    /// it as `async fn` would buy nothing and violate the no-async-in-hot-paths
    /// rule.
    ///
    /// Returns whether the pop request was **handled**. `false` means "bubble":
    /// nobody here dealt with it, so an ancestor navigator or the system should.
    fn maybe_pop_erased(&self, result: Option<AnyResult>) -> bool {
        if !self.is_mounted() {
            // "Forget about this pop, we were disposed in the meantime." (`:5595`)
            // The caller's result has nowhere to go. Reported and dropped here —
            // no guard is held yet, but the reporting is owed either way.
            report_undelivered(Vec::from_iter(result.map(UndeliveredResult::NoTarget)));
            return true;
        }

        // Disposition and the acted-on pop share **one** critical section:
        // deciding "Pop — an entry/route is there" and popping must not be
        // separated by a racing `entry_handle.remove()` or `remove_route`
        // retargeting the answer (ADR-0025). Flutter is immune only by
        // being single-threaded.
        // Three of the four arms below consume nothing, and all three run **under
        // the history guard**, so the result cannot be dropped inline: its `Drop`
        // is user code. They record into the history's own undelivered channel,
        // which `mutate` drains once the guard releases — the same path every
        // other undeliverable result takes.
        //
        // `Bubble` is not an edge case: `popDisposition` is `isFirst ? bubble :
        // pop`, so a lone route bubbles *by design*, and `maybe_pop_with` on a
        // one-route navigator took this arm every time.
        self.shared.mutate(|history| {
            let Some(disposition) = history.pop_disposition_of_top() else {
                history.record_undelivered(result);
                return false;
            };
            match disposition {
                RoutePopDisposition::Bubble => {
                    history.record_undelivered(result);
                    false
                }
                RoutePopDisposition::Pop => {
                    history.pop(result);
                    true
                }
                RoutePopDisposition::DoNotPop => {
                    history.notify_pop_refused();
                    history.record_undelivered(result);
                    true
                }
            }
        })
    }

    /// Consult the top route's `popDisposition` and act on it, with no result.
    ///
    /// Returns whether the pop request was **handled**. `false` means "bubble":
    /// nothing here dealt with it, so an ancestor navigator or the system should —
    /// which is what a lone route does (`popDisposition` is `isFirst ? bubble : pop`).
    pub fn maybe_pop(&self) -> bool {
        self.maybe_pop_erased(None)
    }

    /// [`maybe_pop`](NavigatorHandle::maybe_pop), delivering `result` if it pops.
    pub fn maybe_pop_with<T: Send + 'static>(&self, result: T) -> bool {
        self.maybe_pop_erased(Some(Box::new(result)))
    }

    /// The topmost present route.
    #[must_use]
    pub fn current(&self) -> Option<RouteId> {
        self.shared.history.lock().current()
    }

    /// The route stack, bottom → top.
    #[must_use]
    pub fn route_ids(&self) -> Vec<RouteId> {
        self.shared.history.lock().ids()
    }

    /// Whether `route` is present in this navigator's stack — Flutter's
    /// `Route.isActive`. Used by a gesture's `!isCurrent` fallback
    /// (`back_gesture.rs`): a route that has been swept off the stack
    /// entirely (not just covered) animates forward rather than trying to
    /// pop again.
    pub(crate) fn route_is_active(&self, route: RouteId) -> bool {
        self.shared.history.lock().is_present(route)
    }

    /// The lifecycle state of `id`'s entry. Test-facing.
    #[cfg(test)]
    pub(crate) fn route_state(&self, id: RouteId) -> Option<super::lifecycle::RouteLifecycle> {
        self.shared.history.lock().state_of(id)
    }

    /// The overlay entry `id`'s route presents. Test-facing: `opaque` and
    /// `maintain_state` are written through a `RouteBinding`, and this is the only
    /// way to read back what a route actually wrote.
    #[cfg(test)]
    pub(crate) fn entry_of(&self, id: RouteId) -> Option<OverlayEntry> {
        self.shared.registries.entries.lock().get(&id).cloned()
    }

    /// How many `RouteId -> OverlayEntry` pairs the navigator is holding.
    ///
    /// Test-facing. Must track the route count exactly: an entry left behind for
    /// a disposed route is invisible in the overlay (it was removed from *its*
    /// list) but leaks here, forever.
    #[cfg(test)]
    pub(crate) fn tracked_entry_count(&self) -> usize {
        self.shared.registries.entries.lock().len()
    }

    /// How many `RouteId -> RouteSubtreeCell` pairs the navigator is holding.
    ///
    /// Test-facing, and for the same reason as `tracked_entry_count`: a cell left
    /// behind for a disposed route resolves to `None` (its page is unmounted), so
    /// the leak is invisible through `route_subtree` and visible only here.
    #[cfg(test)]
    pub(crate) fn tracked_subtree_count(&self) -> usize {
        self.shared.registries.subtrees.lock().len()
    }

    /// `id`'s subtree cell, half by half. Test-facing; see
    /// [`RouteSubtreeCell::parts`](super::subtree::RouteSubtreeCell::parts).
    #[cfg(test)]
    pub(crate) fn route_subtree_parts(&self, id: RouteId) -> Option<super::subtree::SubtreeParts> {
        self.shared
            .registries
            .subtrees
            .lock()
            .get(&id)
            .map(super::subtree::RouteSubtreeCell::parts)
    }

    // ── Lookup ───────────────────────────────────────────────────────────────

    /// The nearest enclosing navigator, or `None`.
    ///
    /// Flutter's `Navigator.maybeOf(context)` (`navigator.dart:2992-3001`).
    ///
    /// Clones an owned handle out under the tree borrow and returns it; it takes
    /// no second lock and consults no `GlobalKey` registry. See the module docs.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.find_state::<NavigatorState, _>(NavigatorState::handle)
    }

    /// The **root-most** navigator — Flutter's `Navigator.of(context,
    /// rootNavigator: true)` → `findRootAncestorStateOfType<NavigatorState>()`
    /// (`navigator.dart:2947-2968`), which is how you push above every nested
    /// navigator.
    ///
    /// Flutter falls back to the local navigator when the root walk finds none;
    /// here the root walk cannot find fewer navigators than the nearest walk, so
    /// the fallback is unreachable and omitted.
    #[must_use]
    pub fn maybe_of_root(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.find_root_state::<NavigatorState, _>(NavigatorState::handle)
    }
}

/// Named routes: registration, and the six entry points that resolve a name
/// into a push.
///
/// # Registration
///
/// Flutter spreads these over two widgets — `WidgetsApp` owns the
/// `routes: Map<String, WidgetBuilder>` table and folds it, `home`, and the
/// user's `onGenerateRoute` into the single `Navigator.onGenerateRoute` hook
/// (`app.dart`, `WidgetsApp._onGenerateRoute`). FLUI's `Navigator` widget is a
/// thin shell over this handle and every push already goes through it, so all
/// three register here and `_routeNamed`'s resolution order becomes one
/// function: [`route`](Self::route) table entry →
/// [`on_generate_route`](Self::on_generate_route) →
/// [`on_unknown_route`](Self::on_unknown_route). They are mutators, not
/// constructor state, because Flutter lets a rebuilt `Navigator` swap its
/// callbacks.
///
/// When a `WidgetsApp`-level route table lands (a later slice), the contract is
/// that **the app builder replaces the table wholesale at mount and these
/// mutators serve imperative or late registration** — recorded in
/// `ARCHITECTURE.md`'s `## Mapping decisions` so two registration sites never
/// arrive with no defined winner.
///
/// # The entry points
///
/// Six untyped, mirroring Flutter's `pushNamed`, `pushReplacementNamed`,
/// `popAndPushNamed` and `pushNamedAndRemoveUntil` plus the `result:` variants
/// of the middle two, and one typed
/// [`push_named_typed`](Self::push_named_typed). Each takes an
/// `impl Into<RouteSettings>`, so a bare name needs no `RouteSettings` at the
/// call site and an argument-carrying request builds one:
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::{NavigatorHandle, RouteRequest, RouteSettings, Text};
///
/// let navigator = NavigatorHandle::new();
/// navigator.route("/details", |_request: &RouteRequest<'_>| {
///     Some(SimpleRoute::<i32>::new(|_ctx| Text::new("Details").into_view().boxed()))
/// });
/// navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
///     Text::new("Home").into_view().boxed()
/// }));
///
/// // A bare name. Note there is no `::<T>`: the untyped entry points never
/// // name the route's result type, which is why a `SimpleRoute<i32>` is
/// // reachable from a caller that has never heard of `i32`.
/// let details = navigator.push_named("/details")?;
///
/// // An argument-carrying request.
/// let with_id = navigator
///     .push_named(RouteSettings::named("/details").with_arguments(7_u32))?;
///
/// // And the one call that does name a type, to keep the pop result.
/// let result = navigator.push_named_typed::<i32>("/details")?;
/// # assert_ne!(details, with_id);
/// # let _ = result;
/// # Ok::<(), flui_widgets::NamedRouteError>(())
/// ```
///
/// `_with` on this handle means **"a result delivered to the departing route"**
/// throughout ([`pop_with`](Self::pop_with),
/// [`push_replacement_with`](Self::push_replacement_with),
/// [`remove_route_with`](Self::remove_route_with),
/// [`maybe_pop_with`](Self::maybe_pop_with)) and keeps that meaning here.
/// Arguments ride in the request, never in a `_with`.
impl NavigatorHandle {
    /// Bind `name` to a route, replacing any previous binding — one entry of
    /// Flutter's `WidgetsApp.routes` map.
    ///
    /// Typed sugar over [`on_generate_route`](Self::on_generate_route): one
    /// name maps to one route type, so the factory returns a concrete
    /// [`NavigatorRoute`] and the erasure is this method's business. `None`
    /// declines the request and passes it to the generator, then to the
    /// unknown-route fallback.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_widgets::prelude::*;
    /// use flui_widgets::{NavigatorHandle, RouteRequest, Text};
    ///
    /// let navigator = NavigatorHandle::new();
    /// navigator.route("/details", |request: &RouteRequest<'_>| {
    ///     let title = request.argument::<String>().cloned().unwrap_or_default();
    ///     Some(SimpleRoute::<()>::new(move |_ctx| {
    ///         Text::new(title.clone()).into_view().boxed()
    ///     }))
    /// });
    /// ```
    ///
    /// See [`route_keyed`](Self::route_keyed) for the variant that checks the
    /// route's result type against the name at compile time.
    pub fn route<R, F>(&self, name: impl Into<String>, factory: F)
    where
        R: NavigatorRoute,
        F: Fn(&RouteRequest<'_>) -> Option<R> + 'static,
    {
        self.shared.named_routes.register_named(
            name.into(),
            Rc::new(move |request| Some(GeneratedRoute::new(factory(request)?))),
            TypeId::of::<R::Output>(),
            type_name::<R::Output>(),
        );
    }

    /// Bind a [`RouteKey`] to a route whose `Output` the compiler checks against
    /// the key.
    ///
    /// The typed counterpart to [`route`](Self::route). `R: Route<Output = T>`
    /// is the whole point: registering a `SimpleRoute<String>` under a
    /// `RouteKey<u32>` does not compile.
    ///
    /// That is a check on *this* registration, not a guarantee about the name.
    /// See [`push_keyed`](Self::push_keyed) for the collision it cannot rule
    /// out.
    ///
    /// Registration still lands in the same name-keyed table the untyped path
    /// uses — see [`push_keyed`](Self::push_keyed) for what that means when the
    /// two paths collide on one name.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_widgets::prelude::*;
    /// use flui_widgets::{NavigatorHandle, RouteKey, RouteRequest, Text};
    ///
    /// const COUNT: RouteKey<i32> = RouteKey::new("/count");
    ///
    /// let navigator = NavigatorHandle::new();
    /// navigator.route_keyed(COUNT, |_request: &RouteRequest<'_>| {
    ///     Some(SimpleRoute::<i32>::new(|_ctx| Text::new("Count").into_view().boxed()))
    /// });
    /// ```
    ///
    /// The same registration with a route that delivers something else does not
    /// compile — this is the guarantee the whole keyed path exists for, so it is
    /// pinned here rather than described:
    ///
    /// ```compile_fail
    /// use flui_widgets::prelude::*;
    /// use flui_widgets::{NavigatorHandle, RouteKey, RouteRequest, Text};
    ///
    /// const COUNT: RouteKey<i32> = RouteKey::new("/count");
    ///
    /// let navigator = NavigatorHandle::new();
    /// // `SimpleRoute<String>` does not deliver the key's `i32`.
    /// navigator.route_keyed(COUNT, |_request: &RouteRequest<'_>| {
    ///     Some(SimpleRoute::<String>::new(|_ctx| Text::new("Count").into_view().boxed()))
    /// });
    /// ```
    ///
    /// The two examples differ in exactly one token (`i32` → `String`), which is
    /// what makes the `compile_fail` meaningful: a `compile_fail` block passes
    /// when it fails for *any* reason, so it is only evidence when a
    /// near-identical block above it compiles.
    pub fn route_keyed<T, R, F>(&self, key: RouteKey<T>, factory: F)
    where
        R: NavigatorRoute + Route<Output = T>,
        F: Fn(&RouteRequest<'_>) -> Option<R> + 'static,
    {
        self.route(key.name(), factory);
    }

    /// Install the catch-all route generator — Flutter's
    /// `Navigator.onGenerateRoute`.
    ///
    /// Consulted for every name the [`route`](Self::route) table did not
    /// answer. It cannot be generic over one route type the way `route` is —
    /// one closure must be able to answer different names with differently
    /// typed routes — so it returns an erased [`GeneratedRoute`] and the caller
    /// writes `GeneratedRoute::new(..)`.
    ///
    /// # Do not capture a handle, and do not navigate from here
    ///
    /// A factory builds a route; it is not given a [`NavigatorHandle`] and should
    /// not capture one. To send the user elsewhere, **return the route for
    /// elsewhere** — see [`RouteRequest`]. A route's *content* needs no captured
    /// handle either: a
    /// [`RouteContentBuilder`](super::overlay_route::RouteContentBuilder) gets a
    /// `&dyn BuildContext` and [`maybe_of`](Self::maybe_of) resolves from it,
    /// exactly as Flutter's `Navigator.of(context)` does.
    ///
    /// Capturing one anyway is possible — closures capture freely — and costs two
    /// things. It closes an `Arc` cycle (the navigator owns the registry, the
    /// registry owns the closure, the closure would own the navigator) which
    /// nothing reclaims implicitly, because registrations are deliberately **not**
    /// mount-scoped; [`clear_routes`](Self::clear_routes) is the only thing that
    /// breaks it. And navigating from a factory puts a mutation inside another
    /// operation's resolution: survivable, documented, and not supported — see
    /// `ARCHITECTURE.md` §5 for the two observable consequences.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_widgets::prelude::*;
    /// use flui_widgets::{GeneratedRoute, NavigatorHandle, RouteRequest, Text};
    ///
    /// let navigator = NavigatorHandle::new();
    /// navigator.on_generate_route(|request: &RouteRequest<'_>| match request.name()? {
    ///     "/count" => Some(GeneratedRoute::new(SimpleRoute::<i32>::new(|_ctx| {
    ///         Text::new("Count").into_view().boxed()
    ///     }))),
    ///     _ => None,
    /// });
    /// ```
    pub fn on_generate_route(
        &self,
        factory: impl Fn(&RouteRequest<'_>) -> Option<GeneratedRoute> + 'static,
    ) {
        self.shared
            .named_routes
            .register_generator(Rc::new(factory));
    }

    /// Install the last-resort fallback — Flutter's `Navigator.onUnknownRoute`,
    /// consulted only when neither the table nor the generator produced a
    /// route. It receives the same [`RouteSettings`] they were offered.
    ///
    /// Returning `None` here is the end of the line: the entry point answers
    /// [`NamedRouteError::Unresolved`]. The capture hazard
    /// [`on_generate_route`](Self::on_generate_route) documents applies here too.
    pub fn on_unknown_route(
        &self,
        factory: impl Fn(&RouteRequest<'_>) -> Option<GeneratedRoute> + 'static,
    ) {
        self.shared
            .named_routes
            .register_unknown_fallback(Rc::new(factory));
    }

    /// Drop every route registration — the table, the generator, and the
    /// unknown-route fallback.
    ///
    /// Registrations are **not** mount-scoped: they survive an unmount and
    /// remount over a retained handle, because an app that registers once
    /// against a handle it keeps must not silently lose its routes
    /// (`ARCHITECTURE.md` §6 draws the contrast with observers, which *are*
    /// mount-scoped because an observer holds a handle only while mounted).
    /// Dropping them is therefore a caller decision — only the caller knows
    /// whether it intends to register again — and this is how the caller makes
    /// it.
    ///
    /// It is also the escape for the one cycle this surface can still create:
    /// a factory that captures a [`NavigatorHandle`] despite
    /// no accessor offering one. See
    /// [`on_generate_route`](Self::on_generate_route).
    pub fn clear_routes(&self) {
        self.shared.named_routes.clear();
    }

    /// Dismiss `route` on behalf of an operation that captured it **before**
    /// resolving a name.
    ///
    /// Named operations resolve first, so an unresolvable name adds nothing of
    /// the operation's own —
    /// but resolving runs a user factory, and a factory that captured a handle can
    /// navigating from one a supported shape. By the time the departing route is
    /// dealt with it may no longer be on top, or may be gone. Acting on "the
    /// current top" would then dismiss the *factory's* route and leave the
    /// caller's in place, which is the bug this exists to prevent.
    ///
    /// Three cases, all deliberate:
    ///
    /// - still on top — the ordinary [`pop`](Self::pop) path, so the observer
    ///   sees `didPop` and the route's own `did_pop` may refuse. This is what
    ///   every non-re-entrant call takes, unchanged;
    /// - present but buried under something the factory pushed — removed by id,
    ///   which is the honest operation for a route that is no longer the top;
    /// - already gone (the factory popped it) — nothing to do, and **not** an
    ///   error: a factory's unrelated navigation must not fail an operation that
    ///   otherwise succeeded.
    fn dismiss_captured(&self, route: Option<RouteId>, result: Option<AnyResult>) {
        let Some(route) = route else {
            // Nothing was captured — an empty stack, or a top mid-exit-transition,
            // which is a reachable and documented state. The result must **not**
            // go to whatever a factory left on top; that is what the capture is
            // for. So it is reported and dropped, through the same path as every
            // other undeliverable result.
            report_undelivered(Vec::from_iter(result.map(UndeliveredResult::NoTarget)));
            return;
        };
        if self.current() == Some(route) {
            self.pop_erased(result);
        } else {
            self.remove_route_erased(route, result);
        }
    }

    /// Resolve `request` into a route, or say why it could not be.
    ///
    /// # Errors
    ///
    /// [`NamedRouteError::Unresolved`] when neither the table, the generator,
    /// nor the unknown-route fallback produced a route.
    fn resolve_named(&self, settings: &RouteSettings) -> Result<GeneratedRoute, NamedRouteError> {
        let request = RouteRequest::new(settings);
        self.shared
            .named_routes
            .resolve(&request)
            .ok_or_else(|| NamedRouteError::Unresolved {
                name: requested_name(settings),
            })
    }

    /// Resolve `request` and [`push`](Self::push) the route it names — Flutter's
    /// `NavigatorState.pushNamed`.
    ///
    /// Returns the new route's [`RouteId`], which pairs with
    /// [`current`](Self::current) and feeds [`remove_route`](Self::remove_route).
    /// The route's own pop result is **dropped**: a caller navigating to a screen
    /// has no reason to know what type that screen completes with, and Flutter's
    /// `pushNamed<void>` behaves the same way. Reach for
    /// [`push_named_typed`](Self::push_named_typed) when you want the result.
    ///
    /// Everything below the name layer is the unnamed path: this calls
    /// [`push`](Self::push), so one history state machine and one observer
    /// ordering serve both.
    ///
    /// # Errors
    ///
    /// [`NamedRouteError::Unresolved`] if no registration answered the name.
    /// This operation pushes nothing, and a stage that declines returns `None`
    /// before constructing anything, so the error never has a route to dispose.
    ///
    /// It does **not** promise the stack is unchanged: a factory that navigates
    /// (through a captured handle) and then declines has already made its
    /// own changes, and those are not rolled back — they were deliberate, and
    /// undoing them is no more this operation's business than undoing a nested
    /// push is (see [`pop_and_push_named`](Self::pop_and_push_named)).
    pub fn push_named(
        &self,
        request: impl Into<RouteSettings>,
    ) -> Result<RouteId, NamedRouteError> {
        let request = request.into();
        Ok(self.resolve_named(&request)?.push(self, PushMode::Push).0)
    }

    /// [`push_named`](Self::push_named), keeping the new route's typed
    /// [`RouteResult`] instead of dropping it.
    ///
    /// # Choosing between this and [`push_keyed`](Self::push_keyed)
    ///
    /// They divide by **when the name exists**, not by preference.
    /// [`RouteKey::new`] takes a `&'static str`, so a name computed at run time —
    /// a deep link, a config value, a server-supplied path — cannot be a key, and
    /// this is the only typed push that can reach the
    /// [`on_generate_route`](Self::on_generate_route) hook such names resolve
    /// through. For a name known at compile time prefer
    /// [`push_keyed`](Self::push_keyed): the key checks the route's result type at
    /// the *registration* site rather than at the push.
    ///
    /// This is the one named entry point that can fail on the result type, and
    /// it fails **before** anything is pushed: the generated route is concrete
    /// and carries its own `Output`, so `T` is checked against a `TypeId` the
    /// carrier captured at construction and a mismatch leaves the stack
    /// untouched. Flutter re-types through an unchecked `as Route<T?>?` and
    /// never detects the mismatch.
    ///
    /// # Errors
    ///
    /// [`NamedRouteError::Unresolved`] if no registration answered the name, and
    /// [`NamedRouteError::ResultType`] if the route generated for it delivers
    /// something other than `T`. Neither pushes anything; on `ResultType` the
    /// generated route is disposed, while `Unresolved` never constructed one.
    /// Neither undoes navigation a factory performed before declining — see
    /// [`push_named`](Self::push_named).
    pub fn push_named_typed<T: Send + 'static>(
        &self,
        request: impl Into<RouteSettings>,
    ) -> Result<RouteResult<T>, NamedRouteError> {
        let request = request.into();
        Ok(self
            .resolve_named(&request)?
            .checked::<T>(&request)?
            .push(self, PushMode::Push)
            .1)
    }

    /// Push the route a [`RouteKey`] names, keeping its typed [`RouteResult`].
    ///
    /// The typed push for a name known at compile time: `T` comes from the key
    /// rather than from a turbofish, and [`route_keyed`](Self::route_keyed) already
    /// refused a mismatched route at compile time. It does **not** make
    /// [`NamedRouteError::ResultType`] unreachable — see below.
    ///
    /// # Choosing between this and [`push_named_typed`](Self::push_named_typed)
    ///
    /// They divide by **when the name exists**, and neither substitutes for the
    /// other. [`RouteKey::new`] takes a `&'static str`, so this cannot express a
    /// name computed at run time — a deep link, a config value, a server-supplied
    /// path — and those resolve through
    /// [`on_generate_route`](Self::on_generate_route), which only
    /// [`push_named_typed`](Self::push_named_typed) can reach with a type. Prefer
    /// this one wherever the name *is* a literal: the check moves from the push to
    /// the registration site, where the mistake is actually made.
    ///
    /// A bare key is a request; [`RouteKey::with_arguments`] adds arguments, and
    /// [`KeyedSettings::untyped`] converts one for an untyped operation:
    ///
    /// ```
    /// use flui_widgets::prelude::*;
    /// use flui_widgets::{NavigatorHandle, RouteKey, RouteRequest, Text};
    ///
    /// const ORDER: RouteKey<u32> = RouteKey::new("/order");
    ///
    /// let navigator = NavigatorHandle::new();
    /// navigator.route_keyed(ORDER, |request: &RouteRequest<'_>| {
    ///     let id = request.argument::<u32>().copied().unwrap_or(0);
    ///     Some(SimpleRoute::<u32>::new(move |_ctx| {
    ///         Text::new(format!("Order {id}")).into_view().boxed()
    ///     }))
    /// });
    /// navigator.seed_initial(SimpleRoute::<u32>::new(|_ctx| {
    ///     Text::new("Home").into_view().boxed()
    /// }));
    ///
    /// let plain = navigator.push_keyed(ORDER)?;
    /// let with_id = navigator.push_keyed(ORDER.with_arguments(1776_u32))?;
    /// # let _ = (plain, with_id);
    /// # Ok::<(), flui_widgets::NamedRouteError>(())
    /// ```
    ///
    /// # What the key cannot promise: the registry is name-keyed
    ///
    /// A [`RouteKey`] type-checks one *registration site*. The table it
    /// registers into is keyed by **name**, so any two sites that disagree about
    /// one name still meet at run time, and the key's promise stops describing
    /// what is registered. Three ways in, none of them a mistake the compiler
    /// can see:
    ///
    /// - two [`RouteKey`]s spelling the same string with different `T`, each
    ///   self-consistent — `RouteKey::<u32>::new("/order")` and
    ///   `RouteKey::<String>::new("/order")` both compile, and the second
    ///   registration replaces the first;
    /// - the same name bound through the untyped [`route`](Self::route);
    /// - [`on_generate_route`](Self::on_generate_route) answering the name when
    ///   the table misses.
    ///
    /// The `TypeId` check is kept for all three. It guards against *two
    /// registration sites disagreeing about one name* — which a keyed-only
    /// codebase can absolutely do.
    ///
    /// # What the guard buys
    ///
    /// **A pre-mutation, non-panicking failure.** Not type safety: the
    /// `RouteResult` downcast underneath is checked, so a silently wrong result
    /// was never reachable. Without the guard the push lands *first* and the
    /// downcast then fails a `BUG:` `expect` — a panic, mid-operation, with the
    /// stack already mutated. With it you get [`NamedRouteError::ResultType`]
    /// and an untouched stack.
    ///
    /// # Errors
    ///
    /// [`NamedRouteError::Unresolved`] if nothing answered the key's name;
    /// [`NamedRouteError::ResultType`] in the collision cases above. Neither
    /// mutates the stack.
    pub fn push_keyed<T: Send + 'static>(
        &self,
        request: impl Into<KeyedSettings<T>>,
    ) -> Result<RouteResult<T>, NamedRouteError> {
        self.push_named_typed::<T>(request.into().into_settings())
    }

    /// Resolve `request` and [`push_replacement`](Self::push_replacement) —
    /// Flutter's `NavigatorState.pushReplacementNamed`. The replaced route's
    /// [`RouteResult`] resolves with `None`.
    ///
    /// **Better than the reference, and the accounting for it.** Flutter's
    /// `pushReplacementNamed` is
    /// `pushReplacement<T?, TO>(_routeNamed<T>(..)!, result: result)`: the
    /// generator runs in argument position, before `pushReplacement`, and a Dart
    /// factory reaching `Navigator.of(context)` can move the top in between
    /// exactly as ours can. Flutter then replaces whatever is on top *now*,
    /// which is the factory's route rather than the caller's. FLUI captures the
    /// route to replace **before** resolving, so it replaces the one the caller
    /// meant. Pinned by
    /// `push_replacement_named_replaces_the_route_that_was_current_when_it_was_called`.
    ///
    /// # Errors
    ///
    /// As [`push_named`](Self::push_named); nothing is replaced.
    pub fn push_replacement_named(
        &self,
        request: impl Into<RouteSettings>,
    ) -> Result<RouteId, NamedRouteError> {
        let request = request.into();
        // Captured before resolving. An empty capture is `None` — no target,
        // completes nothing — which `ReplaceTarget` cannot express and so cannot
        // be mistaken for "the current top".
        let target = self.current().map(ReplaceTarget::Route);
        Ok(self
            .resolve_named(&request)?
            .push(
                self,
                PushMode::Replace {
                    target,
                    result: None,
                },
            )
            .0)
    }

    /// [`push_replacement_named`](Self::push_replacement_named), delivering
    /// `result` to whoever awaits the **replaced** route — Flutter's
    /// `pushReplacementNamed(routeName, result: …)`.
    ///
    /// `result` carries the same delivery-time type contract as
    /// [`pop_with`](Self::pop_with): the navigator cannot know the replaced
    /// route's `Output`, so a mismatch logs and completes that route with
    /// `None`.
    ///
    /// # Errors
    ///
    /// As [`push_named`](Self::push_named). On error nothing is replaced and
    /// `result` is dropped undelivered.
    pub fn push_replacement_named_with<TO: Send + 'static>(
        &self,
        request: impl Into<RouteSettings>,
        result: TO,
    ) -> Result<RouteId, NamedRouteError> {
        let request = request.into();
        let target = self.current().map(ReplaceTarget::Route);
        Ok(self
            .resolve_named(&request)?
            .push(
                self,
                PushMode::Replace {
                    target,
                    result: Some(Box::new(result)),
                },
            )
            .0)
    }

    /// [`pop`](Self::pop) the current route and push the one `request` names —
    /// Flutter's `NavigatorState.popAndPushNamed`. Unlike
    /// [`push_replacement_named`](Self::push_replacement_named) the departing
    /// route runs its full exit transition.
    ///
    /// **Documented divergence: this resolves before it pops.** Flutter's
    /// `popAndPushNamed` is `pop<TO>(result); return pushNamed<T>(..)` — it pops
    /// first, so a name its generator declines leaves the stack already mutated
    /// and throws from the middle. Ordering the two the other way makes the
    /// failure total: this operation pops nothing and pushes nothing. (A factory
    /// that navigated before declining keeps its own changes — see
    /// [`push_named`](Self::push_named).)
    ///
    /// **What that divergence costs, stated rather than claimed as pure gain.**
    /// Resolving first means a factory runs before the departing route is dealt
    /// with, and a factory that captured a handle can navigate — an unsupported but
    /// supported shape. So when a factory navigates, its `didPush` is observed
    /// **before** this operation's own dismissal — an ordering Flutter cannot
    /// produce here, because it has already popped. The departing route is then
    /// buried, and is removed by id rather than popped (`didRemove`, not
    /// `didPop`). Pinned by
    /// `a_re_entrant_factory_is_observed_before_the_pop_it_precedes`.
    ///
    /// When no factory navigates — every ordinary call — the pop and the push
    /// are the same two calls in the same order and the stream is identical.
    ///
    /// The route dismissed is the one that was current **when this was called**,
    /// captured before resolving — not whatever a factory left on top. A route
    /// the factory already popped is simply not there to dismiss, and that is a
    /// success, not an error.
    ///
    /// # Errors
    ///
    /// As [`push_named`](Self::push_named); on error, nothing is popped.
    pub fn pop_and_push_named(
        &self,
        request: impl Into<RouteSettings>,
    ) -> Result<RouteId, NamedRouteError> {
        let request = request.into();
        let departing = self.current();
        let generated = self.resolve_named(&request)?;
        self.dismiss_captured(departing, None);
        Ok(generated.push(self, PushMode::Push).0)
    }

    /// [`pop_and_push_named`](Self::pop_and_push_named), delivering `result` to
    /// whoever awaits the **popped** route — Flutter's
    /// `popAndPushNamed(routeName, result: …)`. Same delivery-time contract for
    /// `result` as [`pop_with`](Self::pop_with), and the same resolve-before-pop
    /// divergence.
    ///
    /// # Errors
    ///
    /// As [`push_named`](Self::push_named). On error nothing is popped and
    /// `result` is dropped undelivered.
    pub fn pop_and_push_named_with<TO: Send + 'static>(
        &self,
        request: impl Into<RouteSettings>,
        result: TO,
    ) -> Result<RouteId, NamedRouteError> {
        let request = request.into();
        let departing = self.current();
        let generated = self.resolve_named(&request)?;
        self.dismiss_captured(departing, Some(Box::new(result)));
        Ok(generated.push(self, PushMode::Push).0)
    }

    /// Resolve `request` and
    /// [`push_and_remove_until`](Self::push_and_remove_until) — Flutter's
    /// `NavigatorState.pushNamedAndRemoveUntil`.
    ///
    /// `keep` receives each candidate's [`RouteId`] and runs with the history
    /// lock released, exactly as
    /// [`push_and_remove_until`](Self::push_and_remove_until)'s does.
    ///
    /// **This one needs no captured target, and that is structural — do not
    /// "harmonise" it with its siblings.** Its removal is defined by the
    /// *predicate*, not by a route captured before resolving: the sweep runs
    /// downward from the newly pushed route until `keep` accepts, so anything a
    /// re-entrant factory pushed is swept along with everything else above the
    /// kept route. Its siblings capture a target precisely because they name
    /// "the current top", which a factory can change underneath them.
    ///
    /// # Errors
    ///
    /// As [`push_named`](Self::push_named); on error nothing is pushed and
    /// nothing is removed — `keep` is never consulted.
    pub fn push_named_and_remove_until(
        &self,
        request: impl Into<RouteSettings>,
        mut keep: impl FnMut(RouteId) -> bool,
    ) -> Result<RouteId, NamedRouteError> {
        let request = request.into();
        Ok(self
            .resolve_named(&request)?
            .push(self, PushMode::RemoveUntil { keep: &mut keep })
            .0)
    }
}

/// The introspection seams: everything `HeroController` reads that is
/// not already on the public surface.
///
/// Each method is one thing Flutter reads straight off a `Route` object or off
/// `NavigatorState` — neither of which FLUI can reach, because routes live behind
/// `Box<dyn ErasedRoute>` inside the history's mutex. Nothing here
/// hands out a borrow into the trees, and nothing takes a second lock under a
/// first.
///
/// A `HeroController` is attached to exactly one navigator and drives flights only
/// between *that* navigator's own route changes — these methods still answer only
/// about *this* navigator's stack. Cross-navigator flights still work: a hero inside
/// a nested `Navigator`'s current `PageRoute` is reachable through a
/// `NestedHeroSource`, which this navigator publishes on the nearest enclosing route
/// from every `build` (`heroes.dart:317-333`'s nested-`Navigator` branch), not
/// through anything read here.
impl NavigatorHandle {
    /// Whether `id` names a [`TransitionGroup::Page`] route — Flutter's
    /// `route is PageRoute` (`heroes.dart:331`, `:941-948`). Shared by
    /// `HeroController::maybe_start`'s own eligibility test and by a nested
    /// `Navigator`'s [`NestedHeroSource`] hook, which asks the identical question
    /// about its own current top route.
    pub(crate) fn is_page_route(&self, id: RouteId) -> bool {
        self.route_peer(id)
            .is_some_and(|peer| peer.group == TransitionGroup::Page)
    }

    /// This navigator's overlay — Flutter's `NavigatorState.overlay`, read by
    /// `HeroController._startHeroTransition` (`heroes.dart:990`) to insert the
    /// flight's `OverlayEntry`.
    ///
    /// `pub(crate)`: `Overlay` and `OverlayEntry` stay
    /// unexported, so this widens no public surface.
    pub(crate) fn overlay(&self) -> &OverlayHandle {
        &self.shared.overlay
    }

    /// What `id` publishes about its transition — its primary animation, and the
    /// family it transitions with.
    ///
    /// Flutter reads `route.animation` and tests `route is PageRoute`
    /// (`heroes.dart:331`, `:941-948`). `None` for a route that is not a
    /// `TransitionRoute`, matching `nextRoute is TransitionRoute`
    /// (`routes.dart:429`).
    pub(crate) fn route_peer(&self, id: RouteId) -> Option<TransitionPeer> {
        self.shared.registries.peers.lock().get(&id).cloned()
    }

    /// Where `id`'s page subtree lives — Flutter's `route.subtreeContext`
    /// (`routes.dart:1966`).
    ///
    /// `None` unless the route is a `ModalRoute` whose page is **mounted and
    /// attached**. Resolving to `Some` says nothing about layout: ask
    /// [`PipelineOwner::box_size`] for that, which is `None` until the first
    /// layout commits. See `subtree.rs`.
    ///
    /// [`PipelineOwner::box_size`]: flui_rendering::pipeline::PipelineOwner::box_size
    pub(crate) fn route_subtree(&self, id: RouteId) -> Option<RouteSubtree> {
        self.shared.registries.subtrees.lock().get(&id)?.resolve()
    }

    /// Flutter's `Route.isCurrent` (`routes.dart:196-201`), read by
    /// `Hero._allHeroesFor`'s route guard (`heroes.dart:331`).
    ///
    /// Test-facing: `did_change_top` no longer asserts on it (the over-strict
    /// `is_current` check was removed because FLUI's re-entrant notification model
    /// breaks it).
    #[cfg(test)]
    pub(crate) fn is_current(&self, id: RouteId) -> bool {
        self.current() == Some(id)
    }

    /// `id`'s `offstage` control and animation proxies — Flutter reads them off the
    /// `Route` object (`routes.dart:1951`, `:1969`, `:1973`).
    ///
    /// `None` for a route that is not a `ModalRoute`, or one already disposed.
    pub(crate) fn route_modal(&self, id: RouteId) -> Option<ModalHandle> {
        self.shared.registries.modals.lock().get(&id).cloned()
    }

    /// Whether `route` may start an edge-swipe-back gesture right now.
    /// Flutter's `PageRoute.popGestureEnabled` (`pages.dart:63-66`) composed
    /// with `super.popGestureEnabled` = `ModalRoute.popGestureEnabled`
    /// (`routes.dart:1908-1930`): not the first (present) route, does not
    /// handle its own pop, the pop disposition allows it (no `PopScope` veto,
    /// no `WillPopScope` — FLUI has none, so that half is vacuously clear),
    /// and the primary animation has finished (`animation!.isCompleted`).
    /// `fullscreenDialog` is not ported (see `PageRoute::back_gesture`'s
    /// doc), so that half of `PageRoute`'s own override is skipped.
    ///
    /// **FLUI addition, not in the oracle text:** also `false` while a user
    /// gesture is already in progress on this navigator — the per-pointer-down
    /// predicate must not admit a second, overlapping gesture start.
    pub(crate) fn pop_gesture_enabled(&self, route: RouteId) -> bool {
        if self.user_gesture_in_progress() {
            return false;
        }
        // `popDisposition == RoutePopDisposition.doNotPop` in the oracle
        // reads *this* route's own `popDisposition` (`ModalRoute
        // .popDisposition`, `routes.dart`), not the top of the stack —
        // `RouteHistory::vetoes_pop` is that per-route check. See its doc.
        let (is_first, will_handle_pop_internally, vetoes_pop) = {
            let history = self.shared.history.lock();
            (
                history.first_present() == Some(route),
                history.will_handle_pop_internally(route),
                history.vetoes_pop(route),
            )
        };
        if is_first || will_handle_pop_internally != Some(false) || vetoes_pop != Some(false) {
            return false;
        }
        self.route_modal(route)
            .is_some_and(|modal| modal.primary_animation().status().is_completed())
    }

    /// The binding's post-frame capability — `WidgetsBinding.instance
    /// .addPostFrameCallback` (`heroes.dart:968`), as an owned handle.
    ///
    /// `None` before mount and after unmount, so a stale `HeroController` schedules
    /// nothing. Acquired in `init_state`; never in `build`/layout/paint (trigger #22).
    pub(crate) fn local_post_frame_handle(&self) -> Option<flui_scheduler::LocalPostFrameHandle> {
        self.shared.post_frame.lock().clone()
    }

    /// The render tree this navigator is mounted in, for resolving the `RenderId`s
    /// [`route_subtree`](Self::route_subtree) hands out — Flutter reaches it through
    /// `navigator.context.findRenderObject()` (`heroes.dart:999`).
    ///
    /// `None` before mount and after unmount.
    pub(crate) fn render_tree(&self) -> Option<flui_rendering::pipeline::PipelineCell> {
        self.shared.render_tree.lock().clone()
    }
}

impl Default for NavigatorHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NavigatorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorHandle")
            .field("routes", &self.shared.history.lock().len())
            .field("mounted", &self.is_mounted())
            .field("command_target", &self.command_target)
            .finish()
    }
}

// ============================================================================
// THE VIEW
// ============================================================================

/// A stack of routes, presented in an overlay.
///
/// The stack lives in the [`NavigatorHandle`] the caller supplies, so it survives
/// this view being rebuilt and can be driven from outside the tree.
#[derive(Clone)]
pub struct Navigator {
    handle: NavigatorHandle,
}

impl Navigator {
    /// A navigator backed by `handle`. Seed its initial route(s) first.
    #[must_use]
    pub fn new(handle: NavigatorHandle) -> Self {
        Self { handle }
    }
}

impl fmt::Debug for Navigator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigator").finish_non_exhaustive()
    }
}

impl View for Navigator {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Navigator {
    type State = NavigatorState;

    fn create_state(&self) -> Self::State {
        NavigatorState {
            shared: Arc::clone(&self.handle.shared),
            command_target: self.handle.command_target,
        }
    }
}

/// Persistent state for [`Navigator`]. Flutter's `NavigatorState`.
///
/// Holds nothing of its own: the stack and the overlay live behind the shared
/// `Arc`, because they must be reachable from an owned handle that outlives any
/// borrow of this state.
pub struct NavigatorState {
    shared: Arc<NavigatorShared>,
    command_target: NavigatorCommandTarget,
}

impl fmt::Debug for NavigatorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorState")
            .field("routes", &self.shared.history.lock().len())
            .finish_non_exhaustive()
    }
}

impl NavigatorState {
    /// The owned capability `Navigator::of` hands out. **This is the only thing a
    /// lookup takes from the state**, and it is a clone of two `Arc`s.
    fn handle(&self) -> NavigatorHandle {
        NavigatorHandle {
            shared: Arc::clone(&self.shared),
            command_target: self.command_target,
            _owner_affine: PhantomData,
        }
    }

    /// Keep this navigator's cross-flight visibility hook pointed at the nearest
    /// enclosing route's `HeroScope` — the nested-`Navigator` branch of Flutter's
    /// `Hero._allHeroesFor` (`heroes.dart:317-333`): a hero inside this navigator
    /// is still invited into an *outer* flight when this navigator's own current
    /// route is a `PageRoute`. Independent of `HeroControllerScope`: Flutter's
    /// predicate does not consult it either, only `Route.isCurrent` and
    /// `is PageRoute`.
    ///
    /// Called from `build`, **not** `init_state`. `init_state` runs exactly once
    /// and never again on a `GlobalKey` reparent (`ElementBase::activate` reuses
    /// the same state without re-running it), so a registration made there would
    /// go stale the moment this `Navigator` moves to sit under a different route
    /// — a hook left pointing at the OLD enclosing `HeroScope`, invisible to the
    /// route that now actually contains it. `build`, by contrast, reliably reruns
    /// after a reparent: `ElementTree::insert`'s `try_retake_global_key` path
    /// calls `element.update(view)` right after `activate()`, which
    /// unconditionally marks the element dirty, and `recompute_inherited_subtree`
    /// has by then already rebuilt the moved subtree's ambient-lookup map against
    /// the *new* ancestor chain — so `ctx.get::<HeroScope, _>` here resolves
    /// correctly on the very next build. A cheap identity check keeps the common
    /// case (an ordinary rebuild with the same enclosing route) a no-op.
    ///
    /// **Verification limit, recorded rather than hidden.** A single-reparent
    /// scenario was traced through by hand — instrumenting this method's own
    /// `current`/`slot` identities showed the hook correctly re-pointing at the
    /// new route immediately after `try_retake_global_key` runs. Building a
    /// `flui_widgets`-level regression test on top of that (a `GlobalKey`-pinned
    /// wrapper reparented between two routes, then a *further* route pushed over
    /// the new one to drive a flight measurement) surfaced a **separate, transient
    /// mis-resolution**: on some builds *during that second, immediately-following
    /// cover transition*, `ctx.get::<HeroScope, _>` here briefly answered with an
    /// unrelated route's registry before the churn settled — a symptom that shows
    /// up with or without this fix (a plain, non-reparented three-route chain
    /// with the same double-cover shape did not reproduce it, so it looks tied to
    /// covering a route that itself just finished migrating a child, not to
    /// `GlobalKey` specifically). That looks like a pre-existing timing gap in
    /// how the inherited-ambient lookup interacts with a rapidly-transitioning,
    /// covered route — outside `flui-widgets` and outside this fix's diff — not
    /// something to patch blind here. No widgets-level `GlobalKey`-reparent test
    /// ships with this change as a result; `crates/flui-widgets/src/navigator/hero.rs`'s
    /// module docs and `docs/ROADMAP-TRACKER.md` B1.4 both name this honestly.
    fn sync_nested_hero_registration(&self, ctx: &dyn BuildContext) {
        let current = ctx.get::<HeroScope, _>(HeroScope::registry);
        let mut slot = self.shared.nested_hero_registration.lock();
        let unchanged = match (&current, slot.as_ref()) {
            (Some(new_registry), Some((old_registry, _))) => new_registry.is_same(old_registry),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        if let Some((old_registry, old_source)) = slot.take() {
            old_registry.deregister_nested(&old_source);
        }
        let Some(new_registry) = current else {
            return;
        };
        // A `Weak` — never a strong `NavigatorHandle` — so this closure cannot
        // keep `NavigatorShared` alive through its own `nested_hero_registration`
        // field: a strong capture would be a self-cycle (`shared` ->
        // `nested_hero_registration` -> this closure -> `shared`), leaking the
        // navigator's whole state on every reparent-without-dispose. Reading
        // `shared`'s fields directly (rather than reconstructing a
        // `NavigatorHandle`) needs no `command_target`, which the affine handle
        // carries but this hook never uses.
        let weak_shared = Arc::downgrade(&self.shared);
        let source = NestedHeroSource::new(move || {
            let shared = weak_shared.upgrade()?;
            let top = shared.history.lock().current()?;
            let is_page = shared
                .registries
                .peers
                .lock()
                .get(&top)
                .is_some_and(|peer| peer.group == TransitionGroup::Page);
            if !is_page {
                return None;
            }
            shared
                .registries
                .modals
                .lock()
                .get(&top)
                .map(ModalHandle::heroes)
        });
        new_registry.register_nested(source.clone());
        *slot = Some((new_registry, source));
    }
}

impl ViewState<Navigator> for NavigatorState {
    /// Flush the seeded initial routes, exactly once — Flutter's `restoreState`
    /// tail (`navigator.dart:3922-3934`), which asserts the history is non-empty
    /// and then calls `_flushHistoryUpdates()`.
    ///
    /// The overlay is not mounted yet (it is this view's child, built next), so
    /// the rearrange only fills the overlay's entry list; its first `build` reads
    /// it. Mutating an unmounted `OverlayHandle` is defined behavior.
    ///
    /// No `rebuild_handle()` is acquired here or anywhere in this file: the
    /// overlay owns its own rebuild, so trigger #22 has nothing to guard.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // The navigator owns the clock its route transitions
        // register with — the FLUI shape of Flutter's `vsync: navigator!`. Read
        // once, here, exactly as `AnimatedSize`/`Scrollable` read theirs.
        *self.shared.vsync.lock() = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());

        // Both are *lifecycle-only* acquisitions: a `HeroController`
        // fires them from a post-frame callback, never from a frame phase.
        *self.shared.post_frame.lock() = ctx.local_post_frame_handle();
        *self.shared.render_tree.lock() = ctx.pipeline_owner();

        // Resolve the ambient `HeroControllerScope` and settle which
        // controller (if any) observes this navigator — before `attach_observers`, so
        // it is attached with the rest.
        //
        // * a scope with a controller → that controller;
        // * `HeroControllerScope::none` → nothing (flights disabled);
        // * no scope at all → a fresh default, unless one was attached by hand.
        //   This is the auto-default that removes the `add_observer`
        //   boilerplate; `Navigator::build` re-wraps its subtree in `.none`, so a
        //   nested navigator sees a scope and never auto-defaults.
        match ctx.get::<HeroControllerScope, _>(HeroControllerScope::controller) {
            Some(Some(controller)) => self.shared.observers.lock().push(controller),
            Some(None) => {}
            None => {
                let mut auto = self.shared.auto_hero_observer.lock();
                let mut observers = self.shared.observers.lock();
                let already_manual = observers.iter().any(|o| o.observes_hero_flights());
                if !already_manual {
                    let controller: Arc<dyn NavigatorObserver> = HeroController::new();
                    *auto = Some(Arc::clone(&controller));
                    observers.push(controller);
                }
            }
        }

        // Before the seeded flush, so the first `did_push` an observer sees is
        // already one it can act on — Flutter attaches at `:3834-3837` and only
        // then calls `restoreState` → `_flushHistoryUpdates` (`:3922-3934`).
        self.shared.attach_observers(&self.handle());

        debug_assert!(
            self.shared.history.lock().len() > 0,
            "BUG: a Navigator was mounted with no routes — seed one before mounting \
             (navigator.dart:3922 asserts the same)"
        );
        self.shared.mutate(|history| {
            history.flush(true);
        });
    }

    /// Flutter's `NavigatorState.build` returns an `Overlay` and nothing else that
    /// matters here (`navigator.dart:5984-5990`); its `HeroControllerScope`,
    /// `NavigationNotification` listener, pointer-cancelling `Listener` and
    /// `FocusTraversalGroup` all belong to features deferred for now.
    fn build(&self, _view: &Navigator, ctx: &dyn BuildContext) -> impl IntoView {
        // Re-resolved every build, not just once at mount — see
        // `sync_nested_hero_registration`'s doc for why `init_state` cannot do this.
        self.sync_nested_hero_registration(ctx);

        // `HeroControllerScope.none` (`navigator.dart:5955`): a nested navigator under
        // this one must not pick up this navigator's controller. It resolves the
        // `.none` in its own `init_state` and attaches nothing.
        HeroControllerScope::none(Overlay::new(self.shared.overlay.clone()))
    }

    /// Flutter's `NavigatorState.deactivate` (`navigator.dart:4105-4111`).
    fn deactivate(&mut self) {
        self.shared.detach_observers();
    }

    /// Flutter's `NavigatorState.activate` (`navigator.dart:4114-4123`) — a
    /// navigator moved by a `GlobalKey` is deactivated and reactivated in the same
    /// frame, and its observers must survive the round trip.
    fn activate(&mut self) {
        self.shared.attach_observers(&self.handle());
    }

    /// Flutter asserts `_effectiveObservers.isEmpty` here (`:4133`), because
    /// `deactivate` always precedes `dispose`. FLUI's `ElementBase::unmount` calls
    /// `dispose` directly, so this is the detach that actually runs on a plain
    /// unmount; `detach_observers` is idempotent, so the deactivate-then-dispose
    /// path notifies exactly once.
    fn dispose(&mut self) {
        self.shared.detach_observers();
        // The capabilities die with the tree they name, so a `HeroController` that
        // outlives its navigator schedules nothing and measures nothing.
        *self.shared.post_frame.lock() = None;
        *self.shared.render_tree.lock() = None;
        // The mirror of `sync_nested_hero_registration`'s publish: a disposed
        // navigator's heroes must never be visited by an outer flight again.
        if let Some((registry, source)) = self.shared.nested_hero_registration.lock().take() {
            registry.deregister_nested(&source);
        }
    }
}
