//! The element behavior of a [`StatefulRenderView`]: a state's lifecycle and
//! one render object in the same element (ADR-0322 §4).

use flui_foundation::RenderId;

use super::{
    arity::ElementArity,
    behavior::{
        ElementBehavior, armed_state_hook, contained_state_hook, first_state, make_build_ctx,
        publish_provided,
    },
    behavior_commons::{build_or_recover_begun, should_build_with_trace, single_child_views},
    generic::ElementCore,
    provided::Publication,
    render_custody,
};
use crate::{
    owner::LifecycleHook,
    view::{BoxedView, IntoView, RenderViewState, StatefulRenderView},
};

/// Behavior of a [`StatefulRenderView`] element.
///
/// The state follows the rules of `StatefulBehavior`: `create_state` and
/// `init_state` run at the first build, from the view the element holds
/// then, and `dispose` runs only for a state whose `init_state` returned.
/// The render object follows the rules of `RenderBehavior`: it is created
/// and adopted at mount and removed at unmount.
pub struct StatefulRenderBehavior<V: StatefulRenderView> {
    /// `None` until `create_state` and `init_state` have both returned.
    state: Option<V::State>,
    render_id: Option<RenderId>,
    /// The values the state publishes to its subtree.
    publication: Publication,
}

impl<V: StatefulRenderView> std::fmt::Debug for StatefulRenderBehavior<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatefulRenderBehavior")
            .field("initialized", &self.state.is_some())
            .field("render_id", &self.render_id)
            .finish_non_exhaustive()
    }
}

impl<V: StatefulRenderView> StatefulRenderBehavior<V> {
    /// A behavior whose state the element's first build creates.
    pub fn new() -> Self {
        Self {
            state: None,
            render_id: None,
            publication: Publication::default(),
        }
    }
}

impl<V: StatefulRenderView> Default for StatefulRenderBehavior<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, A> ElementBehavior<V, A> for StatefulRenderBehavior<V>
where
    V: StatefulRenderView,
    A: ElementArity,
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<V::Protocol>>>,
{
    fn debug_kind(&self) -> &'static str {
        "StatefulRenderElement"
    }

    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        self.state.as_ref().map(|state| state as &dyn std::any::Any)
    }

    fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }

    fn as_inherited_access(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        self.publication.access()
    }

    fn as_inherited_access_mut(
        &mut self,
    ) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        self.publication.access_mut()
    }

    fn take_provided_scope_stale(&mut self) -> bool {
        self.publication.take_scope_stale()
    }

    fn on_mount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        render_custody::mount(&mut self.render_id, core, owner, |cx| {
            core.view().create_render_object(cx)
        });
    }

    fn build_into_views(
        &mut self,
        core: &mut ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> crate::view::ChildViews {
        // The context borrows the tree and the sink, not `owner` or `self`.
        let ctx_choice = make_build_ctx(core, owner);
        let ctx = ctx_choice.as_ctx();

        // Before the `should_build` guard, so a mounted element creates and
        // initializes its state exactly once even when it is clean.
        if self.state.is_none() {
            let lifecycle_ctx = ctx_choice.as_lifecycle_ctx();
            let view = core.view();
            let state = first_state(core, owner, "initializing StatefulRenderElement", || {
                let mut state = view.create_state(lifecycle_ctx);
                state.init_state(lifecycle_ctx);
                state
            });
            self.state = Some(state);
        }

        if !should_build_with_trace(core, "StatefulRenderBehavior") {
            return crate::view::ChildViews::none();
        }

        let state = self.state.as_ref().expect(
            "BUG: an initialized StatefulRenderElement holds the state its first build created",
        );
        render_custody::push(
            self.render_id,
            core,
            owner,
            |cx, render_object: &mut V::RenderObject| {
                state.update_render_object(core.view(), cx, render_object)
            },
        );

        // One clone of the view serves `provide` and `child`, taken before
        // the reactive phase is armed: between the two hooks nothing the
        // author wrote runs, so the phase `publish_provided` arms is always
        // handed to `build_or_recover_begun`. The child mounts under what is
        // published here.
        let view = std::rc::Rc::clone(core.shared_view());
        publish_provided(&mut self.publication, core, owner, |values| {
            state.provide(&view, ctx, values);
        });
        let child_view = build_or_recover_begun(core, owner, "StatefulRenderElement", move || {
            let opaque = state.child(&view, ctx);
            BoxedView::new(IntoView::into_view(opaque))
        });
        single_child_views(core, child_view, "StatefulRenderBehavior")
    }

    /// Releases the published values, disposes an initialized state, then
    /// removes the render object. A panicking `dispose` is contained, so the
    /// render object is removed either way.
    fn on_unmount(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        self.publication.release(core.self_id(), owner);
        if let Some(state) = self.state.as_mut() {
            contained_state_hook(
                core,
                owner,
                LifecycleHook::Dispose,
                "disposing StatefulRenderElement",
                || state.dispose(),
            );
        }
        render_custody::unmount(
            &mut self.render_id,
            core,
            owner,
            "StatefulRenderBehavior",
            |cx, render_object: &mut V::RenderObject| {
                core.view().did_unmount_render_object(cx, render_object);
            },
        );
    }

    fn on_activate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        armed_state_hook(
            core,
            owner,
            LifecycleHook::Activate,
            "activating StatefulRenderElement",
            || state.activate(),
        );
    }

    fn on_deactivate(&mut self, core: &mut ElementCore<V, A>, owner: &mut crate::ElementOwner<'_>) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        contained_state_hook(
            core,
            owner,
            LifecycleHook::Deactivate,
            "deactivating StatefulRenderElement",
            || state.deactivate(),
        );
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<V, A>,
        old_view: &V,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        // A parent can update this element between its mount and its first
        // build. The first build creates the state from the newest view, so
        // an element with no state yet has no older view to tell it about.
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.did_update_view(old_view, core.view());
    }

    fn did_change_dependencies(
        &mut self,
        core: &ElementCore<V, A>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let ctx_choice = make_build_ctx(core, owner);
        // A dependency registers during a build, which created the state
        // first; an element with no state has nothing to notify.
        let Some(state) = self.state.as_mut() else {
            return;
        };
        armed_state_hook(
            core,
            owner,
            LifecycleHook::DidChangeDependencies,
            "notifying StatefulRenderElement dependency change",
            || state.did_change_dependencies(ctx_choice.as_lifecycle_ctx()),
        );
    }
}
