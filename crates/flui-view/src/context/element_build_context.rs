//! ElementBuildContext - concrete BuildContext implementation for Elements.
//!
//! This provides the BuildContext interface during the build phase,
//! giving Views access to tree information and dependency injection.

use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId};
use parking_lot::RwLock;

use super::build_context::BuildContext;
use crate::{element::Notification, owner::BuildOwner, tree::ElementTree};

/// Concrete BuildContext implementation for Elements.
///
/// `ElementBuildContext` provides the bridge between Elements and the
/// BuildContext trait, giving Views access to:
/// - Element identity and tree position
/// - InheritedView lookups (O(1) via each node's `inherited` map)
/// - Ancestor traversal
/// - Rebuild scheduling
///
/// # Thread Safety
///
/// This struct holds Arc references to shared state, making it safe to
/// use across threads. The actual tree/owner access is synchronized via RwLock.
///
/// # Flutter Equivalent
///
/// In Flutter, `Element` implements `BuildContext` directly. Here we use
/// a separate struct to avoid borrow checker issues with self-referential
/// borrows during build.
pub struct ElementBuildContext {
    /// The ElementId this context represents.
    element_id: ElementId,

    /// Depth in the element tree.
    depth: usize,

    /// Whether the element is currently mounted.
    mounted: bool,

    /// Reference to the element tree.
    tree: Arc<RwLock<ElementTree>>,

    /// Reference to the build owner.
    owner: Arc<RwLock<BuildOwner>>,

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
    pub fn new(
        element_id: ElementId,
        depth: usize,
        mounted: bool,
        tree: Arc<RwLock<ElementTree>>,
        owner: Arc<RwLock<BuildOwner>>,
    ) -> Self {
        Self {
            element_id,
            depth,
            mounted,
            tree,
            owner,
            #[cfg(debug_assertions)]
            is_building: false,
        }
    }

    /// Create a context for a specific element from the tree.
    ///
    /// Returns None if the element doesn't exist in the tree.
    #[allow(clippy::needless_pass_by_value)] // Arc is cloned into Self, taking by value is idiomatic
    pub fn for_element(
        element_id: ElementId,
        tree: Arc<RwLock<ElementTree>>,
        owner: Arc<RwLock<BuildOwner>>,
    ) -> Option<Self> {
        let tree_guard = tree.read();
        let node = tree_guard.get(element_id)?;

        Some(Self {
            element_id,
            depth: node.depth(),
            mounted: node.element().mounted(),
            tree: tree.clone(),
            owner,
            #[cfg(debug_assertions)]
            is_building: false,
        })
    }

    /// Set the building flag (debug only).
    #[cfg(debug_assertions)]
    pub fn set_building(&mut self, building: bool) {
        self.is_building = building;
    }

    /// Get a reference to the tree.
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        // PORT-CHECK-OK-SP6: ElementBuildContext tree accessor; pre-existing SP-6
        &self.tree
    }

    /// Get a reference to the owner.
    pub fn build_owner(&self) -> &Arc<RwLock<BuildOwner>> {
        // PORT-CHECK-OK-SP6: ElementBuildContext build_owner accessor; pre-existing SP-6
        &self.owner
    }

    /// Nearest in-scope `InheritedElement` of view type `type_id`, in **O(1)**.
    ///
    /// Shared helper for U9 (`depend_on_inherited`) and U10 (`get_inherited`);
    /// only the dependent-recording side differs.
    ///
    /// Reads the resolved inherited scope
    /// ([`ElementNode::inherited`](crate::tree::ElementNode)) instead of
    /// walking the ancestor chain — Flutter parity for `_inheritedElements[T]`
    /// (`framework.dart:5094`, the O(1) per-element map). flui builds that map
    /// at mount as an `Arc<HashMap>` shared by refcount down non-provider runs.
    fn find_inherited_provider(&self, type_id: std::any::TypeId) -> Option<ElementId> {
        self.tree
            .read()
            .get(self.element_id)?
            .inherited_provider(type_id)
    }

    /// Walk strict-ancestors (parent and up) of `self.element_id`,
    /// invoking `predicate(&dyn ElementBase) -> ControlFlow<R>` for each
    /// visited element. Returns `Some(R)` if the predicate breaks with
    /// `ControlFlow::Break`, or `None` if every ancestor is exhausted
    /// without a break.
    ///
    /// Shared helper for the U11 trio
    /// ([`find_ancestor_view`](BuildContext::find_ancestor_view),
    /// [`find_ancestor_state`](BuildContext::find_ancestor_state),
    /// [`find_root_ancestor_state`](BuildContext::find_root_ancestor_state)).
    /// The first two stop on the first match; the last continues to root
    /// and the caller tracks the root-most match in `R` by accumulating
    /// across `Continue` returns.
    ///
    /// The tree read-lock is held for the duration of the walk so the
    /// `&dyn ElementBase` reference handed to `predicate` cannot escape
    /// the closure — preserves the declarative-build invariant
    /// (Constitution Principle 5).
    ///
    /// Flutter parity: `framework.dart:5104-5160`
    /// `_ancestorRenderObjectElement` / `findAncestorStateOfType` /
    /// `findRootAncestorStateOfType` — Flutter uses an inline
    /// `Element ancestor = _parent` loop with the same break-on-match
    /// shape.
    fn walk_strict_ancestors<R>(
        &self,
        mut predicate: impl FnMut(&dyn crate::view::ElementBase) -> std::ops::ControlFlow<R>,
    ) -> Option<R> {
        let tree = self.tree.read();

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

    fn rebuild_handle(&self) -> crate::RebuildHandle {
        // A real handle: the owner Arc is right here. The read lock is held only
        // to clone the shared inbox + frame-request Arcs out; nothing is held
        // across the returned handle's lifetime.
        self.owner.read().rebuild_handle(self.element_id)
    }

    fn async_driver(&self) -> Option<flui_scheduler::AsyncDriver> {
        self.owner.read().async_driver().cloned()
    }

    fn post_frame_handle(&self) -> Option<flui_scheduler::PostFrameHandle> {
        self.owner.read().post_frame_handle().cloned()
    }

    fn depend_on_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Walk ancestors looking for an Element whose view_type_id
        // matches; the first one is the nearest InheritedView<T>.
        //
        // Records this element in the matched InheritedElement's
        // dependent map so a subsequent rebuild with
        // `update_should_notify == true` schedules us for rebuild
        // (R16, plan §U9).
        //
        // Flutter parity: `framework.dart:5081`
        // `dependOnInheritedWidgetOfExactType` -> the matched
        // `InheritedElement` then has `updateDependencies(self, null)`
        // called on it (`framework.dart:5034`).
        let Some(ancestor_id) = self.find_inherited_provider(type_id) else {
            return false;
        };

        // Acquire a write lock so we can mutate the matched
        // InheritedElement's dependent map AND invoke the callback with
        // the inherited view in the same critical section. Reading the
        // view itself only needs a read lock, but we need write access
        // to record the dependency, so a single write lock is
        // sufficient.
        let mut tree = self.tree.write();
        let Some(node) = tree.get_mut(ancestor_id) else {
            // Tree shape changed between lookup and write-lock; treat
            // as miss. Should not happen under normal flow.
            return false;
        };

        // Capture self's depth before we hand `node` out so we can
        // record it as the dependent's depth (used by
        // BuildOwner::schedule_build_for during R16 notify).
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
            // impossible under TypeId semantics — defensive return.
            return false;
        };

        // Register dependency (id + depth).
        accessor.record_dependent(self_id, self_depth);

        // Hand the view out to the callback. The view reference is
        // borrowed for the lifetime of the callback only; it cannot
        // escape into `build()` because the closure is `FnOnce`.
        let view_any = accessor.view_as_any();
        callback(view_any);

        true
    }

    fn get_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Same ancestor walk as depend_on_inherited, but does NOT
        // record a dependency. Reserved for U10 — for now we share the
        // walk + downcast logic and skip the `record_dependent` call.
        let Some(ancestor_id) = self.find_inherited_provider(type_id) else {
            return false;
        };

        let tree = self.tree.read();
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
        let tree = self.tree.read();

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
        // while the tree read-lock is held — preserves declarative-build
        // invariant.
        //
        // Flutter parity: `framework.dart:5122`
        // `findAncestorWidgetOfExactType<T>` returns `element.widget` of
        // the first ancestor whose widget runtimeType equals T. No
        // dependency recording (this is a read-only walk).
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
        // Flutter parity: `framework.dart:5132`
        // `findAncestorStateOfType<T extends State>` matches against
        // the State runtime type (T is the State subtype, not the
        // StatefulWidget). We do the same: `type_id` is
        // `TypeId::of::<S>()` where S is the State type.
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
        // — the tree is single-threaded during build, so the re-lock
        // is essentially free.
        //
        // The shared `walk_strict_ancestors` helper isn't reused here
        // because it surfaces `&dyn ElementBase` but not the matching
        // ancestor's id, and root-most matching needs the id to fetch
        // state via the second borrow. Keeping the helper minimal
        // (no id-yielding variant) is a YAGNI call for U11; if U12 or
        // a future unit needs id-yielding walks we can widen the
        // surface then.
        //
        // Flutter parity: `framework.dart:5146`
        // `findRootAncestorStateOfType<T>` — Flutter walks
        // `element._parent` repeatedly, updating a local `ancestor`
        // whenever a match is found, and returns `ancestor?.state` at
        // the end.
        let mut root_most: Option<ElementId> = None;

        // Phase 1: walk all strict-ancestors, record the root-most match.
        // Use `walk_strict_ancestors` with a closure that captures
        // `root_most` mutably and always returns `Continue` so the walk
        // exhausts the entire ancestor chain. We need the matching
        // ancestor's `ElementId`, but the walker only exposes
        // `&dyn ElementBase` — we work around that by parameterising R
        // on `Option<ElementId>` and threading the candidate id through
        // the closure via a small index counter. Simpler still: do the
        // walk inline but in a `while let` shape that satisfies
        // clippy::while_let_loop while keeping the same semantics.
        {
            let tree = self.tree.read();
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

        let tree = self.tree.read();
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
        // `RenderBehavior<V>` overrides the trait default — every other
        // behavior (Stateless / Proxy / Inherited / Stateful / Animation)
        // keeps `None`, so non-render ancestors are skipped cleanly.
        //
        // Non-callback signature is sound: `RenderId` is `Copy`, so we
        // can hand it out directly without extending a `&self` borrow
        // into the rest of `build()` (plan §D2).
        //
        // Flutter parity: `framework.dart:5160`
        // `findAncestorRenderObjectOfType<T>` — Flutter walks `_parent`
        // and returns the first matching `RenderObjectElement`'s
        // `renderObject`. We do the equivalent walk and read
        // `RenderBehavior::render_id` at the dispatch boundary.
        self.walk_strict_ancestors(|ancestor| {
            if let Some(id) = ancestor.render_id() {
                std::ops::ControlFlow::Break(id)
            } else {
                std::ops::ControlFlow::Continue(())
            }
        })
    }

    /// See [`BuildContext::pipeline_owner`]. The owner is on this element's own
    /// node — no ancestor walk.
    fn pipeline_owner(
        &self,
    ) -> Option<std::sync::Arc<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>> {
        let tree = self.tree.read();
        tree.get(self.element_id)?
            .element()
            .pipeline_owner_any()?
            .downcast::<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>()
            .ok()
    }

    fn visit_ancestor_elements(&self, visitor: &mut dyn FnMut(ElementId) -> bool) {
        let tree = self.tree.read();

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

        // E3 (atomic box→arena swap): a node's children are its
        // slab-resident `child_ids` list — the single element graph.
        let tree = self.tree.read();
        if let Some(node) = tree.get(self.element_id) {
            for child_id in node.child_ids() {
                visitor(*child_id);
            }
        }
    }

    fn mark_needs_build(&self) {
        let mut owner = self.owner.write();
        owner.schedule_build_for(self.element_id, self.depth);
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
        // `Any::type_id` — sound because `Notification: Any` guarantees
        // the concrete-type vtable carries it.
        //
        // Flutter parity: `notification_listener.dart:67`
        // (`Notification.dispatch`) walks `_parent`, invoking each
        // `_NotificationElement.onNotification` handler with the typed
        // notification and stopping when one returns `true`.
        //
        // Plan §U13 / R10 / AE6. Single-`dyn`-boundary discipline per
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

// ============================================================================
// BuildCtx — borrowed, build-time BuildContext (PR-K)
// ============================================================================

/// A dependency a building element registered on an `InheritedElement`.
///
/// Recorded during `build()` while the tree is borrowed read-only (a
/// [`BuildCtx`] holds `&ElementTree`), then applied by
/// [`BuildOwner::build_scope`](crate::BuildOwner) — which holds `&mut tree`
/// again once the built element is put back — onto the provider node's
/// dependent set. Deferring the *write* keeps the build itself read-only
/// while still recording within the same `build_scope` iteration, before
/// the next dirty element is processed.
pub(crate) struct DependentRecord {
    /// The `InheritedElement` the dependent read from.
    pub(crate) provider: ElementId,
    /// The element that read it (and must rebuild when it changes).
    pub(crate) dependent: ElementId,
    /// The dependent's tree depth (for dirty-heap ordering).
    pub(crate) depth: usize,
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
/// Flutter-forbidden no-op.
/// The three things a `BuildContext` is handed that it did not compute itself: two
/// owned frame capabilities the binding installed, and the render tree the element is
/// mounted in.
///
/// A bundle rather than three parameters, because every one of them is minted at the
/// same place (`make_build_ctx`) from the same two sources, and threading them
/// separately made `BuildCtx::new` an eight-argument function.
#[derive(Clone, Default)]
pub(crate) struct BuildCapabilities {
    /// The binding's async task driver.
    pub(crate) async_driver: Option<flui_scheduler::AsyncDriver>,
    /// The binding's post-frame capability.
    pub(crate) post_frame_handle: Option<flui_scheduler::PostFrameHandle>,
    /// The render tree this element is mounted in, cloned from its own
    /// `ElementCore` — see `make_build_ctx` for why not from the tree node.
    pub(crate) pipeline_owner:
        Option<std::sync::Arc<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>>,
}

pub(crate) struct BuildCtx<'b> {
    element_id: ElementId,
    depth: usize,
    tree: &'b ElementTree,
    dep_sink: &'b parking_lot::Mutex<Vec<DependentRecord>>,
    /// Owned rebuild capability for `element_id`, minted by `make_build_ctx`
    /// from the element's own core. Cloned out by
    /// [`BuildContext::rebuild_handle`]; the build itself never schedules —
    /// port-check trigger #22 forbids even acquiring it here.
    rebuild: crate::RebuildHandle,
    /// What the binding and the element's core handed this context.
    capabilities: BuildCapabilities,
}

impl<'b> BuildCtx<'b> {
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
    /// the result is the nearest strict-ancestor provider; matches Flutter's
    /// `_inheritedElements[T]` lookup.
    fn find_inherited_provider(&self, type_id: TypeId) -> Option<ElementId> {
        self.tree.get(self.element_id)?.inherited_provider(type_id)
    }
}

impl BuildContext for BuildCtx<'_> {
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

    fn rebuild_handle(&self) -> crate::RebuildHandle {
        self.rebuild.clone()
    }

    fn async_driver(&self) -> Option<flui_scheduler::AsyncDriver> {
        self.capabilities.async_driver.clone()
    }

    fn post_frame_handle(&self) -> Option<flui_scheduler::PostFrameHandle> {
        self.capabilities.post_frame_handle.clone()
    }

    fn depend_on_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        let Some(provider_id) = self.find_inherited_provider(type_id) else {
            return false;
        };
        let Some(accessor) = self
            .tree
            .get(provider_id)
            .and_then(super::super::tree::ElementNode::element_opt)
            .and_then(crate::view::ElementBase::as_inherited)
        else {
            return false;
        };
        // Buffer the dependent BEFORE invoking the user callback. The tree is
        // read-only here, so the write itself is deferred to the `build_scope`
        // drain (see [`DependentRecord`]) — but it is *recorded* first, matching
        // `ElementBuildContext::depend_on_inherited` and Flutter
        // (`dependOnInheritedElement` calls `updateDependencies` before
        // returning the widget). This matters on the error path: if the user
        // `build()` panics after this `depend_on` (caught by `build_or_recover`,
        // which substitutes an `ErrorView`), the element stays registered as a
        // dependent, so a later inherited change reschedules it and it recovers.
        // Recording only after the callback would drop the registration on that
        // panic and strand the element on the `ErrorView`.
        self.dep_sink.lock().push(DependentRecord {
            provider: provider_id,
            dependent: self.element_id,
            depth: self.depth,
        });
        callback(accessor.view_as_any());
        true
    }

    fn get_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        let Some(provider_id) = self.find_inherited_provider(type_id) else {
            return false;
        };
        let Some(accessor) = self
            .tree
            .get(provider_id)
            .and_then(super::super::tree::ElementNode::element_opt)
            .and_then(crate::view::ElementBase::as_inherited)
        else {
            return false;
        };
        callback(accessor.view_as_any());
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

    /// Cloned at construction from the element's own `ElementCore`: during
    /// `build_scope` the element is *extracted* from its tree node, so a
    /// `BuildContext` cannot look itself up (`ElementNode::element` panics in that
    /// window). See `make_build_ctx`.
    fn pipeline_owner(
        &self,
    ) -> Option<std::sync::Arc<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>> {
        self.capabilities.pipeline_owner.clone()
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
        // Forbidden during build: a `BuildCtx` is ALWAYS mid-build, and the
        // node's `child_ids` here are the PRE-reconcile list — for an update
        // build they may be removed or reordered moments later. Mirrors the
        // guard on `ElementBuildContext::visit_child_elements` and Flutter's
        // build-target check (`framework.dart` `_debugCheckOwnerBuildTargetExists`).
        debug_assert!(
            !self.is_building(),
            "visit_child_elements cannot be called during build (a BuildCtx is always \
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
            "BuildCtx::mark_needs_build called during build — no-op (Flutter forbids \
             mark-dirty during the build of the same element)"
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

// ============================================================================
// Builder for ElementBuildContext
// ============================================================================

/// Builder for creating ElementBuildContext instances.
#[derive(Debug)]
pub struct ElementBuildContextBuilder {
    tree: Option<Arc<RwLock<ElementTree>>>,
    owner: Option<Arc<RwLock<BuildOwner>>>,
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
    pub fn tree(mut self, tree: Arc<RwLock<ElementTree>>) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Set the build owner.
    #[must_use]
    pub fn owner(mut self, owner: Arc<RwLock<BuildOwner>>) -> Self {
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
    ) -> (Self, Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        (self.tree(tree.clone()).owner(owner.clone()), tree, owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{IntoView, ViewExt};
    use crate::{StatelessView, View};

    #[derive(Clone)]
    struct TestView {
        #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
        name: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn test_context_creation() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        // Mount a root element
        let view = TestView {
            name: "root".to_string(),
        };
        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());

        // Create context for it
        let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone());

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.element_id(), root_id);
        assert_eq!(ctx.depth(), 0);
        assert!(ctx.mounted());
    }

    #[test]
    fn test_context_builder() {
        let (builder, tree, owner) = ElementBuildContextBuilder::new().with_new_tree_and_owner();

        let view = TestView {
            name: "test".to_string(),
        };
        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());

        let ctx = builder.build_for(root_id);
        assert!(ctx.is_some());
    }

    #[test]
    fn test_mark_needs_build() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        let view = TestView {
            name: "root".to_string(),
        };
        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());

        let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();

        assert!(!owner.read().has_dirty_elements());

        ctx.mark_needs_build();

        assert!(owner.read().has_dirty_elements());
    }

    #[test]
    fn test_visit_ancestor_elements() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        // Create tree: root -> child -> grandchild
        let root_view = TestView {
            name: "root".to_string(),
        };
        let child_view = TestView {
            name: "child".to_string(),
        };
        let grandchild_view = TestView {
            name: "grandchild".to_string(),
        };

        let root_id = tree
            .write()
            .mount_root(&root_view, &mut owner.write().element_owner_mut());
        let child_id = tree.write().insert(
            &child_view,
            root_id,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let grandchild_id = tree.write().insert(
            &grandchild_view,
            child_id,
            0,
            &mut owner.write().element_owner_mut(),
        );

        // Create context for grandchild
        let ctx =
            ElementBuildContext::for_element(grandchild_id, tree.clone(), owner.clone()).unwrap();

        // Visit ancestors
        let mut ancestors = Vec::new();
        ctx.visit_ancestor_elements(&mut |id| {
            ancestors.push(id);
            true
        });

        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0], child_id);
        assert_eq!(ancestors[1], root_id);
    }

    #[test]
    fn test_find_ancestor_element() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        let view = TestView {
            name: "root".to_string(),
        };
        let child_view = TestView {
            name: "child".to_string(),
        };

        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());
        let child_id = tree.write().insert(
            &child_view,
            root_id,
            0,
            &mut owner.write().element_owner_mut(),
        );

        let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

        // Find ancestor of same type
        let ancestor = ctx.find_ancestor_element(TypeId::of::<TestView>());
        assert_eq!(ancestor, Some(root_id));
    }

    #[test]
    fn test_context_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ElementBuildContext>();
    }

    /// `BuildCtx` is the context handed to a live `build()`, so it is always
    /// mid-build; `visit_child_elements` is forbidden during build (its
    /// `child_ids` are the stale pre-reconcile list). The debug guard must
    /// fire — mirroring `ElementBuildContext::visit_child_elements`.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "visit_child_elements cannot be called during build")]
    fn build_ctx_forbids_visit_child_elements_during_build() {
        let tree = ElementTree::new();
        let dep_sink = parking_lot::Mutex::new(Vec::new());
        // The guard fires before any tree access, so a sentinel id is fine.
        let ctx = BuildCtx::new(
            ElementId::new(1),
            0,
            &tree,
            &dep_sink,
            crate::RebuildHandle::inert(),
            BuildCapabilities::default(),
        );
        ctx.visit_child_elements(&mut |_| {});
    }
}
