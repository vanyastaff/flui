//! [`RouteHistory`] and `flush_history_updates` — the route stack.
//!
//! Private, and **pure data**: this module touches no element tree,
//! no build owner, no render pipeline, and no overlay.
//!
//! # The algorithm
//!
//! The whole algorithm is a function over a `Vec<RouteEntry>` and a set of
//! callbacks. It never mutates a tree. That is the observation ADR-0019 was built
//! on: `push` mutates the history and calls the flush; the flush's only
//! tree-visible effect is the overlay rearrange at the very end.
//!
//! # Two structural choices
//!
//! 1. **The overlay rearrange is hoisted out of the flush.** This module has no
//!    overlay, so the flush ends after disposal and the `Navigator` view performs
//!    the rearrange immediately afterwards, which keeps the rearrange *after*
//!    route disposal. The `rearrange_overlay: false` that `pop` and
//!    `remove_route` imply therefore has nothing to select here; it is recorded on
//!    [`FlushOutcome`] for the `Navigator` to honour.
//!
//! 2. **Routes are named by [`RouteId`], not by object.** Handing out
//!    `&mut dyn ErasedRoute` for one entry while the history holds the rest is not
//!    expressible; ids preserve identity, ordering and arity, which is everything
//!    the route callbacks and observers need. A `TransitionRoute` needs the *next
//!    route's animation*, so it will need a lookup handle — noted as a follow-up.

use std::fmt;
use std::sync::Arc;

use std::cell::Cell;
use std::rc::Rc;

use flui_scheduler::TickerFuture;

use super::binding::{RouteCommand, RouteCommandQueue};
use super::lifecycle::RouteLifecycle;
use super::observer::{Notification, Observation, ObservationQueues};
use super::result::RouteResult;
use super::route::{
    AnyResult, ErasedRoute, PushCompletion, Route, RouteId, RoutePopDisposition, RouteRecord,
    UndeliveredResult,
};

/// What was last announced to a route's `did_change_next` / `did_change_previous`.
///
/// **Not** `Option<RouteId>`: these fields are seeded with a `Never` sentinel,
/// distinct from "no route".
///
/// That distinction is load-bearing. On the first flush the bottom route has no
/// route below it, so `previous` is `None`; `None != Never` is **true**, and
/// `did_change_previous(None)` fires exactly once. Collapsing the sentinel into
/// `None` makes `None != None` false and the call is silently never made.
/// `ModalRoute` drives its internal-state refresh from `did_change_previous`, so a
/// bottom modal route would have missed its initial internal-state init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Announced {
    /// Nothing has been announced yet.
    Never,
    /// The last value announced, which may legitimately be "no route".
    Route(Option<RouteId>),
}

/// One route plus its bookkeeping.
pub(crate) struct RouteEntry {
    route: super::lifecycle::Terminal<Box<dyn ErasedRoute>>,
    state: RouteLifecycle,

    /// The value a queued `pop`/`complete` will deliver.
    pending_result: Option<AnyResult>,

    /// `false` when this route is being *replaced*, so it emits `did_replace`
    /// (from the new route) instead of `did_remove`.
    report_removal_to_observer: bool,

    last_announced_next: Announced,
    last_announced_previous: Announced,
    /// The last popped-next route announced. Seeded with the same `Never`
    /// sentinel, which is what makes
    /// `should_announce_change_to_next` suppress the *first* `didChangeNext(null)`
    /// (already sent by `handle_push` / `did_add` when `is_new_first`).
    last_announced_popped_next: Announced,

    /// For a `PushReplace` entry: the route this one replaced, as resolved by
    /// `push_replacement_with_id` — the **same value its completion used**.
    ///
    /// The observation used to be derived positionally, from the nearest present
    /// entry below this one. That agreed with the completion only while nothing
    /// could run between the two, which stopped being true once a named
    /// replacement captured its target before resolving and a factory could
    /// navigate in between: the captured route was completed, and a route that
    /// was still on the stack was reported as replaced. One source of truth
    /// instead of two that happen to agree.
    ///
    /// `None` on a `PushReplace` entry means "replaced nothing" (the captured
    /// route was already gone), which is authoritative — not "fall back to the
    /// position".
    replacing: Option<RouteId>,
}

/// Which route a replacement completes.
///
/// Replaces an `Option<RouteId>` whose `None` meant **two** things — "the current
/// top" for the unnamed front doors, and "there was nothing to replace" for a
/// named capture that came back empty — and landed both on
/// `last_present_index()`. A named replacement whose factory pushed then
/// completed the factory's own route and handed it the caller's result. Neither
/// variant here can mean the other, and "nothing to replace" is expressed by
/// passing no target at all.
pub(crate) enum ReplaceTarget {
    /// Whatever is present on top when the flush runs. The unnamed
    /// `push_replacement` front doors: nothing can run between their call and
    /// that flush, so "now" and "at the call" are the same route.
    CurrentTop,
    /// One specific route, captured before anything could run. The named front
    /// doors, whose resolution invokes a user factory.
    Route(RouteId),
}

/// What an arming call did, and anything it could not deliver.
#[must_use]
struct Armed {
    /// Whether the entry was armed. `false` when it had already passed `Remove`.
    armed: bool,
    /// A caller-supplied result this call could not deliver: the one it refused,
    /// or the one it displaced.
    ///
    /// Returned rather than dropped. `AnyResult` wraps a value **the caller
    /// supplied**, so dropping it runs user `Drop` — which may reach back into
    /// this navigator, and the history mutex is not reentrant. Same hazard as
    /// dropping a registry closure under its guard, one layer up.
    undelivered: Option<AnyResult>,
}

impl Drop for RouteEntry {
    fn drop(&mut self) {
        let route = self.route.withdraw();
        let result = super::lifecycle::Terminal::new(self.pending_result.take());
        drop(route);
        drop(result);
    }
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
            route: super::lifecycle::Terminal::new(route),
            state: initial_state,
            pending_result: None,
            report_removal_to_observer: true,
            last_announced_next: Announced::Never,
            last_announced_previous: Announced::Never,
            last_announced_popped_next: Announced::Never,
            replacing: None,
        }
    }

    /// Record which route this entry replaced. Only `PushReplace` entries carry
    /// one, and `push_replacement_with_id` is the only caller.
    fn replacing(&mut self, replaced: Option<RouteId>) -> &mut Self {
        debug_assert_eq!(
            self.state,
            RouteLifecycle::PushReplace,
            "BUG: only a PushReplace entry replaces a route"
        );
        self.replacing = replaced;
        self
    }

    pub(crate) fn id(&self) -> RouteId {
        self.route.id()
    }

    pub(crate) fn state(&self) -> RouteLifecycle {
        self.state
    }

    /// Records the result and arms the state. It does **not** call `did_pop`; the
    /// flush does.
    fn arm_pop(&mut self, result: Option<AnyResult>) -> Armed {
        debug_assert!(self.state.is_present());
        let displaced = core::mem::replace(&mut self.pending_result, result);
        self.state = RouteLifecycle::Pop;
        Armed {
            armed: true,
            undelivered: displaced,
        }
    }

    /// Records the result and arms completion.
    ///
    /// The `>= remove` early-return is the guard that makes double completion
    /// impossible: once the entry has passed `Remove`, `did_complete` has already
    /// run and a second `remove_route` cannot re-arm it.
    fn arm_complete(&mut self, result: Option<AnyResult>, is_replaced: bool) -> Armed {
        if self.state >= RouteLifecycle::Remove {
            // Refused. The caller's result goes back out unconsumed — and the
            // caller must learn this entry was NOT armed, because reporting it as
            // replaced when nothing touched it is a separate defect from
            // completing it twice. This guard prevents the second; the return
            // value is what prevents the first.
            return Armed {
                armed: false,
                undelivered: result,
            };
        }
        debug_assert!(self.state.is_present());
        self.report_removal_to_observer = !is_replaced;
        let displaced = core::mem::replace(&mut self.pending_result, result);
        self.state = RouteLifecycle::Complete;
        Armed {
            armed: true,
            undelivered: displaced,
        }
    }

    /// Move an added entry to `Adding` and report the push.
    fn handle_add(&mut self, previous_present: Option<RouteId>) -> Observation {
        debug_assert_eq!(self.state, RouteLifecycle::Add);
        self.state = RouteLifecycle::Adding;
        Observation::Push {
            route: self.id(),
            previous: previous_present,
        }
    }

    /// Install the added route and settle it to `Idle`.
    fn did_add(&mut self, is_new_first: bool) {
        self.route.install();
        self.route.did_add();
        self.state = RouteLifecycle::Idle;
        if is_new_first {
            self.route.did_change_next(None);
        }
    }

    /// Install the pushed (or replacing) route and report it.
    ///
    /// The `pushing` state is entered only when the route reports
    /// [`PushCompletion::Animating`]; see that variant's docs for the divergence
    /// on immediate pushes. When it does, the future travels back to the caller
    /// so the flush can record it as a [`DeferredEffect::AwaitPush`] —
    /// registering a continuation on it is **not** this method's job (or even
    /// the flush's): see that variant's own doc for why it must wait until the
    /// history lock is released.
    fn handle_push(
        &mut self,
        previous: Option<RouteId>,
        previous_present: Option<RouteId>,
        is_new_first: bool,
    ) -> (Observation, Option<TickerFuture>) {
        let previous_state = self.state;
        debug_assert!(matches!(
            previous_state,
            RouteLifecycle::Push | RouteLifecycle::PushReplace | RouteLifecycle::Replace
        ));

        self.route.install();

        let mut await_push = None;
        if matches!(
            previous_state,
            RouteLifecycle::Push | RouteLifecycle::PushReplace
        ) {
            self.state = match self.route.did_push() {
                PushCompletion::Immediate => RouteLifecycle::Idle,
                PushCompletion::Animating(future) => {
                    await_push = Some(future);
                    RouteLifecycle::Pushing
                }
            };
        } else {
            self.route.did_replace(previous);
            self.state = RouteLifecycle::Idle;
        }

        if is_new_first {
            self.route.did_change_next(None);
        }

        let observation = if matches!(
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
        };
        (observation, await_push)
    }

    /// Returns whether the route consented. On consent, `did_pop` completed the
    /// future; if the route is `finished_when_popped` it is finalized straight to
    /// `Dispose`, which is exactly the "pop finished synchronously" case the
    /// flush's `Pop` arm anticipates.
    fn handle_pop(&mut self) -> (bool, Option<UndeliveredResult>) {
        self.state = RouteLifecycle::Popping;

        if self.route.is_completed() {
            // Already completed elsewhere; nothing further to do — but a pending
            // result now has nowhere to go, and it is the caller's value.
            return (
                true,
                self.pending_result.take().map(UndeliveredResult::no_target),
            );
        }

        let result = self.pending_result.take();
        let (popped, undelivered) = self.route.did_pop(result);
        if !popped {
            self.state = RouteLifecycle::Idle;
            return (false, undelivered);
        }

        // Order matters. A route can reach `Dispose` *inside* `did_pop`: it finalizes
        // itself, and only then is the `on_pop_invoked(true)` callback run. So the
        // route is already finalized when its callback runs; this matters once
        // `PopScope` callbacks can inspect navigator state.
        //
        // **Conditionally**, and the condition is the one this crate's deferred-exit
        // fixtures set: only if `finished_when_popped`. A route whose exit
        // transition is still in flight reaches its callback *un*-finalized and
        // stays present for the whole window — which is why `route_ids()` and
        // `current()` diverge there.
        if self.route.finished_when_popped() {
            self.state = RouteLifecycle::Dispose;
        }
        self.route.on_pop_invoked(true);
        (true, undelivered)
    }

    /// Deliver the pending result and move to `Remove`.
    fn handle_complete(&mut self) -> Option<UndeliveredResult> {
        let result = self.pending_result.take();
        let undelivered = self.route.did_complete(result);
        debug_assert!(self.route.is_completed());
        self.state = RouteLifecycle::Remove;
        undelivered
    }

    /// Move a removed entry to `Removing` (or straight to `Dispose` if it was
    /// never installed) and report the removal, unless it is being replaced.
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

    /// Tell the route the route above it was popped.
    fn handle_did_pop_next(&mut self, popped: RouteId) {
        self.route.did_pop_next(popped);
        self.last_announced_popped_next = Announced::Route(Some(popped));
    }

    /// Suppresses a redundant `did_change_next(None)` when the route that vanished
    /// is the one we just announced via `did_pop_next`.
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
/// It cannot run inline at the tail of the flush: a `NavigatorObserver` holds a
/// `NavigatorHandle` and `parking_lot::Mutex` is not reentrant, so notifying an
/// observer under `history.lock()` deadlocks the moment it reads the stack it was
/// just told about. `route.dispose()` has the same shape — it runs arbitrary route
/// teardown. So the flush computes owned data and `NavigatorShared::apply` performs
/// it once the lock is released.
///
/// Not `Clone`/`PartialEq`: it owns the dying routes.
#[derive(Default)]
pub(crate) struct FlushOutcome {
    /// Whether the overlay must be rearranged. `pop` and `remove_route` leave
    /// it `false`, because `OverlayEntry::remove` has already updated the
    /// overlay's own list.
    pub(crate) rearrange_overlay: bool,
    /// What to tell the observers, in delivery order: additions LIFO, then
    /// deletions FIFO, then `did_change_top` — per pass.
    pub(crate) notifications: Vec<Notification>,
    /// The routes disposed by this flush. Their overlay entries must be removed by
    /// the caller, **before** [`dispose_routes`](Self::dispose_routes) runs.
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
///
/// Not `Copy`/`PartialEq`/`Eq`: [`AwaitPush`](Self::AwaitPush) carries a
/// [`TickerFuture`], which is neither.
#[derive(Debug, Clone)]
pub(crate) enum DeferredEffect {
    /// The pop-invoked callback (`did_pop`, …) for this route's `PopScope`s —
    /// raised by `handle_pop` for the `true` case, and by `maybe_pop`'s
    /// do-not-pop arm for a refusal.
    PopInvoked(RouteId, bool),
    /// A `did_pop` that refused — it may have consumed a local-history entry,
    /// whose `on_remove` is owed on the route's registry. A plain refusal drains to nothing.
    LocalHistoryPopped(RouteId),
    /// This route's [`PushCompletion::Animating`] future, handed back by
    /// [`RouteEntry::handle_push`] so a continuation can be registered on it
    /// once the history lock is released — never inside the flush that
    /// produced it. Registering mid-flush would let an already-resolved
    /// future (a zero-duration push) settle within that same flush instead of
    /// on the next one, which is exactly the timing ADR-0064's
    /// navigator-consumer constraint rules out.
    AwaitPush(RouteId, TickerFuture),
}

impl Drop for FlushOutcome {
    fn drop(&mut self) {
        let dying = super::lifecycle::RetiredValues(std::mem::take(&mut self.dying));
        let deferred = super::lifecycle::RetiredValues(std::mem::take(&mut self.deferred));
        drop(dying);
        drop(deferred);
    }
}

impl FlushOutcome {
    /// Fold a follow-up pass's outcome into this one, so the caller applies the
    /// union of everything a single `flush` did. Notifications keep pass order.
    fn absorb(&mut self, mut later: Self) {
        self.rearrange_overlay |= later.rearrange_overlay;
        self.notifications.append(&mut later.notifications);
        self.deferred.append(&mut later.deferred);
        self.disposed.append(&mut later.disposed);
        self.dying.append(&mut later.dying);
    }

    /// `Route::dispose` for every route this flush killed.
    ///
    /// Must run **after** the observers have been notified (the pop observation
    /// precedes disposal) and after the caller has removed each route's
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

/// The route stack plus the flush.
#[derive(Default)]
pub(crate) struct RouteHistory {
    entries: Vec<RouteEntry>,
    queues: ObservationQueues,
    last_topmost: Option<RouteId>,
    /// Whether a flush is running (and the stack is locked against re-entry).
    flushing: Rc<Cell<bool>>,
    /// What the most recent flush left for the caller to apply. This module is
    /// pure data, so it hands the overlay work out instead of doing it inline.
    last_outcome: Option<FlushOutcome>,
    /// Caller-supplied results nothing consumed, awaiting a drop with the guard
    /// released — see [`take_undelivered`](Self::take_undelivered).
    undelivered: Vec<UndeliveredResult>,
    /// Lifecycle transitions raised by routes through a `RouteBinding`.
    /// Drained at the head of every flush, and again after each
    /// pass, so a command raised *during* the walk settles before `flush` returns.
    commands: RouteCommandQueue,
    /// How many `flush_once` passes the last `flush` ran. Test-facing: a deferred
    /// command must cost exactly one extra pass, not a loop.
    #[cfg(test)]
    last_flush_passes: usize,
}

impl Drop for RouteHistory {
    fn drop(&mut self) {
        let entries = super::lifecycle::RetiredValues(std::mem::take(&mut self.entries));
        let outcome = super::lifecycle::Terminal::new(self.last_outcome.take());
        let undelivered = super::lifecycle::RetiredValues(std::mem::take(&mut self.undelivered));
        drop(entries);
        drop(outcome);
        drop(undelivered);
    }
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
    /// discarded; a push-completion is likewise ignored unless the route is
    /// still `Pushing`.
    fn apply_pending_commands(&mut self) -> bool {
        debug_assert!(
            !self.flushing.get(),
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
                        // Finalize the entry.
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

    /// Caller-supplied results this history could not deliver, moved out.
    ///
    /// The same shape as [`take_outcome`](Self::take_outcome) and
    /// `FlushOutcome::dying`, and for the same reason: an `AnyResult` wraps a
    /// value **the caller supplied**, so dropping one runs user `Drop`, which may
    /// reach back into this navigator — and the history mutex is not reentrant.
    /// The history records; the caller drains once the guard is released.
    ///
    /// Recorded here rather than returned from each operation because the
    /// early-return paths (`pop` on an empty stack, `remove_route` on a missing
    /// id) run **no flush at all**, so there is no `FlushOutcome` to ride.
    pub(crate) fn take_undelivered(&mut self) -> Vec<UndeliveredResult> {
        core::mem::take(&mut self.undelivered)
    }

    /// Record a result nothing consumed. `None` is the overwhelmingly common case
    /// and costs nothing.
    ///
    /// `pub(crate)` so a caller already **holding this history's guard** can route
    /// a result it could not deliver into the same channel, rather than dropping
    /// it inline — `NavigatorHandle::maybe_pop_erased` decides inside
    /// `NavigatorShared::mutate`'s closure and cannot drop safely there. One
    /// channel, one reporting path.
    pub(crate) fn record_undelivered(&mut self, result: Option<AnyResult>) {
        self.undelivered
            .extend(result.map(UndeliveredResult::no_target));
    }

    /// Walks the present routes **bottom-up**: no routes → `false`; the *first* one
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

    /// The top present route's pop disposition: bubble if it is the first route,
    /// else pop, unless the route handles the pop itself or a `PopScope` vetoes
    /// it. The veto is checked first.
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

    /// Tell the top present route its pop was refused (`on_pop_invoked(false)`).
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

    /// The present routes' ids, bottom to top.
    pub(crate) fn present_ids(&self) -> impl Iterator<Item = RouteId> + '_ {
        self.entries
            .iter()
            .filter(|entry| entry.state.is_present())
            .map(RouteEntry::id)
    }

    /// Whether the top present route handles a pop itself (a local-history
    /// entry), so that popping it removes no route.
    pub(crate) fn top_handles_pop_internally(&self) -> bool {
        self.entries
            .iter()
            .rfind(|entry| entry.state.is_present())
            .is_some_and(|entry| entry.route.will_handle_pop_internally())
    }

    /// The state of `id`'s entry, or `None` once disposed and dropped.
    pub(crate) fn state_of(&self, id: RouteId) -> Option<RouteLifecycle> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(RouteEntry::state)
    }

    /// Whether `id` names a route that is both in this stack and present (one
    /// lookup, since a route not in this navigator's stack cannot be found at
    /// all).
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
    /// This is *not* `pop_disposition_of_top`: the fallback disposition (first
    /// route bubbles, otherwise pop) never itself yields do-not-pop. So a
    /// "pop gesture enabled" check collapses to exactly this route's own veto
    /// check, for *this* route, not necessarily the top of the stack.
    pub(crate) fn vetoes_pop(&self, id: RouteId) -> Option<bool> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| entry.route.vetoes_pop())
    }

    /// The bottom-most **present** route. Read by
    /// `NavigatorHandle::pop_gesture_enabled`'s "is first" check.
    pub(crate) fn first_present(&self) -> Option<RouteId> {
        self.entries
            .iter()
            .find(|entry| entry.state.is_present())
            .map(RouteEntry::id)
    }

    /// The topmost present route.
    pub(crate) fn current(&self) -> Option<RouteId> {
        self.entries
            .iter()
            .rfind(|entry| entry.state.is_present())
            .map(RouteEntry::id)
    }

    /// The route a user gesture (e.g. an edge swipe-back) is manipulating,
    /// and the route beneath it a completed pop would reveal. Scans with
    /// `will_be_present` (a route mid-push counts, unlike `current`'s
    /// `is_present`), and leaves
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

    // ── Public mutations (each ends in a flush) ──────────────────────────────

    /// Seed an initial route **without flushing**.
    ///
    /// Mounting appends *every* initial route and then flushes exactly once.
    /// That single flush is what makes a deep link like `/a/b` announce its whole
    /// synthesized back-stack in one batch — and it is the only way to observe
    /// the additions queue's LIFO drain.
    ///
    /// Entries enter in `Add`: no push transition, but observers still see a push
    /// observation (`handle_add`).
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
    /// once on mount.
    #[cfg(test)]
    pub(crate) fn add_initial<R: Route>(&mut self, route: R) -> (RouteId, RouteResult<R::Output>) {
        let seeded = self.seed_initial(route);
        self.flush(true);
        seeded
    }

    /// Append, flush, and return the future that was created before any lifecycle ran.
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

    /// `push_replacement`, under an id the caller minted —
    /// the [`push_with_id`](Self::push_with_id) split, so `NavigatorHandle` can bind
    /// the route and insert its overlay entry before the flush.
    /// `replaced` names the entry to complete **as replaced**. `None` means the
    /// current top, which is what the unnamed `push_replacement` front doors
    /// want: nothing can run between their call and this flush.
    ///
    /// A named push *can* have something run in between — its factory may
    /// navigate through a captured handle — so those front doors capture their
    /// target before resolving and name it here. Replacing "whatever is on top
    /// now" would otherwise replace the factory's own route rather than the
    /// caller's. A named target that is no longer present completes nothing and
    /// the push still happens.
    pub(crate) fn push_replacement_with_id<R: Route>(
        &mut self,
        id: RouteId,
        target: Option<ReplaceTarget>,
        route: R,
        result: Option<AnyResult>,
    ) -> (RouteId, RouteResult<R::Output>) {
        // Both arms filter by presence, and they must agree. `is_present()` is
        // `Add..=Remove`, and `Popping`/`Removing` sort after it — a route mid
        // exit transition has *already* completed and is only awaiting
        // finalisation, so replacing it again would report a second replacement
        // of a route nothing touched. Only a present route is a valid target.
        // Previously `Route(id)` used an unfiltered `position()` while
        // `CurrentTop` filtered, so the two disagreed about what counts as live.
        let target_index = match target {
            None => None,
            Some(ReplaceTarget::CurrentTop) => self.last_present_index(),
            Some(ReplaceTarget::Route(id)) => self
                .entries
                .iter()
                .position(|entry| entry.id() == id && entry.state.is_present()),
        };
        // The reported id comes from whether the completion *happened*, not from
        // the lookup: `arm_complete` can still refuse a `Remove`-state entry that
        // passed the presence filter, and reporting that as replaced would name a
        // route nothing touched — and name it twice, since
        // `report_removal_to_observer` is only cleared inside the arming.
        let replaced_id = if let Some(index) = target_index {
            let armed = self.entries[index].arm_complete(result, true);
            self.record_undelivered(armed.undelivered);
            armed.armed.then(|| self.entries[index].id())
        } else {
            // No target: nothing was completed, so nothing is reported as replaced
            // and the caller's result reached no route.
            self.record_undelivered(result);
            None
        };
        let (erased, route_result) = RouteRecord::erase_with_id(id, route);
        let mut entry = RouteEntry::new(erased, RouteLifecycle::PushReplace);
        entry.replacing(replaced_id);
        self.entries.push(entry);
        self.flush(true);
        (id, route_result)
    }

    /// The push half of `NavigatorHandle::push_and_remove_until`, split from
    /// the removal-completion half so the caller can evaluate `keep` with
    /// the history lock **released**: a `RoutePredicate` that queries the
    /// handle back (a "first route" or "route with name" predicate) must not run inside this module's locked section, or it
    /// deadlocks the owner thread against its own non-reentrant
    /// `parking_lot::Mutex`.
    ///
    /// Appends the new route in `Push`, **without** flushing or evaluating
    /// any predicate, and hands back every existing entry's id, top-to-
    /// bottom, exactly as they stood immediately before the push.
    /// [`complete_removed_and_flush`](Self::complete_removed_and_flush)
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
    /// entry named in `remove_ids`, then flush **once**, so the push and every
    /// removal the caller's `keep` decided on land in a single flush.
    pub(crate) fn complete_removed_and_flush(&mut self, remove_ids: &[RouteId]) {
        let mut displaced = Vec::new();
        for &target in remove_ids {
            if let Some(entry) = self.entry_mut(target)
                && entry.state.is_present()
            {
                // Removed routes complete with `None`.
                let armed = entry.arm_complete(None, false);
                displaced.extend(armed.undelivered);
            }
        }
        for result in displaced {
            self.record_undelivered(Some(result));
        }
        self.flush(true);
    }

    /// Replace everything above `keep` with `below` and `top`, in **one** flush —
    /// the shape of a page-list diff: the pages that left complete (observers see `did_remove`), the new pages
    /// beneath the new top enter quietly in `Add` (observers see `did_push`, no
    /// transition runs), and only `top` enters in `Push`, so only it animates.
    ///
    /// `keep: None` completes every present entry. The removed entries and the
    /// quiet additions wait in `Removing` / `Adding` until `top` settles and
    /// covers them (`can_remove_or_add`).
    ///
    /// Each route arrives with the id the caller already bound it to.
    pub(crate) fn replace_tail_with_ids<R: Route>(
        &mut self,
        keep: Option<RouteId>,
        below: Vec<(RouteId, R)>,
        top: (RouteId, R),
    ) {
        let first_removed = match keep {
            None => 0,
            Some(keep) => self
                .entries
                .iter()
                .position(|entry| entry.id() == keep)
                .map_or(self.entries.len(), |index| index + 1),
        };
        let mut displaced = Vec::new();
        for entry in &mut self.entries[first_removed..] {
            if entry.state.is_present() {
                let armed = entry.arm_complete(None, false);
                displaced.extend(armed.undelivered);
            }
        }
        for result in displaced {
            self.record_undelivered(Some(result));
        }
        for (id, route) in below {
            // The result handle is dropped: a page the Router adds quietly has
            // no awaiter, and dropping a `RouteResult` cancels nothing.
            let (erased, _result) = RouteRecord::erase_with_id(id, route);
            self.entries
                .push(RouteEntry::new(erased, RouteLifecycle::Add));
        }
        let (id, route) = top;
        let (erased, _result) = RouteRecord::erase_with_id(id, route);
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Push));
        self.flush(true);
    }

    /// Returns whether a present route was found to arm; a route that *refuses*
    /// the pop (`did_pop` → `false`, e.g. a local-history entry consumed instead)
    /// still counts.
    pub(crate) fn pop(&mut self, result: Option<AnyResult>) -> bool {
        let Some(index) = self.last_present_index() else {
            // No route to deliver to. Recorded, not dropped here — see
            // [`Self::take_undelivered`].
            self.record_undelivered(result);
            return false;
        };
        let armed = self.entries[index].arm_pop(result);
        self.record_undelivered(armed.undelivered);
        if self.entries[index].state == RouteLifecycle::Pop {
            self.flush(false);
        }
        true
    }

    /// **The removed route still completes its future.** `arm_complete` →
    /// `handle_complete` → `did_complete`. Completing only on `pop` would hang
    /// every `await` in an app that uses this.
    /// The `bool` reports "an entry with this id was found", not "it was armed":
    /// an entry already past `Remove` is found, refuses the arming, and has always
    /// reported `true`. Narrowing that would change this method's public contract,
    /// which is not this fix's business — but the refused result is recorded, so it
    /// no longer vanishes.
    pub(crate) fn remove_route(&mut self, id: RouteId, result: Option<AnyResult>) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.id() == id) else {
            self.record_undelivered(result);
            return false;
        };
        let armed = self.entries[index].arm_complete(result, false);
        self.record_undelivered(armed.undelivered);
        self.flush(false);
        true
    }

    // ── The flush ────────────────────────────────────────────────────────────

    fn last_present_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|entry| entry.state.is_present())
    }

    /// The nearest route at or below `index` whose state satisfies `predicate`.
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

    /// The nearest route at or above `index` whose state satisfies `predicate`.
    fn route_after(&self, index: usize, predicate: fn(RouteLifecycle) -> bool) -> Option<RouteId> {
        self.entries[index.min(self.entries.len())..]
            .iter()
            .find(|entry| predicate(entry.state))
            .map(RouteEntry::id)
    }

    /// Advance every route's lifecycle as far as it can go.
    ///
    /// The reverse walk, `can_remove_or_add`, the `popped_route` /
    /// `seen_top_active_route` pair, deferred disposal, then observers →
    /// announcements → `did_change_top` → dispose.
    ///
    /// # Panics
    ///
    /// If re-entered. A route's transition callback firing mid-flush is the way
    /// in, so this is a framework invariant and `PANIC-POLICY` permits the panic.
    pub(crate) fn flush(&mut self, rearrange_overlay: bool) {
        // A *recursive* `flush` is still forbidden and still loud. Route callbacks
        // no longer reach this path: they enqueue a `RouteCommand` instead
        // (see `binding.rs` Correction 1), so this assert now guards
        // only genuine framework misuse.
        assert!(
            !self.flushing.get(),
            "BUG: flush_history_updates re-entered — a route lifecycle callback \
             mutated the history while it was being flushed"
        );

        // Commands raised since the last flush (e.g. an animation status listener
        // firing between frames) take effect before the walk sees the history.
        self.apply_pending_commands();

        let mut outcome = self.flush_once(rearrange_overlay);
        let mut passes = 1;

        // A command raised *during* the walk — `did_pop`'s `reverse()`
        // canceling a still-pending `forward()` run (its `TickerFuture`
        // continuation fires synchronously and queues `PushCompleted`), or
        // `finalize` from `did_pop` — is applied here and settled by another
        // pass.
        while self.apply_pending_commands() {
            passes += 1;
            assert!(
                passes <= MAX_FLUSH_PASSES,
                "BUG: route commands did not converge after {MAX_FLUSH_PASSES} flush passes — \
                 a route is re-raising a command from its own lifecycle callback"
            );
            // `rearrange_overlay: false` — a follow-up pass only disposes and
            // settles; `OverlayEntry::remove` has already updated the overlay's
            // own list.
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
    ///
    /// The flag is cleared by a **guard**, not by the statement after the call. A
    /// `Route` lifecycle hook is user code and may panic;
    /// [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md) forbids it, but
    /// forbidding is not preventing. `parking_lot` does not poison, so an unwind
    /// past a bare `self.flushing = false` left the flag set and every later flush
    /// tripped `assert!(!self.flushing)` — one panicking `did_pop` bricked the
    /// navigator permanently, with no way back.
    ///
    /// The flag is an `Rc<Cell<bool>>` so the guard can own a handle to it without
    /// borrowing `self`, which `flush_inner(&mut self)` needs. One allocation per
    /// navigator.
    ///
    /// **What this does not fix, stated rather than implied.** The same unwind
    /// drops a partially built `FlushOutcome`, and any `RouteEntry` already moved
    /// into its `dying` list is dropped without `Route::dispose` having run. That
    /// is deliberately left alone: a `Drop` impl on `FlushOutcome` would run
    /// `dispose` — user code — during an unwind, *under this lock*, trading a
    /// missed `dispose` for a possible deadlock while already panicking. So a
    /// panicking hook still leaks those routes' own cleanup. What the guard buys is
    /// that the navigator survives the panic in a usable state, which is the half
    /// that is recoverable.
    fn flush_once(&mut self, rearrange_overlay: bool) -> FlushOutcome {
        /// Clears `flushing` on the way out, unwinding included.
        struct FlushingGuard(Rc<Cell<bool>>);

        impl Drop for FlushingGuard {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }

        self.flushing.set(true);
        let _guard = FlushingGuard(Rc::clone(&self.flushing));
        self.flush_inner(rearrange_overlay)
    }

    #[expect(clippy::too_many_lines)] // A 1:1 transcription; splitting it would scramble the mapping.
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

            // Advance to the next entry (the loop tail), unless a `continue`
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
                    // `PushReplace` reports the route its own completion targeted;
                    // the other two keep the positional answer, which is right for
                    // them — see `RouteEntry::replacing`. `Push` means "the route
                    // below", which is positional by definition and replaces
                    // nothing; `Replace` (the generic mid-stack swap) resolves its
                    // target positionally in the first place, so position *is* its
                    // single source of truth.
                    let replaced = match state {
                        RouteLifecycle::PushReplace => self.entries[position].replacing,
                        _ => previous_present,
                    };
                    let (observation, await_push) =
                        self.entries[position].handle_push(previous, replaced, next.is_none());
                    self.queues.enqueue(observation);
                    if let Some(future) = await_push {
                        deferred.push(DeferredEffect::AwaitPush(
                            self.entries[position].id(),
                            future,
                        ));
                    }
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
                    let (popped_ok, undelivered) = self.entries[position].handle_pop();
                    self.undelivered.extend(undelivered);
                    if popped_ok {
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
                    let undelivered = self.entries[position].handle_complete();
                    self.undelivered.extend(undelivered);
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
                    // Delay disposal until did_change_next/did_change_previous
                    // have been sent.
                    to_be_disposed.push(self.entries.remove(position));
                    // `next` is unchanged by the removal.
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

        // Lastly, hand the marked entries to the caller rather than disposing them
        // inline; `Route::dispose` runs arbitrary teardown — an animation controller, a vsync unregistration, a
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

    /// Tell each route about its new neighbours, once per change.
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
                // Updated even when the announcement was suppressed.
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
    /// the history, and push completion now reaches it only as a queued
    /// `RouteCommand` (ADR-0064) — never a direct call a route's own
    /// lifecycle callback could make mid-flush. The guard is kept as defence
    /// in depth regardless. Testing it directly rather than shipping it
    /// untested follows established precedent.
    pub(crate) fn force_flushing_for_test(&mut self) {
        self.flushing.set(true);
    }
}
