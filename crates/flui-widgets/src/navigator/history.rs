//! [`RouteHistory`] and `flush_history_updates` — the route stack.
//!
//! Private, and **pure data**: this module touches no element tree,
//! no build owner, no render pipeline, and no overlay. `route_stack_flush_is_pure_data`
//! enforces that mechanically.
//!
//! # Flutter parity
//!
//! `navigator.dart:4451-4667` — `_flushHistoryUpdates`, `_flushObserverNotifications`,
//! `_flushRouteAnnouncement`, `_getRouteBefore` / `_getRouteAfter` — plus the
//! `_RouteEntry` handlers at `:3245-3444`.
//!
//! The whole algorithm is a function over a `Vec<RouteEntry>` and a set of
//! callbacks. It never mutates a tree. That is the observation ADR-0019 was built
//! on: `push` mutates `_history` and calls the flush; the flush's only
//! tree-visible effect is `overlay.rearrange` at the very end (`:4612`).
//!
//! # Two structural divergences, both deliberate
//!
//! 1. **The overlay rearrange is hoisted out of the flush.** Flutter ends
//!    `_flushHistoryUpdates` with `overlay?.rearrange(_allRouteOverlayEntries)`.
//!    This module has no overlay, so the flush ends after disposal and the
//!    `Navigator` view performs the rearrange immediately afterwards. Ordering
//!    is preserved: Flutter also rearranges *after* `_disposeRouteEntry`
//!    (`:4609-4613`). The `rearrangeOverlay: false` argument that `pop` and
//!    `removeRoute` pass (`:5671`, `:5747`) therefore has nothing to select
//!    here; it is recorded on [`FlushOutcome`] for the `Navigator` to honour.
//!
//! 2. **Routes are named by [`RouteId`], not by object.** Flutter passes `Route`
//!    objects to `didChangeNext` / `didChangePrevious` / `didPopNext` and to
//!    observers. Handing out `&mut dyn ErasedRoute` for one entry while the
//!    history holds the rest is not expressible; ids preserve identity, ordering
//!    and arity, which is everything the oracles assert. A `TransitionRoute`
//!    needs the *next route's animation*, so it will need a lookup handle —
//!    noted as a follow-up.

use std::fmt;
use std::sync::Arc;

use super::binding::{RouteCommand, RouteCommandQueue};
use super::lifecycle::RouteLifecycle;
use super::observer::{Notification, Observation, ObservationQueues};
use super::result::RouteResult;
use super::route::{
    AnyResult, ErasedRoute, PushCompletion, Route, RouteId, RoutePopDisposition, RouteRecord,
};

/// What was last announced to a route's `did_change_next` / `did_change_previous`.
///
/// **Not** `Option<RouteId>`. Flutter seeds these fields with a `notAnnounced`
/// sentinel, distinct from `null` (`navigator.dart:3204-3212`):
///
/// ```dart
/// static const _RoutePlaceholder notAnnounced = _RoutePlaceholder();
/// _RoutePlaceholder? lastAnnouncedPreviousRoute = notAnnounced;
/// ```
///
/// That distinction is load-bearing. On the first flush the bottom route has no
/// route below it, so `previous` is `null`; `null != notAnnounced` is **true**, and
/// `didChangePrevious(null)` fires exactly once. Collapsing the sentinel into
/// `None` makes `None != None` false and the call is silently never made — which
/// is what a parity re-check found FLUI doing. `ModalRoute` drives
/// `changedInternalState()` from `didChangePrevious`, so a bottom modal route
/// would have missed its initial internal-state init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Announced {
    /// Flutter's `notAnnounced`: nothing has been announced yet.
    Never,
    /// The last value announced, which may legitimately be "no route".
    Route(Option<RouteId>),
}

/// One route plus its bookkeeping. Flutter's `_RouteEntry` (`navigator.dart:3178`).
pub(crate) struct RouteEntry {
    route: Box<dyn ErasedRoute>,
    state: RouteLifecycle,

    /// The value a queued `pop`/`complete` will deliver. Flutter's
    /// `_RouteEntry.pendingResult` (`:3420`).
    pending_result: Option<AnyResult>,

    /// `false` when this route is being *replaced*, so it emits `did_replace`
    /// (from the new route) instead of `did_remove`. Flutter's
    /// `_reportRemovalToObserver` (`:3428`).
    report_removal_to_observer: bool,

    last_announced_next: Announced,
    last_announced_previous: Announced,
    /// Flutter's `lastAnnouncedPoppedNextRoute` (`:3209`), a `WeakReference`
    /// there only to avoid retaining a disposed route. Seeded with the same
    /// `notAnnounced` sentinel, which is what makes
    /// `should_announce_change_to_next` suppress the *first* `didChangeNext(null)`
    /// (already sent by `handle_push` / `did_add` when `is_new_first`).
    last_announced_popped_next: Announced,
}

impl RouteEntry {
    fn new(route: Box<dyn ErasedRoute>, initial_state: RouteLifecycle) -> Self {
        debug_assert!(
            matches!(
                initial_state,
                RouteLifecycle::Add
                    | RouteLifecycle::Push
                    | RouteLifecycle::PushReplace
                    | RouteLifecycle::Replace
            ),
            "BUG: a route entry may only start in add/push/pushReplace/replace \
             (navigator.dart:3184-3191)"
        );
        Self {
            route,
            state: initial_state,
            pending_result: None,
            report_removal_to_observer: true,
            last_announced_next: Announced::Never,
            last_announced_previous: Announced::Never,
            last_announced_popped_next: Announced::Never,
        }
    }

    pub(crate) fn id(&self) -> RouteId {
        self.route.id()
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> RouteLifecycle {
        self.state
    }

    /// Flutter's `_RouteEntry.pop` (`navigator.dart:3420-3425`): records the
    /// result and arms the state. It does **not** call `didPop`; the flush does.
    fn arm_pop(&mut self, result: Option<AnyResult>) {
        debug_assert!(self.state.is_present());
        self.pending_result = result;
        self.state = RouteLifecycle::Pop;
    }

    /// Flutter's `_RouteEntry.complete` (`:3430-3439`).
    ///
    /// The `>= remove` early-return is the guard that makes double completion
    /// impossible: once the entry has passed `Remove`, `did_complete` has already
    /// run and a second `remove_route` cannot re-arm it.
    fn arm_complete(&mut self, result: Option<AnyResult>, is_replaced: bool) {
        if self.state >= RouteLifecycle::Remove {
            return;
        }
        debug_assert!(self.state.is_present());
        self.report_removal_to_observer = !is_replaced;
        self.pending_result = result;
        self.state = RouteLifecycle::Complete;
    }

    /// Flutter's `_RouteEntry.handleAdd` (`:3245-3250`).
    fn handle_add(&mut self, previous_present: Option<RouteId>) -> Observation {
        debug_assert_eq!(self.state, RouteLifecycle::Add);
        self.state = RouteLifecycle::Adding;
        Observation::Push {
            route: self.id(),
            previous: previous_present,
        }
    }

    /// Flutter's `_RouteEntry.didAdd` (`:3406-3416`).
    fn did_add(&mut self, is_new_first: bool) {
        self.route.install();
        self.route.did_add();
        self.state = RouteLifecycle::Idle;
        if is_new_first {
            self.route.did_change_next(None);
        }
    }

    /// Flutter's `_RouteEntry.handlePush` (`:3252-3310`).
    ///
    /// The `pushing` state is entered only when the route reports
    /// [`PushCompletion::Animating`]; see that variant's docs for the divergence
    /// on immediate pushes.
    fn handle_push(
        &mut self,
        previous: Option<RouteId>,
        previous_present: Option<RouteId>,
        is_new_first: bool,
    ) -> Observation {
        let previous_state = self.state;
        debug_assert!(matches!(
            previous_state,
            RouteLifecycle::Push | RouteLifecycle::PushReplace | RouteLifecycle::Replace
        ));

        self.route.install();

        if matches!(
            previous_state,
            RouteLifecycle::Push | RouteLifecycle::PushReplace
        ) {
            self.state = match self.route.did_push() {
                PushCompletion::Immediate => RouteLifecycle::Idle,
                PushCompletion::Animating => RouteLifecycle::Pushing,
            };
        } else {
            self.route.did_replace(previous);
            self.state = RouteLifecycle::Idle;
        }

        if is_new_first {
            self.route.did_change_next(None);
        }

        if matches!(
            previous_state,
            RouteLifecycle::Replace | RouteLifecycle::PushReplace
        ) {
            Observation::Replace {
                new_route: Some(self.id()),
                old_route: previous_present,
            }
        } else {
            Observation::Push {
                route: self.id(),
                previous: previous_present,
            }
        }
    }

    /// Flutter's `_RouteEntry.handlePop` (`:3357-3379`).
    ///
    /// Returns whether the route consented. On consent, `did_pop` completed the
    /// future; if the route is `finished_when_popped` it is finalized straight to
    /// `Dispose` — Flutter reaches the same state through
    /// `OverlayRoute.didPop` → `navigator.finalizeRoute` (`routes.dart:87-94`),
    /// which is exactly the "pop finished synchronously" case the flush's `Pop`
    /// arm anticipates (`navigator.dart:4533`).
    fn handle_pop(&mut self) -> bool {
        self.state = RouteLifecycle::Popping;

        if self.route.is_completed() {
            // Already completed elsewhere; nothing further to do.
            return true;
        }

        let result = self.pending_result.take();
        if !self.route.did_pop(result) {
            self.state = RouteLifecycle::Idle;
            return false;
        }

        // Order matters. Flutter reaches `dispose` *inside* `didPop` —
        // `OverlayRoute.didPop` calls `navigator.finalizeRoute(this)`
        // (`routes.dart:90-92`) → `entry.finalize()` → `currentState = dispose` —
        // and only then does `handlePop` call `onPopInvokedWithResult(true, …)`
        // (`navigator.dart:3372`). So the route is already finalized when its
        // callback runs. Found by a parity re-check; matters once `PopScope`
        // callbacks can inspect navigator state.
        if self.route.finished_when_popped() {
            self.state = RouteLifecycle::Dispose;
        }
        self.route.on_pop_invoked(true);
        true
    }

    /// Flutter's `_RouteEntry.handleComplete` (`:3381-3386`).
    fn handle_complete(&mut self) {
        let result = self.pending_result.take();
        self.route.did_complete(result);
        debug_assert!(self.route.is_completed());
        self.state = RouteLifecycle::Remove;
    }

    /// Flutter's `_RouteEntry.handleRemoval` (`:3388-3404`).
    fn handle_removal(&mut self, previous_present: Option<RouteId>) -> Option<Observation> {
        self.state = if self.route.is_installed() {
            RouteLifecycle::Removing
        } else {
            // Never realized: nothing was initialized, so dispose outright.
            RouteLifecycle::Dispose
        };

        self.report_removal_to_observer
            .then(|| Observation::Remove {
                route: self.id(),
                previous: previous_present,
            })
    }

    /// Flutter's `_RouteEntry.handleDidPopNext` (`:3312`).
    fn handle_did_pop_next(&mut self, popped: RouteId) {
        self.route.did_pop_next(popped);
        self.last_announced_popped_next = Announced::Route(Some(popped));
    }

    /// Flutter's `_RouteEntry.shouldAnnounceChangeToNext` (`:3541-3546`).
    ///
    /// Suppresses a redundant `didChangeNext(null)` when the route that vanished
    /// is the one we just announced via `didPopNext`.
    fn should_announce_change_to_next(&self, next: Option<RouteId>) -> bool {
        debug_assert_ne!(Announced::Route(next), self.last_announced_next);
        !(next.is_none() && self.last_announced_popped_next == self.last_announced_next)
    }
}

/// How many `flush_once` passes one `flush` may run before we call it a bug.
///
/// A well-behaved route raises at most one command per lifecycle callback, so two
/// passes is the realistic maximum. The bound exists so a route that re-raises
/// from its own callback fails loudly instead of hanging — the same posture as
/// ADR-0017's `MAX_LAYOUT_BUILD_PASSES`.
const MAX_FLUSH_PASSES: usize = 10;

/// Everything one flush decided but did **not** do, because doing it means leaving
/// the history's mutex.
///
/// Flutter runs all of this inline at the tail of `_flushHistoryUpdates`
/// (`navigator.dart:4583-4613`). FLUI cannot: a `NavigatorObserver` holds a
/// `NavigatorHandle` and `parking_lot::Mutex` is not reentrant, so notifying an
/// observer under `history.lock()` deadlocks the moment it reads the stack it was
/// just told about. `route.dispose()` has the same shape — it runs arbitrary route
/// teardown. So the flush computes owned data and `NavigatorShared::apply` performs
/// it once the lock is released.
///
/// Not `Clone`/`PartialEq`: it owns the dying routes.
#[derive(Default)]
pub(crate) struct FlushOutcome {
    /// Flutter's `rearrangeOverlay` argument (`navigator.dart:4451`). `pop` and
    /// `remove_route` pass `false`, because `OverlayEntry.remove()` has already
    /// updated the overlay's own list.
    pub(crate) rearrange_overlay: bool,
    /// What to tell the observers, in delivery order: additions LIFO, then
    /// deletions FIFO, then `did_change_top` — per pass.
    pub(crate) notifications: Vec<Notification>,
    /// The routes disposed by this flush. Their overlay entries must be removed by
    /// the caller, **before** [`dispose_routes`](Self::dispose_routes) runs —
    /// Flutter's `_disposeRouteEntry` does both in that order (`:3978-3987`).
    pub(crate) disposed: Vec<RouteId>,
    /// The dying entries themselves, moved out of the history so the caller can
    /// run `Route::dispose` outside the lock.
    dying: Vec<RouteEntry>,
    /// Everything the flush owes to **user code**, in the order the flush
    /// produced it.
    ///
    /// These callbacks must run under no lock: user code may call straight back
    /// into the navigator, and the history mutex is not reentrant, so firing
    /// them inline deadlocks the same thread. The flush therefore *records*
    /// them; the caller drains them once the lock is released.
    ///
    /// **One ordered channel, not one vector per kind.** A flush can run
    /// several passes, so a pop in pass 1 and a refusal in pass 2 must reach
    /// the user in that order; per-kind vectors would deliver all of one kind
    /// before the other and invert an ordering the flush had already decided.
    /// Ordering lives in the data, not in the draining code's statement order —
    /// and a new kind of deferred effect adds a variant here, not a fourth
    /// vector with a fourth loop and a fourth registry-lock storm.
    pub(crate) deferred: Vec<DeferredEffect>,
}

/// A user-visible effect the flush owes, delivered after the history lock is
/// released, in the order it was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferredEffect {
    /// `onPopInvokedWithResult(did_pop, …)` for this route's `PopScope`s
    /// (`navigator.dart:3372`; `:5612` for a refusal).
    PopInvoked(RouteId, bool),
    /// A `did_pop` that refused — it may have consumed a local-history entry
    /// (`routes.dart:950-965`), whose `on_remove` is owed on the route's
    /// registry. A plain refusal drains to nothing.
    LocalHistoryPopped(RouteId),
}

impl FlushOutcome {
    /// Fold a follow-up pass's outcome into this one, so the caller applies the
    /// union of everything a single `flush` did. Notifications keep pass order.
    fn absorb(&mut self, later: Self) {
        self.rearrange_overlay |= later.rearrange_overlay;
        self.notifications.extend(later.notifications);
        self.deferred.extend(later.deferred);
        self.disposed.extend(later.disposed);
        self.dying.extend(later.dying);
    }

    /// `entry.route.dispose()` for every route this flush killed — Flutter's
    /// `_disposeRouteEntry` tail (`navigator.dart:3978-3987`).
    ///
    /// Must run **after** the observers have been notified (`observer:pop` precedes
    /// `dispose`, `:4585` vs `:4608`) and after the caller has removed each route's
    /// overlay entries.
    pub(crate) fn dispose_routes(&mut self) {
        for mut entry in self.dying.drain(..) {
            entry.route.dispose();
            entry.state = RouteLifecycle::Disposed;
        }
    }
}

impl fmt::Debug for FlushOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlushOutcome")
            .field("rearrange_overlay", &self.rearrange_overlay)
            .field("notifications", &self.notifications)
            .field("disposed", &self.disposed)
            .finish_non_exhaustive()
    }
}

/// The route stack. Flutter's `NavigatorState._history` plus the flush.
#[derive(Default)]
pub(crate) struct RouteHistory {
    entries: Vec<RouteEntry>,
    queues: ObservationQueues,
    last_topmost: Option<RouteId>,
    /// Flutter's `_flushingHistory` + `_debugLocked` (`:4453`).
    flushing: bool,
    /// What the most recent flush left for the caller to apply. Flutter performs
    /// the overlay work inline; this module is pure data, so it hands it out instead.
    last_outcome: Option<FlushOutcome>,
    /// Lifecycle transitions raised by routes through a `RouteBinding`.
    /// Drained at the head of every flush, and again after each
    /// pass, so a command raised *during* the walk settles before `flush` returns.
    commands: RouteCommandQueue,
    /// How many `flush_once` passes the last `flush` ran. Test-facing: a deferred
    /// command must cost exactly one extra pass, not a loop.
    #[cfg(test)]
    last_flush_passes: usize,
}

impl RouteHistory {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The queue a [`RouteBinding`](super::binding::RouteBinding) writes to.
    /// Cloned into every binding the navigator mints.
    ///
    pub(crate) fn command_queue(&self) -> RouteCommandQueue {
        Arc::clone(&self.commands)
    }

    /// Whether any route has raised a command that has not been applied.
    ///
    pub(crate) fn has_pending_commands(&self) -> bool {
        !self.commands.lock().is_empty()
    }

    /// How many passes the last `flush` ran.
    #[cfg(test)]
    pub(crate) fn last_flush_passes(&self) -> usize {
        self.last_flush_passes
    }

    /// Apply every queued [`RouteCommand`], returning whether any changed a state.
    ///
    /// A command naming a route that has since been disposed and dropped is
    /// discarded — Flutter's equivalent guards are the `currentState == pushing`
    /// check in `whenCompleteOrCancel` (`navigator.dart:3277`) and
    /// `finalizeRoute`'s history lookup.
    fn apply_pending_commands(&mut self) -> bool {
        debug_assert!(
            !self.flushing,
            "BUG: route commands must be applied between flush passes, never during one"
        );

        let drained: Vec<RouteCommand> = self.commands.lock().drain(..).collect();
        let mut changed = false;

        for command in drained {
            match command {
                RouteCommand::PushCompleted(id) => {
                    if let Some(entry) = self.entry_mut(id)
                        && entry.state == RouteLifecycle::Pushing
                    {
                        entry.state = RouteLifecycle::Idle;
                        changed = true;
                    }
                }
                RouteCommand::Finalize(id) => {
                    if let Some(entry) = self.entry_mut(id)
                        && entry.state < RouteLifecycle::Dispose
                    {
                        // Flutter's `entry.finalize()` (`navigator.dart:3441-3444`).
                        entry.state = RouteLifecycle::Dispose;
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    fn entry_mut(&mut self, id: RouteId) -> Option<&mut RouteEntry> {
        self.entries.iter_mut().find(|entry| entry.id() == id)
    }

    /// Take what the most recent flush left to apply. `None` if already taken.
    pub(crate) fn take_outcome(&mut self) -> Option<FlushOutcome> {
        self.last_outcome.take()
    }

    /// Flutter's `NavigatorState.canPop` (`navigator.dart:5551-5566`), which walks
    /// the present routes **bottom-up**: no routes → `false`; the *first* one
    /// handles pops internally → `true`; only one → `false`; otherwise `true`.
    pub(crate) fn can_pop(&self) -> bool {
        let mut present = self.entries.iter().filter(|entry| entry.state.is_present());
        let Some(first) = present.next() else {
            return false;
        };
        if first.route.will_handle_pop_internally() {
            return true;
        }
        present.next().is_some()
    }

    /// The top present route's `popDisposition` (`navigator.dart:382-390`):
    /// `isFirst ? bubble : pop`, unless the route handles the pop itself or a
    /// `PopScope` vetoes it. Veto first, exactly as `ModalRoute.popDisposition`
    /// checks its `_popEntries` before `super.popDisposition`
    /// (`routes.dart:2033-2042` over the `LocalHistoryRoute` layer at `:940-946`).
    pub(crate) fn pop_disposition_of_top(&self) -> Option<RoutePopDisposition> {
        let present: Vec<&RouteEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.state.is_present())
            .collect();
        let top = present.last()?;
        if top.route.vetoes_pop() {
            return Some(RoutePopDisposition::DoNotPop);
        }
        if top.route.will_handle_pop_internally() {
            return Some(RoutePopDisposition::Pop);
        }
        Some(if present.len() == 1 {
            RoutePopDisposition::Bubble
        } else {
            RoutePopDisposition::Pop
        })
    }

    /// Tell the top present route its pop was refused
    /// (`onPopInvokedWithResult(false, result)`, `navigator.dart:5612`).
    ///
    /// The route hook fires here; the user-facing `PopScope` fan-out is owed
    /// through the outcome, so `mutate`'s `apply` delivers it **outside** the
    /// history lock — a callback may call back into the navigator.
    pub(crate) fn notify_pop_refused(&mut self) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rfind(|entry| entry.state.is_present())
        {
            entry.route.on_pop_invoked(false);
            let refused = entry.id();
            self.last_outcome
                .get_or_insert_with(FlushOutcome::default)
                .deferred
                .push(DeferredEffect::PopInvoked(refused, false));
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn ids(&self) -> Vec<RouteId> {
        self.entries.iter().map(RouteEntry::id).collect()
    }

    /// The state of `id`'s entry, or `None` once disposed and dropped.
    #[cfg(test)]
    pub(crate) fn state_of(&self, id: RouteId) -> Option<RouteLifecycle> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(RouteEntry::state)
    }

    /// Whether `id` names a route that is both in this stack and present —
    /// Flutter's `Route.isActive` (`navigator.dart:584-643`: `navigator!
    /// .contains(this) && entry.isPresent`, collapsed into one lookup since a
    /// route not in this navigator's stack cannot be found at all).
    pub(crate) fn is_present(&self, id: RouteId) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.id() == id && entry.state.is_present())
    }

    /// `id`'s own `Route::will_handle_pop_internally` — e.g. a non-empty
    /// `LocalHistoryRoute` claims the pop. `None` if `id` names no entry
    /// (already disposed and dropped, or never existed).
    pub(crate) fn will_handle_pop_internally(&self, id: RouteId) -> Option<bool> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| entry.route.will_handle_pop_internally())
    }

    /// `id`'s own `Route::vetoes_pop` — a registered `PopScope` with
    /// `can_pop = false` on **this** route specifically. `None` if `id`
    /// names no entry.
    ///
    /// This is *not* `pop_disposition_of_top`: Flutter's
    /// `ModalRoute.popDisposition` (`routes.dart`) walks `this` route's own
    /// `_popEntries`, then falls back to `Route.popDisposition`'s `isFirst ?
    /// bubble : pop` (`navigator.dart`) — which never itself yields
    /// `doNotPop`. So `popDisposition == RoutePopDisposition.doNotPop` in
    /// `popGestureEnabled` collapses to exactly this route's own veto check,
    /// for *this* route, not necessarily the top of the stack.
    pub(crate) fn vetoes_pop(&self, id: RouteId) -> Option<bool> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| entry.route.vetoes_pop())
    }

    /// The bottom-most **present** route — Flutter's `Route.isFirst`
    /// (`navigator.dart:601-611`): `_firstRouteEntryWhereOrNull(isPresentPredicate)`.
    /// Read by `NavigatorHandle::pop_gesture_enabled`'s `isFirst` check.
    pub(crate) fn first_present(&self) -> Option<RouteId> {
        self.entries
            .iter()
            .find(|entry| entry.state.is_present())
            .map(RouteEntry::id)
    }

    /// The topmost present route. Flutter's `_lastRouteEntryWhereOrNull(isPresent)`.
    pub(crate) fn current(&self) -> Option<RouteId> {
        self.entries
            .iter()
            .rfind(|entry| entry.state.is_present())
            .map(RouteEntry::id)
    }

    /// The route a user gesture (e.g. an edge swipe-back) is manipulating,
    /// and the route beneath it a completed pop would reveal — Flutter's
    /// inline resolution inside `NavigatorState.didStartUserGesture`
    /// (`navigator.dart:5826-5841`): scans with `willBePresentPredicate`
    /// (a route mid-push counts, unlike `current`'s `isPresent`), and leaves
    /// `previous` `None` when the top route handles its own pop (a
    /// `LocalHistoryRoute` swallows it internally, so there is nothing
    /// "beneath" from the gesture's point of view).
    pub(crate) fn top_and_previous_for_gesture(&self) -> Option<(RouteId, Option<RouteId>)> {
        let route_index = self
            .entries
            .iter()
            .rposition(|entry| entry.state.will_be_present())?;
        let top_entry = &self.entries[route_index];
        let top = top_entry.id();
        let previous = if top_entry.route.will_handle_pop_internally() || route_index == 0 {
            None
        } else {
            // Safety of the cast: `route_index > 0` here, and `entries.len()`
            // is bounded well under `isize::MAX` for any real route stack.
            self.route_before((route_index - 1) as isize, RouteLifecycle::will_be_present)
        };
        Some((top, previous))
    }

    // ── Public mutations (each ends in a flush, as in Flutter) ───────────────

    /// Seed an initial route **without flushing**.
    ///
    /// Flutter's `restoreState` appends *every* route `onGenerateInitialRoutes`
    /// produced and then calls `_flushHistoryUpdates()` exactly once
    /// (`navigator.dart:3900-3934`). That single flush is what makes a deep link
    /// like `/a/b` announce its whole synthesized back-stack in one batch — and
    /// it is the only way to observe the additions queue's LIFO drain.
    ///
    /// Entries enter in `Add`: no push transition, but observers still see a push
    /// observation (`handleAdd`, `:3249`).
    #[cfg(test)]
    pub(crate) fn seed_initial<R: Route>(&mut self, route: R) -> (RouteId, RouteResult<R::Output>) {
        let (erased, result) = RouteRecord::erase(route);
        let id = erased.id();
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Add));
        (id, result)
    }

    /// `seed_initial`, under an id the caller minted so it can bind the route
    /// first. A seeded `PageRoute` needs its binding before
    /// `install()`, exactly as a pushed one does.
    pub(crate) fn seed_initial_with_id<R: Route>(
        &mut self,
        id: RouteId,
        route: R,
    ) -> RouteResult<R::Output> {
        let (erased, result) = RouteRecord::erase_with_id(id, route);
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Add));
        result
    }

    /// Seed one initial route and flush — the common single-route bootstrap.
    ///
    /// Test-only: `NavigatorState::init_state` seeds without flushing and flushes
    /// once on mount, as Flutter's `restoreState` does.
    #[cfg(test)]
    pub(crate) fn add_initial<R: Route>(&mut self, route: R) -> (RouteId, RouteResult<R::Output>) {
        let seeded = self.seed_initial(route);
        self.flush(true);
        seeded
    }

    /// Flutter's `NavigatorState.push` (`:5060-5063`): append, flush, and return
    /// the future that was created before any lifecycle ran.
    #[cfg(test)]
    pub(crate) fn push<R: Route>(&mut self, route: R) -> (RouteId, RouteResult<R::Output>) {
        self.push_with_id(RouteId::next(), route)
    }

    /// `push`, under an id the caller minted.
    pub(crate) fn push_with_id<R: Route>(
        &mut self,
        id: RouteId,
        route: R,
    ) -> (RouteId, RouteResult<R::Output>) {
        let (erased, result) = RouteRecord::erase_with_id(id, route);
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Push));
        self.flush(true);
        (id, result)
    }

    /// Flutter's `NavigatorState.pushReplacement` (`:5245-5268`): complete the
    /// current top with `is_replaced = true` (so it emits **no** `did_remove`),
    /// append the new route in `PushReplace`, then a single flush.
    #[cfg(test)]
    pub(crate) fn push_replacement<R: Route>(
        &mut self,
        route: R,
        result: Option<AnyResult>,
    ) -> (RouteId, RouteResult<R::Output>) {
        self.push_replacement_with_id(RouteId::next(), route, result)
    }

    /// `push_replacement`, under an id the caller minted —
    /// the [`push_with_id`](Self::push_with_id) split, so `NavigatorHandle` can bind
    /// the route and insert its overlay entry before the flush.
    pub(crate) fn push_replacement_with_id<R: Route>(
        &mut self,
        id: RouteId,
        route: R,
        result: Option<AnyResult>,
    ) -> (RouteId, RouteResult<R::Output>) {
        if let Some(top) = self.last_present_index() {
            self.entries[top].arm_complete(result, true);
        }
        let (erased, route_result) = RouteRecord::erase_with_id(id, route);
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::PushReplace));
        self.flush(true);
        (id, route_result)
    }

    /// Flutter's `NavigatorState.pushAndRemoveUntil` → `_pushEntryAndRemoveUntil`
    /// (`navigator.dart:5347-5371`): append the new route, then walk **downward**
    /// from the old top completing every present route with `None` until `keep`
    /// says stop — all before a **single** flush.
    ///
    /// This is the one Flutter API that puts an addition and several deletions in
    /// one flush, which is what makes the additions-before-deletions ordering and
    /// the deletions' FIFO drain observable.
    #[cfg(test)]
    pub(crate) fn push_and_remove_until<R: Route>(
        &mut self,
        route: R,
        keep: impl Fn(RouteId) -> bool,
    ) -> (RouteId, RouteResult<R::Output>) {
        self.push_and_remove_until_with_id(RouteId::next(), route, keep)
    }

    /// `push_and_remove_until`, under an id the caller minted — the
    /// [`push_with_id`](Self::push_with_id) split.
    ///
    /// Test-only. Production (`NavigatorHandle::push_and_remove_until`) uses
    /// the split [`push_for_remove_until_with_id`](Self::push_for_remove_until_with_id)
    /// and [`complete_removed_and_flush`](Self::complete_removed_and_flush)
    /// pair instead, so `keep` never runs inside this module's lock. This
    /// single-locked-section shape stays for the `RouteHistory`-direct
    /// harness in `tests.rs`, which targets flush ordering, not the
    /// `NavigatorHandle` locking concern.
    #[cfg(test)]
    pub(crate) fn push_and_remove_until_with_id<R: Route>(
        &mut self,
        id: RouteId,
        route: R,
        keep: impl Fn(RouteId) -> bool,
    ) -> (RouteId, RouteResult<R::Output>) {
        let mut index = self.entries.len() as isize - 1;

        let (erased, result) = RouteRecord::erase_with_id(id, route);
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Push));

        while index >= 0 && !keep(self.entries[index as usize].id()) {
            let entry = &mut self.entries[index as usize];
            if entry.state.is_present() {
                // Removed routes complete with `None` (`navigator.dart:5360`).
                entry.arm_complete(None, false);
            }
            index -= 1;
        }

        self.flush(true);
        (id, result)
    }

    /// The push half of `NavigatorHandle::push_and_remove_until`, split from
    /// the removal-completion half so the caller can evaluate `keep` with
    /// the history lock **released**: a `RoutePredicate` that queries the
    /// handle back — Flutter's `route.isFirst` / `ModalRoute.withName`
    /// shape — must not run inside this module's locked section, or it
    /// deadlocks the owner thread against its own non-reentrant
    /// `parking_lot::Mutex`.
    ///
    /// Appends the new route in `Push`, **without** flushing or evaluating
    /// any predicate, and hands back every existing entry's id, top-to-
    /// bottom, exactly as they stood immediately before the push — the same
    /// walk order the test-only, single-locked-section `push_and_remove_until_with_id`
    /// used to compute inline. [`complete_removed_and_flush`](Self::complete_removed_and_flush)
    /// is the second half, run under a second, separate lock acquisition.
    pub(crate) fn push_for_remove_until_with_id<R: Route>(
        &mut self,
        id: RouteId,
        route: R,
    ) -> (RouteResult<R::Output>, Vec<RouteId>) {
        let below_top_to_bottom: Vec<RouteId> =
            self.entries.iter().rev().map(RouteEntry::id).collect();

        let (erased, result) = RouteRecord::erase_with_id(id, route);
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Push));

        (result, below_top_to_bottom)
    }

    /// The removal half of `push_and_remove_until`: complete every present
    /// entry named in `remove_ids` — Flutter's `entry.remove()` — then flush
    /// **once**, so the push and every removal the caller's `keep` decided
    /// on land in a single flush (`navigator.dart:5347-5371`), exactly as
    /// `push_and_remove_until_with_id`'s single-locked-section version did.
    pub(crate) fn complete_removed_and_flush(&mut self, remove_ids: &[RouteId]) {
        for &target in remove_ids {
            if let Some(entry) = self.entry_mut(target)
                && entry.state.is_present()
            {
                // Removed routes complete with `None` (`navigator.dart:5360`).
                entry.arm_complete(None, false);
            }
        }
        self.flush(true);
    }

    /// Flutter's `NavigatorState.pop` (`:5642-5675`). Returns whether a present
    /// route was found to arm; a route that *refuses* the pop (`did_pop` →
    /// `false`, e.g. a local-history entry consumed instead) still counts —
    /// Flutter's `pop` is `void` and reports nothing either. (This doc
    /// previously claimed `false` on refusal; the code was right.)
    pub(crate) fn pop(&mut self, result: Option<AnyResult>) -> bool {
        let Some(index) = self.last_present_index() else {
            return false;
        };
        self.entries[index].arm_pop(result);
        if self.entries[index].state == RouteLifecycle::Pop {
            self.flush(false);
        }
        true
    }

    /// Flutter's `NavigatorState.removeRoute` (`:5733-5751`).
    ///
    /// **The removed route still completes its future.** `arm_complete` →
    /// `handle_complete` → `did_complete` (`:3381-3386`). A port that completed
    /// only on `pop` would hang every `await` in an app that uses this.
    pub(crate) fn remove_route(&mut self, id: RouteId, result: Option<AnyResult>) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.id() == id) else {
            return false;
        };
        self.entries[index].arm_complete(result, false);
        self.flush(false);
        true
    }

    // ── The flush ────────────────────────────────────────────────────────────

    fn last_present_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|entry| entry.state.is_present())
    }

    /// Flutter's `_getIndexBefore` (`:4674`): scan downward from `index`.
    fn route_before(&self, index: isize, predicate: fn(RouteLifecycle) -> bool) -> Option<RouteId> {
        let mut index = index;
        while index >= 0 {
            let entry = &self.entries[index as usize];
            if predicate(entry.state) {
                return Some(entry.id());
            }
            index -= 1;
        }
        None
    }

    /// Flutter's `_getRouteAfter` (`:4682`): scan upward from `index`.
    fn route_after(&self, index: usize, predicate: fn(RouteLifecycle) -> bool) -> Option<RouteId> {
        self.entries[index.min(self.entries.len())..]
            .iter()
            .find(|entry| predicate(entry.state))
            .map(RouteEntry::id)
    }

    /// Flutter's `_flushHistoryUpdates` (`navigator.dart:4451-4619`), transcribed.
    ///
    /// The reverse walk, `can_remove_or_add`, the `popped_route` /
    /// `seen_top_active_route` pair, deferred disposal, then observers →
    /// announcements → `did_change_top` → dispose.
    ///
    /// # Panics
    ///
    /// If re-entered. Flutter guards the same invariant with `_debugLocked` +
    /// `_flushingHistory` (`:4452-4453`). A route's transition callback firing
    /// mid-flush is the way in, so this is a framework invariant and
    /// `PANIC-POLICY` permits the panic.
    pub(crate) fn flush(&mut self, rearrange_overlay: bool) {
        // A *recursive* `flush` is still forbidden and still loud. Route callbacks
        // no longer reach this path: they enqueue a `RouteCommand` instead
        // (see `binding.rs` Correction 1), so this assert now guards
        // only genuine framework misuse.
        assert!(
            !self.flushing,
            "BUG: flush_history_updates re-entered — a route lifecycle callback \
             mutated the history while it was being flushed"
        );

        // Commands raised since the last flush (e.g. an animation status listener
        // firing between frames) take effect before the walk sees the history.
        self.apply_pending_commands();

        let mut outcome = self.flush_once(rearrange_overlay);
        let mut passes = 1;

        // A command raised *during* the walk — a zero-duration transition
        // completing inside its own `did_push`, or `finalize` from `did_pop` —
        // is applied here and settled by another pass. This is what Flutter gets
        // from `finalizeRoute`'s `if (!_flushingHistory)` plus the microtask that
        // carries `whenCompleteOrCancel`.
        while self.apply_pending_commands() {
            passes += 1;
            assert!(
                passes <= MAX_FLUSH_PASSES,
                "BUG: route commands did not converge after {MAX_FLUSH_PASSES} flush passes — \
                 a route is re-raising a command from its own lifecycle callback"
            );
            // `rearrange_overlay: false` — a follow-up pass only disposes and
            // settles; `OverlayEntry::remove` has already updated the overlay's
            // own list, exactly as Flutter's `finalizeRoute` argues (`:5827`).
            outcome.absorb(self.flush_once(false));
        }

        #[cfg(test)]
        {
            self.last_flush_passes = passes;
        }

        // Absorb rather than overwrite: an outcome that was never taken owns dying
        // routes, and dropping it would skip their `dispose()`.
        match &mut self.last_outcome {
            Some(pending) => pending.absorb(outcome),
            None => self.last_outcome = Some(outcome),
        }
    }

    /// One walk of the history, with `flushing` held for its duration.
    fn flush_once(&mut self, rearrange_overlay: bool) -> FlushOutcome {
        self.flushing = true;
        let outcome = self.flush_inner(rearrange_overlay);
        self.flushing = false;
        outcome
    }

    #[allow(clippy::too_many_lines)] // A 1:1 transcription; splitting it would scramble the mapping.
    fn flush_inner(&mut self, rearrange_overlay: bool) -> FlushOutcome {
        let mut index: isize = self.entries.len() as isize - 1;
        let mut next: Option<RouteId> = None;
        let mut deferred: Vec<DeferredEffect> = Vec::new();
        let mut can_remove_or_add = false;
        let mut popped_route: Option<RouteId> = None;
        let mut seen_top_active_route = false;
        let mut to_be_disposed: Vec<RouteEntry> = Vec::new();

        while index >= 0 {
            let position = index as usize;
            let state = self.entries[position].state;

            // Advance to the next entry (Flutter's loop tail), unless a `continue`
            // arm re-processes this index with a new state.
            let mut advance = true;

            match state {
                RouteLifecycle::Add => {
                    let previous_present = self.route_before(index - 1, RouteLifecycle::is_present);
                    let observation = self.entries[position].handle_add(previous_present);
                    self.queues.enqueue(observation);
                    advance = false;
                }

                RouteLifecycle::Adding => {
                    if can_remove_or_add || next.is_none() {
                        self.entries[position].did_add(next.is_none());
                        advance = false;
                    }
                }

                RouteLifecycle::Push | RouteLifecycle::PushReplace | RouteLifecycle::Replace => {
                    let previous = (index > 0).then(|| self.entries[position - 1].id());
                    let previous_present = self.route_before(index - 1, RouteLifecycle::is_present);
                    let observation = self.entries[position].handle_push(
                        previous,
                        previous_present,
                        next.is_none(),
                    );
                    self.queues.enqueue(observation);
                    if self.entries[position].state == RouteLifecycle::Idle {
                        advance = false;
                    }
                }

                RouteLifecycle::Pushing => {
                    if !seen_top_active_route && let Some(popped) = popped_route {
                        self.entries[position].handle_did_pop_next(popped);
                    }
                    seen_top_active_route = true;
                }

                RouteLifecycle::Idle => {
                    if !seen_top_active_route && let Some(popped) = popped_route {
                        self.entries[position].handle_did_pop_next(popped);
                    }
                    seen_top_active_route = true;
                    // A settled route covers everything below: routes beneath may
                    // now be silently added or disposed.
                    can_remove_or_add = true;
                }

                RouteLifecycle::Pop => {
                    if self.entries[position].handle_pop() {
                        // The user-facing `PopScope` fan-out is owed but NOT
                        // fired here — deferred through the outcome so it runs
                        // outside the history lock (see `FlushOutcome::pop_invoked`).
                        deferred.push(DeferredEffect::PopInvoked(
                            self.entries[position].id(),
                            true,
                        ));
                        if !seen_top_active_route {
                            if let Some(popped) = popped_route {
                                self.entries[position].handle_did_pop_next(popped);
                            }
                            popped_route = Some(self.entries[position].id());
                        }
                        let previous_present =
                            self.route_before(index, RouteLifecycle::will_be_present);
                        self.queues.enqueue(Observation::Pop {
                            route: self.entries[position].id(),
                            previous: previous_present,
                        });

                        if self.entries[position].state == RouteLifecycle::Dispose {
                            // The pop finished synchronously (no exit transition).
                            advance = false;
                        } else {
                            debug_assert_eq!(self.entries[position].state, RouteLifecycle::Popping);
                            can_remove_or_add = true;
                        }
                    } else {
                        // The route refused the pop; it returns to `Idle`. A
                        // local-history pop is one kind of refusal — its owed
                        // `on_remove` drains in `apply`, outside this lock.
                        deferred.push(DeferredEffect::LocalHistoryPopped(
                            self.entries[position].id(),
                        ));
                        debug_assert_eq!(self.entries[position].state, RouteLifecycle::Idle);
                        advance = false;
                    }
                }

                RouteLifecycle::Popping => {}

                RouteLifecycle::Complete => {
                    self.entries[position].handle_complete();
                    debug_assert_eq!(self.entries[position].state, RouteLifecycle::Remove);
                    advance = false;
                }

                RouteLifecycle::Remove => {
                    // A route that was never installed exits as if it had never
                    // been here, and must not announce its presence.
                    if !seen_top_active_route && self.entries[position].route.is_installed() {
                        if let Some(popped) = popped_route {
                            self.entries[position].handle_did_pop_next(popped);
                        }
                        popped_route = None;
                    }
                    let previous_present =
                        self.route_before(index, RouteLifecycle::will_be_present);
                    if let Some(observation) =
                        self.entries[position].handle_removal(previous_present)
                    {
                        self.queues.enqueue(observation);
                    }
                    debug_assert!(self.entries[position].state >= RouteLifecycle::Removing);
                    advance = false;
                }

                RouteLifecycle::Removing => {
                    if can_remove_or_add || next.is_none() {
                        self.entries[position].state = RouteLifecycle::Dispose;
                        advance = false;
                    }
                }

                RouteLifecycle::Dispose => {
                    // Delay disposal until didChangeNext/didChangePrevious have
                    // been sent (navigator.dart:4571).
                    to_be_disposed.push(self.entries.remove(position));
                    // `next` is unchanged: Flutter sets `entry = next` before the
                    // loop tail re-assigns `next = entry`.
                    index -= 1;
                    continue;
                }

                RouteLifecycle::Disposed => {
                    debug_assert!(false, "BUG: a disposed entry is still in the history");
                }
            }

            if advance {
                next = Some(self.entries[position].id());
                index -= 1;
            }
        }

        // What to tell the observers about route changes — computed, not sent.
        // Flutter's `_flushObserverNotifications` (`navigator.dart:4584-4585`).
        let mut notifications: Vec<Notification> = self
            .queues
            .drain()
            .into_iter()
            .map(Notification::Observed)
            .collect();

        // Now that the list is clean, send the didChangeNext/didChangePrevious
        // notifications. These stay here: they take `&mut` on the entries, so they
        // cannot leave the borrow. See `observer.rs` for the ordering divergence
        // this buys — they precede the observer callbacks rather than following
        // them, and are invisible through the observer surface.
        self.flush_route_announcement();

        let last = self.current();
        if let Some(top) = last
            && self.last_topmost != Some(top)
        {
            notifications.push(Notification::TopChanged {
                top,
                previous_top: self.last_topmost,
            });
        }
        self.last_topmost = last;

        // Lastly, hand the marked entries to the caller. Flutter disposes them here
        // (`_disposeRouteEntry`, `:4607-4609`), inline; `Route::dispose` runs
        // arbitrary teardown — an animation controller, a vsync unregistration, a
        // route below releasing its secondary animation — and none of it belongs
        // under the history's mutex.
        let disposed = to_be_disposed.iter().map(RouteEntry::id).collect();

        FlushOutcome {
            rearrange_overlay,
            notifications,
            disposed,
            dying: to_be_disposed,
            deferred,
        }
    }

    /// Flutter's `_flushRouteAnnouncement` (`navigator.dart:4638-4667`).
    fn flush_route_announcement(&mut self) {
        let mut index: isize = self.entries.len() as isize - 1;
        while index >= 0 {
            let position = index as usize;
            if !self.entries[position].state.suitable_for_announcement() {
                index -= 1;
                continue;
            }

            let next = self.route_after(
                position + 1,
                RouteLifecycle::suitable_for_transition_animation,
            );
            if Announced::Route(next) != self.entries[position].last_announced_next {
                if self.entries[position].should_announce_change_to_next(next) {
                    self.entries[position].route.did_change_next(next);
                }
                // Updated even when the announcement was suppressed — Flutter does
                // the same (`navigator.dart:4651-4656`).
                self.entries[position].last_announced_next = Announced::Route(next);
            }

            let previous =
                self.route_before(index - 1, RouteLifecycle::suitable_for_transition_animation);
            if Announced::Route(previous) != self.entries[position].last_announced_previous {
                self.entries[position].route.did_change_previous(previous);
                self.entries[position].last_announced_previous = Announced::Route(previous);
            }

            index -= 1;
        }
    }
}

#[cfg(test)]
impl RouteHistory {
    /// Force the re-entrancy flag, so `reentrant_flush_panics_with_bug` can
    /// exercise the guard directly.
    ///
    /// Through this module's public surface re-entrancy is *structurally*
    /// unreachable: a `Route` hook receives only `&mut self` and cannot reach
    /// the history. The guard exists for the case where a zero-duration
    /// transition's completion callback re-enters via `notify_push_completed`
    /// mid-flush. Testing it directly rather than shipping it untested
    /// follows established precedent.
    pub(crate) fn force_flushing_for_test(&mut self) {
        self.flushing = true;
    }
}
