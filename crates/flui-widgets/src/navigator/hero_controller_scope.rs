//! [`HeroControllerScope`] — the ambient host for a [`HeroController`].
//!
//! **Public.** An inherited widget that provides an optional `HeroController` to
//! the `Navigator`s beneath it.
//!
//! # Why this exists
//!
//! Without it, an app author had to write `navigator.add_observer(HeroController::new())`
//! by hand. A `Navigator` now resolves the nearest scope in `init_state` and attaches
//! its controller — or, when **no** scope is present, creates a default one so heroes
//! fly with zero boilerplate. `HeroControllerScope::none` blocks that.
//!
//! # The auto-default
//!
//! With no scope above, the outermost `Navigator` self-provides a default controller
//! (a design-system app shell could install an app-level scope instead).

use std::fmt;
use std::sync::Arc;

use flui_view::prelude::*;
use flui_view::{BoxedView, impl_inherited_view};

use super::hero_controller::HeroController;

/// Hosts a [`HeroController`] for the `Navigator`s in its subtree.
///
/// ```ignore
/// // A custom controller for one navigator:
/// HeroControllerScope::new(HeroController::new(), Navigator::new(handle))
///
/// // Disable hero flights for a subtree:
/// HeroControllerScope::none(some_child)
/// ```
///
/// Most apps need neither: a `Navigator` with no enclosing scope creates its own
/// default controller.
#[derive(Clone)]
pub struct HeroControllerScope {
    controller: Option<Arc<HeroController>>,
    child: BoxedView,
}

impl HeroControllerScope {
    /// Provide `controller` to the `Navigator`s below.
    pub fn new(controller: Arc<HeroController>, child: impl IntoView) -> Self {
        Self {
            controller: Some(controller),
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Block the subtree from receiving any hero controller — no flights run under it.
    pub fn none(child: impl IntoView) -> Self {
        Self {
            controller: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// The hosted controller, or `None` for a [`none`](Self::none) scope.
    #[must_use]
    pub(crate) fn controller(&self) -> Option<Arc<HeroController>> {
        self.controller.clone()
    }
}

impl fmt::Debug for HeroControllerScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroControllerScope")
            .field("has_controller", &self.controller.is_some())
            .finish_non_exhaustive()
    }
}

impl InheritedView for HeroControllerScope {
    type Data = Option<Arc<HeroController>>;

    fn data(&self) -> &Self::Data {
        &self.controller
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // A Navigator reads the controller once, in `init_state`; a changed controller
        // is not picked up mid-life — a deliberate read-once simplification. But
        // report the change honestly by identity so a future re-resolving Navigator
        // would see it.
        match (&self.controller, &old.controller) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        }
    }
}

impl_inherited_view!(HeroControllerScope);
