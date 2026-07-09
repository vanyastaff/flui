//! [`NavigatorRoute`] and [`SimpleRoute`] — the bridge from the pure route stack
//! to the overlay.
//!
//! ADR-0019 U3. Private; nothing here is exported.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/routes.dart:55` (`OverlayRoute`) —
//! *"A route that displays widgets in the Navigator's Overlay"*. It is the layer
//! that owns `overlayEntries`, `createOverlayEntries()`, `install()`, and the
//! `didPop` → `navigator.finalizeRoute` bridge.
//!
//! # Where FLUI puts the overlay entry
//!
//! Flutter's `OverlayRoute` **owns** its `List<OverlayEntry>`, and the navigator
//! reaches them through `route.overlayEntries` (`navigator.dart:4151`). FLUI
//! cannot: the route lives behind `Box<dyn ErasedRoute>` inside `RouteHistory`,
//! and exposing overlay entries there would break U2's pure-data invariant, which
//! `route_stack_flush_is_pure_data` enforces.
//!
//! So the `NavigatorState` keeps the entries, in a `RouteId -> OverlayEntry` map
//! it maintains alongside the stack. The route only supplies the *builder*. The
//! two arrangements are observationally identical — `_allRouteOverlayEntries`
//! (`navigator.dart:4151-4153`) flattens the entries in `_history` order, which is
//! exactly what the map lookup over `RouteHistory::ids()` produces.
//!
//! One consequence, recorded rather than hidden: Flutter's `_disposeRouteEntry`
//! removes a route's overlay entries **before** calling `entry.dispose()`
//! (`:3978-3987`). FLUI disposes the route inside the flush and removes the
//! overlay entry just after. Nothing observes the difference, because a FLUI route
//! holds no reference to its overlay entry.
//!
//! # Not implemented
//!
//! No `TransitionRoute`, `ModalRoute`, `PopupRoute` or `PageRoute` (ADR-0019 §2.4,
//! §5 U5): no animation, no barrier, no focus scope, no `offstage`. A
//! [`SimpleRoute`] pushes and pops instantly, which is exactly the floor
//! ADR-0019 §2.4 identified.

use std::sync::Arc;

use flui_view::{BoxedView, BuildContext};

use super::route::{Route, RouteSettings};
use crate::overlay::OverlayBuilder;

/// A route that can be shown in the navigator's overlay.
///
/// The split from [`Route`] is the `OverlayRoute` layer: [`Route`] is lifecycle
/// and result, this adds "what to show".
pub(crate) trait NavigatorRoute: Route {
    /// Builds this route's subtree. Flutter's `OverlayRoute.createOverlayEntries`
    /// (`routes.dart:61`) plus `ModalRoute.buildPage`.
    ///
    /// Returned as a shared closure, not `&self`, because the navigator installs
    /// it into an `OverlayEntry` that outlives any borrow of the route.
    fn overlay_builder(&self) -> OverlayBuilder;
}

/// The floor: a route with content, an instant transition, and a typed result.
///
/// Flutter's nearest equivalent is a bare `OverlayRoute` subclass — no
/// `TransitionRoute`, so `finishedWhenPopped` stays `true` and a pop finalizes
/// synchronously (`routes.dart:84`, `:90`).
pub(crate) struct SimpleRoute<T> {
    settings: RouteSettings,
    builder: OverlayBuilder,
    /// Flutter's `currentResult` (`navigator.dart:426`) — the `??` fallback.
    current_result: Option<T>,
    /// Set by tests / `LocalHistoryRoute`-shaped routes.
    handles_pop_internally: bool,
    /// When `false`, `did_pop` refuses, as `LocalHistoryRoute.didPop` does while
    /// it still has local entries (`routes.dart:950-967`).
    consents_to_pop: bool,
}

impl<T> SimpleRoute<T> {
    /// A route showing whatever `builder` builds.
    pub(crate) fn new(
        builder: impl Fn(&dyn BuildContext) -> BoxedView + Send + Sync + 'static,
    ) -> Self {
        Self {
            settings: RouteSettings::default(),
            builder: Arc::new(builder),
            current_result: None,
            handles_pop_internally: false,
            consents_to_pop: true,
        }
    }

    pub(crate) fn named(mut self, name: impl Into<String>) -> Self {
        self.settings = RouteSettings::named(name);
        self
    }

    /// The `result ?? currentResult` fallback.
    pub(crate) fn with_current_result(mut self, result: T) -> Self {
        self.current_result = Some(result);
        self
    }

    /// Make `did_pop` refuse, modelling `LocalHistoryRoute`.
    pub(crate) fn refusing_pop(mut self) -> Self {
        self.consents_to_pop = false;
        self
    }

    /// Make `will_handle_pop_internally` true, modelling `LocalHistoryRoute`.
    pub(crate) fn handling_pop_internally(mut self) -> Self {
        self.handles_pop_internally = true;
        self
    }
}

impl<T: Send + Sync + Clone + 'static> Route for SimpleRoute<T> {
    type Output = T;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<T> {
        self.current_result.clone()
    }

    fn will_handle_pop_internally(&self) -> bool {
        self.handles_pop_internally
    }

    fn did_pop(&mut self) -> bool {
        self.consents_to_pop
    }
}

impl<T: Send + Sync + Clone + 'static> NavigatorRoute for SimpleRoute<T> {
    fn overlay_builder(&self) -> OverlayBuilder {
        Arc::clone(&self.builder)
    }
}
