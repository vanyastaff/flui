//! [`RouteHistory`] and `flush_history_updates` — the route stack.
//!
//! ADR-0019 U2. Private, and **pure data**: this module touches no element tree,
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
//!    U2 has no overlay, so the flush ends after disposal and U3's `Navigator`
//!    performs the rearrange immediately afterwards. Ordering is preserved:
//!    Flutter also rearranges *after* `_disposeRouteEntry` (`:4609-4613`). The
//!    `rearrangeOverlay: false` argument that `pop` and `removeRoute` pass
//!    (`:5671`, `:5747`) therefore has nothing to select here; it is recorded on
//!    [`FlushOutcome`] for U3 to honour.
//!
//! 2. **Routes are named by [`RouteId`], not by object.** Flutter passes `Route`
//!    objects to `didChangeNext` / `didChangePrevious` / `didPopNext` and to
//!    observers. Handing out `&mut dyn ErasedRoute` for one entry while the
//!    history holds the rest is not expressible; ids preserve identity, ordering
//!    and arity, which is everything the oracles assert. U5's `TransitionRoute`
//!    needs the *next route's animation*, so it will need a lookup handle — noted
//!    in ADR-0019 §7a.

use std::sync::Arc;

use super::lifecycle::RouteLifecycle;
use super::observer::{NavigatorObserver, Observation, ObservationQueues};
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
/// is what U4's parity re-check found FLUI doing. `ModalRoute` drives
/// `changedInternalState()` from `didChangePrevious`, so a bottom modal route
/// would have missed its initial internal-state init at U5.
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
        // callback runs. Found by U4's parity re-check; matters once `PopScope`
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

/// What one flush did that its caller must still act on.
///
/// U2 has no overlay; U3's `Navigator` reads this and performs the overlay work
/// Flutter does at the tail of `_flushHistoryUpdates` (`navigator.dart:4609-4613`):
/// remove each disposed route's overlay entries, then rearrange.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct FlushOutcome {
    /// Flutter's `rearrangeOverlay` argument (`navigator.dart:4451`). `pop` and
    /// `remove_route` pass `false`, because `OverlayEntry.remove()` has already
    /// updated the overlay's own list.
    pub(crate) rearrange_overlay: bool,
    /// The routes disposed at the end of this flush, bottom-up. Their overlay
    /// entries must be removed by the caller.
    pub(crate) disposed: Vec<RouteId>,
}

/// The route stack. Flutter's `NavigatorState._history` plus the flush.
#[derive(Default)]
pub(crate) struct RouteHistory {
    entries: Vec<RouteEntry>,
    observers: Vec<Arc<dyn NavigatorObserver>>,
    queues: ObservationQueues,
    last_topmost: Option<RouteId>,
    /// Flutter's `_flushingHistory` + `_debugLocked` (`:4453`).
    flushing: bool,
    /// What the most recent flush left for the caller to apply. Flutter performs
    /// the overlay work inline; U2 is pure data, so it hands it out instead.
    last_outcome: Option<FlushOutcome>,
}

impl RouteHistory {
    pub(crate) fn new() -> Self {
        Self::default()
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
    /// `isFirst ? bubble : pop`, unless the route handles the pop itself.
    ///
    /// `DoNotPop` has no producer until `PopScope` / page-based routing lands.
    pub(crate) fn pop_disposition_of_top(&self) -> Option<RoutePopDisposition> {
        let present: Vec<&RouteEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.state.is_present())
            .collect();
        let top = present.last()?;
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
    pub(crate) fn notify_pop_refused(&mut self) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rfind(|entry| entry.state.is_present())
        {
            entry.route.on_pop_invoked(false);
        }
    }

    pub(crate) fn add_observer(&mut self, observer: Arc<dyn NavigatorObserver>) {
        self.observers.push(observer);
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

    /// The topmost present route. Flutter's `_lastRouteEntryWhereOrNull(isPresent)`.
    pub(crate) fn current(&self) -> Option<RouteId> {
        self.entries
            .iter()
            .rfind(|entry| entry.state.is_present())
            .map(RouteEntry::id)
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
    pub(crate) fn seed_initial<R: Route>(&mut self, route: R) -> (RouteId, RouteResult<R::Output>) {
        let (erased, result) = RouteRecord::erase(route);
        let id = erased.id();
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Add));
        (id, result)
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
    pub(crate) fn push<R: Route>(&mut self, route: R) -> (RouteId, RouteResult<R::Output>) {
        let (erased, result) = RouteRecord::erase(route);
        let id = erased.id();
        self.entries
            .push(RouteEntry::new(erased, RouteLifecycle::Push));
        self.flush(true);
        (id, result)
    }

    /// Flutter's `NavigatorState.pushReplacement` (`:5245-5268`): complete the
    /// current top with `is_replaced = true` (so it emits **no** `did_remove`),
    /// append the new route in `PushReplace`, then a single flush.
    /// Not exported: `NavigatorHandle` does not surface `pushReplacement` yet, and
    /// widening the U4 surface needs its own sign-off. The algorithm is ported and
    /// tested; only the public front door is missing.
    #[cfg(test)]
    pub(crate) fn push_replacement<R: Route>(
        &mut self,
        route: R,
        result: Option<AnyResult>,
    ) -> (RouteId, RouteResult<R::Output>) {
        if let Some(top) = self.last_present_index() {
            self.entries[top].arm_complete(result, true);
        }
        let (erased, route_result) = RouteRecord::erase(route);
        let id = erased.id();
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
    /// Not exported, for the same reason as [`push_replacement`](Self::push_replacement).
    #[cfg(test)]
    pub(crate) fn push_and_remove_until<R: Route>(
        &mut self,
        route: R,
        keep: impl Fn(RouteId) -> bool,
    ) -> (RouteId, RouteResult<R::Output>) {
        let mut index = self.entries.len() as isize - 1;

        let (erased, result) = RouteRecord::erase(route);
        let id = erased.id();
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

    /// Flutter's `NavigatorState.pop` (`:5642-5675`). Returns whether a route was
    /// found to pop; `false` also when the route refused (`did_pop → false`).
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

    /// The `Pushing → Idle` transition. Flutter's `whenCompleteOrCancel` callback
    /// on the `TickerFuture` `didPush` returned (`:3276-3290`): flip and re-flush.
    ///
    /// U5's `TransitionRoute` will call this from its animation-status listener.
    /// Until then only the tests do — an `Animating` push has no other producer.
    #[cfg(test)]
    pub(crate) fn notify_push_completed(&mut self, id: RouteId) {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id() == id) else {
            return;
        };
        if entry.state != RouteLifecycle::Pushing {
            return;
        }
        entry.state = RouteLifecycle::Idle;
        self.flush(true);
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
    /// mid-flush is the way in (U5), so this is a framework invariant and
    /// `PANIC-POLICY` permits the panic.
    pub(crate) fn flush(&mut self, rearrange_overlay: bool) -> FlushOutcome {
        assert!(
            !self.flushing,
            "BUG: flush_history_updates re-entered — a route lifecycle callback \
             mutated the history while it was being flushed"
        );
        self.flushing = true;
        let outcome = self.flush_inner(rearrange_overlay);
        self.flushing = false;
        self.last_outcome = Some(outcome.clone());
        outcome
    }

    #[allow(clippy::too_many_lines)] // A 1:1 transcription; splitting it would scramble the mapping.
    fn flush_inner(&mut self, rearrange_overlay: bool) -> FlushOutcome {
        let mut index: isize = self.entries.len() as isize - 1;
        let mut next: Option<RouteId> = None;
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
                        // The route refused the pop; it returns to `Idle`.
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

        // Informs navigator observers about route changes.
        self.queues.flush(&self.observers);

        // Now that the list is clean, send the didChangeNext/didChangePrevious
        // notifications.
        self.flush_route_announcement();

        let last = self.current();
        if let Some(top) = last
            && self.last_topmost != Some(top)
        {
            for observer in &self.observers {
                observer.did_change_top(top, self.last_topmost);
            }
        }
        self.last_topmost = last;

        // Lastly, dispose everything marked. Flutter also removes each route's
        // overlay entries here (`_disposeRouteEntry`); U3 owns that.
        // The caller removes each route's overlay entries; Flutter does it here,
        // before `entry.dispose()` (`_disposeRouteEntry`, `:3978-3987`). U3's
        // routes hold no overlay entry themselves, so nothing observes the order.
        let mut disposed = Vec::with_capacity(to_be_disposed.len());
        for mut entry in to_be_disposed {
            disposed.push(entry.id());
            entry.route.dispose();
            entry.state = RouteLifecycle::Disposed;
        }

        FlushOutcome {
            rearrange_overlay,
            disposed,
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
    /// Through U2's surface re-entrancy is *structurally* unreachable: a `Route`
    /// hook receives only `&mut self` and cannot reach the history. The guard
    /// exists for U5, where a zero-duration transition's completion callback
    /// re-enters via `notify_push_completed` mid-flush. Testing it directly
    /// rather than shipping it untested follows the ADR-0018 U4 precedent.
    pub(crate) fn force_flushing_for_test(&mut self) {
        self.flushing = true;
    }
}
