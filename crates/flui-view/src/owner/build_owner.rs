//! BuildOwner - Manages the build phase.
//!
//! The BuildOwner is responsible for:
//! - Tracking dirty elements that need rebuilding
//! - Processing rebuilds in depth-first order
//! - Managing GlobalKey registry
//! - Coordinating InheritedElement lookups

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId};
use parking_lot::{Mutex, RwLock};

use crate::{
    element::child_manager::{ChildManager, ChildManagerRegistry},
    owner::layout_builder::LayoutBuilderRegistry,
    tree::ElementTree,
    view::View,
};

/// A cloneable, owned handle that lets a listener callback — an animation tick
/// fired *outside* any frame, with no `&mut BuildOwner` in scope — enqueue an
/// element for the next [`BuildOwner::build_scope`] drain and request a frame.
///
/// This is the arena analogue of Flutter's `Element.markNeedsBuild` reaching
/// `BuildOwner.scheduleBuildFor` + `SchedulerBinding.scheduleFrame`: an
/// `AnimatedView`'s mark-dirty callback captures one of these at mount (via
/// [`ElementOwner::external_scheduler`](super::ElementOwner::external_scheduler))
/// and calls [`schedule`](Self::schedule) when the listenable changes. The
/// pending ids accumulate in a shared inbox that `build_scope` drains onto its
/// dirty heap at frame start, so the listener never needs to touch the owner.
///
/// The inbox carries the element id ONLY — the dirty-heap ordering key (tree
/// depth) is read authoritatively from the node at drain time, not captured
/// here, because `ElementCore` does not know its own tree depth (its `depth`
/// field is the sibling slot index, not `parent_depth + 1`).
#[derive(Clone)]
pub(crate) struct ExternalBuildScheduler {
    /// Shared inbox drained by `build_scope`; a SET of element ids to rebuild.
    /// A set (not a `Vec`) so repeated ticks between frames — a 60fps animation
    /// while the frame driver is stalled — collapse to one entry per element
    /// instead of growing unbounded.
    inbox: Arc<Mutex<HashSet<ElementId>>>,
    /// Frame-request hook (the binding's `on_build_scheduled`), so a tick
    /// between frames asks the platform for a new frame. `None` in headless
    /// tests, which drive `build_scope` directly.
    request_frame: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ExternalBuildScheduler {
    /// Enqueue `id` for the next `build_scope` drain and request a frame.
    ///
    /// Deduplicating: a repeat tick for an id already queued is a no-op and does
    /// NOT re-request a frame, so a burst of ticks for one element costs one
    /// inbox slot and one frame request. Thread-safe: the inbox lock is held
    /// only for the insert and released before `request_frame` runs (no lock
    /// across the platform wake).
    pub(crate) fn schedule(&self, id: ElementId) {
        let newly_queued = self.inbox.lock().insert(id);
        if newly_queued && let Some(request_frame) = &self.request_frame {
            request_frame();
        }
    }

    /// Build a scheduler from the shared inbox + frame-request handle. Used by
    /// [`ElementOwner::external_scheduler`](super::ElementOwner::external_scheduler).
    pub(crate) fn from_parts(
        inbox: Arc<Mutex<HashSet<ElementId>>>,
        request_frame: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            inbox,
            request_frame,
        }
    }
}

impl std::fmt::Debug for ExternalBuildScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `try_lock`, not `lock`: `parking_lot::Mutex` is non-reentrant, so a
        // `{:?}` while the inbox is already held (e.g. instrumenting the drain)
        // would otherwise deadlock silently.
        f.debug_struct("ExternalBuildScheduler")
            .field("pending", &self.inbox.try_lock().map(|set| set.len()))
            .field("has_request_frame", &self.request_frame.is_some())
            .finish()
    }
}

/// Entry in the dirty elements heap.
///
/// Sorted by depth (shallowest first) for top-down processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirtyElement {
    id: ElementId,
    depth: usize,
}

impl DirtyElement {
    /// Construct a new dirty-elements heap entry.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self { id, depth }
    }

    /// The element id queued for rebuild.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order the heap (shallowest first).
    ///
    /// Currently consumed only by inline tests; a future consumer will read it during
    /// dirty-element drain dispatching. The `Ord` impl reads
    /// `self.depth` directly (private field access from the same `impl`
    /// block), so the accessor stays on the surface for future
    /// `ElementOwner` consumers.
    #[allow(dead_code)]
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
}

impl Ord for DirtyElement {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Min-heap by depth (process shallowest first)
        self.depth.cmp(&other.depth)
    }
}

impl PartialOrd for DirtyElement {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Manages the build phase of the element lifecycle.
///
/// BuildOwner tracks which elements need rebuilding and processes them
/// in the correct order (depth-first, shallowest first).
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `BuildOwner` class.
///
/// # Responsibilities
///
/// - Maintain list of dirty elements
/// - Process rebuilds in correct order
/// - Manage GlobalKey registry
/// - Track inactive elements for finalization
///
/// O(1) InheritedElement lookup is NOT here — it lives structurally in each
/// node's [`inherited`](crate::tree::ElementNode) map, built at mount.
pub struct BuildOwner {
    /// Elements that need rebuild, sorted by depth.
    ///
    /// `pub(crate)` so [`ElementOwner`](super::ElementOwner)'s
    /// split-borrow can pin a `&mut` reference to just this field
    /// during the recursive Element traversal — no full `&mut
    /// BuildOwner` needed.
    pub(crate) dirty_elements: BinaryHeap<Reverse<DirtyElement>>,

    /// Set of dirty element IDs (for deduplication).
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) dirty_set: std::collections::HashSet<ElementId>,

    /// GlobalKey registry: key hash -> element ID.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) global_keys: HashMap<u64, ElementId>,

    /// Elements that have been deactivated and are pending unmount.
    /// These are unmounted in `finalize_tree()`.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) inactive_elements: Vec<InactiveElement>,

    /// Elements that received an inherited-dependency change since their
    /// last build. `build_scope` consults this set right before each
    /// dirty element's `perform_build` and fires
    /// `ElementBase::notify_dependency_change` (which routes through the
    /// behavior to call `ViewState::did_change_dependencies`) when the
    /// id is present, then removes the entry — Flutter parity for
    /// `_didChangeDependencies` flag at `framework.dart:6114`.
    ///
    /// Populated by [`InheritedBehavior::on_view_updated`](crate::element::InheritedBehavior)
    /// when `update_should_notify == true`. Cleared on element unmount
    /// (the dependent leaves the tree before its rebuild ever runs).
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) pending_dependency_changes: std::collections::HashSet<ElementId>,

    /// Whether we're currently in a build phase.
    #[cfg(debug_assertions)]
    building: bool,

    /// Build scope nesting depth.
    #[cfg(debug_assertions)]
    scope_depth: usize,

    /// Callback to be called when a build is scheduled.
    ///
    /// `pub(crate)` so the [`ElementOwner`](super::ElementOwner)
    /// split-borrow can fire it from `schedule_build_for` without
    /// re-borrowing the owner. Stored as `Arc` (not `Box`) so an
    /// `ExternalBuildScheduler` captured by an animation listener can clone
    /// and fire it as a frame request from outside a frame.
    #[allow(clippy::type_complexity)]
    pub(crate) on_build_scheduled: Option<Arc<dyn Fn() + Send + Sync>>,

    /// Inbox of element ids scheduled from *outside* a frame — an
    /// animation/listenable tick whose mark-dirty callback holds an
    /// `ExternalBuildScheduler` but no `&mut BuildOwner`. A SET, so repeated
    /// ticks dedup. Drained onto [`Self::dirty_elements`] at the start of
    /// [`Self::build_scope`], where each id's tree depth is looked up. Shared
    /// (`Arc`) so the listener callbacks and the owner reference the same queue.
    pub(crate) external_inbox: Arc<Mutex<HashSet<ElementId>>>,

    /// Registry of live lazy-sliver [`ChildManager`]s, one per live adaptor
    /// element. Keyed by the sliver's `RenderId`; populated at mount and
    /// cleared at unmount by `SliverListAdaptorBehavior` via the
    /// `ElementOwner::register_child_manager` / `unregister_child_manager`
    /// split-borrow methods.
    ///
    /// `Arc<Mutex<…>>` (not a plain `HashMap`) so `ElementOwner` can carry a
    /// `&'a Arc<…>` reference — the same pattern as `external_inbox`. The outer
    /// `Arc` lets `service_child_requests` clone individual manager `Arc`s out
    /// of the registry before calling service (releasing the registry lock
    /// before the potentially long service call).
    pub(crate) child_manager_registry: ChildManagerRegistry,

    /// Registry of live build-during-layout nodes, one per
    /// mounted layout-builder element, keyed by its render object's `RenderId`.
    /// Drained every layout pass by
    /// [`service_layout_builders`](Self::service_layout_builders).
    ///
    /// Empty unless a `LayoutBuilder` is mounted; the seam is inert until one is.
    pub(crate) layout_builder_registry: LayoutBuilderRegistry,

    /// The binding's frame-driven async task driver, installed by
    /// whichever binding owns this owner. `None` until then — a tree built with
    /// no binding cannot spawn tasks, and `BuildContext::async_driver` reports
    /// that honestly rather than silently spawning into a driver nobody polls.
    ///
    /// This must be the driver the binding's frame step actually polls:
    /// `HeadlessBinding` drives a binding-local `Scheduler`, production drives
    /// `Scheduler::instance()`. Reaching for the singleton from a widget would
    /// make headless tests spawn into a driver that never runs.
    pub(crate) async_driver: Option<flui_scheduler::AsyncDriver>,

    /// The binding's post-frame capability. `None` when no binding
    /// installed one, which makes `BuildContext::post_frame_handle` report the
    /// absence rather than silently scheduling onto a global.
    pub(crate) post_frame_handle: Option<flui_scheduler::PostFrameHandle>,

    /// The binding's IME/text-input attach-detach capability. `None` when no
    /// binding installed one, which makes `BuildContext::text_input_handle`
    /// report the absence rather than a widget silently having no way to
    /// attach an IME client.
    pub(crate) text_input_handle: Option<flui_interaction::TextInputHandle>,

    /// The binding's owner-local interaction dispatch capability (ADR-0027).
    ///
    /// `None` means the owner was built detached from a runtime interaction lane;
    /// render-object lifecycle contexts report that as a typed inactive realm.
    pub(crate) interaction_dispatch: Option<flui_interaction::InteractionDispatchHandle>,
}

/// An element that has been deactivated and is pending unmount.
///
/// Made `pub(crate)` so [`ElementOwner`](super::ElementOwner) can hold a
/// `&mut Vec<InactiveElement>` split-borrow reference. End-of-frame
/// finalization (`BuildOwner::finalize_tree`) drains the queue
/// deepest-first using the recorded `depth`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InactiveElement {
    id: ElementId,
    depth: usize,
}

impl InactiveElement {
    /// Construct a new inactive-element record.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self { id, depth }
    }

    /// The element id queued for end-of-frame unmount.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order finalization (deepest first).
    #[allow(dead_code)] // Used by finalize_tree's sort, kept for symmetry.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
}

impl Default for BuildOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildOwner {
    /// Create a new BuildOwner.
    pub fn new() -> Self {
        Self {
            dirty_elements: BinaryHeap::new(),
            dirty_set: std::collections::HashSet::new(),
            global_keys: HashMap::new(),
            inactive_elements: Vec::new(),
            pending_dependency_changes: std::collections::HashSet::new(),
            #[cfg(debug_assertions)]
            building: false,
            #[cfg(debug_assertions)]
            scope_depth: 0,
            on_build_scheduled: None,
            external_inbox: Arc::new(Mutex::new(HashSet::new())),
            child_manager_registry: Arc::new(Mutex::new(HashMap::new())),
            layout_builder_registry: Arc::new(Mutex::new(HashMap::new())),
            async_driver: None,
            post_frame_handle: None,
            text_input_handle: None,
            interaction_dispatch: None,
        }
    }

    /// Install the binding's async task driver.
    ///
    /// Called once, at wiring time, by `HeadlessBinding` and `AppBinding`. Must
    /// be the same driver the binding's frame step polls.
    pub fn set_async_driver(&mut self, driver: flui_scheduler::AsyncDriver) {
        self.async_driver = Some(driver);
    }

    /// The binding's async task driver, if one was installed.
    #[must_use]
    pub fn async_driver(&self) -> Option<&flui_scheduler::AsyncDriver> {
        self.async_driver.as_ref()
    }

    /// Install the binding's post-frame capability.
    ///
    /// Called once, at wiring time, by `HeadlessBinding` and `AppBinding`. It must
    /// name **that binding's** scheduler — the one whose `drive_frame` drains the
    /// queue. Headless owns a binding-local `Scheduler`; production drives the
    /// `Scheduler::instance()` singleton.
    pub fn set_post_frame_handle(&mut self, handle: flui_scheduler::PostFrameHandle) {
        self.post_frame_handle = Some(handle);
    }

    /// Install the binding's IME/text-input attach-detach capability.
    ///
    /// Called once, at wiring time, by `AppBinding` (via `UiRealm::bind_to_app`).
    /// `HeadlessBinding` installs none, so headless-tree tests observe
    /// `BuildContext::text_input_handle() == None` honestly rather than a
    /// stub that silently accepts attaches nobody delivers events to.
    pub fn set_text_input_handle(&mut self, handle: flui_interaction::TextInputHandle) {
        self.text_input_handle = Some(handle);
    }

    /// Install the binding's owner-local interaction dispatch handle (ADR-0027).
    pub fn set_interaction_dispatch_handle(
        &mut self,
        handle: flui_interaction::InteractionDispatchHandle,
    ) {
        self.interaction_dispatch = Some(handle);
    }

    /// The binding's post-frame capability, if one was installed.
    #[must_use]
    pub fn post_frame_handle(&self) -> Option<&flui_scheduler::PostFrameHandle> {
        self.post_frame_handle.as_ref()
    }

    /// The binding's IME/text-input attach-detach capability, if one was
    /// installed.
    #[must_use]
    pub fn text_input_handle(&self) -> Option<&flui_interaction::TextInputHandle> {
        self.text_input_handle.as_ref()
    }

    /// Set the callback for when a build is scheduled.
    ///
    /// This is called by `schedule_build_for` to notify the binding
    /// that a visual update is needed.
    ///
    /// Set this BEFORE mounting any element. Each element captures a clone of
    /// the current callback `Arc` into its `ExternalBuildScheduler` at mount
    /// (for out-of-frame rebuild requests); replacing the callback afterwards
    /// does not retroactively update already-mounted elements, which keep
    /// firing the previous `Arc`. The binding wires this once at startup.
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Arc::new(callback));
    }

    /// Schedule an element for rebuild.
    ///
    /// Elements are processed in depth order (shallowest first) so parent
    /// rebuilds happen before child rebuilds. The `depth` is a best-effort
    /// ordering hint: [`build_scope`](Self::build_scope) re-derives every
    /// queued element's authoritative tree depth from its node before draining
    /// (see `rekey_dirty_depths`), so a caller that
    /// only knows the sibling slot index (e.g. `setState` via
    /// `ElementCore::schedule_self_build`) cannot mis-order the drain.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize) {
        if self.dirty_set.insert(id) {
            self.dirty_elements
                .push(Reverse(DirtyElement::new(id, depth)));

            // Notify that a build was scheduled
            if let Some(callback) = self.on_build_scheduled.as_deref() {
                callback();
            }
        }
    }

    /// Mark every live element dirty so the next [`build_scope`](Self::build_scope)
    /// re-runs all `build()` methods.
    ///
    /// Flutter parity: `BuildOwner.reassemble()` / `Element.reassemble()` during
    /// hot reload. **Does not** unmount elements or dispose `State` — stateful
    /// elements keep their in-tree `ViewState` across the call.
    pub fn reassemble(&mut self, tree: &ElementTree) {
        for (id, node) in tree.iter_nodes() {
            self.schedule_build_for(id, node.depth);
        }
        tracing::info!(
            count = self.dirty_count(),
            "BuildOwner::reassemble — all elements marked dirty for hot reload"
        );
    }

    /// Re-key every queued dirty element to its authoritative TREE depth.
    ///
    /// The dirty heap orders by depth, but `schedule_build_for` is handed a
    /// depth by its caller — and the `setState` path
    /// (`ElementCore::schedule_self_build`) plus the live `BuildCtx` both pass
    /// `ElementCore::depth`, which is the sibling SLOT index, not
    /// `parent_depth + 1`. Left as-is, a deeply-nested `setState` would sort as
    /// if it were shallow and a child could rebuild before its parent —
    /// violating Flutter's shallowest-first contract. Rebuilding the heap keyed
    /// on each node's real depth (`ElementNode::depth`, the same authority the
    /// external-inbox drain uses) restores the contract regardless of what
    /// `schedule_build_for` was told.
    fn rekey_dirty_depths(&mut self, tree: &ElementTree) {
        if self.dirty_elements.is_empty() {
            return;
        }
        let queued: Vec<ElementId> = std::mem::take(&mut self.dirty_elements)
            .into_iter()
            .map(|Reverse(dirty)| dirty.id())
            .collect();
        for id in queued {
            let depth = tree.get(id).map_or(0, |node| node.depth);
            self.dirty_elements
                .push(Reverse(DirtyElement::new(id, depth)));
        }
    }

    /// Acquire an [`ElementOwner`](super::ElementOwner) split-borrow
    /// handle for the duration of an Element lifecycle traversal.
    ///
    /// The returned handle holds disjoint `&mut` references to
    /// `global_keys`, `dirty_elements`, `dirty_set`, and
    /// `inactive_elements` — every field an `Element::mount` /
    /// `unmount` / `update` path may write. The borrow checker proves
    /// non-aliasing because each field is borrowed once.
    pub fn element_owner_mut(&mut self) -> super::ElementOwner<'_> {
        super::ElementOwner {
            global_keys: &mut self.global_keys,
            dirty_elements: &mut self.dirty_elements,
            dirty_set: &mut self.dirty_set,
            inactive_elements: &mut self.inactive_elements,
            pending_dependency_changes: &mut self.pending_dependency_changes,
            on_build_scheduled: self.on_build_scheduled.as_deref(),
            external_inbox: &self.external_inbox,
            external_request_frame: self.on_build_scheduled.as_ref(),
            // Lifecycle paths (mount/unmount/update) get no live-tree view;
            // only the `build_scope` drain sets `build_view`.
            build_view: None,
            child_manager_registry: &self.child_manager_registry,
            layout_builder_registry: &self.layout_builder_registry,
            async_driver: &self.async_driver,
            post_frame_handle: &self.post_frame_handle,
            text_input_handle: &self.text_input_handle,
            interaction_dispatch: &self.interaction_dispatch,
        }
    }

    /// Build an [`ExternalBuildScheduler`] over this owner's shared inbox and
    /// frame-request hook.
    ///
    /// The single construction point for the out-of-frame rebuild channel:
    /// [`ElementOwner::external_scheduler`](super::ElementOwner::external_scheduler)
    /// and `ElementBuildContext::rebuild_handle` both route through it, so there
    /// is exactly one inbox and one frame-request path.
    pub(crate) fn external_scheduler(&self) -> ExternalBuildScheduler {
        ExternalBuildScheduler::from_parts(
            Arc::clone(&self.external_inbox),
            self.on_build_scheduled.clone(),
        )
    }

    /// A [`RebuildHandle`](super::RebuildHandle) for `element`, scheduling
    /// through this owner's inbox.
    pub(crate) fn rebuild_handle(&self, element: ElementId) -> super::RebuildHandle {
        super::RebuildHandle::new(self.external_scheduler(), element)
    }

    /// Number of elements queued in the out-of-frame inbox, awaiting the next
    /// [`build_scope`](Self::build_scope) drain.
    ///
    /// Observability for the `RebuildHandle` channel: a
    /// `schedule()` from a worker thread is visible here before any frame runs.
    /// Returns a count, never a guard — the lock stays private (SP-6).
    #[must_use]
    pub fn pending_external_builds(&self) -> usize {
        self.external_inbox.lock().len()
    }

    /// Check if there are dirty elements.
    pub fn has_dirty_elements(&self) -> bool {
        !self.dirty_elements.is_empty()
    }

    /// Get the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }

    /// Process all dirty elements.
    ///
    /// Rebuilds elements in depth order (shallowest first). This ensures
    /// that when a parent rebuilds, any children that become dirty are
    /// processed after the parent.
    ///
    /// # Arguments
    ///
    /// * `tree` - The element tree to rebuild
    pub fn build_scope(&mut self, tree: &mut ElementTree) {
        #[cfg(debug_assertions)]
        {
            assert!(!self.building, "build_scope called while already building");
            self.building = true;
            self.scope_depth += 1;
        }

        // Drain elements scheduled from OUTSIDE a frame (animation / listenable
        // ticks whose mark-dirty callback holds an `ExternalBuildScheduler`).
        // Pushed straight onto the heap — we are already in a frame, so the
        // `on_build_scheduled` frame request the callback already fired is
        // enough; re-firing it here would loop. A tick landing mid-drain stays
        // in the inbox for the next frame (Flutter defers mid-frame schedules).
        //
        // The heap key is the element's TREE depth, looked up from its node
        // here (`&mut tree` is in scope) rather than captured in the callback —
        // `ElementCore::depth` is the sibling slot index, not `parent_depth+1`,
        // so capturing it would mis-order a nested animated element as if it
        // were the root.
        let externally_scheduled: Vec<ElementId> = self.external_inbox.lock().drain().collect();
        for id in externally_scheduled {
            // Mark dirty here, not in the caller. A `RebuildHandle`
            // carries no reference to the element's dirty flag — it is a plain
            // `(inbox, ElementId)` pair — so the drain is the one place that both
            // knows the id and holds `&mut tree`. Without this the element lands
            // on the heap but `perform_build` short-circuits on `!should_build()`,
            // and a `build_into_views` that returns no views would reconcile the
            // element's children away. Idempotent: `AnimatedView`'s mark-dirty
            // callback already set the flag, and a node that has since been
            // unmounted is a no-op lookup.
            tree.mark_needs_build(id);
            if self.dirty_set.insert(id) {
                let depth = tree.get(id).map_or(0, |node| node.depth);
                self.dirty_elements
                    .push(Reverse(DirtyElement::new(id, depth)));
            }
        }

        // Re-key every element already on the heap to its AUTHORITATIVE tree
        // depth before draining. `schedule_build_for` trusts the depth its
        // caller passes, but the `setState` path (`ElementCore::schedule_self_build`)
        // and the live `BuildCtx` both pass `ElementCore::depth` — the sibling
        // SLOT index, not `parent_depth + 1`. Trusting it lets a deeply-nested
        // `setState` sort as if it were shallow, so a child could build before
        // its parent and violate Flutter's shallowest-first build contract
        // (`framework.dart` `_dirtyElements.sort(Element._sort)` keys on the
        // element's real depth). Re-derive each id's depth from its node — the
        // same authority the external-inbox drain just above already uses.
        self.rekey_dirty_depths(tree);

        // Process dirty elements in depth order, extract-then-apply
        // (E3 — atomic box→arena swap).
        //
        // The hard problem: the old loop held `&mut tree.get_mut(id).element`
        // while calling `perform_build`, so `perform_build` could not also
        // take `&mut ElementTree` to reconcile slab-resident children —
        // the exact double-borrow that cost the render-tree PRs. The fix
        // is the same extract-then-apply discipline E2.5 proves out, lifted
        // to the build seam:
        //
        //   1. Take a `&mut element` borrowed FROM the tree, run the
        //      behavior's build half (`build_into_views`), capture the
        //      OWNED child views, and DROP the element borrow.
        //   2. With a FRESH `&mut tree` borrow, feed those views to the
        //      id-reconciler, which inserts / updates / removes the
        //      slab-resident child nodes.
        //
        // No `&mut` into the slab is ever live across a second slab access.
        //
        // Each iteration pops one entry first so `pop()`'s mutation of
        // `self.dirty_elements` (a field the split-borrow handle aliases)
        // is released before the handle is reborrowed.
        while let Some(Reverse(dirty)) = self.dirty_elements.pop() {
            let id = dirty.id();
            self.dirty_set.remove(&id);

            // Flutter parity (`framework.dart:5977-5982`): if this
            // dependent received an inherited-dependency change since its
            // last build, fire `ViewState::did_change_dependencies` BEFORE
            // the build. Consumed here so the typed hook runs exactly once
            // per dependency-change-then-rebuild cycle.
            let needs_did_change = self.pending_dependency_changes.remove(&id);

            // Guard (still holding only a brief `&mut node`): an
            // inherited-dependency change marks an otherwise-clean dependent
            // dirty so its build re-runs against the new value; then skip
            // unless the element is both buildable (lifecycle) AND dirty. A
            // clean element's build half returns an empty view list, and the
            // phase-2 reconcile would then wrongly REMOVE all its children —
            // so a clean entry must never reach reconcile.
            {
                let Some(node) = tree.get_mut(id) else {
                    // Stale / removed id — nothing to build.
                    continue;
                };
                if needs_did_change {
                    node.element_mut().mark_needs_build();
                }
                if !node.element().lifecycle().can_build() || !node.element().is_dirty() {
                    continue;
                }
            }

            // ── Phase 1: extract BY VALUE, build against a LIVE read view.
            // Taking the element out of its slot frees the tree for a shared
            // `&` borrow, so the element's `build()` can resolve InheritedView
            // / ancestor lookups against the REAL tree (via the `BuildCtx`
            // the behaviour builds from `ElementOwner::build_view`) — no empty
            // dummy, and no deadlock against the `Arc<RwLock>` write lock the
            // frame driver holds (the borrowed view sidesteps the lock).
            // Inherited dependents are buffered (the tree is read-only here)
            // and applied below once `&mut tree` is free again.
            let Some(mut element) = tree.take_element(id) else {
                continue;
            };
            let dep_sink: parking_lot::Mutex<Vec<crate::context::DependentRecord>> =
                parking_lot::Mutex::new(Vec::new());

            // Run the build half under `catch_unwind` so the extracted element
            // is ALWAYS restored to its slot, even on an unwind. The user
            // `build()` is already caught one level down (`build_or_recover`
            // substitutes an `ErrorView`), but the other user hooks reachable
            // in this window — `did_change_dependencies` (via
            // `notify_dependency_change`) and `init_state` (inside
            // `StatefulBehavior::build_into_views`) — are not. Without this
            // guard a panic in either would drop `element` and leave a
            // permanent `None` hole, turning every later
            // `element()`/`element_mut()` access on this node into an
            // `ELEMENT_PRESENT` panic. `AssertUnwindSafe` is sound because the
            // sole cross-unwind invariant — the slot is whole again — is
            // re-established by the unconditional `put_element` below.
            let build_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut element_owner = super::ElementOwner {
                    global_keys: &mut self.global_keys,
                    dirty_elements: &mut self.dirty_elements,
                    dirty_set: &mut self.dirty_set,
                    inactive_elements: &mut self.inactive_elements,
                    pending_dependency_changes: &mut self.pending_dependency_changes,
                    on_build_scheduled: self.on_build_scheduled.as_deref(),
                    external_inbox: &self.external_inbox,
                    external_request_frame: self.on_build_scheduled.as_ref(),
                    build_view: Some(super::BuildHandle {
                        tree: &*tree,
                        dep_sink: &dep_sink,
                    }),
                    child_manager_registry: &self.child_manager_registry,
                    layout_builder_registry: &self.layout_builder_registry,
                    async_driver: &self.async_driver,
                    post_frame_handle: &self.post_frame_handle,
                    text_input_handle: &self.text_input_handle,
                    interaction_dispatch: &self.interaction_dispatch,
                };
                if needs_did_change {
                    element
                        .element_mut()
                        .notify_dependency_change(&mut element_owner);
                }
                element.element_mut().build_into_views(&mut element_owner)
            })); // `element_owner` + its `&*tree` borrow drop here.

            // Restore the element BEFORE anything else — the slot must be whole
            // whether the build returned or unwound. With `&mut tree` free
            // again we then apply the dependents buffered during the read-only
            // build onto their provider nodes; recording in the SAME iteration
            // (before the next dirty pop) preserves Flutter's
            // record-before-notify ordering (`framework.dart:5086`).
            tree.put_element(id, element);

            let new_views: Vec<Box<dyn View>> = match build_outcome {
                Ok(views) => views,
                // Slot restored above; re-raise so the frame aborts exactly as
                // it did before (no behavior change beyond keeping the slab
                // consistent). Partial `dep_sink` records are intentionally
                // dropped — the build did not complete.
                Err(payload) => std::panic::resume_unwind(payload),
            };
            for record in dep_sink.into_inner() {
                if let Some(node) = tree.get_mut(record.provider)
                    && let Some(accessor) = node.element_mut().as_inherited_mut()
                {
                    accessor.record_dependent(record.dependent, record.depth);
                }
            }

            // ── Phase 2: reconcile the returned views against the node's
            // slab-resident children with a fresh `&mut tree`. Newly inserted
            // children are scheduled inside the reconciler so this same drain
            // loop reaches them.
            let mut element_owner = super::ElementOwner {
                global_keys: &mut self.global_keys,
                dirty_elements: &mut self.dirty_elements,
                dirty_set: &mut self.dirty_set,
                inactive_elements: &mut self.inactive_elements,
                pending_dependency_changes: &mut self.pending_dependency_changes,
                on_build_scheduled: self.on_build_scheduled.as_deref(),
                external_inbox: &self.external_inbox,
                external_request_frame: self.on_build_scheduled.as_ref(),
                build_view: None,
                child_manager_registry: &self.child_manager_registry,
                layout_builder_registry: &self.layout_builder_registry,
                async_driver: &self.async_driver,
                post_frame_handle: &self.post_frame_handle,
                text_input_handle: &self.text_input_handle,
                interaction_dispatch: &self.interaction_dispatch,
            };
            crate::tree::id_reconcile::reconcile_children_by_id(
                tree,
                id,
                &new_views,
                &mut element_owner,
            );
        }

        // The build drained: every render child has attached. Settle each
        // render parent's children into element-slot order (no-op unless an
        // insert flagged a possible drift), so a render sibling that attached
        // before a component-deferred sibling does not invert their layout.
        tree.reorder_render_children_after_build();

        #[cfg(debug_assertions)]
        {
            self.building = false;
            self.scope_depth -= 1;
        }
    }

    // ========================================================================
    // Lazy-sliver child manager service
    // ========================================================================

    /// Drive lazy-sliver child managers post-layout.
    ///
    /// Called by `HeadlessBinding::pump_frame` (and future production bindings)
    /// immediately **after** `PipelineOwner::run_frame` and the pipeline lock
    /// is released, so the render tree is quiescent and no `NodePtr` alias is
    /// live.
    ///
    /// # Ordering
    ///
    /// 1. Drain `PipelineOwner`'s `pending_child_requests` and
    ///    `pending_retain_bands` accumulated during the most recent layout pass.
    /// 2. Group by `RenderId`.
    /// 3. Clone each affected manager `Arc` out of the registry (releases the
    ///    registry lock before the service calls).
    /// 4. For each manager, call `ChildManager::service` with an inline
    ///    `ElementOwner` split-borrow — mirrors the pattern in `build_scope`'s
    ///    `catch_unwind` closure.
    /// 5. A second `build_scope` expands the newly-built children's subtrees
    ///    (the items' own views — e.g. `Padding(Text)` — need their own build
    ///    pass since `SparseChildren::ensure` only mounts the top-level node).
    /// 6. Mark each affected sliver as needing layout so the next frame
    ///    re-measures with the newly-present render nodes.
    /// 7. `finalize_tree` cleans up any evicted children pushed to inactive by
    ///    `retain_band` or `on_unmount`.
    ///
    /// # Production and headless bindings
    ///
    /// Called by both `HeadlessBinding::pump_frame` (step 6) and
    /// `AppBinding::draw_frame` (after `run_frame` drops the pipeline write-lock)
    /// — the two frame paths are now converged at this call site. Headless
    /// tests drive it directly via `HeadlessBinding`; a real window drives it via
    /// `WidgetsBinding::service_child_requests`, which `AppBinding` invokes after
    /// each `run_frame`.
    pub fn service_child_requests(
        &mut self,
        tree: &mut ElementTree,
        pipeline: &Arc<RwLock<flui_rendering::pipeline::PipelineOwner>>,
    ) {
        // 1. Drain pending buffers from the pipeline (under a brief write lock).
        let (pending_requests, retain_bands) = {
            let mut guard = pipeline.write();
            let requests = guard.take_pending_child_requests();
            let bands = guard.take_pending_retain_bands();
            (requests, bands)
        };

        // Finalize BEFORE the early-return: `build_scope` does not call
        // `finalize_tree`, so sparse children pushed to `inactive_elements` by
        // `SliverListAdaptorBehavior::on_unmount` (F3) — during a reconcile that
        // removed their host — must be cleaned up here. Without this, those
        // elements and their render nodes are leaked until the next
        // `service_child_requests` call that has pending layout requests.
        if !self.inactive_elements.is_empty() {
            self.finalize_tree(tree);
        }

        if pending_requests.is_empty() && retain_bands.is_empty() {
            return;
        }

        tracing::debug!(
            requests = pending_requests.len(),
            bands = retain_bands.len(),
            "service_child_requests: draining lazy-sliver pending buffers"
        );

        // 2. Group requests and retain-bands by sliver RenderId.
        let mut requests_by_sliver: HashMap<RenderId, Vec<usize>> = HashMap::new();
        for (sliver_id, logical_index) in pending_requests {
            requests_by_sliver
                .entry(sliver_id)
                .or_default()
                .push(logical_index);
        }
        let mut bands_by_sliver: HashMap<RenderId, (usize, usize)> = HashMap::new();
        for (sliver_id, first, last) in retain_bands {
            bands_by_sliver.insert(sliver_id, (first, last));
        }

        // Collect all affected sliver ids (may have requests, bands, or both).
        let affected_ids: Vec<RenderId> = {
            let mut ids: HashSet<RenderId> = requests_by_sliver.keys().copied().collect();
            ids.extend(bands_by_sliver.keys().copied());
            ids.into_iter().collect()
        };

        // 3. Clone manager Arcs out of the registry before any service call.
        //    This releases the registry lock so that service calls that call
        //    `register_child_manager` / `unregister_child_manager` through an
        //    `ElementOwner` can re-enter the registry without deadlocking.
        let manager_arcs: Vec<(RenderId, Arc<Mutex<dyn ChildManager>>)> = {
            let registry = self.child_manager_registry.lock();
            affected_ids
                .iter()
                .filter_map(|&id| registry.get(&id).map(|m| (id, Arc::clone(m))))
                .collect()
        };

        if manager_arcs.is_empty() {
            tracing::debug!("service_child_requests: no registered managers for affected slivers");
            return;
        }

        // 4. Call service on each manager. Each iteration builds an inline
        //    ElementOwner split-borrow, calls service (which may call
        //    `ensure`/`evict` and mutate the tree + dirty heap), then drops
        //    the inline owner — the borrow ends before the next iteration.
        //
        //    Track whether any manager did real work (built or evicted a child).
        //    When all managers return `false` (settled state — band unchanged,
        //    no new requests in-band) we skip `mark_needs_layout` so the sliver
        //    stays clean and the frame quiesces rather than looping forever.
        let mut any_service_did_work = false;
        for (sliver_id, manager_arc) in &manager_arcs {
            let requested = requests_by_sliver
                .get(sliver_id)
                .map_or(&[][..], Vec::as_slice);
            let (retain_first, retain_last) = bands_by_sliver
                .get(sliver_id)
                .copied()
                .unwrap_or((0, usize::MAX));

            // Inline split-borrow (same pattern as `build_scope` catch_unwind).
            let mut inline_owner = super::ElementOwner {
                global_keys: &mut self.global_keys,
                dirty_elements: &mut self.dirty_elements,
                dirty_set: &mut self.dirty_set,
                inactive_elements: &mut self.inactive_elements,
                pending_dependency_changes: &mut self.pending_dependency_changes,
                on_build_scheduled: self.on_build_scheduled.as_deref(),
                external_inbox: &self.external_inbox,
                external_request_frame: self.on_build_scheduled.as_ref(),
                build_view: None,
                child_manager_registry: &self.child_manager_registry,
                layout_builder_registry: &self.layout_builder_registry,
                async_driver: &self.async_driver,
                post_frame_handle: &self.post_frame_handle,
                text_input_handle: &self.text_input_handle,
                interaction_dispatch: &self.interaction_dispatch,
            };

            let did_work = manager_arc.lock().service(
                requested,
                retain_first,
                retain_last,
                tree,
                &mut inline_owner,
                pipeline,
            );
            if did_work {
                any_service_did_work = true;
            }
        } // `inline_owner` drops here — all `&mut` borrows released.

        // 5. Second build_scope: expand newly-built children's subtrees.
        //    `SparseChildren::ensure` mounts the top-level lazy-child node and
        //    pushes it onto the dirty heap (F1), but the child's own sub-views
        //    (e.g. a Padding wrapping a Text) need a dedicated build pass.
        self.build_scope(tree);

        // 6. Mark each serviced sliver as needing re-layout ONLY if service
        //    did real work (built or evicted children). When all service calls
        //    returned `false`, the list has settled: the viewport, band, and
        //    built-child set are all unchanged. Skipping `mark_needs_layout`
        //    keeps the sliver clean so `flush_layout` skips it, `perform_layout`
        //    does not run, no new `emit_retain_band` fires, and
        //    `service_child_requests` early-returns on the next frame.
        //    This breaks the unconditional dirty-loop that prevented quiescence.
        if any_service_did_work {
            let mut guard = pipeline.write();
            for (sliver_id, _) in &manager_arcs {
                guard.mark_needs_layout(*sliver_id);
            }
        } else {
            tracing::debug!(
                "service_child_requests: no work done, skipping mark_needs_layout \
                 (frame quiesces)"
            );
        }

        // 7. Finalize: unmount evicted children (pushed to inactive by
        //    `retain_band` → `evict` → `tree.remove_subtree`) and the lazy
        //    children pushed by `on_unmount` (F3).
        self.finalize_tree(tree);
    }

    // ========================================================================
    // Inactive Elements (for finalization)
    // ========================================================================

    /// Add an element to the inactive list.
    ///
    /// Called when an element is deactivated (e.g., its parent rebuilds without
    /// it). The element will be unmounted in `finalize_tree()`.
    pub fn add_to_inactive(&mut self, id: ElementId, depth: usize) {
        self.inactive_elements.push(InactiveElement::new(id, depth));
    }

    /// Remove an element from the inactive list.
    ///
    /// Called when an element is reactivated (e.g., moved via GlobalKey).
    pub fn remove_from_inactive(&mut self, id: ElementId) {
        self.inactive_elements.retain(|e| e.id() != id);
    }

    /// Check if there are inactive elements pending unmount.
    pub fn has_inactive_elements(&self) -> bool {
        !self.inactive_elements.is_empty()
    }

    /// Complete the element build pass by unmounting inactive elements.
    ///
    /// This is called by `WidgetsBinding.draw_frame()` after `build_scope()`
    /// and `super.draw_frame()` (layout/paint).
    ///
    /// Elements are unmounted in reverse depth order (deepest first) to ensure
    /// children are unmounted before parents.
    pub fn finalize_tree(&mut self, tree: &mut ElementTree) {
        if self.inactive_elements.is_empty() {
            return;
        }

        tracing::debug!(
            count = self.inactive_elements.len(),
            "Finalizing tree - unmounting inactive elements"
        );

        // Sort by depth (deepest first for unmounting)
        self.inactive_elements
            .sort_by_key(|entry| std::cmp::Reverse(entry.depth()));

        // Take ownership of inactive elements to avoid borrow conflicts.
        // `mem::take` snapshots the queue before the unmount sweep so
        // mid-iteration `ElementOwner::push_inactive` calls (e.g. children
        // deactivating as a parent unmounts) land in the *next* frame's
        // queue rather than re-entering this drain — same snapshot-then-fire
        // discipline as `ChangeNotifier::notify_listeners` (foundation
        // notifier.rs:158-163).
        let inactive_elements: Vec<_> = std::mem::take(&mut self.inactive_elements);

        // Collect all elements to unmount (including children)
        let mut elements_to_unmount = Vec::new();
        for inactive in &inactive_elements {
            Self::collect_elements_to_unmount(tree, inactive.id(), &mut elements_to_unmount);
        }

        // Build the split-borrow handle once for the entire unmount sweep.
        // The handle survives `tree.get_mut` borrows because it points into
        // disjoint `BuildOwner` fields. No live build runs here, so the
        // build-time tree handle is absent.
        let mut element_owner = super::ElementOwner {
            global_keys: &mut self.global_keys,
            dirty_elements: &mut self.dirty_elements,
            dirty_set: &mut self.dirty_set,
            inactive_elements: &mut self.inactive_elements,
            pending_dependency_changes: &mut self.pending_dependency_changes,
            on_build_scheduled: self.on_build_scheduled.as_deref(),
            external_inbox: &self.external_inbox,
            external_request_frame: self.on_build_scheduled.as_ref(),
            build_view: None,
            child_manager_registry: &self.child_manager_registry,
            layout_builder_registry: &self.layout_builder_registry,
            async_driver: &self.async_driver,
            post_frame_handle: &self.post_frame_handle,
            text_input_handle: &self.text_input_handle,
            interaction_dispatch: &self.interaction_dispatch,
        };

        // Finalize all elements (deepest first - already sorted by collect order).
        //
        // `remove_finalized` bypasses the soft-remove
        // path that `remove` takes for keyed elements. At this point
        // we've already given mid-frame state migration its chance —
        // anything still in the inactive queue is genuinely going away,
        // so we slab-remove + unregister the GlobalKey directly.
        for id in elements_to_unmount.iter().rev() {
            tree.remove_finalized(*id, &mut element_owner);
        }

        tracing::debug!("Finalize tree complete");
    }

    /// Iteratively collect all element IDs to unmount, parent before
    /// children (pre-order DFS, children in
    /// [`ElementNode::child_ids`](crate::tree::ElementNode) slot order).
    ///
    /// E3: children come from the slab-resident `child_ids` list — the
    /// single element graph. `finalize_tree` reverses the collected order
    /// so `remove_finalized` runs deepest-first; pre-order guarantees a
    /// parent always precedes every one of its descendants, so the
    /// reversed sweep never frees a parent slot before its children.
    ///
    /// The walk is driven by an explicit `Vec` work-stack instead of
    /// recursion: the element tree nests several times deeper than the
    /// render tree, and a recursive shape overflowed the 1 MiB Windows
    /// main-thread stack on deep chains (the failure class PR #177
    /// closed for the render-tree walks). To preserve the recursive
    /// shape's visit order on a LIFO stack, children are pushed in
    /// reverse slot order so the leftmost child is popped next — same
    /// discipline as `WidgetsBinding::collect_all_elements`.
    ///
    /// Complexity: O(n) time over the n reachable nodes, average and
    /// worst case (each node pushed/popped exactly once); the work-stack
    /// peaks at O(n) heap in the degenerate all-siblings case and O(tree
    /// height) for a chain. Call-stack usage is constant.
    fn collect_elements_to_unmount(tree: &ElementTree, id: ElementId, result: &mut Vec<ElementId>) {
        let mut stack: Vec<ElementId> = vec![id];
        while let Some(id) = stack.pop() {
            result.push(id);
            // The `tree.get` shared borrow ends with the statement; the
            // extend writes only into the local stack, never the slab.
            if let Some(node) = tree.get(id) {
                stack.extend(node.child_ids().iter().rev().copied());
            }
        }
    }

    /// Lock the build scope (for debugging).
    ///
    /// Returns a guard that unlocks when dropped.
    #[cfg(debug_assertions)]
    pub fn lock_build_scope(&mut self) -> BuildScopeGuard<'_> {
        assert!(!self.building, "Already in build scope");
        self.building = true;
        BuildScopeGuard { owner: self }
    }

    // ========================================================================
    // GlobalKey Registry
    // ========================================================================

    /// Register a GlobalKey for an element.
    ///
    /// GlobalKeys allow elements to be found and reparented across the tree.
    pub fn register_global_key(&mut self, key_hash: u64, element: ElementId) {
        self.global_keys.insert(key_hash, element);
    }

    /// Unregister a GlobalKey.
    pub fn unregister_global_key(&mut self, key_hash: u64) {
        self.global_keys.remove(&key_hash);
    }

    /// Look up an element by GlobalKey.
    pub fn element_for_global_key(&self, key_hash: u64) -> Option<ElementId> {
        self.global_keys.get(&key_hash).copied()
    }

    /// Atomically remove and return the element registered under
    /// `key_hash` for a reparent operation.
    ///
    /// Closes the race window that a
    /// two-call sequence (`element_for_global_key` followed by
    /// `unregister_global_key`) would leave open if any other code
    /// path mutates the registry between the two calls — a real
    /// risk in Phase 2 testing infrastructure where multiple
    /// parents may rebuild concurrently in test fixtures.
    ///
    /// The caller (the keyed reconciler's middle-walk) consults
    /// this method on an unmatched-by-position keyed view whose
    /// `is_global_key()` is true; on `Some`, it claims the element
    /// for the new parent. Returning `Some` AND removing the entry
    /// in one operation guarantees a second concurrent claim of
    /// the same key sees `None`, not a stale id.
    ///
    /// Re-registering at the new parent is the caller's
    /// responsibility — typically through the standard
    /// [`Self::register_global_key`] path after the element is
    /// re-attached to its new slot.
    pub fn take_global_key_for_reparent(&mut self, key_hash: u64) -> Option<ElementId> {
        self.global_keys.remove(&key_hash)
    }

    /// Number of `GlobalKey`s currently registered.
    ///
    /// Test surface — production code reads
    /// [`BuildOwner::element_for_global_key`] on a single hash rather
    /// than scanning size. Tests use this to confirm the registry
    /// stays at the expected size across mount / unmount cycles.
    pub fn global_keys_len(&self) -> usize {
        self.global_keys.len()
    }

    /// Check if we're currently building.
    #[cfg(debug_assertions)]
    pub fn is_building(&self) -> bool {
        self.building
    }

    /// Get the current scope depth.
    #[cfg(debug_assertions)]
    pub fn scope_depth(&self) -> usize {
        self.scope_depth
    }
}

impl std::fmt::Debug for BuildOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOwner")
            .field("dirty_count", &self.dirty_elements.len())
            .field("global_keys", &self.global_keys.len())
            .finish_non_exhaustive()
    }
}

/// Guard for build scope (debug only).
#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct BuildScopeGuard<'a> {
    owner: &'a mut BuildOwner,
}

#[cfg(debug_assertions)]
impl Drop for BuildScopeGuard<'_> {
    fn drop(&mut self) {
        self.owner.building = false;
    }
}

#[cfg(test)]
mod tests {
    use flui_objects::RenderSizedBox;
    use flui_rendering::protocol::BoxProtocol;

    use super::*;
    use crate::{View, tree::ElementTree};

    /// A render-family leaf view with no child views.
    #[derive(Clone)]
    struct TestView;

    impl crate::RenderView for TestView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) {
        }
    }

    impl View for TestView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    #[test]
    fn test_build_owner_creation() {
        let owner = BuildOwner::new();
        assert!(!owner.has_dirty_elements());
        assert_eq!(owner.dirty_count(), 0);
    }

    #[test]
    fn test_schedule_build() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(1);

        owner.schedule_build_for(id, 0);
        assert!(owner.has_dirty_elements());
        assert_eq!(owner.dirty_count(), 1);

        // Duplicate scheduling should not increase count
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_build_scope() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();

        let view = TestView;
        let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

        owner.schedule_build_for(root_id, 0);
        assert!(owner.has_dirty_elements());

        owner.build_scope(&mut tree);
        assert!(!owner.has_dirty_elements());
    }

    #[test]
    fn test_depth_ordering() {
        let mut owner = BuildOwner::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        // Schedule in reverse depth order
        owner.schedule_build_for(id3, 2);
        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);

        // Should process shallowest first
        let Reverse(first) = owner.dirty_elements.pop().unwrap();
        assert_eq!(first.depth(), 0);

        let Reverse(second) = owner.dirty_elements.pop().unwrap();
        assert_eq!(second.depth(), 1);

        let Reverse(third) = owner.dirty_elements.pop().unwrap();
        assert_eq!(third.depth(), 2);
    }

    /// A `setState` hands `schedule_build_for` the element's SLOT index, not its
    /// tree depth. `rekey_dirty_depths` (run at the top of `build_scope`) must
    /// override that with each node's authoritative `parent_depth + 1` so a
    /// deeply-nested rebuild never drains before its shallower parent.
    ///
    /// This is RED without the re-key: the elements are scheduled with
    /// deliberately INVERTED depths (the deepest leaf gets `0`, the root gets
    /// `2`), so trusting the scheduled depth would drain the leaf first —
    /// violating Flutter's shallowest-first contract.
    #[test]
    fn rekey_dirty_depths_restores_shallowest_first_from_inverted_slots() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let view = TestView;

        // A single-child chain: root (depth 0) → mid (depth 1) → leaf (depth 2).
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());
        let mid = tree.insert(&view, root, 0, &mut owner.element_owner_mut());
        let leaf = tree.insert(&view, mid, 0, &mut owner.element_owner_mut());
        assert_eq!(tree.get(root).map(|n| n.depth), Some(0));
        assert_eq!(tree.get(mid).map(|n| n.depth), Some(1));
        assert_eq!(tree.get(leaf).map(|n| n.depth), Some(2));

        // Schedule with INVERTED depths — what a `setState` on each would pass if
        // it trusted the slot index (all three are slot 0 here; we exaggerate to
        // an outright inversion to make the mis-order deterministic).
        owner.schedule_build_for(leaf, 0);
        owner.schedule_build_for(mid, 1);
        owner.schedule_build_for(root, 2);

        owner.rekey_dirty_depths(&tree);

        // Drains shallowest-first by AUTHORITATIVE tree depth, not the scheduled
        // (inverted) depth.
        let Reverse(first) = owner.dirty_elements.pop().unwrap();
        let Reverse(second) = owner.dirty_elements.pop().unwrap();
        let Reverse(third) = owner.dirty_elements.pop().unwrap();
        assert_eq!(first.id(), root, "root (tree depth 0) drains first");
        assert_eq!(second.id(), mid, "mid (tree depth 1) drains second");
        assert_eq!(third.id(), leaf, "leaf (tree depth 2) drains last");
        assert_eq!((first.depth(), second.depth(), third.depth()), (0, 1, 2));
    }

    #[test]
    fn test_global_key_registry() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(42);
        let key_hash = 12345u64;

        owner.register_global_key(key_hash, id);
        assert_eq!(owner.element_for_global_key(key_hash), Some(id));

        owner.unregister_global_key(key_hash);
        assert_eq!(owner.element_for_global_key(key_hash), None);
    }

    /// `take_global_key_for_reparent` returns
    /// the registered id AND removes it atomically. A second call for
    /// the same hash returns `None` — proving the second of two
    /// concurrent reparent claims (the rare same-frame collision)
    /// cannot stale-read.
    #[test]
    fn test_take_global_key_for_reparent_is_atomic() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(7);
        let hash = 0x00C0_FFEE_u64;

        owner.register_global_key(hash, id);

        // First caller wins.
        assert_eq!(owner.take_global_key_for_reparent(hash), Some(id));

        // Second caller sees None — the entry was removed atomically.
        assert_eq!(owner.take_global_key_for_reparent(hash), None);
        assert_eq!(owner.element_for_global_key(hash), None);
    }

    /// Deep-tree stack-safety: `finalize_tree`'s subtree collection must
    /// survive an element chain far deeper than the fixed OS stack would
    /// allow with plain recursion. The element tree nests several times
    /// deeper than the render tree (every render object is wrapped in
    /// multiple composition views), so it hits the 1 MiB Windows
    /// main-thread stack earlier — same failure class PR #177 closed in
    /// flui-rendering. The collection frame is small, so the depth is
    /// 20 000 (the small-frame sizing the flui-rendering
    /// compositing-bits test established; 2 500 survived unprotected
    /// there by luck).
    ///
    /// Ignored under miri: the interpreter cannot finish a 20 000-level
    /// walk in reasonable time; the shallow finalize-path coverage in
    /// this module exercises the same code natively.
    #[test]
    #[cfg_attr(miri, ignore = "20k-node walk too slow for the interpreter")]
    fn finalize_tree_survives_deep_chain() {
        const DEPTH: usize = 20_000;

        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();

        let view = TestView;
        let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

        // Build a root → c1 → c2 → … single-child chain. `insert` wires
        // the child's parent edge; the parent's `child_ids` list (what
        // the unmount collection walks) is stamped explicitly.
        let mut parent_id = root_id;
        for _ in 1..DEPTH {
            let child_id = tree.insert(&view, parent_id, 0, &mut owner.element_owner_mut());
            tree.get_mut(parent_id)
                .expect("freshly inserted parent resolves")
                .set_child_ids(vec![child_id]);
            parent_id = child_id;
        }
        assert_eq!(tree.len(), DEPTH);

        // Park the chain root in the inactive queue and finalize — the
        // collection must reach all 20 000 chain nodes (the root plus
        // its 19 999 descendants) without exhausting the stack, then
        // tear them down deepest-first.
        owner.add_to_inactive(root_id, 0);
        owner.finalize_tree(&mut tree);

        assert_eq!(
            tree.len(),
            0,
            "every chain node must be collected and unmounted"
        );
    }

    /// `take_global_key_for_reparent` on an unknown hash returns
    /// `None` without side effects.
    #[test]
    fn test_take_global_key_for_reparent_unknown_hash() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(7);
        let known = 1_u64;
        let unknown = 99_u64;

        owner.register_global_key(known, id);
        assert_eq!(owner.take_global_key_for_reparent(unknown), None);
        // Known mapping unaffected by the failed claim on a different
        // hash.
        assert_eq!(owner.element_for_global_key(known), Some(id));
    }
}
