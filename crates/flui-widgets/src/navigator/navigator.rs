//! [`Navigator`], [`NavigatorState`] and [`NavigatorHandle`].
//!
//! The private widget came first, then the signed-off
//! `Navigator` baseline was exported; public page/popup routes were added, then
//! the public Hero baseline that rides on this navigator.
//!
//! # Why no `GlobalKey`s, and the lock hazard behind them
//!
//! `BuildContext::find_ancestor_state` yields `&dyn Any` —
//! *immutable* — while the element tree is borrowed. So `Navigator::of` can never
//! return `&mut NavigatorState`, and it must not perform a second lookup inside
//! that callback: a `GlobalKey` lookup would take the registry's
//! `WidgetsBinding::inner.read()` while the tree borrow is held, and
//! `parking_lot::RwLock` is not reentrant.
//!
//! Both problems dissolve the same way. `Navigator::of` clones an owned,
//! `'static` [`NavigatorHandle`] out of the state *inside* the callback and does
//! nothing else there; every mutation runs after the borrow is released. Navigator
//! and Overlay couple through an `Arc`, not through the tree — so no
//! `GlobalKey<OverlayState>` is needed.
//!
//! # Not implemented, and not claimed
//!
//! No Navigator 2.0/page-list API here: the typed `Router` in `crate::router`
//! (ADR-0330) drives this navigator as its page stack, through the doors in
//! `addressing.rs`, which also make its facade refuse unaddressable pages. No
//! restoration, `PopScope`,
//! `LocalHistoryRoute`, `HeroControllerScope`, `NavigationNotification`,
//! pointer-cancelling wrapper, or per-route focus scope in the navigator's own
//! `build`. `TransitionRoute` / `ModalRoute` stay implementation details
//! behind public `PageRoute` / `PopupRoute`, not public API (nameable only
//! through the doc-hidden, temporary `__test_access`, ADR-0083 §4).
//!
//! There are no named routes: a page with a name is a `Router` route value,
//! and its location is its name (ADR-0330).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use flui_animation::AnimationRunFuture;
use flui_foundation::ChangeNotifier;
use flui_view::BuildContextExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::binding::{
    PopPacing, RouteBinding, RouteCommand, RouteRegistries, RouteVsync, TransitionGroup,
    TransitionPeer,
};
use super::hero::{HeroRegistry, HeroScope, NestedHeroSource};
use super::hero_controller::HeroController;
use super::hero_controller_scope::HeroControllerScope;
use super::history::{DeferredEffect, FlushOutcome, ReplaceTarget, RouteHistory};
use super::modal_route::ModalHandle;
use super::observer::{NavigatorObserver, Notification, deliver};
use super::overlay_route::NavigatorRoute;
use super::page_transitions::{AdoptedTransitions, PageTransitions};
use super::result::RouteResult;
use super::route::{AnyResult, RouteId, RoutePopDisposition, UndeliveredResult};
use super::subtree::RouteSubtree;
use crate::{Overlay, OverlayEntry, OverlayEntryId, OverlayHandle};
use flui_view::environment::VsyncScope;

// A child module, so the admission rules and the `Router`'s doors can reach
// `NavigatorShared` without widening it (`navigator/navigator/addressing.rs`).
mod addressing;
use addressing::Addressing;

#[cfg(test)]
pub(super) mod identity_tests;

/// Every identity one route admission spends: its [`RouteId`] and the
/// [`OverlayEntryId`] of its overlay entry.
///
/// Minted before the admission publishes anything — a recorded page, a filled
/// binding slot, an inserted entry, a dismissed predecessor — so a capacity
/// refusal fails the whole operation with nothing changed. A reservation an
/// operation abandons is never reissued; identities stay unique either way.
#[derive(Debug, Clone, Copy)]
pub(super) struct RouteReservation {
    route: RouteId,
    entry: OverlayEntryId,
}

impl RouteReservation {
    /// Mint both identities, refusing permanently at either's exhaustion.
    pub(super) fn reserve() -> Self {
        Self::reserve_using(RouteId::next, OverlayEntryId::next)
    }

    /// The reserved route identity.
    pub(super) fn route(self) -> RouteId {
        self.route
    }

    pub(super) fn reserve_using(
        route: impl FnOnce() -> RouteId,
        entry: impl FnOnce() -> OverlayEntryId,
    ) -> Self {
        Self {
            route: route(),
            entry: entry(),
        }
    }
}

/// What a push-completion continuation runs to settle the history: a pump of
/// the route-command queue through a `Weak` navigator.
type SettlePump = Rc<dyn Fn()>;

/// Everything a [`NavigatorHandle`] and the mounted [`NavigatorState`] share.
///
/// The route stack lives behind a private `RefCell` because `ViewState::build` takes
/// `&self` and nothing can obtain `&mut NavigatorState`.
struct NavigatorShared {
    history: super::lifecycle::Terminal<RefCell<RouteHistory>>,

    /// The overlay this navigator presents its routes in, held as a capability
    /// directly rather than looked up through a `GlobalKey`.
    overlay: super::lifecycle::Terminal<OverlayHandle>,

    /// The `RouteId`-keyed maps every binding shares: overlay entries, transition
    /// peers, page subtrees and modal (`offstage`) controls. FLUI's routes live
    /// behind `Box<dyn ErasedRoute>` inside the history's cell, so they publish
    /// here instead of being read off the route object (ADR-0019).
    registries: super::lifecycle::Terminal<Rc<RouteRegistries>>,

    /// The clock this navigator's route transitions register with.
    /// Resolved from an ambient `VsyncScope` in `init_state`; `None` when there is
    /// none, in which case each controller falls back to its own wall-clock ticker.
    vsync: super::lifecycle::Terminal<RouteVsync>,

    /// The binding's post-frame capability and render tree, both read **once** from
    /// `NavigatorState::init_state` — a lifecycle hook, since only `LifecycleContext`
    /// offers them. `HeroController` reaches them through its `NavigatorHandle`, which
    /// is how it schedules its measurement and then resolves the geometry that
    /// measurement was waiting for.
    ///
    /// `None` when the navigator is unmounted, or when the binding installed no
    /// post-frame handle — a `HeroController` then simply never measures.
    post_frame: RefCell<Option<flui_scheduler::PostFrameHandle>>,
    render_tree: RefCell<Option<flui_rendering::pipeline::PipelineCell>>,

    /// Whether the mounted `NavigatorState` currently holds the observers
    /// attached. It is one flag, because every observer of one navigator attaches
    /// and detaches together.
    observers_attached: Cell<bool>,

    /// The observers notified of route changes.
    ///
    /// **Not on `RouteHistory`.** An observer holds a [`NavigatorHandle`], so
    /// notifying one is re-entrant by construction; the route stack must therefore
    /// neither own observers nor call them.
    observers: RefCell<Vec<Rc<dyn NavigatorObserver>>>,
    /// The controller the navigator auto-created when no `HeroControllerScope` was
    /// present. Kept separate so a later hand-attached `HeroController` can replace
    /// it instead of doubling the flight observer count.
    auto_hero_observer: RefCell<Option<Rc<dyn NavigatorObserver>>>,

    /// This navigator's cross-flight visibility hook, published to the nearest
    /// enclosing route's `HeroScope` from every `build` (see
    /// `NavigatorState::sync_nested_hero_registration`), so heroes inside a nested
    /// navigator are visible to an outer flight.
    /// `None` for a top-level navigator, which has no enclosing route to publish
    /// to. Cleared in `dispose`, together with the registry it was registered
    /// on, so a disposed navigator's heroes are never visited again.
    nested_hero_registration: RefCell<Option<(HeroRegistry, NestedHeroSource)>>,

    /// The nearest navigator enclosing this one, re-resolved by every `build`
    /// (a `GlobalKey` reparent moves a navigator without re-running
    /// `init_state`) and cleared in `dispose`. A pop request this navigator
    /// bubbles goes there.
    ///
    /// A `Weak`: the enclosing navigator's routes own this navigator's view,
    /// which owns a handle to this state, so a strong reference would be a
    /// cycle.
    enclosing: RefCell<Option<Weak<NavigatorShared>>>,

    /// How many overlapping user gestures (e.g. edge swipe-backs) are
    /// currently manipulating this navigator. Only
    /// the 0→1 and 1→0 transitions notify observers, so nested gestures on
    /// the same navigator collapse to one notification pair.
    ///
    /// `Rc`-shared so [`UserGestureSignal`] can hand this exact counter
    /// to a hero flight's data-plane animation
    /// listener without holding the owner-affine [`NavigatorHandle`] itself.
    user_gestures_in_progress: Rc<Cell<u32>>,

    /// Fires exactly on the 0→1 and 1→0 transitions
    /// of [`user_gestures_in_progress`](Self::user_gestures_in_progress) —
    /// the same edges that notify [`NavigatorObserver::did_start_user_gesture`]
    /// / [`did_stop_user_gesture`](NavigatorObserver::did_stop_user_gesture).
    ///
    /// A pure notification relay rather than a `ValueNotifier<bool>` clone:
    /// this codebase's `ValueNotifier::clone` shares only the listener
    /// registry, not the value (each clone owns an independent `value`
    /// field), which would desync the instant it was cloned into a flight —
    /// the same [`ChangeNotifier`] idiom `ModalInner::relay` and
    /// `TransitionRouteInner::status_wake` already use to bridge an
    /// animation callback back to owner-local code. `HeroFlight` subscribes
    /// once, for its whole life, to replay a terminal status update parked
    /// mid-gesture.
    user_gesture_in_progress_notifier: super::lifecycle::Terminal<ChangeNotifier>,

    /// What a push's
    /// [`PushCompletion::Animating`](super::route::PushCompletion::Animating)
    /// continuation settles the history through, once its future resolves: a
    /// pump of the route-command queue, run in the animation tick that resolved
    /// the future, so the next route's `did_add` and entrance start in that same
    /// frame and never inside `build`, where a controller refuses mutation
    /// (ADR-0086 §7). The continuation holds only this `Rc` (and a clone of the
    /// route-command queue); the pump reaches the navigator through a `Weak`.
    ///
    /// Installed by [`NavigatorState::init_state`](struct@NavigatorState),
    /// cleared in `dispose`, and — this is the part that matters — **read at
    /// the moment the continuation fires**, not captured when it is
    /// registered: `push_with_id` flushes before the Navigator ever mounts
    /// (`history.rs`), so a route pushed pre-mount registers its continuation
    /// while this slot is still `None`, and the mount's own flush drains the
    /// command it queued.
    settle_wake: super::lifecycle::Terminal<Rc<RefCell<Option<SettlePump>>>>,

    /// `Some` when a `Router` drives this navigator; fixed at construction.
    /// See `addressing.rs` for what it refuses.
    addressing: Option<Addressing>,

    /// The build phase of the UI runtime the navigator mounts in, bound at
    /// construction: a [`PageHandle`](super::PageHandle) mutation made while
    /// it builds is refused (ADR-0330 §3).
    build_phase: flui_foundation::BuildPhase,
}

impl Drop for NavigatorShared {
    fn drop(&mut self) {
        self.registries.close();
        let history = self.history.withdraw();
        let overlay = self.overlay.withdraw();
        let registries = self.registries.withdraw();
        let vsync = self.vsync.withdraw();
        let post_frame = super::lifecycle::Terminal::new(self.post_frame.get_mut().take());
        let render_tree = super::lifecycle::Terminal::new(self.render_tree.get_mut().take());
        self.observers_attached.set(false);
        let observers = super::lifecycle::RetiredValues(std::mem::take(self.observers.get_mut()));
        let auto = super::lifecycle::Terminal::new(self.auto_hero_observer.get_mut().take());
        let nested = self.nested_hero_registration.get_mut().take();
        let (nested_registry, nested_source) = match nested {
            Some((registry, source)) => (Some(registry), Some(source)),
            None => (None, None),
        };
        let nested_registry = super::lifecycle::Terminal::new(nested_registry);
        let nested_source = super::lifecycle::Terminal::new(nested_source);
        let notifier = self.user_gesture_in_progress_notifier.withdraw();
        let settle = self.settle_wake.withdraw();
        drop((
            history,
            overlay,
            registries,
            vsync,
            post_frame,
            render_tree,
            observers,
            auto,
            nested_registry,
            nested_source,
            notifier,
            settle,
        ));
    }
}

#[cfg(test)]
pub(super) fn terminal_binding_authority_is_closed_before_route_retirement() {
    use super::binding::RouteBindingSlot;
    use super::route::{Route, RouteSettings};
    use std::cell::Cell;

    struct ClosingRoute {
        settings: RouteSettings,
        slot: RouteBindingSlot,
        on_drop: Rc<dyn Fn()>,
    }
    impl Route for ClosingRoute {
        type Output = ();
        fn settings(&self) -> &RouteSettings {
            &self.settings
        }
    }
    impl NavigatorRoute for ClosingRoute {
        fn content_builder(&self) -> super::overlay_route::RouteContentBuilder {
            Rc::new(|_| crate::Text::new("closing route").boxed())
        }
        fn binding_slot(&self) -> Option<&RouteBindingSlot> {
            Some(&self.slot)
        }
    }
    impl Drop for ClosingRoute {
        fn drop(&mut self) {
            (self.on_drop)();
        }
    }

    let navigator = NavigatorHandle::new(flui_foundation::BuildPhase::default());
    let entries = navigator.shared.registries.entries.clone();
    let queue = navigator.shared.history.borrow().command_queue();
    let slot = RouteBindingSlot::new();
    let observed = Rc::new(Cell::new(false));
    let callback_slot = slot.clone();
    let callback_entries = Rc::clone(&entries);
    let callback_queue = Rc::clone(&queue);
    let callback_observed = Rc::clone(&observed);
    navigator.seed_initial(ClosingRoute {
        settings: RouteSettings::default(),
        slot: slot.clone(),
        on_drop: Rc::new(move || {
            let binding = callback_slot.get().expect("installed route has a binding");
            binding.set_entry_opaque(true);
            binding.finalize();
            assert!(
                !callback_entries
                    .borrow_mut()
                    .values()
                    .next()
                    .expect("physical alias retains entry")
                    .opaque()
            );
            assert!(callback_queue.borrow().is_empty());
            callback_observed.set(true);
        }),
    });
    drop(navigator);
    assert!(observed.get(), "the actual route owner must retire");
    let binding = slot.get().expect("a genuine slot alias remains usable");
    binding.set_entry_opaque(true);
    binding.finalize();
    assert!(
        !entries
            .borrow_mut()
            .values()
            .next()
            .expect("physical alias retains entry")
            .opaque()
    );
    assert!(
        queue.borrow().is_empty(),
        "closed authority cannot enqueue orphan commands"
    );
}

impl NavigatorShared {
    fn rebind_clock(&self, current: Option<flui_animation::Vsync>) {
        let unchanged = match (&*self.vsync.borrow(), &current) {
            (Some(old), Some(new)) => old.is_same(new),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        let outgoing = std::mem::replace(&mut *self.vsync.borrow_mut(), current);
        let mut peers: Vec<_> = self
            .registries
            .peers
            .borrow_mut()
            .iter()
            .map(|(id, peer)| (*id, Rc::clone(&peer.rebind_clock)))
            .collect();
        peers.sort_unstable_by_key(|(id, _)| *id);
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        for (_, rebind) in peers {
            let current = self.vsync.borrow().clone();
            recovery.run(|| rebind(current.as_ref()));
        }
        recovery.retire(outgoing);
        recovery.finish();
    }

    /// Apply what a flush left behind, in this order:
    ///
    /// 1. remove each disposed route's overlay entries;
    /// 2. rearrange the overlay to match the route stack, but **only** when the
    ///    flush asked for it. `pop` and `remove_route` do not ask, precisely
    ///    because step 1 already updated the overlay's list.
    ///
    /// Every step runs though user code in an earlier one panicked; the first
    /// panic propagates once all of them have run.
    fn apply(&self, mut outcome: FlushOutcome) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        // 0. Everything the flush owed to user code, **in the order it was
        //    produced** — a multi-pass flush must not deliver a later pass's
        //    refusal ahead of an earlier pass's pop — and before the observers
        //    hear the pop: the pop-invoked callback fires inside the flush, while
        //    the pop observation is only queued there and delivered afterwards.
        //
        //    Note what this order does NOT do. FLUI permits navigating
        //    from a callback fired here, deliberately (ARCHITECTURE.md mapping
        //    decision 13), which makes the resulting "effect observed before its
        //    cause" sequence reachable.
        //
        //    With **no borrow held**: these are user callbacks, they may call
        //    straight back into this navigator, and even a `can_pop()` read
        //    would panic on the history it already borrowed.
        if !outcome.deferred.is_empty() {
            // Cloned lazily, on the first `PopInvoked`/`LocalHistoryPopped`:
            // an animated push's own `AwaitPush` effect (the common case,
            // since every push produces one) never needs this map at all.
            let mut modals: Option<HashMap<RouteId, ModalHandle>> = None;
            // Read once, as the flush left it: where its owed `on_remove`s go.
            let post_frame = self.post_frame.borrow().clone();
            for effect in std::mem::take(&mut outcome.deferred) {
                match effect {
                    DeferredEffect::PopInvoked(route, did_pop) => {
                        let modals =
                            modals.get_or_insert_with(|| self.registries.modals.borrow().clone());
                        if let Some(modal) = modals.get(&route) {
                            recovery.run(|| modal.notify_pop_invoked(did_pop));
                        }
                    }
                    DeferredEffect::LocalHistoryPopped(route) => {
                        let modals =
                            modals.get_or_insert_with(|| self.registries.modals.borrow().clone());
                        if let Some(modal) = modals.get(&route) {
                            modal.drain_local_history(post_frame.as_ref(), &mut recovery);
                        }
                    }
                    // **Load-bearing placement (ADR-0064).** Registered here,
                    // after the flush that produced `future` has released the
                    // history borrow — never from inside that flush. `flush()`
                    // re-drains queued `RouteCommand`s between passes, so a
                    // continuation registered mid-flush on an
                    // already-resolved future (a zero-duration push) would
                    // settle within that same flush instead of on the next
                    // one.
                    DeferredEffect::AwaitPush(route, future) => self.await_push(route, future),
                    // Cloned out, so the registry guard is released before
                    // `mark_needs_build` schedules the rebuild.
                    DeferredEffect::NeighboursChanged(route) => {
                        let entry = { self.registries.entries.borrow().get(&route).cloned() };
                        if let Some(entry) = entry {
                            entry.mark_needs_build();
                        }
                    }
                }
            }
            recovery.retire(modals);
        }

        // 0b. The new top route takes the keyboard: its scope becomes active and
        //     the focus it remembers is restored.
        //     Also outside the borrow — moving the focus fires user focus-change
        //     listeners, and one that calls back into this navigator would
        //     panic on a second borrow if this ran inside the flush.
        if let Some((top, replaces_a_route)) =
            outcome
                .notifications
                .iter()
                .find_map(|notification| match notification {
                    Notification::TopChanged { top, previous_top } => {
                        Some((*top, previous_top.is_some()))
                    }
                    Notification::Observed(_) => None,
                })
        {
            // The new top becomes focusable and takes the focus first, then
            // every other route stops being focusable: there is never a frame
            // where the leaving route is the only one that can hold a key.
            let modals = self.registries.modals.borrow().clone();
            if let Some(modal) = modals.get(&top) {
                if replaces_a_route {
                    recovery.run(|| modal.supersede_inner_first_focus());
                }
                recovery.run(|| modal.set_focusable(true));
                // The first route of a navigator takes no focus: the first Tab
                // enters it.
                recovery.run(|| modal.activate_focus_scope(replaces_a_route));
            }
            for (route, modal) in &modals {
                if *route != top {
                    recovery.run(|| modal.set_focusable(false));
                }
            }
            recovery.retire(modals);
        }

        // 1. Observers, with **no borrow held** — this is the whole reason the flush
        //    returns data instead of calling them. An observer may read the stack
        //    through its `NavigatorHandle`, and may mutate it (which runs a fresh
        //    flush whose notifications land after this loop drains).
        let observers = self.observers();
        deliver(&outcome.notifications, &observers, &mut recovery);
        recovery.retire(observers);

        // 2. Each disposed route's overlay entries, then the route itself.
        self.registries
            .retire_entries(&outcome.disposed, &mut recovery);
        outcome.dispose_routes(&mut recovery);

        if outcome.rearrange_overlay {
            // The entries of every route in history order, bottom → top.
            let ordered: Vec<OverlayEntry> = {
                let ids = self.history.borrow().ids();
                let entries = self.registries.entries.borrow_mut();
                ids.iter()
                    .filter_map(|id| entries.get(id).cloned())
                    .collect()
            };
            recovery.run(|| self.overlay.rearrange(&ordered));
        }
        recovery.retire(outcome);
        recovery.finish();
    }

    /// Register the continuation a
    /// [`PushCompletion::Animating`](super::route::PushCompletion::Animating)
    /// future settles through — the whole of what a push's entrance transition
    /// gets to do to this navigator.
    ///
    /// Called only from [`apply`](Self::apply), with the history borrow already
    /// released; see that call site's comment for why the placement itself is
    /// load-bearing. The registered closure captures nothing but a clone of
    /// the route-command queue and this `settle_wake` slot's `Rc` — never
    /// `self` — so it reaches the history only through the pump a mounted
    /// navigator installed, which leaves the command to a flush already
    /// running when it finds the history borrowed.
    fn await_push(&self, route: RouteId, future: AnimationRunFuture) {
        let queue = self.history.borrow().command_queue();
        let settle_wake = Rc::clone(&self.settle_wake);
        future.when_complete_or_cancel(move |_outcome| {
            // Complete and cancel settle the entry the same way: there is no
            // separate "the push was canceled" state at this layer, only
            // "Pushing" and "Idle" (or whatever the entry has since moved to
            // — the `state == Pushing` guard in `apply_pending_commands`
            // handles a stale command, e.g. a pop that raced this future).
            queue
                .borrow_mut()
                .push_back(RouteCommand::PushCompleted(route));
            // Read now, not at registration: a route pushed before this
            // navigator mounted registers this closure while the slot is
            // still `None`, and only a later mount fills it. Cloned out and
            // dropped before the pump runs: the pump re-enters the navigator,
            // and an if-let-scrutinee guard held across it is the shape #1177
            // eliminated elsewhere in the workspace.
            let pump = settle_wake.borrow().clone();
            if let Some(pump) = pump {
                pump();
            }
        });
    }

    /// Apply any [`RouteCommand`]s a route raised,
    /// and settle the history — the `wake` half of the route-binding seam.
    ///
    /// **`try_borrow_mut`, deliberately.** If the history is borrowed we are
    /// inside a flush on this thread (`mutate` holds the borrow for the whole
    /// walk), and that flush drains the queue itself before returning — so there
    /// is nothing to do here. If it is free we are between frames (an animation
    /// status listener), and the commands take effect now. See `binding.rs`,
    /// *Correction 1*.
    ///
    /// # A flush driver drains *both* channels
    ///
    /// This is the third place that runs a flush and releases the guard —
    /// [`mutate_committing`](Self::mutate_committing) and
    /// `NavigatorHandle::push_prepared` are the others. Every such driver owes two
    /// drains, not one: `take_outcome` **and** `take_undelivered`. A flush can
    /// complete a route carrying a `pending_result`, and if the value's type does
    /// not match, `did_complete` records a `TypeMismatch` — which a driver that
    /// only takes the outcome would leave in the history until some later
    /// operation happened to pick it up, reporting it under the wrong operation
    /// name and dropping it at an unrelated moment.
    ///
    /// No producer reachable from `RouteCommand` exists today — the command
    /// vocabulary carries no caller-supplied result — so this is the *invariant*
    /// being enforced rather than a live defect being fixed. Enforced rather than
    /// merely stated, because "a driver drains both" is the kind of obligation this
    /// feature has now failed to notice six times, and a comment does not survive
    /// the seventh.
    fn pump_route_commands(&self) {
        let (outcome, undelivered) = {
            let Ok(mut history) = self.history.try_borrow_mut() else {
                return; // A flush is running; it will drain the queue.
            };
            if !history.has_pending_commands() {
                return;
            }
            history.flush(false);
            (history.take_outcome(), history.take_undelivered())
        };
        // Same order as every other driver: apply first, then report — see
        // `mutate_committing`.
        if let Some(outcome) = outcome {
            self.apply(outcome);
        }
        report_undelivered("pump_route_commands", undelivered);
    }

    /// The observers, cloned out in registration order.
    ///
    /// Cloned, not borrowed: every notification runs with this lock released too,
    /// so an observer may register another one — or drop its own `Rc` — from a
    /// callback without a second borrow of `observers`.
    fn observers(&self) -> Vec<Rc<dyn NavigatorObserver>> {
        self.observers.borrow().clone()
    }

    /// Remove the auto-created hero controller, if one exists.
    ///
    /// Returns the removed observer so the caller can run `did_detach` with no
    /// `observers` borrow held. Observer callbacks are user code; holding the
    /// borrow would reintroduce the re-entry class this design removed.
    fn take_auto_hero_observer(&self) -> Option<Rc<dyn NavigatorObserver>> {
        let mut auto = self.auto_hero_observer.borrow_mut();
        let removed = auto.take()?;
        self.observers
            .borrow_mut()
            .retain(|observer| !Rc::ptr_eq(observer, &removed));
        Some(removed)
    }

    /// Hand every observer, in registration order, the capability it observes.
    fn attach_observers(&self, handle: &NavigatorHandle) {
        if self.observers_attached.replace(true) {
            return;
        }
        for observer in self.observers() {
            observer.did_attach(handle.clone());
        }
    }

    /// Tell every observer it no longer observes a navigator.
    fn detach_observers(&self) {
        if !self.observers_attached.replace(false) {
            return;
        }
        for observer in self.observers() {
            observer.did_detach();
        }
    }

    /// Complete every still-pending route result with `Ok(None)`: the
    /// navigator is unmounting, and no pop will ever deliver them. The
    /// completions are prepared under the history borrow and run, waking their
    /// awaiting tasks, after it is released; each runs under `first`.
    fn complete_terminal_results(&self, first: &mut flui_foundation::panic::FirstPanic) {
        let completions = self.history.borrow_mut().terminal_result_completions();
        for complete in completions {
            first.contain(complete);
        }
    }

    /// Begin a user gesture (e.g. an edge swipe-back).
    ///
    /// Only the 0→1 transition resolves the current route and notifies
    /// observers — a nested/overlapping gesture on the same navigator just
    /// bumps the count. The history borrow is **released before** observers run:
    /// `did_start_user_gesture` hands out a `NavigatorHandle`, and an observer
    /// reading the stack back through it would find the history borrowed.
    fn did_start_user_gesture(&self) {
        let count_before = self.user_gestures_in_progress.get();
        self.user_gestures_in_progress
            .set(count_before.saturating_add(1));
        if count_before != 0 {
            return;
        }
        // The gesture notifier fires before the observer loop — no navigator borrow
        // is held at this call, so a listener that reads back through a
        // `NavigatorHandle` finds nothing borrowed.
        self.user_gesture_in_progress_notifier.notify_listeners();
        let Some((route, previous)) = self.history.borrow_mut().top_and_previous_for_gesture()
        else {
            return;
        };
        for observer in self.observers() {
            observer.did_start_user_gesture(route, previous);
        }
    }

    /// End a user gesture. Only the 1→0 transition notifies observers.
    fn did_stop_user_gesture(&self) {
        // Saturating: an unmatched call at 0 must stay at 0 in release (the
        // debug_assert below is compiled out there), or every later
        // `user_gesture_in_progress()` would read `true` forever.
        let count_before = self.user_gestures_in_progress.get();
        self.user_gestures_in_progress
            .set(count_before.saturating_sub(1));
        debug_assert!(
            count_before > 0,
            "BUG: did_stop_user_gesture called without a matching did_start_user_gesture"
        );
        if count_before != 1 {
            return;
        }
        // Same ordering as `did_start_user_gesture`: the notifier fires
        // before the observer loop, with no navigator borrow held. A `HeroFlight` parked on this notifier
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
    /// navigator.
    fn user_gesture_in_progress(&self) -> bool {
        self.user_gestures_in_progress.get() > 0
    }

    /// Run `mutate` against the stack, then apply whatever it flushed.
    ///
    /// The history borrow is **released before** the overlay work, so no lock is
    /// held across `RebuildHandle::schedule`.
    /// `operation` names the caller for the undelivered-result report, so a reader
    /// of the log — and a test — can tell *which* operation discarded a value.
    /// Nine sites emitted one indistinguishable message before this.
    fn mutate<R>(&self, operation: &'static str, mutate: impl FnOnce(&mut RouteHistory) -> R) -> R {
        let (value, (), undelivered) = self.mutate_committing(mutate, || ());
        report_undelivered(operation, undelivered);
        value
    }

    /// [`mutate`](Self::mutate) with a framework-only `commit` between
    /// releasing the history borrow and applying the flushed outcome, so state it
    /// publishes is visible to every observer and lifecycle hook `apply`
    /// reaches. The commit's value is returned, and dropped by unwind if
    /// `apply` panics. The undelivered values come back for the caller to
    /// report once it has no observations left to make.
    fn mutate_committing<R, C>(
        &self,
        mutate: impl FnOnce(&mut RouteHistory) -> R,
        commit: impl FnOnce() -> C,
    ) -> (R, C, Vec<UndeliveredResult>) {
        let (value, outcome, undelivered) = {
            let mut history = self.history.borrow_mut();
            let value = mutate(&mut history);
            (value, history.take_outcome(), history.take_undelivered())
        };
        let committed = commit();
        // Guard released. Both of the following may re-enter this navigator — a
        // `Route` lifecycle hook or observer inside `apply`, and the `Drop` of a
        // caller-supplied result inside `report_undelivered`.
        //
        // **Order matters, and it is causal.** `apply` first, so this operation's
        // own observations reach observers *before* anything a re-entrant drop
        // triggers. Reporting first left a window in which history had advanced but
        // the outcome had not been applied: a nested push from a drop path emitted
        // its `did_push` ahead of the `did_pop` that caused it.
        //
        // `apply` is already the re-entrant half — it delivers deferred `PopScope`
        // effects, notifies observers, and runs `Route::dispose`, all deliberately
        // unlocked — so putting the report after it adds no new hazard and puts the
        // drop last, which is where a consequence belongs.
        if let Some(outcome) = outcome {
            self.apply(outcome);
        }
        (value, committed, undelivered)
    }
}

/// Drop caller-supplied results the history could not deliver, and say so.
///
/// Two obligations, both easy to miss. **Dropped with the history guard
/// released**: an `AnyResult` wraps a value the caller supplied, so its `Drop` is
/// user code and may reach back into this navigator, which the held
/// history borrow would refuse — the same hazard as dropping a registry
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
fn report_undelivered(operation: &'static str, undelivered: Vec<UndeliveredResult>) {
    for result in undelivered {
        match result {
            UndeliveredResult::NoTarget { value, supplied } => {
                tracing::warn!(
                    operation,
                    supplied,
                    "a result supplied to a navigator operation reached no route and was \
                     discarded: the stack had no present route to deliver it to, or the \
                     target had already completed. Dropped outside the history borrow."
                );
                drop(value);
            }
            UndeliveredResult::TypeMismatch {
                route,
                expected,
                value,
            } => {
                let supplied = value.supplied();
                // The message and fields `RouteRecord::did_complete` used to emit
                // inline, moved here so neither the subscriber nor the value's
                // `Drop` runs under the history borrow.
                tracing::error!(
                    operation,
                    route = route.get(),
                    expected,
                    supplied,
                    "pop result has the wrong type for this route; completed with None"
                );
                drop(value);
            }
        }
    }
}

/// A view onto a navigator's user-gesture state: the live
/// in-progress count and its change notifier, bundled.
///
/// What a data-plane animation status listener needs to defer a terminal
/// status update until a gesture ends, without capturing the owner-affine
/// [`NavigatorHandle`] itself. Clones share the gesture counter and notifier;
/// the listener only needs these gesture observations.
#[derive(Clone)]
pub(crate) struct UserGestureSignal {
    in_progress: Rc<Cell<u32>>,
    notifier: ChangeNotifier,
}

impl UserGestureSignal {
    /// Whether a user gesture is in progress right now, read from an animation
    /// listener.
    pub(crate) fn in_progress(&self) -> bool {
        self.in_progress.get() > 0
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
/// builders and observer callbacks that belong to the UI owner thread. Another
/// thread that needs to navigate writes a signal the owner thread reacts to.
#[derive(Clone)]
pub struct NavigatorHandle {
    shared: Rc<NavigatorShared>,
    _owner_affine: PhantomData<Rc<()>>,
}

impl NavigatorHandle {
    /// A handle to an empty, unmounted navigator. Seed it, hand it to
    /// [`Navigator::new`], and keep a clone. `build_phase` is the build phase
    /// of the UI runtime it will mount in.
    #[must_use]
    pub fn new(build_phase: flui_foundation::BuildPhase) -> Self {
        Self::with_addressing(None, build_phase)
    }

    fn with_addressing(
        addressing: Option<Addressing>,
        build_phase: flui_foundation::BuildPhase,
    ) -> Self {
        let shared = Rc::new(NavigatorShared {
            history: super::lifecycle::Terminal::new(RefCell::new(RouteHistory::new())),
            overlay: super::lifecycle::Terminal::new(OverlayHandle::new()),
            vsync: super::lifecycle::Terminal::new(Rc::new(RefCell::new(None))),
            registries: super::lifecycle::Terminal::new(Rc::new(RouteRegistries::new())),
            post_frame: RefCell::new(None),
            render_tree: RefCell::new(None),
            observers: RefCell::new(Vec::new()),
            auto_hero_observer: RefCell::new(None),
            observers_attached: Cell::new(false),
            nested_hero_registration: RefCell::new(None),
            enclosing: RefCell::new(None),
            user_gestures_in_progress: Rc::new(Cell::new(0)),
            user_gesture_in_progress_notifier: super::lifecycle::Terminal::new(
                ChangeNotifier::new(),
            ),
            settle_wake: super::lifecycle::Terminal::new(Rc::new(RefCell::new(None))),
            addressing,
            build_phase,
        });
        Self {
            shared,
            _owner_affine: PhantomData,
        }
    }

    /// How many attached observers drive hero flights — the auto-default plus any
    /// hand-attached `HeroController`s. Test-facing: pins that automatic attach adds
    /// exactly one, and that a manual controller suppresses it. Read through
    /// `crate::__test_access::NavigatorProbe`.
    pub(crate) fn hero_observer_count(&self) -> usize {
        self.shared
            .observers
            .borrow_mut()
            .iter()
            .filter(|observer| observer.observes_hero_flights())
            .count()
    }

    /// Register an observer.
    ///
    /// If the navigator is already mounted the observer is attached at once.
    /// Registered before mount, it is attached by `init_state` instead. Either way it holds a
    /// handle exactly while the navigator is mounted.
    pub fn add_observer(&self, observer: Rc<dyn NavigatorObserver>) {
        let replaced_auto = if observer.observes_hero_flights() {
            self.shared.take_auto_hero_observer()
        } else {
            None
        };
        if let Some(auto) = replaced_auto
            && self.shared.observers_attached.get()
        {
            auto.did_detach();
        }

        self.shared
            .observers
            .borrow_mut()
            .push(Rc::clone(&observer));
        if self.shared.observers_attached.get() {
            observer.did_attach(self.clone());
        }
    }

    /// Deregister one previous [`add_observer`](Self::add_observer)
    /// registration of `observer` (matched by `Rc` identity), notifying it
    /// with [`NavigatorObserver::did_detach`] if the navigator is currently
    /// mounted — the deregistration half of observer reconciliation.
    ///
    /// Crate-internal: `WidgetsApp` uses it to keep a caller-retained handle
    /// free of stale shell registrations across unmount/remount and
    /// configuration updates. A no-op when `observer` is not registered.
    ///
    /// Removing a hero-flight observer does **not** resurrect the automatic
    /// default hero controller its registration suppressed — re-add one
    /// explicitly instead (the auto-default is created only at navigator
    /// mount).
    pub(crate) fn remove_observer(&self, observer: &Rc<dyn NavigatorObserver>) {
        let removed = {
            let mut observers = self.shared.observers.borrow_mut();
            observers
                .iter()
                .position(|registered| Rc::ptr_eq(registered, observer))
                .map(|position| observers.remove(position))
        };
        // Notify outside the borrow, mirroring add_observer's no-lock-held
        // attach notification.
        if let Some(removed) = removed
            && self.shared.observers_attached.get()
        {
            removed.did_detach();
        }
    }

    /// Rebuild `route`'s content subtree on the next frame, so a route whose
    /// builder reads the navigator widget's configuration re-reads it.
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
        let entry = { self.shared.registries.entries.borrow().get(&route).cloned() };
        if let Some(entry) = entry {
            entry.mark_needs_build();
        }
    }

    /// Whether the navigator is mounted; `maybe_pop` consults it.
    ///
    /// Derived from the overlay rather than a separate flag: the overlay is this
    /// navigator's only child, so it is mounted exactly when the navigator is.
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        self.shared.overlay.is_mounted()
    }

    /// Report that a user gesture (e.g. an edge swipe-back) started
    /// manipulating this navigator.
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
    ///
    /// # Panics (debug only)
    ///
    /// Debug-asserts it is never called more often than
    /// [`did_start_user_gesture`](Self::did_start_user_gesture) — an unmatched
    /// call is a caller bug.
    pub fn did_stop_user_gesture(&self) {
        self.shared.did_stop_user_gesture();
    }

    /// Whether at least one user gesture is currently in progress on this
    /// navigator.
    #[must_use]
    pub fn user_gesture_in_progress(&self) -> bool {
        self.shared.user_gesture_in_progress()
    }

    /// A snapshot handle of this navigator's user-gesture state — the
    /// live count and its notifier, bundled — for a data-plane animation
    /// listener that cannot hold this owner-affine handle. `HeroFlight` uses
    /// it to defer a terminal status update mid-gesture.
    pub(crate) fn user_gesture_signal(&self) -> UserGestureSignal {
        UserGestureSignal {
            in_progress: Rc::clone(&self.shared.user_gestures_in_progress),
            notifier: self.shared.user_gesture_in_progress_notifier.clone(),
        }
    }

    /// Whether both handles name the same navigator — an `Rc` identity check, for the
    /// shared-`HeroController` guard.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }
    /// once, on mount, after every seeded route is appended.
    ///
    /// Seed before handing the handle to [`Navigator::new`]. A deep link's
    /// synthesized back-stack is several `seed_initial` calls.
    ///
    /// # Panics
    ///
    /// Panics if route identity capacity is exhausted. Exhaustion permanently
    /// refuses further identities. The rejected route and its captures are
    /// destroyed by the unwind, before any binding or history admission.
    pub fn seed_initial<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        if let Some(refusal) = self.placement_refusal(&route) {
            return self.refuse("seed_initial", refusal, route);
        }
        self.seed_reporting_id(route).1
    }

    /// [`seed_initial`](Self::seed_initial) past the admission check, handing
    /// back the id it minted.
    fn seed_reporting_id<R: NavigatorRoute>(&self, route: R) -> (RouteId, RouteResult<R::Output>) {
        let mut route = super::lifecycle::Terminal::new(route);
        let id = self.prepare(&*route);
        (
            id,
            self.shared
                .history
                .borrow_mut()
                .seed_initial_with_id(id, route.take_value()),
        )
    }

    /// Reserve `route`'s identities, then [`prepare_reserved`](Self::prepare_reserved).
    fn prepare<R: NavigatorRoute>(&self, route: &R) -> RouteId {
        self.prepare_reserved(route, RouteReservation::reserve())
    }

    /// Fill `route`'s binding slot and insert its overlay entry under the
    /// identities in `reservation` — everything a route needs before it enters
    /// the history — and, under a `Router`, record it as a page when it is one.
    ///
    /// Spends no identity of its own: capacity refusal happened, if at all,
    /// when the reservation was made, before anything here was published.
    pub(super) fn prepare_reserved<R: NavigatorRoute>(
        &self,
        route: &R,
        reservation: RouteReservation,
    ) -> RouteId {
        let RouteReservation { route: id, entry } = reservation;
        self.record_page(id, route);
        self.bind(route, id);
        let builder = route.content_builder();
        self.shared.registries.entries.borrow_mut().insert(
            id,
            OverlayEntry::with_reserved_id(entry, move |ctx| builder(ctx))
                .with_modal(route.is_modal()),
        );
        id
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
        let queue = self.shared.history.borrow().command_queue();
        let weak: Weak<NavigatorShared> = Rc::downgrade(&self.shared);
        RouteBinding::new(
            route,
            queue,
            Rc::new(move || {
                if let Some(shared) = weak.upgrade() {
                    shared.pump_route_commands();
                }
            }),
            Rc::clone(&self.shared.vsync),
            self.shared.registries.clone(),
        )
    }

    /// Push `route`. The future is created before any lifecycle runs.
    ///
    /// The route is bound and its overlay entry inserted **before** the flush.
    /// `install()` and a zero-duration route's first animation status change both
    /// run inside `push_with_id`, and both reach for that entry.
    ///
    /// # Under a `Router`
    ///
    /// A navigator a [`Router`](crate::Router) drives admits only pageless
    /// popups here (`PopupRoute`); any other route has no path and is refused
    /// (ADR-0330 §4): it is disposed unpushed, the returned result is already
    /// complete with `Ok(None)`, the refusal is logged, and a debug build panics.
    /// `seed_initial`, `push_replacement[_with]` and `push_and_remove_until`
    /// refuse a popup too, the same way: what they replace, sweep or seed
    /// beneath belongs to the Router.
    ///
    /// # Panics
    ///
    /// Panics on a Router placement refusal in debug builds, or if route
    /// identity capacity is exhausted. Exhaustion permanently refuses further
    /// identities. The rejected route and its captures are destroyed by the
    /// unwind, before any binding or history admission (ADR-0281).
    pub fn push<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        if let Some(refusal) = self.push_refusal(&route) {
            return self.refuse("push", refusal, route);
        }
        self.push_reporting_id(route, RouteReservation::reserve).1
    }

    /// [`push`](Self::push) for a caller that later removes the route it
    /// pushed by its id; `None`, with the route refused as `push` refuses it,
    /// when the navigator does not take it.
    pub(crate) fn push_identified<R: NavigatorRoute>(&self, route: R) -> Option<RouteId> {
        if let Some(refusal) = self.push_refusal(&route) {
            drop(self.refuse("push", refusal, route));
            return None;
        }
        Some(self.push_reporting_id(route, RouteReservation::reserve).0)
    }

    /// [`push`](Self::push), also handing back the [`RouteId`] it minted.
    ///
    /// The id is minted inside `push_prepared`, so reading it back off
    /// [`current`](Self::current) afterwards would be inferring a fact the push
    /// already knows.
    fn push_reporting_id<R: NavigatorRoute>(
        &self,
        route: R,
        reserve: impl FnOnce() -> RouteReservation,
    ) -> (RouteId, RouteResult<R::Output>) {
        self.push_prepared("push", route, reserve, |history, id, route| {
            history.push_with_id(id, route).1
        })
    }

    /// Push `route` and complete the current top **as replaced** — observers see
    /// `did_replace`, never `did_remove`, and the replaced route's future resolves
    /// with `Ok(None)`.
    ///
    /// # Panics
    ///
    /// Has the placement and permanent identity-capacity panic behavior of
    /// [`push`](Self::push); the rejected route is destroyed by the capacity unwind.
    pub fn push_replacement<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        if let Some(refusal) = self.placement_refusal(&route) {
            return self.refuse("push_replacement", refusal, route);
        }
        self.push_replacement_erased(route, None)
    }

    /// [`push_replacement`](Self::push_replacement), delivering `result` to
    /// whoever awaits the **replaced** route's [`RouteResult`]. Same delivery-time
    /// type contract as
    /// [`pop_with`](Self::pop_with).
    ///
    /// # Panics
    ///
    /// Has the placement and permanent identity-capacity panic behavior of
    /// [`push`](Self::push). On capacity unwind, both the rejected route and
    /// `result` are destroyed by the unwind, before any binding or history
    /// admission.
    pub fn push_replacement_with<R: NavigatorRoute, T: 'static>(
        &self,
        route: R,
        result: T,
    ) -> RouteResult<R::Output> {
        if let Some(refusal) = self.placement_refusal(&route) {
            // The caller's value reached no route; dropped here, with no borrow held.
            drop(result);
            return self.refuse("push_replacement_with", refusal, route);
        }
        self.push_replacement_erased(route, Some(AnyResult::new(result)))
    }

    fn push_replacement_erased<R: NavigatorRoute>(
        &self,
        route: R,
        result: Option<AnyResult>,
    ) -> RouteResult<R::Output> {
        // `CurrentTop`: nothing can run between this call and the flush, so "the
        // top now" is still the caller's route.
        let target = Some(ReplaceTarget::CurrentTop);
        self.push_prepared(
            "push_replacement",
            route,
            RouteReservation::reserve,
            |history, id, route| {
                history
                    .push_replacement_with_id(id, target, route, result)
                    .1
            },
        )
        .1
    }

    /// Push `route`, then walk downward from the old top completing every present
    /// route until `keep` answers `true` — the addition and all removals share one
    /// flush. Removed routes complete their futures with `Ok(None)` and
    /// observers see `did_remove` for each.
    ///
    /// `keep` receives each route's [`RouteId`]; `|_| false` clears everything
    /// beneath the new route. FLUI routes name each other by id, so the
    /// predicate is handed an id rather than the route object (ADR-0019).
    ///
    /// `keep` runs with the history borrow **released**: a
    /// predicate that queries the handle back (`|id| handle.route_ids().first()
    /// == Some(&id)`, an "is first route" or "route with name" check) would
    /// otherwise re-enter `NavigatorShared`'s
    /// `RefCell` while it is borrowed and panic the owner thread
    /// against itself. The push and the removal-completion are two separate
    /// locked sections around that unlocked evaluation; `NavigatorHandle` is
    /// owner-thread-bound, so nothing else can interleave between them.
    ///
    /// # Panics
    ///
    /// Has the placement and permanent identity-capacity panic behavior of
    /// [`push`](Self::push), retaining the rejected route before any binding
    /// or history admission. User callbacks can also panic.
    pub fn push_and_remove_until<R: NavigatorRoute>(
        &self,
        route: R,
        keep: impl FnMut(RouteId) -> bool,
    ) -> RouteResult<R::Output> {
        if let Some(refusal) = self.placement_refusal(&route) {
            return self.refuse("push_and_remove_until", refusal, route);
        }
        // The caller's predicate outlives a capacity refusal in the push below.
        let mut keep = super::lifecycle::Terminal::new(keep);
        let (_, (result, below_top_to_bottom)) = self.push_prepared(
            "push_and_remove_until",
            route,
            RouteReservation::reserve,
            RouteHistory::push_for_remove_until_with_id,
        );

        let mut remove_ids = Vec::new();
        for candidate in below_top_to_bottom {
            if (*keep)(candidate) {
                break;
            }
            remove_ids.push(candidate);
        }

        self.shared.mutate("push_and_remove_until", |history| {
            history.complete_removed_and_flush(&remove_ids);
        });

        result
    }

    /// The shared push shape: take the route's reservation, fill its binding
    /// slot, insert its overlay entry, then `commit` against the locked history
    /// and apply the flush outcome. The route is bound and its entry inserted
    /// **before** the flush — `install()` and a zero-duration route's first
    /// status change both reach for the entry.
    ///
    /// `reserve` either mints the identities or hands back ones an operation
    /// reserved before its own first side effect. It runs with `route` and
    /// `commit` already held, so a capacity refusal retains both.
    pub(super) fn push_prepared<R: NavigatorRoute, O>(
        &self,
        operation: &'static str,
        route: R,
        reserve: impl FnOnce() -> RouteReservation,
        commit: impl FnOnce(&mut RouteHistory, RouteId, R) -> O,
    ) -> (RouteId, O) {
        let mut route = super::lifecycle::Terminal::new(route);
        let mut commit = super::lifecycle::Terminal::new(commit);
        let mut reserve = super::lifecycle::Terminal::new(reserve);
        let id = self.prepare_reserved(&*route, reserve.take_value()());

        let (result, outcome, undelivered) = {
            let mut history = self.shared.history.borrow_mut();
            let result = commit.take_value()(&mut history, id, route.take_value());
            (result, history.take_outcome(), history.take_undelivered())
        };

        // Guard released — the same drain `NavigatorShared::mutate` performs, in
        // the same order and for the same reason, and needed here too because
        // `push_replacement_with_id` runs under this lock rather than that one.
        if let Some(outcome) = outcome {
            self.shared.apply(outcome);
        }
        report_undelivered(operation, undelivered);
        (id, result)
    }

    fn pop_erased(&self, result: Option<AnyResult>) -> bool {
        self.shared.mutate("pop", |history| {
            // A refused pop's result reaches no route; the history's channel
            // reports it once the guard releases.
            if self.pop_removes_last_page(history) {
                history.record_undelivered(result);
                return false;
            }
            history.pop(result)
        })
    }

    fn remove_route_erased(&self, id: RouteId, result: Option<AnyResult>) -> bool {
        self.shared.mutate("remove_route", |history| {
            if self.removes_last_page(history, id) {
                history.record_undelivered(result);
                return false;
            }
            history.remove_route(id, result)
        })
    }

    /// Pop the top route with no result.
    ///
    /// The popped route's future resolves with its `current_result()` fallback, or
    /// `Ok(None)`. Returns whether a present route was found. A route that refuses
    /// (`Route::did_pop` → `false`) stays, and this still returns `true`.
    ///
    /// Under a [`Router`](crate::Router) the Router's last page is never
    /// popped or removed, whether it is on top or beneath a popup: this, and
    /// [`pop_with`](Self::pop_with), [`maybe_pop`](Self::maybe_pop) and
    /// [`remove_route`](Self::remove_route) of that page, return `false` and
    /// change nothing, because a Router always has a location.
    pub fn pop(&self) -> bool {
        self.pop_erased(None)
    }

    /// Pop the top route, delivering `result` to whoever awaits its
    /// [`RouteResult`].
    ///
    /// `T` is checked at **delivery**, not at the call site: the navigator holds a
    /// heterogeneous stack and cannot know the top route's `Output`. Passing the
    /// wrong type does not panic: the route completes with
    /// [`RouteResultError::TypeMismatch`](crate::RouteResultError::TypeMismatch),
    /// and the value is reported and dropped outside the history borrow.
    pub fn pop_with<T: 'static>(&self, result: T) -> bool {
        self.pop_erased(Some(AnyResult::new(result)))
    }

    /// Pop `route`, but drive its exit transition with `pacing` instead of
    /// its own default reverse pacing — for a gesture-driven pop whose pacing
    /// comes from the drag itself (fling velocity, or the flat "stay"
    /// pacing), not the route's static configuration.
    ///
    /// Returns `false` and pops nothing if `route` is no longer the current
    /// top route: a route swept away by something else between the gesture's last frame and this call must not
    /// have a stale drag finish it. Only a [`TransitionRoute`](super::transition_route::TransitionRoute)
    /// (directly, or via `ModalRoute`/`PageRoute`) consumes the pacing; a plain
    /// route's `did_pop` never asks for it.
    pub(crate) fn pop_paced(&self, route: RouteId, pacing: PopPacing) -> bool {
        if self.current() != Some(route) {
            return false;
        }
        // Both `insert` and `remove` hand back the displaced `PopPacing`, which
        // may own an `Rc<dyn Curve>` — a caller-supplied value whose
        // `Drop` is user code. The guard is the first temporary in the statement,
        // so it would otherwise still be alive when that value drops. Same rule as
        // the registry's displaced closures and the history's undelivered results.
        let displaced = {
            self.shared
                .registries
                .pop_pacing
                .borrow_mut()
                .insert(route, pacing)
        };
        drop(displaced);

        let popped = self.pop_erased(None);

        // `did_pop` consumes this on a successful pop; a refused pop (or one
        // that somehow never reached `route`) must not leave a stale override
        // for a later, unrelated pop of the same route.
        let consumed = {
            self.shared
                .registries
                .pop_pacing
                .borrow_mut()
                .remove(&route)
        };
        drop(consumed);
        popped
    }

    /// Remove `id` without popping it.
    ///
    /// **The removed route still completes its future**, with its
    /// `current_result()` fallback or `Ok(None)`. Completing only on `pop` would hang
    /// every awaiter.
    pub fn remove_route(&self, id: RouteId) -> bool {
        self.remove_route_erased(id, None)
    }

    /// Remove `id`, delivering `result`. Same type contract as
    /// [`pop_with`](NavigatorHandle::pop_with).
    pub fn remove_route_with<T: 'static>(&self, id: RouteId, result: T) -> bool {
        self.remove_route_erased(id, Some(AnyResult::new(result)))
    }

    /// Pop routes one at a time until `keep` accepts the route now on top.
    ///
    /// The route `keep` accepts is **not** popped. Unlike
    /// [`push_and_remove_until`](Self::push_and_remove_until), which removes
    /// its whole sweep in one flush, this is a bare loop over
    /// [`pop`](Self::pop): every popped route runs its full lifecycle
    /// (`did_pop`/`did_complete`/`did_pop_next`/`dispose`) and gets its own
    /// flush before the next candidate is read, exactly as repeated calls to
    /// `pop()` would. A predicate that never accepts empties the stack with
    /// no error (the "able to pop all routes" case).
    ///
    /// The loop also stops at a pop that is refused: under a
    /// [`Router`](crate::Router) it stops at the Router's last page, which
    /// [`pop`](Self::pop) never takes.
    ///
    /// `keep` receives each candidate's [`RouteId`]; same shape as
    /// [`push_and_remove_until`](Self::push_and_remove_until)'s `keep`.
    ///
    /// `keep` runs with the history borrow **released** — see
    /// [`push_and_remove_until`](Self::push_and_remove_until)'s doc for why a
    /// predicate that queries the handle back needs this. The candidate is
    /// read in one lock acquisition ([`current`](Self::current)), `keep`
    /// evaluates it unlocked, and a genuine pop is a second, independent
    /// acquisition ([`pop`](Self::pop)) — never the same critical section.
    pub fn pop_until(&self, mut keep: impl FnMut(RouteId) -> bool) {
        while let Some(candidate) = self.current() {
            if keep(candidate) || !self.pop() {
                break;
            }
        }
    }

    /// Whether a pop would remove a route (or be handled internally by one).
    #[must_use]
    pub fn can_pop(&self) -> bool {
        self.shared.history.borrow().can_pop()
    }

    /// Consult the top route's pop disposition and act on it. This is a
    /// synchronous `match` on the disposition; there is no async pop-approval
    /// step, which would buy nothing and violate the no-async-in-hot-paths rule.
    ///
    /// Returns whether the pop request was **handled**. `false` means "bubble":
    /// nobody here dealt with it, so an ancestor navigator or the system should.
    fn maybe_pop_erased(&self, result: Option<AnyResult>) -> bool {
        if !self.is_mounted() {
            // Forget about this pop, we were disposed in the meantime.
            // The caller's result has nowhere to go. Reported and dropped here —
            // no guard is held yet, but the reporting is owed either way.
            report_undelivered(
                "maybe_pop",
                Vec::from_iter(result.map(UndeliveredResult::no_target)),
            );
            return true;
        }

        // Disposition and the acted-on pop share **one** critical section:
        // deciding "Pop — an entry/route is there" and popping must not be
        // separated by a racing `entry_handle.remove()` or `remove_route`
        // retargeting the answer (ADR-0025).
        // Three of the four arms below consume nothing, and all three run **under
        // the history guard**, so the result cannot be dropped inline: its `Drop`
        // is user code. They record into the history's own undelivered channel,
        // which `mutate` drains once the guard releases — the same path every
        // other undeliverable result takes.
        //
        // `Bubble` is not an edge case: the disposition is bubble for the first
        // route and pop otherwise, so a lone route bubbles *by design*, and `maybe_pop_with` on a
        // one-route navigator took this arm every time.
        self.shared.mutate("maybe_pop", |history| {
            let Some(disposition) = history.pop_disposition_of_top() else {
                history.record_undelivered(result);
                return false;
            };
            match disposition {
                RoutePopDisposition::Bubble => {
                    history.record_undelivered(result);
                    false
                }
                // A Router's last page bubbles, as a lone route does, even
                // when a popup below it makes it not the first route.
                RoutePopDisposition::Pop if self.pop_removes_last_page(history) => {
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

    /// Consult the top route's pop disposition and act on it, with no result.
    ///
    /// Returns whether the pop request was **handled**. `false` means "bubble":
    /// nothing here dealt with it, so an ancestor navigator or the system should —
    /// which is what a lone route does (the disposition is bubble for the first
    /// route, pop otherwise).
    pub fn maybe_pop(&self) -> bool {
        self.maybe_pop_erased(None)
    }

    /// [`maybe_pop`](NavigatorHandle::maybe_pop), delivering `result` if it pops.
    pub fn maybe_pop_with<T: 'static>(&self, result: T) -> bool {
        self.maybe_pop_erased(Some(AnyResult::new(result)))
    }

    /// This navigator, then each mounted navigator enclosing it, nearest
    /// first: where a pop request this one bubbles goes next.
    pub(super) fn self_and_enclosing(&self) -> impl Iterator<Item = Self> {
        std::iter::successors(Some(self.clone()), |navigator| {
            let shared = navigator.shared.enclosing.borrow().clone()?;
            Some(Self {
                shared: shared.upgrade()?,
                _owner_affine: PhantomData,
            })
        })
    }

    /// The topmost present route.
    #[must_use]
    pub fn current(&self) -> Option<RouteId> {
        self.shared.history.borrow().current()
    }

    /// The route stack, bottom → top.
    #[must_use]
    pub fn route_ids(&self) -> Vec<RouteId> {
        self.shared.history.borrow().ids()
    }

    /// Whether `route` is present in this navigator's stack. Used by a gesture's
    /// "no longer current" fallback (`back_gesture.rs`): a route that has been swept off the stack
    /// entirely (not just covered) animates forward rather than trying to
    /// pop again.
    pub(crate) fn route_is_active(&self, route: RouteId) -> bool {
        self.shared.history.borrow().is_present(route)
    }

    /// Whether a present route sits below `route` that popping it would
    /// return to, without taking a Router's last page: what
    /// [`PageHandle::can_pop`](super::PageHandle::can_pop) answers.
    pub(crate) fn route_can_pop(&self, route: RouteId) -> bool {
        let history = self.shared.history.borrow_mut();
        history.is_present(route)
            && history
                .present_ids()
                .take_while(|id| *id != route)
                .next()
                .is_some()
            && !self.removes_last_page(&history, route)
    }

    /// Whether this navigator's UI runtime is building now. A mutation a
    /// [`PageHandle`](super::PageHandle) makes then is refused (ADR-0330 §3).
    pub(crate) fn is_building(&self) -> bool {
        self.shared.build_phase.is_building()
    }

    /// Whether `self` and `other` name the same navigator.
    pub(crate) fn same_navigator(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }

    /// The lifecycle state of `id`'s entry. Test-facing, read through
    /// `crate::__test_access::NavigatorProbe`; the same applies to the four
    /// accessors below.
    pub(crate) fn route_state(&self, id: RouteId) -> Option<super::lifecycle::RouteLifecycle> {
        self.shared.history.borrow().state_of(id)
    }

    /// The overlay entry `id`'s route presents. Test-facing: `opaque` and
    /// `maintain_state` are written through a `RouteBinding`, and this is the only
    /// way to read back what a route actually wrote.
    pub(crate) fn entry_of(&self, id: RouteId) -> Option<OverlayEntry> {
        self.shared.registries.entries.borrow().get(&id).cloned()
    }

    /// How many `RouteId -> OverlayEntry` pairs the navigator is holding.
    ///
    /// Test-facing. Must track the route count exactly: an entry left behind for
    /// a disposed route is invisible in the overlay (it was removed from *its*
    /// list) but leaks here, forever.
    pub(crate) fn tracked_entry_count(&self) -> usize {
        self.shared.registries.entries.borrow().len()
    }

    /// How many `RouteId -> RouteSubtreeCell` pairs the navigator is holding.
    ///
    /// Test-facing, and for the same reason as `tracked_entry_count`: a cell left
    /// behind for a disposed route resolves to `None` (its page is unmounted), so
    /// the leak is invisible through `route_subtree` and visible only here.
    pub(crate) fn tracked_subtree_count(&self) -> usize {
        self.shared.registries.subtrees.borrow().len()
    }

    /// `id`'s subtree cell, half by half. Test-facing; see
    /// [`RouteSubtreeCell::parts`](super::subtree::RouteSubtreeCell::parts).
    pub(crate) fn route_subtree_parts(&self, id: RouteId) -> Option<super::subtree::SubtreeParts> {
        self.shared
            .registries
            .subtrees
            .borrow_mut()
            .get(&id)
            .map(super::subtree::RouteSubtreeCell::parts)
    }

    // ── Lookup ───────────────────────────────────────────────────────────────

    /// The nearest enclosing navigator, or `None`.
    ///
    /// Clones an owned handle out under the tree borrow and returns it; it takes
    /// no second lock and consults no `GlobalKey` registry. See the module docs.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.find_state::<NavigatorState, _>(NavigatorState::handle)
    }

    /// The **root-most** navigator, which is how you push above every nested
    /// navigator.
    ///
    /// The root walk cannot find fewer navigators than the nearest walk, so no
    /// fall back to the local navigator is needed.
    #[must_use]
    pub fn maybe_of_root(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.find_root_state::<NavigatorState, _>(NavigatorState::handle)
    }
}

/// The introspection seams: everything `HeroController` reads that is
/// not already on the public surface.
///
/// Each method is one thing that would otherwise be read straight off a `Route`
/// object — which FLUI cannot reach, because routes live behind
/// `Box<dyn ErasedRoute>` inside the history's cell. Nothing here
/// hands out a borrow into the trees, and nothing takes a second lock under a
/// first.
///
/// A `HeroController` is attached to exactly one navigator and drives flights only
/// between *that* navigator's own route changes — these methods still answer only
/// about *this* navigator's stack. Cross-navigator flights still work: a hero inside
/// a nested `Navigator`'s current `PageRoute` is reachable through a
/// `NestedHeroSource`, which this navigator publishes on the nearest enclosing route
/// from every `build`, not through anything read here.
impl NavigatorHandle {
    /// Whether `id` names a [`TransitionGroup::Page`] route. Shared by
    /// `HeroController::maybe_start`'s own eligibility test and by a nested
    /// `Navigator`'s [`NestedHeroSource`] hook, which asks the identical question
    /// about its own current top route.
    pub(crate) fn is_page_route(&self, id: RouteId) -> bool {
        self.route_peer(id)
            .is_some_and(|peer| peer.group == TransitionGroup::Page)
    }

    /// This navigator's overlay, read by `HeroController` to insert a flight's
    /// `OverlayEntry`.
    ///
    /// `pub(crate)`: `Overlay` and `OverlayEntry` stay
    /// unexported, so this widens no public surface.
    pub(crate) fn overlay(&self) -> &OverlayHandle {
        &self.shared.overlay
    }

    /// What `id` publishes about its transition — its primary animation, and the
    /// family it transitions with.
    ///
    /// `None` for a route that is not a `TransitionRoute`.
    pub(crate) fn route_peer(&self, id: RouteId) -> Option<TransitionPeer> {
        self.shared.registries.peers.borrow().get(&id).cloned()
    }

    /// Where `id`'s page subtree lives.
    ///
    /// `None` unless the route is a `ModalRoute` whose page is **mounted and
    /// attached**. Resolving to `Some` says nothing about layout: ask
    /// [`PipelineOwner::box_size`] for that, which is `None` until the first
    /// layout commits. See `subtree.rs`.
    ///
    /// [`PipelineOwner::box_size`]: flui_rendering::pipeline::PipelineOwner::box_size
    pub(crate) fn route_subtree(&self, id: RouteId) -> Option<RouteSubtree> {
        self.shared.registries.subtrees.borrow().get(&id)?.resolve()
    }

    /// Whether `id` is the current (topmost present) route.
    ///
    /// Test-facing: `did_change_top` no longer asserts on it (the over-strict
    /// `is_current` check was removed because FLUI's re-entrant notification model
    /// breaks it). Read through `crate::__test_access::NavigatorProbe`.
    pub(crate) fn is_current(&self, id: RouteId) -> bool {
        self.current() == Some(id)
    }

    /// `id`'s `offstage` control and animation proxies.
    ///
    /// `None` for a route that is not a `ModalRoute`, or one already disposed.
    pub(crate) fn route_modal(&self, id: RouteId) -> Option<ModalHandle> {
        self.shared.registries.modals.borrow().get(&id).cloned()
    }

    /// Whether `route` may start an edge-swipe-back gesture right now.
    /// Requires: not the first (present) route, does not handle its own pop, the
    /// pop disposition allows it (no `PopScope` veto), and the primary animation
    /// has finished. There is no fullscreen-dialog route flag (see
    /// `PageRoute::back_gesture`'s doc), so that condition is not checked.
    ///
    /// Also `false` while a user gesture is already in progress on this
    /// navigator — the per-pointer-down predicate must not admit a second,
    /// overlapping gesture start.
    pub(crate) fn pop_gesture_enabled(&self, route: RouteId) -> bool {
        if self.user_gesture_in_progress() {
            return false;
        }
        // The veto is *this* route's own, not the top of the stack's —
        // `RouteHistory::vetoes_pop` is that per-route check. See its doc.
        let (is_first, will_handle_pop_internally, vetoes_pop) = {
            let history = self.shared.history.borrow_mut();
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
            .is_some_and(|modal| modal.primary_animation().status().is_forward_settled())
    }

    /// The binding's post-frame capability, as an owned handle.
    ///
    /// `None` before mount and after unmount, so a stale `HeroController` schedules
    /// nothing. Acquired in `init_state`; never in `build`/layout/paint.
    pub(crate) fn post_frame_handle(&self) -> Option<flui_scheduler::PostFrameHandle> {
        self.shared.post_frame.borrow().clone()
    }

    /// The render tree this navigator is mounted in, for resolving the `RenderId`s
    /// [`route_subtree`](Self::route_subtree) hands out.
    ///
    /// `None` before mount and after unmount.
    pub(crate) fn render_tree(&self) -> Option<flui_rendering::pipeline::PipelineCell> {
        self.shared.render_tree.borrow().clone()
    }
}

impl fmt::Debug for NavigatorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorHandle")
            .field("routes", &self.shared.history.borrow().len())
            .field("mounted", &self.is_mounted())
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
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Navigator {
    type State = NavigatorState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        NavigatorState {
            shared: Rc::clone(&self.handle.shared),
        }
    }
}

/// Persistent state for [`Navigator`].
///
/// Holds nothing of its own: the stack and the overlay live behind the shared
/// `Rc`, because they must be reachable from an owned handle that outlives any
/// borrow of this state.
pub struct NavigatorState {
    shared: Rc<NavigatorShared>,
}

impl fmt::Debug for NavigatorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorState")
            .field("routes", &self.shared.history.borrow().len())
            .finish_non_exhaustive()
    }
}

impl NavigatorState {
    /// The owned capability `Navigator::of` hands out. **This is the only thing a
    /// lookup takes from the state**, and it is a clone of two `Rc`s.
    fn handle(&self) -> NavigatorHandle {
        NavigatorHandle {
            shared: Rc::clone(&self.shared),
            _owner_affine: PhantomData,
        }
    }

    /// Keep this navigator's cross-flight visibility hook pointed at the nearest
    /// enclosing route's `HeroScope`: a hero inside this navigator is still
    /// invited into an *outer* flight when this navigator's own current route is
    /// a `PageRoute`. Independent of `HeroControllerScope`: only the route being
    /// current and a `PageRoute` matter.
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
    /// module docs name this honestly.
    fn sync_nested_hero_registration(&self, ctx: &dyn BuildContext) {
        let current = ctx.get::<HeroScope, _>(HeroScope::registry);
        let mut slot = self.shared.nested_hero_registration.borrow_mut();
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
        // `NavigatorHandle`) is enough for this hook.
        let weak_shared = Rc::downgrade(&self.shared);
        let source = NestedHeroSource::new(move || {
            let shared = weak_shared.upgrade()?;
            let top = shared.history.borrow().current()?;
            let is_page = shared
                .registries
                .peers
                .borrow_mut()
                .get(&top)
                .is_some_and(|peer| peer.group == TransitionGroup::Page);
            if !is_page {
                return None;
            }
            shared
                .registries
                .modals
                .borrow_mut()
                .get(&top)
                .map(ModalHandle::heroes)
        });
        new_registry.register_nested(source.clone());
        *slot = Some((new_registry, source));
    }
}

impl ViewState<Navigator> for NavigatorState {
    /// Flush the seeded initial routes, exactly once, after asserting the history
    /// is non-empty.
    ///
    /// The overlay is not mounted yet (it is this view's child, built next), so
    /// the rearrange only fills the overlay's entry list; its first `build` reads
    /// it. Mutating an unmounted `OverlayHandle` is defined behavior.
    ///
    /// Installs the pump a push's entrance-transition continuation settles
    /// the history through (`settle_wake`), alongside the three lifecycle-only
    /// captures below.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.rebind_clock(VsyncScope::maybe_of(ctx));

        // Both are *lifecycle-only* acquisitions: a `HeroController`
        // fires them from a post-frame callback, never from a frame phase.
        *self.shared.post_frame.borrow_mut() = ctx.post_frame_handle();
        let _prev = std::mem::replace(
            &mut *self.shared.render_tree.borrow_mut(),
            ctx.pipeline_owner(),
        );
        let weak: Weak<NavigatorShared> = Rc::downgrade(&self.shared);
        let pump: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.pump_route_commands();
            }
        });
        let _prev = self.shared.settle_wake.borrow_mut().replace(pump);

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
            Some(Some(controller)) => self.shared.observers.borrow_mut().push(controller),
            Some(None) => {}
            None => {
                let mut auto = self.shared.auto_hero_observer.borrow_mut();
                let mut observers = self.shared.observers.borrow_mut();
                let already_manual = observers.iter().any(|o| o.observes_hero_flights());
                if !already_manual {
                    let controller: Rc<dyn NavigatorObserver> = HeroController::new();
                    *auto = Some(Rc::clone(&controller));
                    observers.push(controller);
                }
            }
        }

        // Before the seeded flush, so the first `did_push` an observer sees is
        // already one it can act on.
        self.shared.attach_observers(&self.handle());

        // Reading the durations runs user code that may push. With the
        // observers attached and the hooks still unresolved, such a push
        // installs its route as waiting, and it adopts what resolves here
        // (ADR-0221 D2).
        self.shared
            .registries
            .set_transition_hooks(PageTransitions::hooks_of(ctx).adopt());

        debug_assert!(
            self.shared.history.borrow().len() > 0,
            "BUG: a Navigator was mounted with no routes — seed one before mounting \
             (navigator.dart:3922 asserts the same)"
        );
        self.shared.mutate("mount", |history| {
            history.flush(true);
        });
    }

    /// Builds an `Overlay` and nothing else that matters here; a
    /// `NavigationNotification` listener, pointer-cancelling `Listener` and
    /// `FocusTraversalGroup` all belong to features deferred for now.
    ///
    /// Drains no `RouteCommand`: a flush mutates route controllers, which
    /// refuse mutation during `build` (ADR-0086 §7). A push-completion
    /// continuation pumps the queue itself, in the tick that settled it.
    fn build(&self, _view: &Navigator, ctx: &dyn BuildContext) -> impl IntoView {
        // Re-resolved every build, not just once at mount — see
        // `sync_nested_hero_registration`'s doc for why `init_state` cannot do this.
        self.sync_nested_hero_registration(ctx);
        let enclosing =
            ctx.find_state::<NavigatorState, _>(|enclosing| Rc::downgrade(&enclosing.shared));
        *self.shared.enclosing.borrow_mut() = enclosing;

        // `HeroControllerScope::none`: a nested navigator under this one must not pick up this navigator's controller. It resolves the
        // `.none` in its own `init_state` and attaches nothing.
        HeroControllerScope::none(Overlay::new(self.shared.overlay.clone()))
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.rebind_clock(VsyncScope::maybe_of(ctx));
        // Reaches routes installed from now on; a route already installed
        // keeps the transition it adopted (ADR-0221 D2). The outgoing hooks
        // retire first, so a route pushed while a builder's durations are read
        // waits for the new hooks instead of adopting the old.
        let hooks = PageTransitions::hooks_of(ctx);
        self.shared.registries.suspend_transition_hooks();
        self.shared.registries.set_transition_hooks(hooks.adopt());
    }

    /// Detach the observers.
    fn deactivate(&mut self) {
        self.shared.detach_observers();
    }

    /// A navigator moved by a `GlobalKey` is deactivated and reactivated in the
    /// same frame, and its observers must survive the round trip.
    fn activate(&mut self) {
        self.shared.attach_observers(&self.handle());
    }

    /// `deactivate` does not always precede `dispose`: FLUI's
    /// `ElementBase::unmount` calls `dispose` directly, so this is the detach that
    /// actually runs on a plain unmount; `detach_observers` is idempotent, so the deactivate-then-dispose
    /// path notifies exactly once.
    fn dispose(&mut self) {
        self.shared.rebind_clock(None);
        self.shared
            .registries
            .set_transition_hooks(AdoptedTransitions::default());
        // Observers and wakers are user code: a panic in one still lets every
        // pending result complete and the rest of the teardown run, and the
        // first panic is raised once it has.
        let mut first = flui_foundation::panic::FirstPanic::new();
        first.contain(|| self.shared.detach_observers());
        self.shared.complete_terminal_results(&mut first);
        // The capabilities die with the tree they name, so a `HeroController` that
        // outlives its navigator schedules nothing and measures nothing, and a
        // push-completion continuation that outlives it finds no one to wake.
        *self.shared.post_frame.borrow_mut() = None;
        let _prev = self.shared.render_tree.borrow_mut().take();
        let _prev = self.shared.settle_wake.borrow_mut().take();
        *self.shared.enclosing.borrow_mut() = None;
        // The mirror of `sync_nested_hero_registration`'s publish: a disposed
        // navigator's heroes must never be visited by an outer flight again.
        let nested_hero_registration = self.shared.nested_hero_registration.borrow_mut().take();
        if let Some((registry, source)) = nested_hero_registration {
            registry.deregister_nested(&source);
        }
        first.resume();
    }
}
