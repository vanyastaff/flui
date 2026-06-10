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
/// - InheritedView lookups (O(1) via BuildOwner registry)
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

    /// Whether this context is the minimal/dummy variant created by
    /// [`Self::new_minimal`]. The `tree` and `owner` Arcs of a minimal
    /// context point at the process-shared dummy cache (plan §U12 /
    /// audit V-13 — cheap part); mutating that shared state from a
    /// user `build()` accumulates writes into a long-lived global heap.
    /// [`BuildContext::mark_needs_build`] checks this flag and degrades
    /// to a `tracing::warn!`-and-no-op on minimal contexts to prevent
    /// unbounded growth of the shared dummy's `dirty_elements` heap
    /// (PR #119 review — copilot).
    is_minimal: bool,

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
            is_minimal: false,
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
            is_minimal: false,
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

    /// Walk ancestors of `self.element_id` looking for an element whose
    /// `view_type_id()` matches `type_id`. Returns the first matching
    /// ancestor's `ElementId`.
    ///
    /// Shared helper for U9 (`depend_on_inherited`) and U10
    /// (`get_inherited`). Both perform the same ancestor scan; only the
    /// dependent-recording side differs. Extracting the helper now also
    /// gives U11/U12 an obvious reuse target.
    ///
    /// Flutter parity: `framework.dart:5028-5060` `getElementForInheritedWidgetOfExactType` —
    /// Flutter uses a per-element `_inheritedElements: PersistentHashMap`,
    /// flui walks the ancestor chain directly because the per-element
    /// hash-map isn't necessary at our scale and avoids the
    /// reconciliation-time map-clone cost.
    fn walk_ancestors_for_inherited(&self, type_id: std::any::TypeId) -> Option<ElementId> {
        let tree = self.tree.read();

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

    /// Create a minimal context for use when full tree/owner aren't available.
    ///
    /// This is useful for StatelessElement::perform_build where we just need
    /// a context to pass to view.build() but don't have full tree
    /// infrastructure.
    ///
    /// The dummy `tree` / `owner` Arcs are pulled from a process-shared
    /// cache initialized lazily on first call — see
    /// [`BuildOwner::shared_dummy_tree`] /
    /// [`BuildOwner::shared_dummy_owner`]. Plan §U12 / R15, audit V-13
    /// (cheap separable part). Eliminates the
    /// `Arc::new(RwLock::new(ElementTree::new()))` /
    /// `Arc::new(RwLock::new(BuildOwner::new()))` allocations from the
    /// per-build hot path — each call now does two `Arc::clone`s
    /// (atomic refcount bumps) instead of two heap-arena allocations
    /// plus two `RwLock` payload allocations plus two empty inner
    /// structures.
    ///
    /// **Why this is sound.** The dummy tree is empty, so every
    /// production `BuildContext` accessor that walks the tree
    /// (`walk_ancestors_for_inherited`, `find_ancestor_element`, …)
    /// returns `None` / `false` after the first `tree.get(id)` lookup —
    /// the same return shape as the previous per-build dummy. The dummy
    /// is never written to during build because the early-`None` exit
    /// happens before any `write()` site. A user `build()` that calls
    /// `ctx.mark_needs_build()` (Flutter-forbidden, flui matches that
    /// policy) does write into the shared dummy's `BuildOwner`, but
    /// that owner's `dirty_elements` is never drained — the write is as
    /// lossy as the per-build dummy was, just to a different heap
    /// location.
    pub fn new_minimal(depth: usize) -> Self {
        let tree = BuildOwner::shared_dummy_tree();
        let owner = BuildOwner::shared_dummy_owner();
        // ElementId::new(1) is safe - 1 is non-zero
        let element_id = ElementId::new(1);

        Self {
            element_id,
            depth,
            mounted: true,
            tree,
            owner,
            is_minimal: true,
            #[cfg(debug_assertions)]
            is_building: true,
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

    fn owner(&self) -> Option<&BuildOwner> {
        // We can't return a reference to data behind RwLock directly
        // This method may need redesign or the trait needs adjustment
        // For now, return None - callers should use build_owner() method instead
        None
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
        let Some(ancestor_id) = self.walk_ancestors_for_inherited(type_id) else {
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
        let Some(ancestor_id) = self.walk_ancestors_for_inherited(type_id) else {
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
        if self.is_minimal {
            // The minimal/dummy context backs `new_minimal` builds and
            // shares a single process-global `BuildOwner` (plan §U12 /
            // audit V-13 — cheap part). Scheduling a build on it would
            // accumulate a sentinel `ElementId(1)` into the shared
            // dummy's long-lived `dirty_elements` heap with no drain
            // path — unbounded growth on a Flutter-forbidden misuse
            // path. Degrade to a warning instead (PR #119 review —
            // copilot).
            tracing::warn!(
                "mark_needs_build called on minimal ElementBuildContext — no-op (Flutter-forbidden during build; ctx.mark_needs_build would corrupt the process-shared dummy BuildOwner's dirty_elements heap)"
            );
            return;
        }
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
    use crate::{StatelessElement, StatelessView, View};

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
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
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
}
