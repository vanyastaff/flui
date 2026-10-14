//! What the back and dismiss keys do inside a route.
//!
//! [`BackIntent`] pops the navigator's current route; [`DismissIntent`] pops it
//! only when the route is a dismissible modal, so Escape closes a dialog and
//! never a page. Both are answered by the focused route's own scope, which only
//! the current route keeps focusable, so a key never reaches a route that is
//! entering below or leaving above the top one.

use flui_view::{BoxedView, EventCx, IntoView, ViewExt};

use super::navigator::NavigatorHandle;
use crate::interaction::{Action, ActionOutcome, Actions, BackIntent, DismissIntent};

/// Pops the current route; declines at the bottom of the stack so the key
/// reaches the host.
struct PopOnBack(NavigatorHandle);

impl Action<BackIntent> for PopOnBack {
    fn is_enabled(&self, _intent: &BackIntent) -> bool {
        self.0.can_pop()
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &BackIntent) -> ActionOutcome {
        let _ = self.0.maybe_pop();
        ActionOutcome::Performed
    }
}

/// Pops the current route when it is a dismissible modal.
struct PopOnDismiss(NavigatorHandle);

impl Action<DismissIntent> for PopOnDismiss {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &DismissIntent) -> ActionOutcome {
        let _ = self.0.maybe_pop();
        ActionOutcome::Performed
    }
}

/// `page` with the route's key actions around it. Without a navigator in
/// context there is nothing to pop and `page` is returned as it is.
pub(super) fn route_keys(
    navigator: Option<NavigatorHandle>,
    dismissible: bool,
    page: BoxedView,
) -> BoxedView {
    let Some(navigator) = navigator else {
        return page;
    };
    let mut actions = Actions::new(page).action(PopOnBack(navigator.clone()));
    if dismissible {
        actions = actions.action(PopOnDismiss(navigator));
    }
    actions.into_view().boxed()
}
