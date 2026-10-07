//! [`VsyncScope`] — provides a shared [`Vsync`] to a subtree so a binding can
//! drive every implicitly-animated widget below it off one virtual timeline.

use flui_animation::Vsync;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Provides a shared [`Vsync`] registry to its descendant implicitly-animated
/// widgets.
///
/// A binding (or a test harness) wraps the application subtree in
/// `VsyncScope::new(binding.vsync(), child)`. Every implicitly-animated widget
/// below (`AnimatedOpacity`, …) reads this registry ambiently in `init_state`
/// (via `ctx.get::<VsyncScope, _>(..)`) and registers its controller in it, so
/// the binding's `pump_frame` advances all of them on the same virtual clock —
/// deterministically, with no `thread::sleep`.
///
/// FLUI is non-singleton, so there is no ambient ticker owner: the registry is
/// handed down explicitly as inherited data, scoped to a subtree.
///
/// An implicitly-animated widget with no `VsyncScope` above it still functions:
/// its controller is created with its own scheduler-ticker (which drives it off
/// wall-clock time on a real display), it is simply not binding-driven. Gesture
/// ownership is stricter and unrelated: gesture widgets require their
/// presentation's `GestureArenaScope`.
///
/// Replacing the registry notifies retained states, which migrate their ticker registrations.
#[derive(Clone)]
pub struct VsyncScope {
    /// The shared registry handed to descendants. Cloning the scope clones this
    /// `Arc`-backed handle, so all clones observe the same registry.
    vsync: Vsync,
    /// The wrapped subtree the registry is provided to.
    child: BoxedView,
}

impl VsyncScope {
    /// Wrap `child` in a scope that provides `vsync` to its descendants.
    #[must_use]
    pub fn new(vsync: Vsync, child: impl IntoView) -> Self {
        Self {
            vsync,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// The shared registry this scope provides — what a descendant implicitly-
    /// animated widget reads in `init_state` to register its controller against.
    #[must_use]
    pub fn vsync(&self) -> &Vsync {
        &self.vsync
    }
}

impl std::fmt::Debug for VsyncScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VsyncScope")
            .field("vsync", &self.vsync)
            .finish_non_exhaustive()
    }
}

impl InheritedView for VsyncScope {
    type Data = Vsync;

    fn data(&self) -> &Self::Data {
        &self.vsync
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !self.vsync.is_same(&old.vsync)
    }
}

impl_inherited_view!(VsyncScope);
