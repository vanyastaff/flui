//! The presentation's focus-tree root: the inherited node each mounting focus
//! widget hangs under, and the view that publishes it (T17; moved out of
//! `flui-widgets` so `flui-runtime` names no widget catalog).

use std::rc::Rc;

use flui_interaction::focus::{FocusManager, FocusNode};

use crate::impl_inherited_view;
use crate::prelude::*;

/// The nearest enclosing focus node, as an inherited dependency.
///
/// The parent being a *node*, not always a scope, is what makes the
/// leaf→root key-dispatch walk (ADR-0023) match the widget tree: a
/// `Shortcuts`-style non-scope `Focus` above a field is a node **ancestor**
/// of the field, so keys the field ignores bubble through it.
#[derive(Clone)]
pub struct FocusParentProvider {
    parent: Rc<FocusNode>,
    revision: u64,
    child: BoxedView,
}

impl FocusParentProvider {
    /// Publish `parent` and its notification `revision` around `child`.
    #[must_use]
    pub fn new(parent: Rc<FocusNode>, revision: u64, child: BoxedView) -> Self {
        Self {
            parent,
            revision,
            child,
        }
    }
}

impl std::fmt::Debug for FocusParentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusParentProvider")
            .field("parent_id", &self.parent.id())
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl InheritedView for FocusParentProvider {
    type Data = Rc<FocusNode>;

    fn data(&self) -> &Self::Data {
        &self.parent
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.parent, &old.parent) || self.revision != old.revision
    }
}

impl_inherited_view!(FocusParentProvider);

/// The node a mounting/reparenting focus node hangs under.
///
/// Every presentation is rooted in [`FocusRoot`], so a missing provider is a
/// broken embedder invariant rather than a reason to reach for the
/// lifecycle-only `LifecycleContext::focus_manager` capability from arbitrary
/// build paths.
pub fn enclosing_focus_parent(ctx: &dyn BuildContext) -> Rc<FocusNode> {
    ctx.depend_on::<FocusParentProvider, _>(|provider| Rc::clone(&provider.parent))
        .expect(
            "BUG: focus widget mounted outside FocusRoot; every presentation root must install \
             the focus-tree provider",
        )
}

/// Raw lookup behind the catalog's `Focus::of`/`FocusScope::of`: the nearest
/// enclosing [`FocusParentProvider`]'s node, registering a dependency, with
/// **no** scope-node filtering.
pub fn nearest_focus_node(ctx: &dyn BuildContext) -> Option<Rc<FocusNode>> {
    ctx.depend_on::<FocusParentProvider, _>(|provider| Rc::clone(&provider.parent))
}

/// Establishes the focus tree for one presentation.
///
/// Embedders install exactly one `FocusRoot` around each element-tree root. It
/// publishes that build owner's root scope as the first
/// [`FocusParentProvider`]. Standard keyboard bindings are catalog policy:
/// `flui-app` and `HeadlessHost` add them with the catalog's own root wrapper.
#[derive(Clone, Debug, StatefulView)]
pub struct FocusRoot {
    child: BoxedView,
}

impl FocusRoot {
    /// Create the focus root for a presentation subtree.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }
}

/// Presentation-local state behind [`FocusRoot`].
pub struct FocusRootState {
    manager: Option<Rc<FocusManager>>,
}

impl std::fmt::Debug for FocusRootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusRootState")
            .field("initialized", &self.manager.is_some())
            .finish()
    }
}

impl StatefulView for FocusRoot {
    type State = FocusRootState;

    fn create_state(&self) -> Self::State {
        FocusRootState { manager: None }
    }
}

impl ViewState<FocusRoot> for FocusRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.manager = Some(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.manager = Some(ctx.focus_manager());
    }

    fn build(&self, view: &FocusRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        let manager = self
            .manager
            .as_ref()
            .expect("BUG: FocusRoot built before init_state");
        FocusParentProvider::new(
            Rc::clone(manager.root_scope().as_focus_node()),
            0,
            view.child.clone(),
        )
    }
}
