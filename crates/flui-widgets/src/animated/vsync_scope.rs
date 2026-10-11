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
/// below (`AnimatedOpacity`, …) acquires this registry in `init_state`
/// through [`VsyncScope::maybe_of`] and creates an owning controller in it, so
/// the binding's `pump_frame` advances all of them on the same virtual clock —
/// deterministically, with no `thread::sleep`.
///
/// FLUI is non-singleton, so there is no ambient ticker owner: the registry is
/// handed down explicitly as inherited data, scoped to a subtree.
///
/// Without a registry, finite animations settle through the common unbound
/// policy; infinite repeats park until a registry is provided. Gesture
/// ownership is stricter and unrelated: gesture widgets require their
/// presentation's `GestureArenaScope`.
///
/// Replacing the registry notifies retained states, which rebind their owning
/// controllers in `did_change_dependencies` without restarting accepted motion.
#[derive(Clone)]
pub struct VsyncScope {
    /// The shared registry handed to descendants. Cloning the scope clones this
    /// `Rc`-backed handle, so all clones observe the same registry.
    vsync: Option<Vsync>,
    /// The wrapped subtree the registry is provided to.
    child: BoxedView,
}

impl VsyncScope {
    /// Read the subtree's registry while registering a lifecycle dependency.
    /// Rebind owned animations from init_state and did_change_dependencies.
    #[must_use]
    pub fn maybe_of(ctx: &dyn LifecycleContext) -> Option<Vsync> {
        ctx.depend_on::<Self, _>(|scope| scope.vsync.clone())
            .flatten()
    }

    /// Wrap `child` in a scope that provides `vsync` to its descendants.
    #[must_use]
    pub fn new(vsync: Vsync, child: impl IntoView) -> Self {
        Self {
            vsync: Some(vsync),
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// The shared registry this scope provides — what a descendant implicitly-
    /// animated widget reads in `init_state` to register its controller against.
    #[must_use]
    pub fn vsync(&self) -> Option<&Vsync> {
        self.vsync.as_ref()
    }

    /// Provide a subtree with no frame registry. Finite animations settle.
    #[must_use]
    pub fn detached(child: impl IntoView) -> Self {
        Self {
            vsync: None,
            child: BoxedView(Box::new(child.into_view())),
        }
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
    type Data = Option<Vsync>;

    fn data(&self) -> &Self::Data {
        &self.vsync
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        match (&self.vsync, &old.vsync) {
            (Some(new), Some(old)) => !new.is_same(old),
            (None, None) => false,
            _ => true,
        }
    }
}

impl_inherited_view!(VsyncScope);
