//! [`ScrollPositionScope`] — provides the enclosing scrollable's shared
//! [`ScrollPosition`] to its subtree.

use flui_rendering::view::ScrollPosition;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Provides the enclosing [`Scrollable`](super::Scrollable)'s shared
/// [`ScrollPosition`] to descendant widgets.
///
/// `Scrollable` installs this around the product of its
/// [`viewport_builder`](super::Scrollable::viewport_builder), so anything
/// living inside the scrolled content — a floating header's snap trigger, a
/// scrollbar, an "is the user scrolling" fade — can reach the SAME position
/// the gestures drive, without threading it through every intermediate
/// widget by hand.
///
/// Flutter parity: the discovery role of `Scrollable.of(context).position`,
/// carried as inherited data because FLUI's `Scrollable` state is not
/// otherwise reachable from a descendant.
///
/// The handle itself is `Arc`-backed and stable for a given controller;
/// [`update_should_notify`](InheritedView::update_should_notify) fires only
/// when a rebuild swaps in a genuinely different position (a controller
/// swap), which is exactly when a dependent must re-subscribe its listeners.
#[derive(Clone)]
pub struct ScrollPositionScope {
    position: ScrollPosition,
    child: BoxedView,
}

impl ScrollPositionScope {
    /// Wrap `child` in a scope providing `position` to its descendants.
    #[must_use]
    pub fn new(position: ScrollPosition, child: impl IntoView) -> Self {
        Self {
            position,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// The shared position this scope provides.
    #[must_use]
    pub fn position(&self) -> &ScrollPosition {
        &self.position
    }
}

impl std::fmt::Debug for ScrollPositionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollPositionScope")
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl InheritedView for ScrollPositionScope {
    type Data = ScrollPosition;

    fn data(&self) -> &Self::Data {
        &self.position
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // Same underlying position ⇒ dependents' subscriptions stay valid.
        !self.position.ptr_eq(&old.position)
    }
}

impl_inherited_view!(ScrollPositionScope);
