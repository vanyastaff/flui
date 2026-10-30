//! What the back and dismiss keys do inside a route.
//!
//! [`BackIntent`] pops the current route of the nearest navigator that has one
//! to pop: the route's own, or, from a nested navigator's first route, an
//! enclosing one's. [`DismissIntent`] pops the route only when it is a
//! dismissible modal, so Escape closes a dialog and never a page. Both are
//! answered by the focused route's own scope, which only the current route
//! keeps focusable, so a key never reaches a route that is entering below or
//! leaving above the top one.
//!
//! The nearest route's scope declares `BackIntent`, so it shadows every
//! enclosing route's binding (ADR-0023): the action itself walks the
//! navigators outward, as `maybe_pop`'s "bubble" asks, and a request no
//! navigator handles is reported unperformed, so the key reaches the host.

use flui_view::element::ElementKind;
use flui_view::{BoxedView, BuildContext, EventCx, IntoView, StatelessView, View, ViewExt};

use super::navigator::NavigatorHandle;
use crate::interaction::{Action, ActionOutcome, Actions, BackIntent, DismissIntent};

/// Pops the current route of the nearest navigator, from the route's own
/// outward, that handles the pop; declines when none can, so the key reaches
/// the host.
struct PopOnBack(NavigatorHandle);

impl Action<BackIntent> for PopOnBack {
    fn is_enabled(&self, _intent: &BackIntent) -> bool {
        self.0
            .self_and_enclosing()
            .any(|navigator| navigator.can_pop())
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &BackIntent) -> ActionOutcome {
        if self
            .0
            .self_and_enclosing()
            .any(|navigator| navigator.maybe_pop())
        {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Pops the current route when it is a dismissible modal.
struct PopOnDismiss(NavigatorHandle);

impl Action<DismissIntent> for PopOnDismiss {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &DismissIntent) -> ActionOutcome {
        if self.0.maybe_pop() {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// The route's key actions around its focus scope.
///
/// They sit above the scope, not inside it: a page with nothing focusable
/// (an `ErrorView` in place of a page whose build failed, a page of plain
/// text) leaves the focus on the route's scope itself, and the key's actions
/// are looked up from the focused node outward.
#[derive(Clone)]
pub(super) struct RouteKeys {
    /// The route is a dismissible modal, so `DismissIntent` pops it.
    pub(super) dismissible: bool,
    /// The route's focus scope.
    pub(super) scope: BoxedView,
}

impl View for RouteKeys {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for RouteKeys {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // Without a navigator in context there is nothing to pop.
        let Some(navigator) = NavigatorHandle::maybe_of(ctx) else {
            return self.scope.clone();
        };
        let mut actions = Actions::new(self.scope.clone()).action(PopOnBack(navigator.clone()));
        if self.dismissible {
            actions = actions.action(PopOnDismiss(navigator));
        }
        actions.into_view().boxed()
    }
}
