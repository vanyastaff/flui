//! [`PageRoute`] and [`PopupRoute`] — the two public route shapes.
//!
//! These are the first routes with an animation that an app author
//! can construct, and the API sign-off gate that lets them out.
//!
//! # Closures, not subclasses
//!
//! Rust has no subclassing, and this crate declines to export
//! `TransitionRoute` / `ModalRoute` as extensible bases until a trait shape is
//! designed and signed off. So [`PageRoute`] takes a
//! [`RoutePageBuilder`] and an optional [`RouteTransitionsBuilder`], and every
//! property is a builder method.
//!
//! One consequence, stated rather than hidden: **`PageRoute` is not extensible.**
//! An app cannot today write a route with custom page-building *state*. Closures
//! cover the cases that matter now; the trait shape is a problem for a later pass.
//!
//! # What these two fix that `ModalRoute` alone cannot
//!
//! `opaque` and the transition family. A [`PageRoute`] is `opaque` — once its
//! entrance transition completes, the routes beneath it leave the widget tree
//! unless they set `maintain_state` (`RenderTheater`'s skip-count work). A
//! [`PopupRoute`] is not: the page under a dialog stays visible.
//!
//! And `PageRoute` coordinates its secondary animation only with other
//! `PageRoute`s, expressed as a
//! [`TransitionGroup`] travelling with the published peer — a popup opening over a
//! page must not slide the page away.
//!
//! # Not implemented, inherited from the private layers
//!
//! Everything `modal_route.rs` records: no `FocusScope`, no `BlockSemantics` or
//! barrier semantics, no animated barrier colour tween, no `PopScope`, no
//! local-history routes, no predictive back. Plus, here: no fullscreen-dialog flag,
//! no snapshotting, no barrier label, no backdrop filter. None of these is claimed.
//!
//! [`RouteTransitionsBuilder`]: super::overlay_route::RouteTransitionsBuilder

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_painting::styling::Color;
use flui_view::{BoxedView, BuildContext};

use super::binding::{RouteBindingSlot, TransitionGroup};
use super::modal_route::ModalRoute;
use super::overlay_route::{NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};

/// The default transition duration for both route shapes.
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
/// The default is a jump cut. Supply one with [`transitions`](Self::transitions); the
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
                .opaque(true)
                // Coordinates transitions only with other `PageRoute`s.
                .group(TransitionGroup::Page),
        }
    }

    /// Wrap the page in its entrance and exit animation.
    #[must_use]
    pub fn transitions<F>(mut self, transitions: F) -> Self
    where
        F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation, BoxedView) -> BoxedView
            + 'static,
    {
        self.modal = self.modal.transitions(Rc::new(transitions));
        self
    }

    /// The route's name.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.modal = self.modal.named(name);
        self
    }

    /// The entrance duration. Defaults to 300 ms.
    #[must_use]
    pub fn transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.duration(duration);
        self
    }

    /// The exit duration; defaults to
    /// [`transition_duration`](Self::transition_duration).
    #[must_use]
    pub fn reverse_transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.reverse_duration(duration);
        self
    }

    /// Whether the route keeps its state while covered; default `true`.
    ///
    /// `false` lets an opaque route above this one destroy its subtree, and rebuild
    /// it fresh when uncovered — cheaper, but the page loses its state.
    #[must_use]
    pub fn maintain_state(mut self, maintain_state: bool) -> Self {
        self.modal = self.modal.maintain_state(maintain_state);
        self
    }

    /// Whether tapping the barrier dismisses the route; default `false`.
    ///
    /// A page route's barrier is invisible and covers the whole screen, so this is
    /// mostly useful for full-screen dialogs.
    #[must_use]
    pub fn barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.modal = self.modal.barrier_dismissible(dismissible);
        self
    }

    /// The barrier colour. Absent (the default), the
    /// barrier is invisible — but still absorbs pointers once this route's entry
    /// transition has covered the routes below (the route is `opaque`).
    ///
    /// The Cupertino page route needs its own transition-only dim, so this
    /// mirrors `PopupRoute::barrier_color`.
    ///
    /// The colour is painted flat for the barrier's entire visible lifetime, not
    /// faded in with the transition.
    #[must_use]
    pub fn barrier_color(mut self, color: Color) -> Self {
        self.modal = self.modal.barrier_color(color);
        self
    }

    /// The `result ?? current_result` fallback: what a `pop()` with no value
    /// delivers.
    #[must_use]
    pub fn with_current_result(mut self, result: T) -> Self {
        self.modal = self.modal.with_current_result(result);
        self
    }

    /// Opt into an iOS-style edge-swipe-back gesture. Default `false`.
    ///
    /// There is no platform-theme selection yet, so the gesture is an explicit
    /// opt-in per route rather than something a Cupertino theme turns on. There is
    /// no fullscreen-dialog flag either, so this opt-in itself is the gate.
    ///
    /// **Not a mid-life toggle.** Set it once, before pushing the route:
    /// `ModalScopeState::build` reads the flag on every build to decide
    /// whether to wrap the page in the detector, and flipping it after the
    /// route is live would change the page subtree's identity out from under
    /// any `StatefulView` inside it, discarding its state (a cancelled
    /// mid-gesture drag must not do this).
    #[must_use]
    pub fn back_gesture(mut self, enabled: bool) -> Self {
        self.modal = self.modal.back_gesture(enabled);
        self
    }
}

impl<T: Send + Clone + 'static> PageRoute<T> {
    /// The modal handle, whose `set_offstage` is the seam `HeroController` drives
    /// to measure a route's final hero geometry. Test-facing
    /// until `HeroController` gives it a production caller; read through
    /// `crate::__test_access::RouteProbe`.
    pub(crate) fn modal_handle(&self) -> super::modal_route::ModalHandle {
        self.modal.handle()
    }

    /// The animation handle, for driving a transition by hand. Test-facing: a
    /// test drives the transition with `set_value` through this handle
    /// rather than awaiting the `TickerFuture` `did_push` returns, which needs a
    /// real `Vsync`. Read through `crate::__test_access::RouteProbe`.
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
/// `opaque` is `false` and `maintain_state` is `true`, so the route below stays
/// built **and** visible.
///
/// # Example
///
/// ```
/// use flui_painting::styling::Color;
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
/// resolves to `None`.
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
            // Not opaque, and keeps its state; `TransitionGroup::Default`
            // coordinates with any transition route.
            modal: ModalRoute::new(DEFAULT_TRANSITION_DURATION, page_builder(page))
                .opaque(false)
                .maintain_state(true),
        }
    }

    /// Wrap the page in its entrance and exit animation.
    #[must_use]
    pub fn transitions<F>(mut self, transitions: F) -> Self
    where
        F: Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation, BoxedView) -> BoxedView
            + 'static,
    {
        self.modal = self.modal.transitions(Rc::new(transitions));
        self
    }

    /// The route's name.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.modal = self.modal.named(name);
        self
    }

    /// The entrance duration. Defaults to 300 ms.
    #[must_use]
    pub fn transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.duration(duration);
        self
    }

    /// The exit duration; defaults to the entrance duration.
    #[must_use]
    pub fn reverse_transition_duration(mut self, duration: Duration) -> Self {
        self.modal = self.modal.reverse_duration(duration);
        self
    }

    /// Whether tapping the barrier dismisses the route; default `false`.
    ///
    /// When `true`, a tap on the barrier pops this route with no value.
    #[must_use]
    pub fn barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.modal = self.modal.barrier_dismissible(dismissible);
        self
    }

    /// The barrier colour. Absent, the barrier is invisible —
    /// but it still absorbs pointers.
    ///
    /// The colour is painted flat, not faded with the transition.
    #[must_use]
    pub fn barrier_color(mut self, color: Color) -> Self {
        self.modal = self.modal.barrier_color(color);
        self
    }

    /// Whether the route keeps its state while covered; default `true`.
    #[must_use]
    pub fn maintain_state(mut self, maintain_state: bool) -> Self {
        self.modal = self.modal.maintain_state(maintain_state);
        self
    }

    /// The `result ?? current_result` fallback.
    #[must_use]
    pub fn with_current_result(mut self, result: T) -> Self {
        self.modal = self.modal.with_current_result(result);
        self
    }
}

impl<T: Send + Clone + 'static> PopupRoute<T> {
    /// See [`PageRoute::transition_handle`].
    pub(crate) fn transition_handle(&self) -> super::transition_route::TransitionHandle {
        self.modal.transition_handle()
    }
}

delegate_modal_route!(PopupRoute);
