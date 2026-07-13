//! [`PageRoute`] and [`PopupRoute`] — the two public route shapes.
//!
//! These are the first routes with an animation that an app author
//! can construct, and the API sign-off gate that lets them out.
//!
//! # Flutter parity
//!
//! - `PageRoute` (`.flutter/packages/flutter/lib/src/widgets/pages.dart:23-67`)
//! - `PageRouteBuilder` (`pages.dart:89-170`)
//! - `PopupRoute` (`.../widgets/routes.dart:2380-2399`)
//!
//! master `3.33.0-0.0.pre-6280-g88e87cd963f`.
//!
//! # Why there is no `PageRouteBuilder`
//!
//! Flutter needs two types because `PageRoute` is an abstract class you subclass,
//! and `PageRouteBuilder` is the escape hatch for people who would rather pass
//! closures. Rust has no subclassing, and this crate declines to export
//! `TransitionRoute` / `ModalRoute` as extensible bases until a trait shape is
//! designed and signed off. So [`PageRoute`] *is* the builder: it takes a
//! [`RoutePageBuilder`] and an optional [`RouteTransitionsBuilder`], and every
//! property `PageRouteBuilder` exposes as a constructor argument is a method here.
//!
//! One consequence, stated rather than hidden: **`PageRoute` is not extensible.**
//! An app cannot today write a route with custom `buildPage` *state* (Flutter's
//! `CupertinoPageRoute` pattern). Closures cover the cases that matter now; the
//! trait shape is a problem for a later pass.
//!
//! # What these two fix that `ModalRoute` alone cannot
//!
//! `opaque` and the transition family. A [`PageRoute`] is `opaque` — once its
//! entrance transition completes, the routes beneath it leave the widget tree
//! unless they set `maintain_state` (`RenderTheater`'s skip-count work). A
//! [`PopupRoute`] is not: the page under a dialog stays visible.
//!
//! And `PageRoute` coordinates its `secondaryAnimation` only with other
//! `PageRoute`s (`pages.dart:58-61`), which FLUI expresses as a
//! [`TransitionGroup`] travelling with the published peer — a popup opening over a
//! page must not slide the page away.
//!
//! # Divergences, inherited from the private layers
//!
//! Everything `modal_route.rs` records: no `FocusScope`, no `BlockSemantics` or
//! barrier semantics, no `AnimatedModalBarrier` colour tween, no `PopScope`, no
//! `LocalHistoryRoute`, no predictive back. Plus, here: no `fullscreenDialog`, no
//! `allowSnapshotting`, no `barrierLabel`, no `filter`. None of these is claimed.
//!
//! [`RouteTransitionsBuilder`]: super::overlay_route::RouteTransitionsBuilder

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_types::Color;
use flui_view::{BoxedView, BuildContext};

use super::binding::{RouteBindingSlot, TransitionGroup};
use super::modal_route::ModalRoute;
use super::overlay_route::{NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};

/// `PageRouteBuilder`'s `transitionDuration` default (`pages.dart:95`).
///
/// `PageRoute` and `PopupRoute` are abstract in Flutter and declare no default;
/// 300 ms is the one every concrete builder in the framework picks.
const DEFAULT_TRANSITION_DURATION: Duration = Duration::from_millis(300);

/// Erase a page closure into a [`RoutePageBuilder`].
fn page_builder<F>(page: F) -> RoutePageBuilder
where
    F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView + 'static,
{
    Rc::new(page)
}

/// Generate `Route` + `NavigatorRoute` for a newtype that wraps a [`ModalRoute`].
///
/// Rust has no `extends`, and these two routes differ only in the values they set
/// on the modal beneath them. Fifteen forwarding methods, written once.
macro_rules! delegate_modal_route {
    ($ty:ident) => {
        impl<T: Send + Clone + 'static> Route for $ty<T> {
            type Output = T;

            fn settings(&self) -> &RouteSettings {
                self.modal.settings()
            }

            fn current_result(&mut self) -> Option<T> {
                self.modal.current_result()
            }

            fn finished_when_popped(&self) -> bool {
                self.modal.finished_when_popped()
            }

            fn will_handle_pop_internally(&self) -> bool {
                self.modal.will_handle_pop_internally()
            }

            fn vetoes_pop(&self) -> bool {
                self.modal.vetoes_pop()
            }

            fn install(&mut self) {
                self.modal.install();
            }

            fn did_push(&mut self) -> PushCompletion {
                self.modal.did_push()
            }

            fn did_add(&mut self) {
                self.modal.did_add();
            }

            fn did_replace(&mut self, previous: Option<RouteId>) {
                self.modal.did_replace(previous);
            }

            fn did_pop(&mut self) -> bool {
                self.modal.did_pop()
            }

            fn did_complete(&mut self, result: Option<&T>) {
                self.modal.did_complete(result);
            }

            fn did_pop_next(&mut self, popped: RouteId) {
                self.modal.did_pop_next(popped);
            }

            fn did_change_next(&mut self, next: Option<RouteId>) {
                self.modal.did_change_next(next);
            }

            fn did_change_previous(&mut self, previous: Option<RouteId>) {
                self.modal.did_change_previous(previous);
            }

            fn on_pop_invoked(&mut self, did_pop: bool) {
                self.modal.on_pop_invoked(did_pop);
            }

            fn dispose(&mut self) {
                self.modal.dispose();
            }
        }

        impl<T: Send + Clone + 'static> NavigatorRoute for $ty<T> {
            fn content_builder(&self) -> RouteContentBuilder {
                self.modal.content_builder()
            }

            fn binding_slot(&self) -> Option<&RouteBindingSlot> {
                self.modal.binding_slot()
            }
        }

        impl<T: Send + Clone + 'static> fmt::Debug for $ty<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($ty))
                    .field("name", &self.modal.settings().name())
                    .finish_non_exhaustive()
            }
        }
    };
}

// ============================================================================
// PageRoute
// ============================================================================

/// A modal route that replaces the entire screen.
///
/// Flutter's `PageRoute` (`pages.dart:23`) with `PageRouteBuilder`'s closures.
/// `opaque` is `true`, so once the entrance transition completes the routes below
/// are dropped from the widget tree — unless they set `maintain_state`.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
///
/// use flui_widgets::prelude::*;
/// use flui_widgets::{PageRoute, Text};
///
/// let route = PageRoute::<()>::new(|_ctx, _animation, _secondary| {
///     Text::new("Details").into_view().boxed()
/// })
/// .named("/details");
///
/// // `navigator.push(route)` returns a `RouteResult<()>` that resolves on pop.
/// # let _ = route;
/// ```
///
/// # Transitions
///
/// The default is a jump cut, as `PageRouteBuilder`'s is
/// (`pages.dart:68-75`). Supply one with [`transitions`](Self::transitions); the
/// `animation` argument drives this route's entrance and exit, and
/// `secondary_animation` drives it while *another* `PageRoute` covers it.
pub struct PageRoute<T> {
    modal: ModalRoute<T>,
}

impl<T: Send + Clone + 'static> PageRoute<T> {
    /// A page route showing `page`, with a 300 ms jump-cut transition.
    #[must_use]
    pub fn new<F>(page: F) -> Self
    where
        F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView + 'static,
    {
        Self {
            modal: ModalRoute::new(DEFAULT_TRANSITION_DURATION, page_builder(page))
                // `PageRoute.opaque => true` (`pages.dart:50`).
                .opaque(true)
                // `canTransitionTo/From(other) => other is PageRoute`
                // (`pages.dart:58-61`).
                .group(TransitionGroup::Page),
        }
    }

    /// `buildTransitions` (`routes.dart:1591`): wrap the page in its entrance and
    /// exit animation.
    #[must_use]
    pub fn transitions<F>(mut self, transitions: F) -> Self
    where
        F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation, BoxedView) -> BoxedView
            + 'static,
    {
        self.modal = self.modal.transitions(Rc::new(transitions));
        self
    }

    /// `RouteSettings.name`.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.modal = self.modal.named(name);
        self
    }

    /// `transitionDuration` (`routes.dart:140`). Defaults to 300 ms.
    #[must_use]
    pub fn transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.duration(duration);
        self
    }

    /// `reverseTransitionDuration` (`routes.dart:148`), which defaults to
    /// [`transition_duration`](Self::transition_duration).
    #[must_use]
    pub fn reverse_transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.reverse_duration(duration);
        self
    }

    /// `maintainState` (`routes.dart:1893`), default `true`.
    ///
    /// `false` lets an opaque route above this one destroy its subtree, and rebuild
    /// it fresh when uncovered — cheaper, but the page loses its state.
    #[must_use]
    pub fn maintain_state(mut self, maintain_state: bool) -> Self {
        self.modal = self.modal.maintain_state(maintain_state);
        self
    }

    /// `barrierDismissible` (`pages.dart:32`, `:53`), default `false`.
    ///
    /// A page route's barrier is invisible and covers the whole screen, so this is
    /// mostly useful for full-screen dialogs.
    #[must_use]
    pub fn barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.modal = self.modal.barrier_dismissible(dismissible);
        self
    }

    /// The `result ?? currentResult` fallback (`navigator.dart:426`): what a
    /// `pop()` with no value delivers.
    #[must_use]
    pub fn with_current_result(mut self, result: T) -> Self {
        self.modal = self.modal.with_current_result(result);
        self
    }
}

impl<T: Send + Clone + 'static> PageRoute<T> {
    /// The modal handle, whose `set_offstage` is the seam `HeroController` drives
    /// to measure a route's final hero geometry (`heroes.dart:967`). Test-facing
    /// until `HeroController` gives it a production caller.
    #[cfg(test)]
    pub(crate) fn modal_handle(&self) -> super::modal_route::ModalHandle {
        self.modal.handle()
    }

    /// The animation handle, for driving a transition by hand. Test-facing:
    /// FLUI's controller returns no `TickerFuture`, so a unit test cannot await
    /// one. `tests/routes.rs` drives the real clock instead.
    #[cfg(test)]
    pub(crate) fn transition_handle(&self) -> super::transition_route::TransitionHandle {
        self.modal.transition_handle()
    }
}

delegate_modal_route!(PageRoute);

// ============================================================================
// PopupRoute
// ============================================================================

/// A modal route that shows over the current page — a dialog, a menu, a sheet.
///
/// Flutter's `PopupRoute` (`routes.dart:2380`): `opaque` is `false` and
/// `maintainState` is `true`, so the route below stays built **and** visible.
///
/// # Example
///
/// ```
/// use flui_types::Color;
/// use flui_widgets::prelude::*;
/// use flui_widgets::{PopupRoute, Text};
///
/// let route = PopupRoute::<bool>::new(|_ctx, _animation, _secondary| {
///     Text::new("Delete this?").into_view().boxed()
/// })
/// .barrier_color(Color::rgba(0, 0, 0, 128))
/// .barrier_dismissible(true);
/// # let _ = route;
/// ```
///
/// A dismissible barrier pops the route with no value, so `RouteResult<bool>`
/// resolves to `None` — Flutter's `Navigator.maybePop(context)` from
/// `ModalBarrier`.
pub struct PopupRoute<T> {
    modal: ModalRoute<T>,
}

impl<T: Send + Clone + 'static> PopupRoute<T> {
    /// A popup showing `page`, with a 300 ms jump-cut transition, an invisible
    /// non-dismissible barrier, and `maintain_state = true`.
    #[must_use]
    pub fn new<F>(page: F) -> Self
    where
        F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView + 'static,
    {
        Self {
            // `PopupRoute.opaque => false`, `maintainState => true`
            // (`routes.dart:2391-2394`); `TransitionGroup::Default` is
            // `TransitionRoute`'s `canTransitionTo/From => true`.
            modal: ModalRoute::new(DEFAULT_TRANSITION_DURATION, page_builder(page))
                .opaque(false)
                .maintain_state(true),
        }
    }

    /// `buildTransitions` (`routes.dart:1591`).
    #[must_use]
    pub fn transitions<F>(mut self, transitions: F) -> Self
    where
        F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation, BoxedView) -> BoxedView
            + 'static,
    {
        self.modal = self.modal.transitions(Rc::new(transitions));
        self
    }

    /// `RouteSettings.name`.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.modal = self.modal.named(name);
        self
    }

    /// `transitionDuration` (`routes.dart:140`). Defaults to 300 ms.
    #[must_use]
    pub fn transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.duration(duration);
        self
    }

    /// `reverseTransitionDuration` (`routes.dart:148`).
    #[must_use]
    pub fn reverse_transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.reverse_duration(duration);
        self
    }

    /// `barrierDismissible` (`routes.dart:1804`), default `false`.
    ///
    /// When `true`, a tap on the barrier pops this route with no value.
    #[must_use]
    pub fn barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.modal = self.modal.barrier_dismissible(dismissible);
        self
    }

    /// `barrierColor` (`routes.dart:1774`). Absent, the barrier is invisible —
    /// but it still absorbs pointers.
    ///
    /// **Divergence:** Flutter drives the colour through `barrierCurve` with an
    /// `AnimatedModalBarrier`; FLUI paints it flat.
    #[must_use]
    pub fn barrier_color(mut self, color: Color) -> Self {
        self.modal = self.modal.barrier_color(color);
        self
    }

    /// `maintainState` (`routes.dart:2394`), default `true`.
    #[must_use]
    pub fn maintain_state(mut self, maintain_state: bool) -> Self {
        self.modal = self.modal.maintain_state(maintain_state);
        self
    }

    /// The `result ?? currentResult` fallback (`navigator.dart:426`).
    #[must_use]
    pub fn with_current_result(mut self, result: T) -> Self {
        self.modal = self.modal.with_current_result(result);
        self
    }
}

impl<T: Send + Clone + 'static> PopupRoute<T> {
    /// See [`PageRoute::transition_handle`].
    #[cfg(test)]
    pub(crate) fn transition_handle(&self) -> super::transition_route::TransitionHandle {
        self.modal.transition_handle()
    }
}

delegate_modal_route!(PopupRoute);
