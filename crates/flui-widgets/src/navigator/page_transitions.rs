//! [`PageTransitionBuilder`] and [`PageTransitions`] — the ambient page-transition
//! hook (ADR-0221).
//!
//! A [`PageRoute`](super::PageRoute) or [`PopupRoute`](super::PopupRoute) given
//! no `.transitions(…)` asks the navigator that installs it for the nearest
//! builder of its kind. The navigator reads the hook from its own position in
//! the tree, in `init_state` and `did_change_dependencies`, and hands it to each
//! route it installs. The route adopts the builder and its durations **once**,
//! before its push animation starts: a hook replaced later reaches routes
//! installed after the change, never a transition already running.
//!
//! A route installed with no hook in scope keeps the jump cut. A route pushed
//! before its navigator mounts is installed by the mount's first flush, which
//! runs after the navigator has read the hook.
//!
//! Policy (which builder a platform gets) belongs to a design-system package;
//! this module is only the mechanism, plus the one transition more than one
//! package shares ([`ParallaxPageTransition`], ADR-0221 D4).

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt, impl_inherited_view};

use super::overlay_route::{RouteAnimation, RouteTransitionsBuilder};

/// Wraps a route's page in its entrance and exit transition, with the durations
/// that transition runs over.
///
/// One trait serves both paths: passed to a route directly
/// ([`PageRoute::transition_builder`](super::PageRoute::transition_builder)) or
/// installed for a subtree through [`PageTransitions`].
///
/// `build` runs on every tick of either animation, so it should build cheap
/// views. Under [`MotionPolicy::Reduce`](flui_animation::MotionPolicy::Reduce)
/// the route animation settles through the frame clock like any other
/// controller run, so `build` sees the settled value on the first frame; a
/// builder that branches on the policy must still produce that settled frame.
pub trait PageTransitionBuilder {
    /// How long the entrance runs.
    fn duration(&self) -> Duration;

    /// How long the exit runs; defaults to [`duration`](Self::duration).
    fn reverse_duration(&self) -> Duration {
        self.duration()
    }

    /// Wrap `child`, the route's page. `primary` runs 0 → 1 as the route
    /// enters and 1 → 0 as it leaves; `secondary` is the primary animation of
    /// the route covering this one, when the two coordinate.
    fn build(
        &self,
        ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView;
}

/// Erase a [`PageTransitionBuilder`] into the closure a route stores.
pub(crate) fn transitions_of(builder: Rc<dyn PageTransitionBuilder>) -> RouteTransitionsBuilder {
    Rc::new(move |ctx, primary, secondary, child| builder.build(ctx, primary, secondary, child))
}

/// The two hook slots a navigator resolves: one per route kind.
#[derive(Clone, Default)]
pub(crate) struct PageTransitionHooks {
    /// For page routes (`PageRoute`).
    pub(crate) page: Option<Rc<dyn PageTransitionBuilder>>,
    /// For popup routes (`PopupRoute`).
    pub(crate) popup: Option<Rc<dyn PageTransitionBuilder>>,
}

impl PageTransitionHooks {
    fn same_as(&self, other: &Self) -> bool {
        fn same(
            a: Option<&Rc<dyn PageTransitionBuilder>>,
            b: Option<&Rc<dyn PageTransitionBuilder>>,
        ) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        same(self.page.as_ref(), other.page.as_ref())
            && same(self.popup.as_ref(), other.popup.as_ref())
    }
}

/// Installs page-transition builders for the navigators below it.
///
/// Each slot is optional. A slot left empty here defers to the nearest
/// enclosing `PageTransitions` that fills it, so a subtree can replace the
/// popup transition and keep the page transition it inherits.
///
/// ```
/// use std::rc::Rc;
/// use std::time::Duration;
///
/// use flui_widgets::prelude::*;
/// use flui_widgets::{
///     Navigator, NavigatorHandle, PageTransitionBuilder, PageTransitions, ParallaxPageTransition,
/// };
///
/// let navigator = NavigatorHandle::new();
/// let app = PageTransitions::new(Navigator::new(navigator))
///     .page(Rc::new(ParallaxPageTransition::new()));
/// # let _ = app;
/// ```
#[derive(Clone, StatelessView)]
pub struct PageTransitions {
    page: Option<Rc<dyn PageTransitionBuilder>>,
    popup: Option<Rc<dyn PageTransitionBuilder>>,
    child: BoxedView,
}

impl PageTransitions {
    /// A scope over `child` that fills no slot yet.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            page: None,
            popup: None,
            child: child.into_view().boxed(),
        }
    }

    /// The transition page routes below adopt.
    #[must_use]
    pub fn page(mut self, builder: Rc<dyn PageTransitionBuilder>) -> Self {
        self.page = Some(builder);
        self
    }

    /// The transition popup routes below adopt.
    #[must_use]
    pub fn popup(mut self, builder: Rc<dyn PageTransitionBuilder>) -> Self {
        self.popup = Some(builder);
        self
    }

    /// The hooks a navigator at `ctx` adopts, registering a dependency so a
    /// replaced hook reaches the next route it installs.
    pub(crate) fn hooks_of(ctx: &dyn BuildContext) -> PageTransitionHooks {
        ctx.depend_on::<PageTransitionsScope, _>(|scope| scope.hooks.clone())
            .unwrap_or_default()
    }
}

impl fmt::Debug for PageTransitions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageTransitions")
            .field("page", &self.page.is_some())
            .field("popup", &self.popup.is_some())
            .finish_non_exhaustive()
    }
}

impl StatelessView for PageTransitions {
    /// Merge this scope's slots over the enclosing scope's and publish the
    /// result, so the nearest builder of each kind wins.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let outer = Self::hooks_of(ctx);
        PageTransitionsScope {
            hooks: PageTransitionHooks {
                page: self.page.clone().or(outer.page),
                popup: self.popup.clone().or(outer.popup),
            },
            child: self.child.clone(),
        }
    }
}

/// The merged slots, published to the subtree.
#[derive(Clone)]
struct PageTransitionsScope {
    hooks: PageTransitionHooks,
    child: BoxedView,
}

impl InheritedView for PageTransitionsScope {
    type Data = Option<Rc<dyn PageTransitionBuilder>>;

    /// The page slot, the one most readers want. The popup slot is read
    /// through the scope itself.
    fn data(&self) -> &Self::Data {
        &self.hooks.page
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !self.hooks.same_as(&old.hooks)
    }
}

impl_inherited_view!(PageTransitionsScope);
