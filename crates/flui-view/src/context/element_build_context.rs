//! The build-time [`BuildContext`]/[`LifecycleContext`] implementation a
//! behavior constructs during [`BuildOwner::build_scope`](crate::BuildOwner):
//! a borrowed view of the live tree and the deferred dependent-record sink,
//! plus the shared [`DependentRecord`] and [`BuildCapabilities`] it carries.

use std::{
    any::{Any, TypeId},
    rc::Rc,
};

use flui_foundation::{ElementId, RenderId};
use flui_interaction::FocusManager;

use super::build_context::{BuildContext, LifecycleContext};
use crate::{
    element::Notification,
    reactive::{ElementReads, ReadScope, ScopeRef},
    tree::ElementTree,
};

#[derive(Debug)]
pub(crate) struct DependentRecord {
    /// The provider read from, or an ancestry-dependent lookup that missed.
    pub(crate) provider: Option<ElementId>,
    /// The element that read it (and must rebuild when it changes).
    pub(crate) dependent: ElementId,
    /// The dependent's tree depth (for dirty-heap ordering).
    pub(crate) depth: usize,
    /// The provider fields the dependent read (#1090).
    pub(crate) mask: crate::view::FieldSet,
    /// Recorded from `init_state` / `did_change_dependencies` rather than
    /// `build`: such reads are kept until unmount (they accumulate),
    /// not re-derived per build (ADR-0074 §5.5). Set by the
    /// stateful behavior after the hook returns.
    pub(crate) lifecycle: bool,
}

/// Build-time [`BuildContext`] backed by a live, borrowed read view of the
/// real [`ElementTree`].
///
/// Replaces the empty process-shared dummy that made `depend_on` /
/// `find_ancestor_*` / notification dispatch inert in production. The
/// building element is extracted from the slab by value for the duration
/// of its `build()`
/// (see [`ElementNode::element`](crate::tree::ElementNode)), so this can
/// hold a shared `&ElementTree` without aliasing: ancestor walks read every
/// live node, and the in-flight node reads back as a hole via
/// [`element_opt`](crate::tree::ElementNode::element_opt).
///
/// Inherited dependencies cannot be written here (the tree is read-only),
/// so they are buffered into `dep_sink` and applied by `build_scope` after
/// the element is restored. `mark_needs_build` during build is a
/// forbidden no-op.
/// Owner- and presentation-scoped capabilities a `BuildContext` is handed
/// rather than computing or selecting from ambient state.
///
/// They are bundled because every field is minted at the same place
/// (`make_build_ctx`) from the element core and its exact `BuildOwner`;
/// threading them separately would obscure that shared ownership origin.
#[derive(Clone)]
pub(crate) struct BuildCapabilities {
    /// The exact focus manager owned by the surrounding build owner.
    pub(crate) focus_manager: Rc<FocusManager>,
    pub(crate) lifecycle_handle: Option<crate::LifecycleHandle>,
    pub(crate) window_handle: Option<crate::WindowHandle>,
    /// The binding's async task driver.
    pub(crate) async_driver: Option<flui_scheduler::AsyncDriver>,
    /// The binding's post-frame capability.
    pub(crate) post_frame_handle: Option<flui_scheduler::PostFrameHandle>,
    /// The presentation capabilities `LifecycleContext` hands out.
    pub(crate) presentation: crate::owner::PresentationCapabilities,
    /// The host's flush registry; crate-private, for `Persisted`.
    pub(crate) flush_registry: Option<crate::flush_registry::FlushRegistry>,
    /// The UI runtime's fresh-hit-test capability, narrowed from its interaction
    /// dispatch handle.
    pub(crate) hit_test_handle: Option<flui_interaction::HitTestHandle>,
    /// The render tree this element is mounted in, cloned from its own
    /// `ElementCore` — see `make_build_ctx` for why not from the tree node.
    pub(crate) pipeline_owner: Option<flui_rendering::pipeline::PipelineCell>,
    /// The presentation's keep-alive table, so an item can take a hold on the
    /// lazy sliver child it lives inside from `init_state`.
    pub(crate) keep_alive: crate::owner::KeepAliveHolds,
    /// The presentation's unmount hooks, for `on_unmount` from `init_state`.
    pub(crate) unmount_hooks: crate::owner::UnmountHooks,
    /// The owner's reactive graph with the building element as its reader:
    /// the sink every signal read in this build subscribes through.
    pub(crate) reads: ElementReads,
}

pub(crate) struct LiveBuildContext<'b> {
    element_id: ElementId,
    depth: usize,
    tree: &'b ElementTree,
    dep_sink: &'b parking_lot::Mutex<Vec<DependentRecord>>,
    /// Owned rebuild capability for `element_id`, minted by `make_build_ctx`
    /// from the element's own core. Cloned out by
    /// [`LifecycleContext::rebuild_handle`]; the build itself never schedules —
    /// `build` sees this context as a `BuildContext`, which cannot hand it out.
    rebuild: crate::RebuildHandle,
    /// What the binding and the element's core handed this context.
    capabilities: BuildCapabilities,
}

impl<'b> LiveBuildContext<'b> {
    /// Construct a build-time context for `element_id` (at `depth`) over a
    /// borrowed view of `tree`, buffering inherited dependencies into
    /// `dep_sink`.
    pub(crate) fn new(
        element_id: ElementId,
        depth: usize,
        tree: &'b ElementTree,
        dep_sink: &'b parking_lot::Mutex<Vec<DependentRecord>>,
        rebuild: crate::RebuildHandle,
        capabilities: BuildCapabilities,
    ) -> Self {
        Self {
            element_id,
            depth,
            tree,
            dep_sink,
            rebuild,
            capabilities,
        }
    }

    /// Walk strict-ancestors (parent and up), invoking `predicate` on each
    /// live ancestor element. The in-flight node (extracted during build)
    /// is skipped via [`element_opt`](crate::tree::ElementNode::element_opt).
    fn walk_strict_ancestors<R>(
        &self,
        mut predicate: impl FnMut(&dyn crate::view::ElementBase) -> std::ops::ControlFlow<R>,
    ) -> Option<R> {
        let mut current = self.element_id;
        loop {
            let parent_id = self.tree.get(current)?.parent()?;
            if let Some(elem) = self.tree.get(parent_id)?.element_opt()
                && let std::ops::ControlFlow::Break(result) = predicate(elem)
            {
                return Some(result);
            }
            current = parent_id;
        }
    }

    /// Nearest in-scope `InheritedElement` whose view type is `type_id`, in
    /// **O(1)**.
    ///
    /// Reads the building element's own resolved inherited scope
    /// ([`ElementNode::inherited`](crate::tree::ElementNode)) — a node field
    /// that survives the `build_scope` element hole — rather than walking the
    /// ancestor chain. For a non-provider that scope is its parent's set, so
    /// the result is the nearest strict-ancestor provider.
    fn find_inherited_provider(&self, type_id: TypeId) -> Option<ElementId> {
        self.tree.enclosing_provider(self.element_id, type_id)
    }
}

impl super::build_context::sealed::Sealed for LiveBuildContext<'_> {}

/// Every read subscribes the element: this context exists only for the
/// element's build (and its `init_state`, whose reads the following build's
/// `begin_element_build` forgets before re-deriving the set).
impl ReadScope for LiveBuildContext<'_> {
    fn scope(&self) -> ScopeRef<'_> {
        ScopeRef::new(
            self.capabilities.reads.graph(),
            Some(&self.capabilities.reads),
        )
    }
}

impl BuildContext for LiveBuildContext<'_> {
    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn mounted(&self) -> bool {
        true
    }

    fn is_building(&self) -> bool {
        true
    }

    fn graph(&self, _: super::build_context::sealed::CrateToken) -> &crate::reactive::Reactive {
        self.capabilities.reads.graph()
    }

    fn depend_on_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        self.depend_on_inherited_fields(
            super::build_context::sealed::CrateToken::new(),
            type_id,
            crate::view::FieldSet::ALL,
            callback,
        )
    }

    fn depend_on_inherited_fields(
        &self,
        _token: super::build_context::sealed::CrateToken,
        type_id: TypeId,
        mask: crate::view::FieldSet,
        callback: &mut dyn FnMut(&dyn Any),
    ) -> bool {
        let provider_id = self.find_inherited_provider(type_id);
        let provided = provider_id.and_then(|provider_id| {
            self.tree
                .get(provider_id)
                .and_then(super::super::tree::ElementNode::element_opt)
                .and_then(crate::view::ElementBase::as_inherited)
                .and_then(|accessor| accessor.provided(type_id))
        });
        // An element that publishes several values keeps one field per type:
        // the dependent records that field, whatever the caller named.
        let mask = provided.and_then(|provided| provided.field).unwrap_or(mask);
        // Buffer the dependent BEFORE invoking the user callback. The tree is
        // read-only here, so the write itself is deferred to the `build_scope`
        // drain (see [`DependentRecord`]) — but it is *recorded* first:
        // registration precedes returning the provider's value. This matters on the error path: if the user
        // `build()` panics after this `depend_on` (caught by `build_or_recover`,
        // which substitutes an `ErrorView`), the element stays registered as a
        // dependent, so a later inherited change reschedules it and it recovers.
        // Recording only after the callback would drop the registration on that
        // panic and strand the element on the `ErrorView`.
        self.dep_sink.lock().push(DependentRecord {
            provider: provider_id,
            dependent: self.element_id,
            depth: self.depth,
            mask,
            lifecycle: false,
        });
        let Some(provided) = provided else {
            return false;
        };
        callback(provided.value);
        true
    }

    fn get_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        let Some(provider_id) = self.find_inherited_provider(type_id) else {
            return false;
        };
        let Some(provided) = self
            .tree
            .get(provider_id)
            .and_then(super::super::tree::ElementNode::element_opt)
            .and_then(crate::view::ElementBase::as_inherited)
            .and_then(|accessor| accessor.provided(type_id))
        else {
            return false;
        };
        callback(provided.value);
        true
    }

    fn find_ancestor_element(&self, type_id: TypeId) -> Option<ElementId> {
        let mut current = self.element_id;
        loop {
            let parent_id = self.tree.get(current)?.parent()?;
            if self
                .tree
                .get(parent_id)?
                .element_opt()
                .is_some_and(|e| e.view_type_id() == type_id)
            {
                return Some(parent_id);
            }
            current = parent_id;
        }
    }

    fn find_ancestor_view(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        self.walk_strict_ancestors::<()>(|elem| {
            if elem.view_type_id() == type_id
                && let Some(view_any) = elem.view_as_any()
            {
                callback(view_any);
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })
        .is_some()
    }

    fn find_ancestor_state(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        self.walk_strict_ancestors::<()>(|elem| {
            if let Some(state_any) = elem.state_as_any()
                && (*state_any).type_id() == type_id
            {
                callback(state_any);
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })
        .is_some()
    }

    fn find_root_ancestor_state(
        &self,
        type_id: TypeId,
        callback: &mut dyn FnMut(&dyn Any),
    ) -> bool {
        let mut root_most: Option<ElementId> = None;
        let mut current = self.element_id;
        while let Some(node) = self.tree.get(current) {
            let Some(parent_id) = node.parent() else {
                break;
            };
            if self
                .tree
                .get(parent_id)
                .and_then(super::super::tree::ElementNode::element_opt)
                .and_then(crate::view::ElementBase::state_as_any)
                .is_some_and(|s| (*s).type_id() == type_id)
            {
                root_most = Some(parent_id);
            }
            current = parent_id;
        }
        let Some(matched) = root_most else {
            return false;
        };
        let Some(state_any) = self
            .tree
            .get(matched)
            .and_then(super::super::tree::ElementNode::element_opt)
            .and_then(crate::view::ElementBase::state_as_any)
        else {
            return false;
        };
        callback(state_any);
        true
    }

    fn find_render_object(&self) -> Option<RenderId> {
        self.walk_strict_ancestors(|elem| match elem.render_id() {
            Some(id) => std::ops::ControlFlow::Break(id),
            None => std::ops::ControlFlow::Continue(()),
        })
    }

    fn visit_ancestor_elements(&self, visitor: &mut dyn FnMut(ElementId) -> bool) {
        let mut current = self.element_id;
        while let Some(node) = self.tree.get(current) {
            let Some(parent_id) = node.parent() else {
                break;
            };
            if !visitor(parent_id) {
                break;
            }
            current = parent_id;
        }
    }

    fn visit_child_elements(&self, visitor: &mut dyn FnMut(ElementId)) {
        // Forbidden during build: a `LiveBuildContext` is ALWAYS mid-build, and the
        // node's `child_ids` here are the PRE-reconcile list — for an update
        // build they may be removed or reordered moments later.
        debug_assert!(
            !self.is_building(),
            "visit_child_elements cannot be called during build (a LiveBuildContext is always \
             mid-build; its child_ids are the stale pre-reconcile list)"
        );
        if let Some(node) = self.tree.get(self.element_id) {
            for &child_id in node.child_ids() {
                visitor(child_id);
            }
        }
    }

    fn mark_needs_build(&self) {
        tracing::warn!(
            "LiveBuildContext::mark_needs_build called during build — no-op (an element cannot \
             be marked dirty during its own build)"
        );
    }

    fn dispatch_notification(&self, notification: &dyn Notification) {
        let notification_any: &dyn Any = notification;
        let type_id = notification_any.type_id();
        self.walk_strict_ancestors::<()>(|ancestor| {
            if ancestor.on_notification(type_id, notification_any) {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        });
    }
}

impl LifecycleContext for LiveBuildContext<'_> {
    fn rebuild_handle(&self) -> crate::RebuildHandle {
        self.rebuild.clone()
    }
    fn writer_source(&self) -> crate::WriterSource {
        crate::WriterSource::new(self.capabilities.reads.graph().clone())
    }
    fn build_phase(&self) -> flui_foundation::BuildPhase {
        self.capabilities.reads.graph().build_phase()
    }
    fn async_driver(&self) -> Option<flui_scheduler::AsyncDriver> {
        self.capabilities.async_driver.clone()
    }
    fn post_frame_handle(&self) -> Option<flui_scheduler::PostFrameHandle> {
        self.capabilities.post_frame_handle.clone()
    }
    fn text_input_handle(&self) -> Option<flui_interaction::TextInputHandle> {
        self.capabilities.presentation.text_input.clone()
    }
    fn capability_erased(
        &self,
        capability: std::any::TypeId,
        name: &'static str,
    ) -> Result<Box<dyn std::any::Any>, flui_platform::Unsupported> {
        let source = self.capabilities.presentation.capabilities.clone();
        acquire_capability(source.as_deref(), capability, name)
    }
    fn announcer(&self) -> Option<flui_rendering::semantics::Announcer> {
        self.capabilities.presentation.announcer.clone()
    }
    fn assistive_focus_support(&self) -> flui_rendering::semantics::AssistiveFocusSupport {
        self.capabilities.presentation.assistive_focus
    }
    fn haptics(&self) -> Option<flui_interaction::HapticsHandle> {
        let handle = self.capabilities.presentation.haptics.clone()?;
        Some(crate::haptics_mode::scoped(self, handle))
    }
    fn drag_and_drop(&self) -> Option<flui_interaction::drag::DragAndDrop> {
        self.capabilities.presentation.drag_and_drop.clone()
    }
    fn texture_registry(&self) -> Option<flui_rendering::layer::TextureRegistry> {
        self.capabilities.presentation.textures.clone()
    }
    fn close_guard(&self) -> Option<crate::CloseGuard> {
        self.capabilities.presentation.close_guard.clone()
    }
    fn flush_registry_in_crate(
        &self,
        _token: super::build_context::sealed::CrateToken,
    ) -> Option<crate::flush_registry::FlushRegistry> {
        self.capabilities.flush_registry.clone()
    }
    fn hit_test_handle(&self) -> Option<flui_interaction::HitTestHandle> {
        self.capabilities.hit_test_handle.clone()
    }
    fn keep_alive_lease(&self) -> crate::owner::KeepAliveLease {
        // `init_state` runs with a `LiveBuildContext` — the same context type `build`
        // gets — so this must serve a real lease rather than refuse one. The
        // "never from a frame phase" half is a TYPE rule: `build` sees this
        // context only as `&dyn BuildContext`, which has no `keep_alive_lease`,
        // exactly as for `text_input_handle` and `focus_manager`, which are
        // acquired from `init_state` through this same context.
        self.keep_alive_handle().hold()
    }
    fn on_unmount(&self, hook: Box<dyn FnOnce()>) -> crate::UnmountHook {
        self.capabilities
            .unmount_hooks
            .register(self.element_id, hook)
    }
    fn keep_alive_handle(&self) -> crate::owner::KeepAliveHandle {
        self.capabilities.keep_alive.handle(self.element_id)
    }
    fn lifecycle_handle(&self) -> Option<crate::LifecycleHandle> {
        self.capabilities.lifecycle_handle.clone()
    }
    fn window(&self) -> Option<crate::WindowHandle> {
        self.capabilities.window_handle.clone()
    }
    fn focus_manager(&self) -> Rc<FocusManager> {
        Rc::clone(&self.capabilities.focus_manager)
    }
    /// Cloned at construction from the element's own `ElementCore`: during
    /// `build_scope` the element is *extracted* from its tree node, so a
    /// `BuildContext` cannot look itself up (`ElementNode::element` panics in that
    /// window). See `make_build_ctx`.
    fn pipeline_owner(&self) -> Option<flui_rendering::pipeline::PipelineCell> {
        self.capabilities.pipeline_owner.clone()
    }
}

/// Resolve a typed capability from the presentation's source, or refuse it by
/// name when the runtime registered none.
pub(crate) fn acquire_capability(
    source: Option<&dyn crate::CapabilitySource>,
    capability: std::any::TypeId,
    name: &'static str,
) -> Result<Box<dyn std::any::Any>, flui_platform::Unsupported> {
    match source {
        Some(source) => source.acquire(capability, name),
        None => Err(flui_platform::Unsupported::named(
            name,
            flui_platform::UnsupportedReason::NotRegistered,
        )),
    }
}
