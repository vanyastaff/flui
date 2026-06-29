//! `SliverListAdaptorElement` — element-tree backend for `RenderSliverList`.
//!
//! # What this is
//!
//! Flutter's `SliverMultiBoxAdaptorElement` is the element responsible for
//! lazily building and disposing the children of a `RenderSliverMultiBoxAdaptor`
//! (and its subclass `RenderSliverList`). FLUI splits this responsibility in
//! two crates:
//!
//! - **Render half** (`flui-objects`): `RenderSliverList` — emits build
//!   requests via `SliverLayoutContext::request_child_build` for absent slots,
//!   and emits `emit_retain_band` for eviction.
//! - **Element half** (this module): `SliverListAdaptorElement` — registered
//!   as a `ChildManager` in `BuildOwner`; receives the post-layout requests
//!   and retain-bands via `service_child_requests` and drives `SparseChildren`
//!   to build or evict lazy children.
//!
//! # Lifecycle
//!
//! 1. **mount**: `SliverListAdaptorBehavior::on_mount` creates the
//!    `RenderSliverList` (via the inner `RenderBehavior`) and then registers
//!    `Arc::clone(&self.manager)` in `BuildOwner::child_manager_registry` keyed
//!    by the sliver's `RenderId`. Registration happens in the adaptor's own
//!    `on_mount`, NOT in the generic `behavior.rs:789` site — F8 in the plan.
//! 2. **service**: `BuildOwner::service_child_requests` drains the
//!    `PipelineOwner`'s pending buffers, groups by `RenderId`, and calls
//!    `SliverListAdaptorManager::service` — which evicts out-of-band children
//!    via `SparseChildren::retain_band` and builds new ones via
//!    `SparseChildren::ensure`.
//! 3. **unmount**: `SliverListAdaptorBehavior::on_unmount` pushes all live
//!    sparse children to `owner.push_inactive` (F3), then unregisters the
//!    manager, then removes the render object. `finalize_tree` finds the lazy
//!    children' descendants via each sparse child's own `child_ids`.
//!
//! # F4 invariant — host `child_ids` stays empty
//!
//! `build_into_views` returns an empty `Vec` so the dense reconciler in
//! `build_scope` never touches the lazy children. The lazy children live only
//! in `SparseChildren::by_logical_index`; they are managed solely by
//! `service_child_requests`.

use std::sync::Arc;

use flui_foundation::{ElementId, RenderId};
use flui_objects::RenderSliverList;
use flui_rendering::{pipeline::PipelineOwner, protocol::SliverProtocol};
use parking_lot::{Mutex, RwLock};

use super::{
    Variable,
    behavior::{ElementBehavior, RenderBehavior},
    child_manager::ChildManager,
    generic::ElementCore,
    sparse_children::SparseChildren,
    unified::Element,
};
use crate::{
    BoxedView, ElementOwner,
    tree::ElementTree,
    view::{ElementBase, RenderView, View},
};

// ============================================================================
// VIEW CONFIG
// ============================================================================

/// View configuration for a lazy-sliver adaptor element.
///
/// Holds the item count, per-item extent estimate, and the item builder.
/// The element this view creates wraps [`RenderSliverList`] (the render half)
/// and owns a `SliverListAdaptorManager` that services
/// `ChildManager::service` calls post-layout.
///
/// # F4 invariant
///
/// [`has_children`](Self::has_children) returns `false` so
/// `build_into_views` returns an empty `Vec`. The dense reconciler must
/// never touch the lazy children — they are managed by `SparseChildren`
/// via `BuildOwner::service_child_requests`.
#[derive(Clone)]
pub struct SliverList {
    /// Total number of items in the data source.
    pub(crate) item_count: usize,
    /// Default per-item extent (logical pixels), used to seed the virtualizer
    /// until real measurements arrive from laid-out children.
    pub(crate) item_extent_estimate: f32,
    /// Given a logical index, produces the item's view. Returns `None` when
    /// the index is past the end of the data source.
    pub(crate) builder: Arc<dyn Fn(usize) -> Option<BoxedView> + Send + Sync>,
}

impl SliverList {
    /// Construct a new lazy-sliver adaptor view configuration.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent_estimate` is not finite and positive — a zero or
    /// negative estimate seeds the virtualizer with an invalid band width.
    pub fn new(
        item_count: usize,
        item_extent_estimate: f32,
        builder: Arc<dyn Fn(usize) -> Option<BoxedView> + Send + Sync>,
    ) -> Self {
        assert!(
            item_extent_estimate.is_finite() && item_extent_estimate > 0.0,
            "item_extent_estimate must be finite and positive, got {item_extent_estimate}",
        );
        Self {
            item_count,
            item_extent_estimate,
            builder,
        }
    }
}

impl std::fmt::Debug for SliverList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverList")
            .field("item_count", &self.item_count)
            .field("item_extent_estimate", &self.item_extent_estimate)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// RenderView impl
// ============================================================================

impl RenderView for SliverList {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverList;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverList::new(self.item_count, self.item_extent_estimate)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_item_count(self.item_count);
    }

    /// F4 invariant: no dense children — the dense reconciler must not touch
    /// lazy children.
    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {
        // F4: no dense children to visit.
    }
}

// ============================================================================
// View impl — creates a SliverListAdaptorElement with the custom behavior
// ============================================================================

impl View for SliverList {
    fn create_element(&self) -> Box<dyn ElementBase> {
        // Creates the adaptor element with the custom behavior instead of the
        // generic `RenderBehavior::new()` produced by `impl_render_view!`.
        // This is required so on_mount registers the ChildManager — which the
        // generic RenderBehavior does not do (F8).
        Box::new(SliverListAdaptorElement::new(
            self,
            SliverListAdaptorBehavior::new(self),
        ))
    }
}

// ============================================================================
// MANAGER
// ============================================================================

/// The `ChildManager` implementation for one live lazy-sliver adaptor element.
///
/// Holds the `SparseChildren` bookkeeping, the host element id, and the item
/// builder. Called by `BuildOwner::service_child_requests` after each layout
/// pass; not reachable from any other path (single-threaded call site).
pub(crate) struct SliverListAdaptorManager {
    /// Sparse logical-index → ElementId map for built children.
    sparse_children: SparseChildren,
    /// The element id of the adaptor host element. `None` until `on_mount`
    /// stamps it; the host is always mounted before `service` runs.
    host_element_id: Option<ElementId>,
    /// Item factory. `Arc` so it's shared with `SliverList` and the
    /// behavior without cloning the closure.
    builder: Arc<dyn Fn(usize) -> Option<BoxedView> + Send + Sync>,
}

impl std::fmt::Debug for SliverListAdaptorManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverListAdaptorManager")
            .field("built_children", &self.sparse_children.len())
            .field("host_element_id", &self.host_element_id)
            .finish_non_exhaustive()
    }
}

impl ChildManager for SliverListAdaptorManager {
    fn service(
        &mut self,
        requested_indices: &[usize],
        retain_first: usize,
        retain_last: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &Arc<RwLock<PipelineOwner>>,
    ) -> bool {
        let Some(host) = self.host_element_id else {
            // service called before mount: programming-contract violation;
            // warn loudly but do not panic (production robustness).
            tracing::warn!(
                "SliverListAdaptorManager::service called before host element was mounted"
            );
            return false;
        };

        // Evict out-of-band children FIRST so the retain-band contract is
        // satisfied before we try to build new children. An index that falls
        // outside the band and was also requested (rare edge case from a
        // mid-scroll jump) is correctly evicted then not rebuilt.
        let retain_did_work =
            self.sparse_children
                .retain_band(retain_first, retain_last, tree, owner);

        // Build each requested index that is (a) within the retain band and
        // (b) not already built. We check first to avoid calling the builder
        // for already-present indices (idempotency without closure overhead)
        // and to accurately track whether any new child was mounted.
        let mut any_new_build = false;
        for &logical_index in requested_indices {
            if logical_index < retain_first || logical_index >= retain_last {
                // Fell outside the band we just retained — skip.
                continue;
            }
            if self.sparse_children.get(logical_index).is_some() {
                // Already built — no work needed.
                continue;
            }
            if let Some(view) = (self.builder)(logical_index) {
                self.sparse_children.ensure(
                    logical_index,
                    view.0.as_ref(),
                    host,
                    tree,
                    owner,
                    pipeline,
                );
                any_new_build = true;
            }
        }

        retain_did_work || any_new_build
    }
}

// ============================================================================
// BEHAVIOR
// ============================================================================

/// `ElementBehavior` for the lazy-sliver adaptor element.
///
/// Wraps [`RenderBehavior<SliverList>`] (which handles render-object
/// creation and removal) and additionally:
/// - **mount**: stamps `host_element_id` on the manager and registers it in
///   `BuildOwner::child_manager_registry` keyed by the sliver's `RenderId`.
/// - **unmount**: pushes live sparse children to the inactive queue (F3) and
///   unregisters from the registry.
///
/// Registration happens in the adaptor's own `on_mount`, not in the generic
/// `behavior.rs:789` site — F8 in the approved plan.
pub(crate) struct SliverListAdaptorBehavior {
    /// Handles `RenderSliverList` creation / update / removal.
    inner: RenderBehavior<SliverList>,
    /// Shared manager; Arc lets `on_mount` insert a clone into the registry
    /// without moving out of `self`.
    manager: Arc<Mutex<SliverListAdaptorManager>>,
}

impl std::fmt::Debug for SliverListAdaptorBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverListAdaptorBehavior")
            .field("render_id", &self.inner.render_id)
            .field("manager", &*self.manager.lock())
            .finish()
    }
}

impl SliverListAdaptorBehavior {
    fn new(view: &SliverList) -> Self {
        Self {
            inner: RenderBehavior::new(),
            manager: Arc::new(Mutex::new(SliverListAdaptorManager {
                sparse_children: SparseChildren::new(),
                host_element_id: None,
                builder: Arc::clone(&view.builder),
            })),
        }
    }
}

impl ElementBehavior<SliverList, Variable> for SliverListAdaptorBehavior
where
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<SliverProtocol>>>,
{
    fn debug_kind(&self) -> &'static str {
        "SliverListAdaptorElement"
    }

    /// F4: returns empty — the dense reconciler must not touch lazy children.
    ///
    /// The inner `RenderBehavior::build_into_views` also returns empty because
    /// `SliverList::has_children() = false`; we forward for the
    /// `should_build` guard and `clear_dirty` side effect.
    fn build_into_views(
        &mut self,
        core: &mut ElementCore<SliverList, Variable>,
        owner: &mut ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        self.inner.build_into_views(core, owner)
    }

    /// Creates the `RenderSliverList`, registers the manager, and stamps
    /// `host_element_id` on the manager for later `service` calls.
    fn on_mount(
        &mut self,
        core: &mut ElementCore<SliverList, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        // Step 1: create the render object via the inner RenderBehavior.
        self.inner.on_mount(core, owner);

        // Step 2: stamp the host element id on the manager now that the element
        // is slab-stamped (set_self_id fires before on_mount in ElementTree::insert).
        if let Some(self_id) = core.self_id() {
            self.manager.lock().host_element_id = Some(self_id);
        } else {
            tracing::warn!(
                "SliverListAdaptorBehavior::on_mount: no self_id stamped — \
                 ChildManager service will be a no-op"
            );
        }

        // Step 3: register the manager keyed by the sliver's RenderId.
        // F8 — this registration belongs here, NOT in generic behavior.rs:789.
        match self.inner.render_id {
            Some(render_id) => {
                owner.register_child_manager(
                    render_id,
                    Arc::clone(&self.manager) as Arc<Mutex<dyn ChildManager + Send>>,
                );
                tracing::debug!(
                    ?render_id,
                    "SliverListAdaptorBehavior: registered child manager"
                );
            }
            None => {
                // Happens when there is no PipelineOwner in scope (e.g. in
                // a pure-element test). `service_child_requests` will find no
                // entry for this sliver and skip it gracefully.
                tracing::warn!(
                    "SliverListAdaptorBehavior::on_mount: no render_id yet (no PipelineOwner) — \
                     child manager not registered"
                );
            }
        }
    }

    /// Pushes live sparse children to the inactive queue (F3), unregisters the
    /// manager, and removes the render object.
    fn on_unmount(
        &mut self,
        core: &mut ElementCore<SliverList, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        // F3: host.child_ids is empty (F4 invariant), so `finalize_tree`'s
        // `collect_elements_to_unmount` cannot reach the lazy children via the
        // normal dense walk. Push each sparse child to the inactive queue at
        // a sentinel depth so `finalize_tree` unmounts them and recurses into
        // their own `child_ids` for descendants.
        //
        // Sentinel depth=1: an approximation. `finalize_tree` sorts deepest-
        // first; using 1 means lazy children appear near the top of the order.
        // This is safe because each sparse child is an independent subtree; the
        // only ordering contract finalize_tree has is parent-before-children
        // WITHIN a single subtree, which `collect_elements_to_unmount` already
        // enforces via pre-order + reverse-sweep.
        {
            let manager = self.manager.lock();
            for (_logical_index, child_id) in manager.sparse_children.iter_built() {
                owner.push_inactive(child_id, 1);
            }
        }

        // Unregister from the child-manager registry so no future
        // `service_child_requests` call hits a stale entry.
        if let Some(render_id) = self.inner.render_id {
            owner.unregister_child_manager(render_id);
            tracing::debug!(
                ?render_id,
                "SliverListAdaptorBehavior: unregistered child manager"
            );
        }

        // Remove the render object via the inner behavior.
        self.inner.on_unmount(core, owner);
    }

    fn on_update(&mut self, core: &ElementCore<SliverList, Variable>) {
        self.inner.on_update(core);
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<SliverList, Variable>,
        old_view: &SliverList,
        owner: &mut ElementOwner<'_>,
    ) {
        self.inner.on_view_updated(core, old_view, owner);
    }

    fn render_id(&self) -> Option<RenderId> {
        self.inner.render_id()
    }
}

// ============================================================================
// TYPE ALIAS
// ============================================================================

/// Element type for the lazy-sliver adaptor.
///
/// Wraps [`RenderSliverList`] (via `SliverListAdaptorBehavior`) and owns
/// a `SliverListAdaptorManager` registered in `BuildOwner`'s
/// `child_manager_registry`. Post-layout, `BuildOwner::service_child_requests`
/// drives the manager to build or evict lazy children.
///
/// External consumers create adaptor elements through
/// [`SliverList::create_element`] (or [`ListView::builder`](crate::BuildContext)) —
/// not through this alias directly — so `pub(crate)` is sufficient.
pub(crate) type SliverListAdaptorElement = Element<SliverList, Variable, SliverListAdaptorBehavior>;

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::{ElementId, RenderId};
    use flui_objects::RenderSizedBox;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use flui_types::geometry::px;
    use parking_lot::RwLock;

    use super::*;
    use crate::element::{RenderBehavior, Variable, unified::Element};
    use crate::view::RenderView;
    use crate::{BuildOwner, ElementTree};

    // -------------------------------------------------------------------------
    // Shared test fixture — minimal item view used as a list placeholder.
    // Defined at module level to satisfy `clippy::items_after_statements`.
    // -------------------------------------------------------------------------

    #[derive(Clone)]
    struct ItemView;

    impl RenderView for ItemView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;
        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(48.0)), Some(px(48.0)))
        }
        fn update_render_object(&self, _: &mut Self::RenderObject) {}
    }

    impl View for ItemView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(
                Element::<ItemView, Variable, RenderBehavior<ItemView>>::new(
                    self,
                    RenderBehavior::new(),
                ),
            )
        }
    }

    fn make_builder(item_count: usize) -> Arc<dyn Fn(usize) -> Option<BoxedView> + Send + Sync> {
        Arc::new(move |idx: usize| {
            if idx < item_count {
                Some(BoxedView(Box::new(ItemView)))
            } else {
                None
            }
        })
    }

    // -------------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------------

    /// `SliverList::new` panics on a zero extent estimate.
    #[test]
    fn new_panics_on_zero_estimate() {
        let builder = make_builder(10);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SliverList::new(10, 0.0, builder)
        }));
        assert!(result.is_err(), "zero estimate must panic");
    }

    /// `SliverList::new` panics on a negative extent estimate.
    #[test]
    fn new_panics_on_negative_estimate() {
        let builder = make_builder(10);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SliverList::new(10, -1.0, builder)
        }));
        assert!(result.is_err(), "negative estimate must panic");
    }

    /// Valid construction sets all fields and enforces the F4 invariant.
    #[test]
    fn new_succeeds_with_valid_parameters() {
        let builder = make_builder(100);
        let view = SliverList::new(100, 48.0, builder);
        assert_eq!(view.item_count, 100);
        assert!((view.item_extent_estimate - 48.0).abs() < f32::EPSILON);
        assert!(
            !view.has_children(),
            "adaptor view must have no dense children (F4)"
        );
    }

    /// Builder is called with the expected index; returns `Some` for valid
    /// indices and `None` for out-of-range.
    #[test]
    fn builder_returns_some_for_valid_index_and_none_for_out_of_range() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let builder: Arc<dyn Fn(usize) -> Option<BoxedView> + Send + Sync> =
            Arc::new(move |idx: usize| {
                call_count_clone.fetch_add(1, Ordering::Relaxed);
                if idx < 5 {
                    Some(BoxedView(Box::new(ItemView)))
                } else {
                    None
                }
            });

        let view = SliverList::new(5, 48.0, Arc::clone(&builder));
        assert!(!view.has_children(), "F4: no dense children");
        assert!((view.builder)(3).is_some());
        assert!((view.builder)(5).is_none());
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }

    /// `SliverList` is `Clone` (required by `View` + `RenderView`).
    #[test]
    fn view_is_clone() {
        let builder = make_builder(10);
        let view = SliverList::new(10, 48.0, builder);
        let cloned = view.clone();
        assert_eq!(cloned.item_count, 10);
        assert!((cloned.item_extent_estimate - 48.0).abs() < f32::EPSILON);
    }

    /// `create_element` produces a `SliverListAdaptorElement` (the view type id
    /// round-trips through the `dyn ElementBase` interface).
    ///
    /// Specifically: `view_type_id() == TypeId::of::<SliverList>()`, NOT
    /// `TypeId::of::<SliverListAdaptorElement>()` or any internal adaptor name.
    /// This is the identity the reconciler checks in `can_update_by_id` — if it
    /// were wrong, the element would be torn down and rebuilt on every parent
    /// rebuild that produces a new `SliverList` view (BLOCKER 1).
    #[test]
    fn create_element_produces_adaptor_element() {
        let builder = make_builder(10);
        let view = SliverList::new(10, 48.0, builder);
        let element = view.create_element();
        assert_eq!(element.view_type_id(), TypeId::of::<SliverList>());
    }

    // =========================================================================
    // Helper: minimal tree wired to a PipelineOwner, for service + round-trip.
    // =========================================================================

    /// Mount a render-bearing `ItemView` root wired to a fresh `PipelineOwner`.
    /// Returns `(tree, build_owner, pipeline, host_element_id)`.
    fn host_tree() -> (
        ElementTree,
        BuildOwner,
        Arc<RwLock<PipelineOwner>>,
        ElementId,
    ) {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let host = tree.mount_root_with_pipeline_owner(
            &ItemView,
            Some(Arc::clone(&pipeline)),
            &mut build_owner.element_owner_mut(),
        );
        (tree, build_owner, pipeline, host)
    }

    // =========================================================================
    // Test gap 6a: `ChildManager::service` bool-return unit tests.
    // =========================================================================

    /// `ChildManager::service` must return `false` when no children are evicted
    /// and no new children are built — the quiescence signal that prevents
    /// `service_child_requests` from calling `mark_needs_layout` and therefore
    /// issuing another layout pass on an already-settled sliver.
    #[test]
    fn service_returns_false_when_no_work_done() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        // Manager with no pre-built children; no requested indices; full retain
        // band [0, usize::MAX) ≡ keep everything.
        let mut manager = SliverListAdaptorManager {
            sparse_children: SparseChildren::new(),
            host_element_id: Some(host),
            builder: make_builder(5),
        };

        let did_work = manager.service(
            &[],        // no children requested
            0,          // retain_first
            usize::MAX, // retain_last — nothing is out-of-band
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            !did_work,
            "service with no evictions and no builds must return false (quiescence gate)"
        );
    }

    /// `ChildManager::service` must return `true` when it builds at least one
    /// new child. `true` tells `service_child_requests` to call
    /// `mark_needs_layout` so the sliver lays out the freshly-built children.
    #[test]
    fn service_returns_true_when_children_are_built() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = SliverListAdaptorManager {
            sparse_children: SparseChildren::new(),
            host_element_id: Some(host),
            builder: make_builder(5),
        };

        // Request index 0, retain band [0, 1): service must build item 0.
        let did_work = manager.service(
            &[0],
            0,
            1,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            did_work,
            "service that builds at least one child must return true"
        );
        assert!(
            manager.sparse_children.get(0).is_some(),
            "the requested child must be present in SparseChildren after service"
        );
    }

    /// `ChildManager::service` must return `true` when it evicts at least one
    /// child that has scrolled outside the retain band. This is the off-band
    /// eviction path (F5).
    #[test]
    fn service_returns_true_when_children_are_evicted() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = SliverListAdaptorManager {
            sparse_children: SparseChildren::new(),
            host_element_id: Some(host),
            builder: make_builder(5),
        };

        // Seed two pre-built children at indices 0 and 1.
        manager.service(
            &[0, 1],
            0,
            2,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        assert_eq!(
            manager.sparse_children.len(),
            2,
            "pre-condition: 2 children built"
        );

        // Retain band [5, 10): both pre-built children (0, 1) are out-of-band.
        let did_work = manager.service(
            &[],
            5,
            10,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            did_work,
            "service that evicts at least one child must return true"
        );
        assert_eq!(
            manager.sparse_children.len(),
            0,
            "all out-of-band children must be evicted"
        );
    }

    // =========================================================================
    // Test gap 6b: register/unregister round-trip via element lifecycle.
    // =========================================================================

    /// Mounting a `SliverList` element must register its `ChildManager` in the
    /// `BuildOwner`'s registry (keyed by the sliver's `RenderId`), and unmounting
    /// it must remove that entry. This end-to-end path exercises
    /// `SliverListAdaptorBehavior::on_mount` → `ElementOwner::register_child_manager`
    /// and `on_unmount` → `ElementOwner::unregister_child_manager`.
    #[test]
    fn child_manager_registered_on_mount_and_unregistered_on_unmount() {
        let (mut tree, mut build_owner, _pipeline, host) = host_tree();

        let sliver = SliverList::new(5, 48.0, make_builder(5));

        // Mount: `on_mount` must register the ChildManager.
        let sliver_id = tree.insert(&sliver, host, 0, &mut build_owner.element_owner_mut());

        // The element's render node carries the RenderId used as the registry key.
        let sliver_render_id: Option<RenderId> =
            tree.get(sliver_id).and_then(|n| n.element().render_id());
        let sliver_render_id =
            sliver_render_id.expect("SliverList element must have a render node after mount");

        {
            let registry = build_owner.child_manager_registry.lock();
            assert!(
                registry.contains_key(&sliver_render_id),
                "ChildManager must be registered in the BuildOwner registry after on_mount"
            );
        }

        // Unmount: `on_unmount` must unregister the ChildManager.
        tree.remove_subtree(sliver_id, &mut build_owner.element_owner_mut());

        {
            let registry = build_owner.child_manager_registry.lock();
            assert!(
                !registry.contains_key(&sliver_render_id),
                "ChildManager must be removed from the BuildOwner registry after on_unmount"
            );
        }
    }
}
