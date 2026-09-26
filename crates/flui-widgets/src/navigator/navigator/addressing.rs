//! Admission under a `Router` (ADR-0093 §4), and the `Router`'s own doors.
//!
//! A navigator made by [`NavigatorHandle::addressed`] belongs to a `Router`:
//! every page on it is a route value with a path. Its public doors — the typed
//! pushes, `seed_initial`, and the named doors — admit only **pageless popups**
//! (`PopupRoute`, so `show_dialog` keeps working until dialogs move to overlay
//! entries) and refuse everything else: `PageRoute`, `SimpleRoute`, and any
//! third-party `NavigatorRoute`. The `Router` places its pages through the
//! crate-private doors below, which skip the check.
//!
//! A refused typed push cannot answer with a `Result` without changing a
//! public signature, so it returns a [`RouteResult`] already completed with
//! `None`, logs the refusal with `tracing::error!`, and fails a debug
//! assertion. A named door answers `NamedRouteError::NotAddressable`. Either
//! way the stack is untouched and the refused route is disposed unpushed.
//!
//! The facade's pops never remove a Router's last page: a Router always has a
//! location.

use std::fmt;

use crate::navigator::binding::is_pageless_popup;
use crate::navigator::history::ReplaceTarget;
use crate::navigator::named_route::GeneratedRoute;
use crate::navigator::overlay_route::NavigatorRoute;
use crate::navigator::result::{Completer, RouteResult};
use crate::navigator::route::{Route, RouteId};

use super::NavigatorHandle;

/// Why a Router's navigator refused a route. Its text is the text of
/// `RouterError::NotAddressable`, which a test in `router` pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unaddressable {
    /// `type_name` of the Router's route type.
    pub(crate) route_type: &'static str,
}

impl fmt::Display for Unaddressable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "only `{}` values may enter a Router's stack; a route with no path was pushed",
            self.route_type
        )
    }
}

impl NavigatorHandle {
    /// An empty, unmounted navigator that a `Router<R>` drives; `route_type`
    /// names `R` in refusals.
    pub(crate) fn addressed(route_type: &'static str) -> Self {
        Self::with_addressing(Some(route_type))
    }

    /// Whether a public door may place `route` on this navigator.
    pub(super) fn admits<P: NavigatorRoute>(&self, route: &P) -> bool {
        self.shared.addressing.is_none() || is_pageless_popup(route.binding_slot())
    }

    /// [`admits`](Self::admits) for a route a named door generated.
    pub(super) fn admits_generated(&self, route: &GeneratedRoute) -> bool {
        self.shared.addressing.is_none() || route.is_pageless_popup()
    }

    /// Whether a facade pop must be refused because it would take a Router's
    /// last page.
    pub(super) fn keeps_last_route(&self) -> bool {
        self.shared.addressing.is_some() && !self.can_pop()
    }

    /// Refuse `route` at a typed public door: dispose it unpushed, report, and
    /// hand back a result that is already complete with `None`.
    ///
    /// # Panics
    ///
    /// In a debug build, always — after disposing the route and with no lock
    /// held — so the unaddressable push is found where it was written.
    pub(super) fn refuse<P: NavigatorRoute>(
        &self,
        operation: &'static str,
        mut route: P,
    ) -> RouteResult<P::Output> {
        let refusal = Unaddressable {
            route_type: self
                .shared
                .addressing
                .expect("BUG: only a Router's navigator refuses a route"),
        };
        Route::dispose(&mut route);
        drop(route);
        tracing::error!(operation, route_type = refusal.route_type, "{refusal}");
        let (completer, result) = Completer::new();
        completer.complete(None);
        debug_assert!(false, "{refusal}");
        result
    }

    /// Seed a Router page before the navigator mounts.
    pub(crate) fn seed_page<P: NavigatorRoute>(&self, route: P) -> RouteId {
        self.seed_reporting_id(route).0
    }

    /// Push a Router page with its entrance transition.
    pub(crate) fn push_page<P: NavigatorRoute>(&self, route: P) -> RouteId {
        self.push_reporting_id(route).0
    }

    /// Push a Router page that replaces `target` — Flutter's `pushReplacement`
    /// aimed at one captured route.
    pub(crate) fn push_replacement_page<P: NavigatorRoute>(
        &self,
        target: RouteId,
        route: P,
    ) -> RouteId {
        self.push_replacement_erased_reporting_id(route, Some(ReplaceTarget::Route(target)), None)
            .0
    }

    /// Replace every route above `keep` (every route, for `None`) with `below`
    /// and `top` in one flush: the leaving routes are removed, `below` is added
    /// quietly, and only `top` runs an entrance transition. Returns the new
    /// routes' ids, bottom to top.
    pub(crate) fn replace_tail<P: NavigatorRoute>(
        &self,
        keep: Option<RouteId>,
        below: Vec<P>,
        top: P,
    ) -> Vec<RouteId> {
        let below: Vec<(RouteId, P)> = below
            .into_iter()
            .map(|route| (self.prepare(&route), route))
            .collect();
        let top_id = self.prepare(&top);
        let mut ids: Vec<RouteId> = below.iter().map(|(id, _)| *id).collect();
        ids.push(top_id);
        self.shared.mutate("replace_tail", |history| {
            history.replace_tail_with_ids(keep, below, (top_id, top));
        });
        ids
    }
}
