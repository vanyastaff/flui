//! Where shown focus goes to be seen: a [`Focus`](super::Focus) whose node
//! takes the primary focus while focus is shown (keyboard traversal,
//! assistive technology, or a key pressed since the last pointer press)
//! hands its rectangle to the nearest [`FocusRevealScope`] above it.
//!
//! A scope is a widget that can expose part of its subtree: a scrollable
//! moves its offset, a collapsing header expands. Each scope reveals what
//! it is handed, then hands the part of it that lies in its own bounds to
//! the scope above (ADR-0168's nearest-first walk), so a row in a list in a
//! pager is brought into the list's viewport and the list into the pager's.
//! Every rectangle is in root coordinates from the last layout, so a scope
//! never waits for the layout its own move causes before handing on.

use std::rc::Rc;

use flui_foundation::geometry::Rect;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;

/// A widget that can expose part of its subtree.
pub(crate) trait RevealTarget {
    /// Bring `rect`, in root coordinates, into view as far as this target
    /// can, and hand on to the scope above it.
    fn reveal(&self, rect: Rect<f64>);
}

/// A shared [`RevealTarget`], compared by identity.
pub(crate) type SharedRevealTarget = Rc<dyn RevealTarget>;

/// Publishes the nearest [`RevealTarget`] to a subtree.
#[derive(Clone)]
pub(crate) struct FocusRevealScope {
    target: SharedRevealTarget,
    child: BoxedView,
}

impl std::fmt::Debug for FocusRevealScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusRevealScope").finish_non_exhaustive()
    }
}

impl FocusRevealScope {
    pub(crate) fn new(target: SharedRevealTarget, child: impl IntoView) -> Self {
        Self {
            target,
            child: child.into_view().boxed(),
        }
    }

    /// The nearest target above `ctx`, depending on it.
    pub(crate) fn of(ctx: &dyn BuildContext) -> Option<SharedRevealTarget> {
        ctx.depend_on::<Self, _>(|scope| Rc::clone(&scope.target))
    }
}

impl InheritedView for FocusRevealScope {
    type Data = SharedRevealTarget;

    fn data(&self) -> &Self::Data {
        &self.target
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.target, &old.target)
    }
}

impl_inherited_view!(FocusRevealScope);

/// How far `rect` moves along one axis to come into the span
/// `viewport_start..viewport_end`: the distance of its nearest obscured edge,
/// none when it shows, or when it is larger than the span and covers it.
/// Positive when the rectangle lies past the span's end.
pub(crate) fn reveal_distance(start: f64, end: f64, viewport_start: f64, viewport_end: f64) -> f64 {
    if start < viewport_start && end > viewport_end {
        0.0
    } else if start < viewport_start {
        start - viewport_start
    } else if end > viewport_end {
        end - viewport_end
    } else {
        0.0
    }
}
