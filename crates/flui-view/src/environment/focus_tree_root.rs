//! [`FocusTreeRoot`]: the presentation's root focus scope as inherited data.

use std::rc::Rc;

use flui_interaction::focus::{FocusManager, FocusNode};

use crate::impl_inherited_view;
use crate::prelude::*;

/// Publishes one presentation's root focus scope to its subtree.
///
/// The UI runtime installs exactly one `FocusTreeRoot` above each root it
/// attaches, so every focus widget below has a node to hang under. The root is
/// that presentation's own [`FocusManager::root_scope`], read through the
/// lifecycle-only [`LifecycleContext::focus_manager`] capability: descendants
/// read the node with [`maybe_of`](Self::maybe_of) and never reach the
/// manager from a build path.
///
/// Only the focus tree lives here. Keyboard traversal (Tab, Shift+Tab, the
/// clipboard chords) is widget-catalog policy, which `flui_widgets::app_root`
/// installs below this root.
///
/// An embedder that mounts a tree on a bare `WidgetsBinding`, without a UI
/// runtime, wraps its root in one `FocusTreeRoot` itself.
#[derive(Clone, Debug, StatefulView)]
pub struct FocusTreeRoot {
    child: BoxedView,
}

impl FocusTreeRoot {
    /// Root the focus tree of the presentation that mounts `child`.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }

    /// The root focus scope's node of the presentation `ctx` is built in,
    /// registering a dependency; `None` outside a `FocusTreeRoot`.
    ///
    /// The focus catalog prefers the nearest enclosing focus widget's node and
    /// falls back to this one, so a widget with no focus ancestor hangs
    /// directly under the root scope.
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Rc<FocusNode>> {
        ctx.depend_on::<RootScopeProvider, _>(|provider| Rc::clone(&provider.root))
    }
}

/// Presentation-local state behind [`FocusTreeRoot`].
pub struct FocusTreeRootState {
    manager: Option<Rc<FocusManager>>,
}

impl std::fmt::Debug for FocusTreeRootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusTreeRootState")
            .field("initialized", &self.manager.is_some())
            .finish()
    }
}

impl StatefulView for FocusTreeRoot {
    type State = FocusTreeRootState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        FocusTreeRootState { manager: None }
    }
}

impl ViewState<FocusTreeRoot> for FocusTreeRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.manager = Some(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.manager = Some(ctx.focus_manager());
    }

    fn build(&self, view: &FocusTreeRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        let manager = self
            .manager
            .as_ref()
            .expect("BUG: FocusTreeRoot built before init_state");
        RootScopeProvider {
            root: Rc::clone(manager.root_scope().as_focus_node()),
            child: view.child.clone(),
        }
    }
}

/// The inherited view behind [`FocusTreeRoot::maybe_of`]. Private, so the
/// only way to publish a presentation's root scope is to mount a
/// `FocusTreeRoot`, which reads it from the presentation's own manager.
#[derive(Clone)]
struct RootScopeProvider {
    root: Rc<FocusNode>,
    child: BoxedView,
}

impl std::fmt::Debug for RootScopeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootScopeProvider")
            .field("root_id", &self.root.id())
            .finish_non_exhaustive()
    }
}

impl InheritedView for RootScopeProvider {
    type Data = Rc<FocusNode>;

    fn data(&self) -> &Self::Data {
        &self.root
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.root, &old.root)
    }
}

impl_inherited_view!(RootScopeProvider);
