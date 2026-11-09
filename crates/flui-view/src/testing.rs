//! Test-support build context: an owned, detached BuildContext a test can
//! construct without running `build_scope` (ADR-0243). The production path
//! uses the borrowed BuildCtx; this owned form exists only for tests.

use std::{
    any::{Any, TypeId},
    rc::Rc,
};

use flui_foundation::{ElementId, RenderId};
use flui_interaction::FocusManager;
use std::cell::RefCell;

use crate::context::build_context::{BuildContext, LifecycleContext};
use crate::{
    context::acquire_capability,
    element::Notification,
    owner::BuildOwner,
    reactive::{ElementReads, ReadScope, ReaderSink, ScopeRef},
    tree::ElementTree,
};

/// Concrete BuildContext implementation for Elements.
///
/// `ElementBuildContext` presents one element's owner-local
/// [`BuildContext`] surface, giving Views access to:
/// - Element identity and tree position
/// - InheritedView lookups (O(1) via each node's `inherited` map)
/// - Ancestor traversal
/// - Rebuild scheduling
///
/// # Thread Safety
///
/// The context is owner-local: `Rc<RefCell<…>>` tree and owner, so it is
/// `!Send + !Sync`.
///
/// `Element` does not implement `BuildContext` directly: a separate struct
/// avoids borrow checker issues with self-referential borrows during build.
pub struct ElementBuildContext {
    /// The ElementId this context represents.
    element_id: ElementId,

    /// Depth in the element tree.
    depth: usize,

    /// Whether the element is currently mounted.
    mounted: bool,

    /// Reference to the element tree.
    tree: Rc<RefCell<ElementTree>>,

    /// Reference to the build owner.
    owner: Rc<RefCell<BuildOwner>>,

    /// The owner's reactive graph with this element as its reader, captured
    /// at construction so a read's [`ScopeRef`] can borrow it.
    reads: ElementReads,

    /// Whether we're currently in a build phase (debug only).
    #[cfg(debug_assertions)]
    is_building: bool,
}

impl std::fmt::Debug for ElementBuildContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementBuildContext")
            .field("element_id", &self.element_id)
            .field("depth", &self.depth)
            .field("mounted", &self.mounted)
            .finish_non_exhaustive()
    }
}

impl ElementBuildContext {
    /// Create a new ElementBuildContext.
    ///
    /// # Arguments
    ///
    /// * `element_id` - The ElementId this context represents
    /// * `depth` - Depth in the element tree
    /// * `mounted` - Whether the element is currently mounted
    /// * `tree` - Shared reference to the element tree
    /// * `owner` - Shared reference to the build owner
    ///
    /// Takes a read lock on `owner` to capture its reactive graph, so it must
    /// not be called while the caller holds `owner`'s write lock.
    pub fn new(
        element_id: ElementId,
        depth: usize,
        mounted: bool,
        tree: Rc<RefCell<ElementTree>>,
        owner: Rc<RefCell<BuildOwner>>,
    ) -> Self {
        let reads = ElementReads::new(owner.borrow().reactive().clone(), element_id);
        Self {
            element_id,
            depth,
            mounted,
            tree,
            owner,
            reads,
            #[cfg(debug_assertions)]
            is_building: false,
        }
    }

    /// Create a context for a specific element from the tree.
    ///
    /// Returns None if the element doesn't exist in the tree.
    ///
    /// Takes a read lock on `owner` (as [`new`](Self::new) does).
    #[expect(clippy::needless_pass_by_value)] // Arc is cloned into Self, taking by value is idiomatic
    pub fn for_element(
        element_id: ElementId,
        tree: Rc<RefCell<ElementTree>>,
        owner: Rc<RefCell<BuildOwner>>,
    ) -> Option<Self> {
        let tree_guard = tree.borrow();
        let node = tree_guard.get(element_id)?;
        let reads = ElementReads::new(owner.borrow().reactive().clone(), element_id);

        Some(Self {
            element_id,
            depth: node.depth(),
            mounted: node.element().mounted(),
            tree: tree.clone(),
            owner,
            reads,
            #[cfg(debug_assertions)]
            is_building: false,
        })
    }

    /// Set the building flag (debug only).
    #[cfg(debug_assertions)]
    pub fn set_building(&mut self, building: bool) {
        self.is_building = building;
    }

    /// Nearest in-scope `InheritedElement` of view type `type_id`, in **O(1)**.
    ///
    /// Shared helper for `depend_on_inherited` and `get_inherited`;
    /// only the dependent-recording side differs.
    ///
    /// Reads the resolved inherited scope
    /// ([`ElementNode::inherited`](crate::tree::ElementNode)) instead of
    /// walking the ancestor chain, so the lookup is O(1). FLUI builds that map
    /// at mount as an `Arc<HashMap>` shared by refcount down non-provider runs.
    fn find_inherited_provider(&self, type_id: std::any::TypeId) -> Option<ElementId> {
        self.tree
            .borrow()
            .get(self.element_id)?
            .inherited_provider(type_id)
    }

    /// Walk strict-ancestors (parent and up) of `self.element_id`,
    /// invoking `predicate(&dyn ElementBase) -> ControlFlow<R>` for each
    /// visited element. Returns `Some(R)` if the predicate breaks with
    /// `ControlFlow::Break`, or `None` if every ancestor is exhausted
    /// without a break.
    ///
    /// Shared helper for the ancestor-finder trio
    /// ([`find_ancestor_view`](BuildContext::find_ancestor_view),
    /// [`find_ancestor_state`](BuildContext::find_ancestor_state),
    /// [`find_root_ancestor_state`](BuildContext::find_root_ancestor_state)).
    /// The first two stop on the first match; the last continues to root
    /// and the caller tracks the root-most match in `R` by accumulating
    /// across `Continue` returns.
    ///
    /// The tree read-lock is held for the duration of the walk so the
    /// `&dyn ElementBase` reference handed to `predicate` cannot escape
    /// the closure ΓÇö preserves the declarative-build invariant
    /// (Constitution Principle 5).
    fn walk_strict_ancestors<R>(
        &self,
        mut predicate: impl FnMut(&dyn crate::view::ElementBase) -> std::ops::ControlFlow<R>,
    ) -> Option<R> {
        let tree = self.tree.borrow();

        let mut current_id = self.element_id;
        loop {
            let node = tree.get(current_id)?;
            let parent_id = node.parent()?;
            let parent_node = tree.get(parent_id)?;
            match predicate(parent_node.element()) {
                std::ops::ControlFlow::Break(result) => return Some(result),
                std::ops::ControlFlow::Continue(()) => {}
            }
            current_id = parent_id;
        }
    }
}

impl crate::context::build_context::sealed::Sealed for ElementBuildContext {}

/// Reads resolve against the owner's graph; they subscribe this element only
/// while it is marked as building (outside a build a read is a plain read).
impl ReadScope for ElementBuildContext {
    fn scope(&self) -> ScopeRef<'_> {
        let sink = BuildContext::is_building(self).then_some(&self.reads as &dyn ReaderSink);
        ScopeRef::new(self.reads.graph(), sink)
    }
}

impl BuildContext for ElementBuildContext {
    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn mounted(&self) -> bool {
        self.mounted
    }

    fn is_building(&self) -> bool {
        #[cfg(debug_assertions)]
        {
            self.is_building
        }
        #[cfg(not(debug_assertions))]
        {
            false
        }
    }

    fn graph(
        &self,
        _: crate::context::build_context::sealed::CrateToken,
    ) -> &crate::reactive::Reactive {
        self.reads.graph()
    }

    fn depend_on_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        self.depend_on_inherited_fields(
            crate::context::build_context::sealed::CrateToken::new(),
            type_id,
            crate::view::FieldSet::ALL,
            callback,
        )
    }

    fn depend_on_inherited_fields(
        &self,
        _token: crate::context::build_context::sealed::CrateToken,
        type_id: TypeId,
        mask: crate::view::FieldSet,
        callback: &mut dyn FnMut(&dyn Any),
    ) -> bool {
        // Walk ancestors looking for an Element whose view_type_id
        // matches; the first one is the nearest InheritedView<T>.
        //
        // Records this element (with the fields it read, #1090) in the
        // matched InheritedElement's dependent map so a later provider update
        // whose `changed_fields` intersects that mask schedules us for rebuild.
        let Some(ancestor_id) = self.find_inherited_provider(type_id) else {
            self.owner
                .borrow_mut()
                .register_inherited_miss(self.element_id);
            return false;
        };

        // Lock owner before tree, matching the frame driver's ownership
        // order. Both registrations happen before the callback: the
        // provider's forward notification map and the BuildOwner's sparse
        // reverse ownership index cannot diverge if user code panics.
        let mut owner = self.owner.borrow_mut();
        let mut tree = self.tree.borrow_mut();

        let Some(node) = tree.get_mut(ancestor_id) else {
            // Tree shape changed between lookup and write-lock; treat
            // as miss. Should not happen under normal flow.
            return false;
        };

        // Capture self's depth before we hand `node` out so we can
        // record it as the dependent's depth (used by
        // BuildOwner::schedule_build_for during dependency-change notify).
        let self_depth = self.depth;
        let self_id = self.element_id;

        // The element behind `node.element_mut()` is a
        // `Box<dyn ElementBase>`; downcast to the parametric
        // `InheritedElement<V>` using its TypeId. Because we only have
        // the user-facing TypeId (which is `TypeId::of::<V>()`, the
        // view's TypeId), we can't directly downcast to
        // `InheritedElement<V>` without `V` in scope. Instead we use
        // the `InheritedElementAccess` object-safe protocol surface
        // declared on `ElementBase` via the optional helper trait.
        let Some(accessor) = node.element_mut().as_inherited_mut() else {
            // Matched view-type but not actually an InheritedElement;
            // means a non-inherited view shares the TypeId, which is
            // impossible under TypeId semantics ΓÇö defensive return.
            return false;
        };

        // Register dependency (id + depth).
        // Outside a build drain there is no build whose reads could
        // re-derive this one, so it is kept like a lifecycle read.
        accessor.record_lifecycle_dependent(
            crate::context::CrateToken::new(),
            self_id,
            self_depth,
            mask,
        );
        owner.register_inherited_dependency(self_id, ancestor_id);
        drop(owner);

        // Hand the view out to the callback. The view reference is
        // borrowed for the lifetime of the callback only; it cannot
        // escape into `build()` because the closure is `FnOnce`.
        let view_any = accessor.view_as_any();
        callback(view_any);

        true
    }

    fn get_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Same ancestor walk as depend_on_inherited, but does NOT
        // record a dependency ΓÇö we share the walk + downcast logic and
        // skip the `record_dependent` call.
        let Some(ancestor_id) = self.find_inherited_provider(type_id) else {
            return false;
        };

        let tree = self.tree.borrow();
        let Some(node) = tree.get(ancestor_id) else {
            return false;
        };
        let Some(accessor) = node.element().as_inherited() else {
            return false;
        };
        let view_any = accessor.view_as_any();
        callback(view_any);
        true
    }

    fn find_ancestor_element(&self, type_id: TypeId) -> Option<ElementId> {
        let tree = self.tree.borrow();

        // Walk up from current element
        let mut current_id = self.element_id;
        loop {
            let node = tree.get(current_id)?;
            let parent_id = node.parent()?;

            let parent_node = tree.get(parent_id)?;
            if parent_node.element().view_type_id() == type_id {
                return Some(parent_id);
            }

            current_id = parent_id;
        }
    }

    fn find_ancestor_view(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Walk strict-ancestors, break on the first whose View's
        // TypeId matches. The callback runs inside `walk_strict_ancestors`
        // while the tree read-lock is held ΓÇö preserves declarative-build
        // invariant.
        //
        // Returns the view of the first ancestor whose view type equals the
        // requested one. No dependency recording (this is a read-only walk).
        let invoked = self.walk_strict_ancestors::<()>(|element| {
            if element.view_type_id() == type_id
                && let Some(view_any) = element.view_as_any()
            {
                callback(view_any);
                return std::ops::ControlFlow::Break(());
            }
            std::ops::ControlFlow::Continue(())
        });
        invoked.is_some()
    }

    fn find_ancestor_state(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Walk strict-ancestors, break on the first whose State's
        // `TypeId` (via `Any::type_id`) matches. Stateless elements
        // return `None` from `state_as_any`, so they're skipped.
        //
        // Matches against the State runtime type, not the StatefulView's:
        // `type_id` is `TypeId::of::<S>()` where S is the State type.
        let invoked = self.walk_strict_ancestors::<()>(|element| {
            if let Some(state_any) = element.state_as_any()
                && (*state_any).type_id() == type_id
            {
                callback(state_any);
                return std::ops::ControlFlow::Break(());
            }
            std::ops::ControlFlow::Continue(())
        });
        invoked.is_some()
    }

    fn find_root_ancestor_state(
        &self,
        type_id: TypeId,
        callback: &mut dyn FnMut(&dyn Any),
    ) -> bool {
        // Walk strict-ancestors all the way to root, recording the
        // root-most (i.e. the last visited matching) ancestor's
        // `ElementId`. After the walk, run the callback against that
        // ancestor's state inside a fresh read-lock.
        //
        // Two-phase shape (resolve id, then re-borrow for callback) is
        // used because the in-loop predicate would need to both
        // mutably borrow the accumulator (to remember the last match)
        // AND mutably borrow `callback: &mut dyn FnMut` (to invoke it
        // on that last match). The two-phase approach keeps each
        // borrow disjoint and gives us O(depth) walks with no clones
        // ΓÇö the tree is single-threaded during build, so the re-lock
        // is essentially free.
        //
        // The shared `walk_strict_ancestors` helper isn't reused here
        // because it surfaces `&dyn ElementBase` but not the matching
        // ancestor's id, and root-most matching needs the id to fetch
        // state via the second borrow. Keeping the helper minimal
        // (no id-yielding variant) is a YAGNI call; if a future need
        // arises for id-yielding walks we can widen the surface then.
        //
        let mut root_most: Option<ElementId> = None;

        // Phase 1: walk all strict-ancestors, record the root-most match.
        // Use `walk_strict_ancestors` with a closure that captures
        // `root_most` mutably and always returns `Continue` so the walk
        // exhausts the entire ancestor chain. We need the matching
        // ancestor's `ElementId`, but the walker only exposes
        // `&dyn ElementBase` ΓÇö we work around that by parameterising R
        // on `Option<ElementId>` and threading the candidate id through
        // the closure via a small index counter. Simpler still: do the
        // walk inline but in a `while let` shape that satisfies
        // clippy::while_let_loop while keeping the same semantics.
        {
            let tree = self.tree.borrow();
            let mut next_id: Option<ElementId> = Some(self.element_id);
            while let Some(current_id) = next_id {
                let Some(node) = tree.get(current_id) else {
                    break;
                };
                let Some(parent_id) = node.parent() else {
                    break;
                };
                let Some(parent_node) = tree.get(parent_id) else {
                    break;
                };
                if let Some(state_any) = parent_node.element().state_as_any()
                    && (*state_any).type_id() == type_id
                {
                    root_most = Some(parent_id);
                }
                next_id = Some(parent_id);
            }
        }

        // Phase 2: invoke the callback against the root-most match.
        let Some(matched_id) = root_most else {
            return false;
        };

        let tree = self.tree.borrow();
        let Some(matched_node) = tree.get(matched_id) else {
            return false;
        };
        let Some(state_any) = matched_node.element().state_as_any() else {
            return false;
        };
        callback(state_any);
        true
    }

    fn find_render_object(&self) -> Option<RenderId> {
        // Walk strict-ancestors, break on the first whose
        // `ElementBase::render_id` returns `Some`. Only
        // `RenderBehavior<V>` overrides the trait default ΓÇö every other
        // behavior (Stateless / Proxy / Inherited / Stateful / Animation)
        // keeps `None`, so non-render ancestors are skipped cleanly.
        //
        // Non-callback signature is sound: `RenderId` is `Copy`, so we
        // can hand it out directly without extending a `&self` borrow
        // into the rest of `build()` (plan ┬ºD2).
        //
        // The walk reads `RenderBehavior::render_id` at the dispatch boundary
        // of the first matching render element.
        self.walk_strict_ancestors(|ancestor| {
            if let Some(id) = ancestor.render_id() {
                std::ops::ControlFlow::Break(id)
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })
    }

    fn visit_ancestor_elements(&self, visitor: &mut dyn FnMut(ElementId) -> bool) {
        let tree = self.tree.borrow();

        let mut current_id = self.element_id;
        while let Some(node) = tree.get(current_id) {
            let Some(parent_id) = node.parent() else {
                break;
            };

            if !visitor(parent_id) {
                break;
            }

            current_id = parent_id;
        }
    }

    fn visit_child_elements(&self, visitor: &mut dyn FnMut(ElementId)) {
        #[cfg(debug_assertions)]
        {
            assert!(
                !self.is_building,
                "visit_child_elements cannot be called during build"
            );
        }

        // E3 (atomic boxΓåÆarena swap): a node's children are its
        // slab-resident `child_ids` list ΓÇö the single element graph.
        let tree = self.tree.borrow();
        if let Some(node) = tree.get(self.element_id) {
            for child_id in node.child_ids() {
                visitor(*child_id);
            }
        }
    }

    fn mark_needs_build(&self) {
        let mut owner = self.owner.borrow_mut();
        owner.schedule_build_for(
            self.element_id,
            self.depth,
            crate::RebuildReason::StateChange,
        );
    }

    fn dispatch_notification(&self, notification: &dyn Notification) {
        // Walk strict-ancestors (self-exclusive, parent-first) and invoke
        // each ancestor's object-safe
        // [`ElementBase::on_notification`](crate::view::ElementBase::on_notification)
        // handler. The bubble stops on the first ancestor that returns
        // `true` (handled); a `false` return continues the walk; reaching
        // the root with no `true` exhausts the walk silently.
        //
        // The notification is coerced from `&dyn Notification` to
        // `&dyn Any` via the `Any` supertrait (Rust 1.86+ trait upcasting,
        // stable in this workspace). `TypeId::of::<N>()` for the static
        // type is recovered from the notification value itself via
        // `Any::type_id` ΓÇö sound because `Notification: Any` guarantees
        // the concrete-type vtable carries it.
        //
        // The dispatch walks the ancestors, invoking each notification
        // listener's handler with the typed notification and stopping when
        // one returns `true`.
        //
        // Single-`dyn`-boundary discipline per
        // Constitution Principle 4: the walk uses `&dyn ElementBase` (the
        // existing tree shape) but the handler call site is the only
        // place a typed downcast happens, in the listener Element's own
        // `on_notification` body.
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

impl LifecycleContext for ElementBuildContext {
    fn rebuild_handle(&self) -> crate::RebuildHandle {
        // A real handle: the owner Arc is right here. The read lock is held only
        // to clone the shared inbox + frame-request Arcs out; nothing is held
        // across the returned handle's lifetime.
        self.owner.borrow().rebuild_handle(self.element_id)
    }
    fn writer_source(&self) -> crate::WriterSource {
        crate::WriterSource::new(self.reads.graph().clone())
    }
    fn build_phase(&self) -> flui_foundation::BuildPhase {
        self.reads.graph().build_phase()
    }
    fn async_driver(&self) -> Option<flui_scheduler::AsyncDriver> {
        self.owner.borrow().async_driver().cloned()
    }
    fn post_frame_handle(&self) -> Option<flui_scheduler::PostFrameHandle> {
        self.owner.borrow().post_frame_handle().cloned()
    }
    fn keep_alive_lease(&self) -> crate::owner::KeepAliveLease {
        self.keep_alive_handle().hold()
    }
    fn on_unmount(&self, hook: Box<dyn FnOnce()>) -> crate::UnmountHook {
        let hooks = self.owner.borrow().unmount_hooks.clone();
        hooks.register(self.element_id, hook)
    }
    fn keep_alive_handle(&self) -> crate::owner::KeepAliveHandle {
        self.owner.borrow().keep_alive.handle(self.element_id)
    }
    fn text_input_handle(&self) -> Option<flui_interaction::TextInputHandle> {
        self.owner.borrow().capabilities().text_input.clone()
    }
    fn capability_erased(
        &self,
        capability: std::any::TypeId,
        name: &'static str,
    ) -> Result<Box<dyn std::any::Any>, flui_platform::Unsupported> {
        let source = self.owner.borrow().capabilities().capabilities.clone();
        acquire_capability(source.as_deref(), capability, name)
    }
    fn announcer(&self) -> Option<flui_rendering::semantics::Announcer> {
        self.owner.borrow().capabilities().announcer.clone()
    }
    fn assistive_focus_support(&self) -> flui_rendering::semantics::AssistiveFocusSupport {
        self.owner.borrow().capabilities().assistive_focus
    }
    fn haptics(&self) -> Option<flui_interaction::HapticsHandle> {
        let handle = self.owner.borrow().capabilities().haptics.clone()?;
        Some(crate::haptics_mode::scoped(self, handle))
    }
    fn drag_and_drop(&self) -> Option<flui_interaction::drag::DragAndDrop> {
        self.owner.borrow().capabilities().drag_and_drop.clone()
    }
    fn texture_registry(&self) -> Option<flui_rendering::layer::TextureRegistry> {
        self.owner.borrow().capabilities().textures.clone()
    }
    fn close_guard(&self) -> Option<crate::CloseGuard> {
        self.owner.borrow().capabilities().close_guard.clone()
    }
    fn flush_registry_in_crate(
        &self,
        _token: crate::context::build_context::sealed::CrateToken,
    ) -> Option<crate::flush_registry::FlushRegistry> {
        self.owner.borrow().flush_registry.clone()
    }
    fn hit_test_handle(&self) -> Option<flui_interaction::HitTestHandle> {
        self.owner.borrow().hit_test_handle().cloned()
    }
    fn lifecycle_handle(&self) -> Option<crate::LifecycleHandle> {
        self.owner.borrow().lifecycle_handle()
    }
    fn window(&self) -> Option<crate::WindowHandle> {
        self.owner.borrow().window_handle()
    }
    fn focus_manager(&self) -> Rc<FocusManager> {
        self.owner.borrow().focus_manager()
    }
    /// See [`LifecycleContext::pipeline_owner`]. The owner is on this element's own
    /// node ΓÇö no ancestor walk.
    fn pipeline_owner(&self) -> Option<flui_rendering::pipeline::PipelineCell> {
        let tree = self.tree.borrow();
        tree.get(self.element_id)?.element().pipeline_owner()
    }
}
// Builder for ElementBuildContext
// ============================================================================

/// Builder for creating ElementBuildContext instances.
#[derive(Debug)]
pub struct ElementBuildContextBuilder {
    tree: Option<Rc<RefCell<ElementTree>>>,
    owner: Option<Rc<RefCell<BuildOwner>>>,
}

impl Default for ElementBuildContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ElementBuildContextBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            tree: None,
            owner: None,
        }
    }

    /// Set the element tree.
    #[must_use]
    pub fn tree(mut self, tree: Rc<RefCell<ElementTree>>) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Set the build owner.
    #[must_use]
    pub fn owner(mut self, owner: Rc<RefCell<BuildOwner>>) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Build a context for the given element.
    pub fn build_for(self, element_id: ElementId) -> Option<ElementBuildContext> {
        let tree = self.tree?;
        let owner = self.owner?;

        ElementBuildContext::for_element(element_id, tree, owner)
    }

    /// Create shared tree and owner, returning them along with the builder.
    pub fn with_new_tree_and_owner(
        self,
    ) -> (Self, Rc<RefCell<ElementTree>>, Rc<RefCell<BuildOwner>>) {
        let tree = Rc::new(RefCell::new(ElementTree::new()));
        let owner = Rc::new(RefCell::new(BuildOwner::new()));

        (self.tree(tree.clone()).owner(owner.clone()), tree, owner)
    }
}
