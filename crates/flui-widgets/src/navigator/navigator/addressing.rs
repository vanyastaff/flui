//! Admission under a `Router` (ADR-0093 §4), and the `Router`'s own doors.
//!
//! A navigator made by [`NavigatorHandle::addressed`] belongs to a `Router`:
//! every page on it is a route value with a path, and only the `Router` places
//! or replaces pages, through the crate-private doors below. Its public facade
//! keeps two things:
//!
//! - **A plain push of a pageless popup** (`PopupRoute`, so `show_dialog`
//!   keeps working). `push` refuses every other route: `PageRoute`,
//!   `SimpleRoute`, and any third-party `NavigatorRoute`. The doors that
//!   replace, sweep or seed (`push_replacement[_with]`,
//!   `push_and_remove_until`, `seed_initial`) refuse every route, a popup
//!   included, because what they would remove or place beneath is the
//!   Router's.
//! - **Pops and removals that leave a page.** `pop`, `pop_with`, `maybe_pop`,
//!   `remove_route` and `pop_until` never take the Router's last page, whether
//!   it is on top or under a popup: a Router always has a location.
//!
//! A refused typed push cannot answer with a `Result` without changing a
//! public signature, so it returns a [`RouteResult`] already completed with
//! `Ok(None)`, logs the refusal with `tracing::error!`, and fails a debug
//! assertion. The stack is untouched and the refused route is disposed
//! unpushed.

use std::collections::HashSet;
use std::fmt;

use parking_lot::Mutex;

use crate::navigator::binding::is_pageless_popup;
use crate::navigator::history::{ReplaceTarget, RouteHistory};
use crate::navigator::overlay_route::NavigatorRoute;
use crate::navigator::result::{Completer, RouteResult};
use crate::navigator::route::{Route, RouteId};

use super::{NavigatorHandle, RouteReservation, report_undelivered};

/// What a Router's navigator knows about the Router: its route type, and
/// which routes are its pages.
pub(super) struct Addressing {
    /// `type_name` of the Router's route type, for refusals.
    route_type: &'static str,
    /// The pages the Router placed. A page that has left the history lingers
    /// until the next pop or removal asks about the last page, which prunes
    /// it; only present pages are ever counted.
    pages: Mutex<HashSet<RouteId>>,
}

impl Addressing {
    fn new(route_type: &'static str) -> Self {
        Self {
            route_type,
            pages: Mutex::new(HashSet::new()),
        }
    }

    /// Whether removing `id` from `history` would leave no present page.
    ///
    /// Takes `pages` while the caller holds the history lock; nothing takes
    /// the history lock while holding `pages`, so the order is fixed.
    fn is_last_page(&self, history: &RouteHistory, id: RouteId) -> bool {
        let mut pages = self.pages.lock();
        pages.retain(|page| history.state_of(*page).is_some());
        pages.contains(&id)
            && history.is_present(id)
            && history.present_ids().filter(|p| pages.contains(p)).count() == 1
    }
}

/// Why a Router's navigator refused a route at a typed door.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The route is not a pageless popup, so it has no path.
    NoPath,
    /// The door would replace, sweep or seed the Router's pages.
    OwnsPages,
}

/// The text a refusal logs and asserts with.
struct RefusalReport {
    refusal: Refusal,
    operation: &'static str,
    route_type: &'static str,
}

impl fmt::Display for RefusalReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.refusal {
            Refusal::NoPath => write!(
                f,
                "`{}` refused a route with no path: only `{}` values may enter a Router's stack",
                self.operation, self.route_type
            ),
            Refusal::OwnsPages => write!(
                f,
                "`{}` refused: it would replace or seed pages of a Router<{}>; \
                 only a plain push of a popup may reach a Router's navigator",
                self.operation, self.route_type
            ),
        }
    }
}

impl NavigatorHandle {
    /// An empty, unmounted navigator that a `Router<R>` drives; `route_type`
    /// names `R` in refusals.
    pub(crate) fn addressed(route_type: &'static str) -> Self {
        Self::with_addressing(Some(Addressing::new(route_type)))
    }

    /// Whether [`push`](Self::push) admits a page route here. `false` for a
    /// navigator a [`Router`](crate::Router) drives: the Router owns its pages,
    /// so a push there places only a pageless popup (ADR-0093 §4).
    ///
    /// A widget that pushes its own transient page (a container transform)
    /// asks this to choose the route it builds instead of being refused.
    #[must_use]
    pub fn admits_pages(&self) -> bool {
        self.shared.addressing.is_none()
    }

    /// Why a plain push may not place `route`, if it may not.
    pub(super) fn push_refusal<P: NavigatorRoute>(&self, route: &P) -> Option<Refusal> {
        (self.shared.addressing.is_some() && !is_pageless_popup(route.binding_slot()))
            .then_some(Refusal::NoPath)
    }

    /// Why a door that replaces, sweeps or seeds may not place `route`, if it
    /// may not: under a Router, it never may.
    pub(super) fn placement_refusal<P: NavigatorRoute>(&self, route: &P) -> Option<Refusal> {
        if self.shared.addressing.is_none() {
            None
        } else if is_pageless_popup(route.binding_slot()) {
            Some(Refusal::OwnsPages)
        } else {
            Some(Refusal::NoPath)
        }
    }

    /// Whether removing `id` would take a Router's last page.
    pub(super) fn removes_last_page(&self, history: &RouteHistory, id: RouteId) -> bool {
        self.shared
            .addressing
            .as_ref()
            .is_some_and(|addressing| addressing.is_last_page(history, id))
    }

    /// Whether popping the top route would take a Router's last page. A top
    /// route that handles the pop itself (a local-history entry) removes
    /// nothing.
    pub(super) fn pop_removes_last_page(&self, history: &RouteHistory) -> bool {
        history.current().is_some_and(|top| {
            !history.top_handles_pop_internally() && self.removes_last_page(history, top)
        })
    }

    /// Record `id` as one of the Router's pages when `route` is not a popup.
    ///
    /// Called from `prepare`, before the route enters the history — so it
    /// must not prune ids the history does not hold yet: `replace_tail`
    /// prepares several pages before any of them enters. Under a Router only
    /// a popup passes a public door, so a route that is not one came through
    /// the Router's own doors.
    pub(super) fn record_page<P: NavigatorRoute>(&self, id: RouteId, route: &P) {
        let Some(addressing) = &self.shared.addressing else {
            return;
        };
        if is_pageless_popup(route.binding_slot()) {
            return;
        }
        addressing.pages.lock().insert(id);
    }

    /// Refuse `route` at a typed public door: dispose it unpushed, report, and
    /// hand back a result that is already complete with `Ok(None)`.
    ///
    /// # Panics
    ///
    /// In a debug build, always — after disposing the route and with no lock
    /// held — so the refused push is found where it was written.
    pub(super) fn refuse<P: NavigatorRoute>(
        &self,
        operation: &'static str,
        refusal: Refusal,
        mut route: P,
    ) -> RouteResult<P::Output> {
        let report = RefusalReport {
            refusal,
            operation,
            route_type: self
                .shared
                .addressing
                .as_ref()
                .expect("BUG: only a Router's navigator refuses a route")
                .route_type,
        };
        Route::dispose(&mut route);
        drop(route);
        tracing::error!(operation, route_type = report.route_type, "{report}");
        let (completer, result) = Completer::new();
        completer.complete(Ok(None));
        debug_assert!(false, "{report}");
        result
    }

    /// Seed a Router page before the navigator mounts.
    pub(crate) fn seed_page<P: NavigatorRoute>(&self, route: P) -> RouteId {
        self.seed_reporting_id(route).0
    }

    /// Push a Router page with its entrance transition. Commit its typed
    /// entry before observers receive the push, and hand back the page's
    /// result, which an observer that pops it at once has already completed.
    pub(crate) fn push_page<P: NavigatorRoute, O>(
        &self,
        route: P,
        commit: impl FnOnce(RouteId) -> O,
    ) -> (O, RouteResult<P::Output>) {
        let mut route = super::super::lifecycle::Terminal::new(route);
        let mut commit = super::super::lifecycle::Terminal::new(commit);
        let id = self.prepare(&*route);
        let (result, committed) = self.commit_pages(
            "push",
            |history| history.push_with_id(id, route.take_value()).1,
            || commit.take_value()(id),
        );
        (committed, result)
    }

    /// Push a Router page that replaces `target`: a push-replacement aimed at
    /// one captured route. Commit its typed replacement before delivery.
    pub(crate) fn push_replacement_page<P: NavigatorRoute, O>(
        &self,
        target: RouteId,
        route: P,
        commit: impl FnOnce(RouteId) -> O,
    ) -> O {
        let mut route = super::super::lifecycle::Terminal::new(route);
        let mut commit = super::super::lifecycle::Terminal::new(commit);
        let id = self.prepare(&*route);
        let (value, committed) = self.commit_pages(
            "push_replacement",
            |history| {
                history
                    .push_replacement_with_id(
                        id,
                        Some(ReplaceTarget::Route(target)),
                        route.take_value(),
                        None,
                    )
                    .1
            },
            || commit.take_value()(id),
        );
        drop(value);
        committed
    }

    /// Replace every route above `keep` (every route, for `None`) with `below`
    /// and `top` in one flush: the leaving routes are removed, `below` is added
    /// quietly, and only `top` runs an entrance transition. Returns the new
    /// routes' ids to the typed commit, bottom to top, before delivery.
    pub(crate) fn replace_tail<P: NavigatorRoute, O>(
        &self,
        keep: Option<RouteId>,
        below: Vec<P>,
        top: P,
        commit: impl FnOnce(Vec<RouteId>) -> O,
    ) -> O {
        self.replace_tail_using(keep, below, top, commit, RouteReservation::reserve)
    }

    pub(super) fn replace_tail_using<P: NavigatorRoute, O>(
        &self,
        keep: Option<RouteId>,
        below: Vec<P>,
        top: P,
        commit: impl FnOnce(Vec<RouteId>) -> O,
        reserve: impl FnMut() -> RouteReservation,
    ) -> O {
        let mut remaining = super::super::lifecycle::Terminal::new(below.into_iter());
        let mut below = super::super::lifecycle::Terminal::new(Vec::new());
        let mut top = super::super::lifecycle::Terminal::new(top);
        let mut commit = super::super::lifecycle::Terminal::new(commit);
        let mut reserve = super::super::lifecycle::Terminal::new(reserve);
        let mut reservations = Vec::new();
        for route in &mut *remaining {
            let mut route = super::super::lifecycle::Terminal::new(route);
            let reservation = (*reserve)();
            reservations.push(reservation);
            below.push((reservation.route(), route.take_value()));
        }
        let top_reservation = (*reserve)();
        // Every route and overlay identity of the batch is reserved before any
        // page is recorded, slot bound, builder run or entry inserted, so a
        // capacity refusal publishes nothing. Identities a rejected batch
        // reserved remain unique and are never rolled back or reissued.
        for ((_, route), reservation) in below.iter().zip(reservations) {
            self.prepare_reserved(route, reservation);
        }
        let top_id = self.prepare_reserved(&*top, top_reservation);
        let mut ids: Vec<RouteId> = below.iter().map(|(id, _)| *id).collect();
        ids.push(top_id);
        let ((), committed) = self.commit_pages(
            "replace_tail",
            |history| {
                history.replace_tail_with_ids(keep, below.take_value(), (top_id, top.take_value()));
            },
            || commit.take_value()(ids),
        );
        committed
    }

    /// Commit the Router's typed stack before the flush delivers user effects.
    /// The framework-only commit runs without the history lock. Its return
    /// value owns outgoing entries until delivery finishes, including unwind;
    /// the mutation's own value comes back beside it.
    fn commit_pages<V, O>(
        &self,
        operation: &'static str,
        mutate: impl FnOnce(&mut RouteHistory) -> V,
        commit: impl FnOnce() -> O,
    ) -> (V, O) {
        let (value, retired, undelivered) = self.shared.mutate_committing(mutate, commit);
        report_undelivered(operation, undelivered);
        (value, retired)
    }
}
