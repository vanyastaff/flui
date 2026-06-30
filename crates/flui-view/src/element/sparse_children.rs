//! Sparse, on-demand child storage for lazy slivers — the FLUI analogue of the
//! child bookkeeping in Flutter's `SliverMultiBoxAdaptorElement`.
//!
//! A normal multi-child element keeps a *dense* `Vec<ElementId>` reconciled
//! top-down. A lazy sliver instead builds only the children whose logical
//! indices fall inside the viewport's visible-plus-cache band, in arbitrary
//! order, and disposes them when they scroll off. [`SparseChildren`] is that
//! bookkeeping: a `logical index -> ElementId` map plus mount/evict operations
//! that reuse [`ElementTree::insert`]/[`ElementTree::remove`] and stamp each
//! freshly-built child's render node with its [`SliverMultiBoxAdaptorParentData`]
//! index. Stamping is what lets the lazy sliver recover `logical -> dense slot`
//! from parent-data alone (ADR-0003), so children may be attached in any order —
//! FLUI has no equivalent of Flutter's `_currentBeforeChild` insertion cursor.

use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::btree_map::Keys;
use std::sync::Arc;

use flui_foundation::{ElementId, RenderId};
use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;

use crate::ElementOwner;
use crate::tree::ElementNode;
use crate::tree::ElementTree;
use crate::view::View;

/// Bookkeeping for a lazy sliver's on-demand children.
///
/// Children are keyed by *logical index* (their position in the data source),
/// not by dense slot — the map is sparse because only the visible-plus-cache
/// band is built. Ordered (`BTreeMap`) so band eviction sweeps in index order.
///
/// # F4 invariant — host `child_ids` stays empty
///
/// The adaptor element that owns a `SparseChildren` must **never** append its
/// lazy children to the host's `ElementNode::child_ids` list. If it did, a
/// dense reconcile of the host (e.g. on a rebuild triggered by an unrelated
/// state change) would call `reconcile(host, [])` and delete all lazy children
/// via the normal dense teardown path before `SparseChildren` can evict them
/// gracefully. `RenderSliverList` indexes children by their
/// `SliverMultiBoxAdaptorParentData.index` field (stamped at `ensure` time),
/// not by dense slot order, so the empty `child_ids` is safe and intentional.
#[derive(Debug, Default)]
pub(crate) struct SparseChildren {
    by_logical_index: BTreeMap<usize, ElementId>,
}

impl SparseChildren {
    /// An empty manager — no children built yet.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of currently-built children.
    pub(crate) fn len(&self) -> usize {
        self.by_logical_index.len()
    }

    /// Whether no child is currently built.
    ///
    /// Used in tests; suppressed in release builds to avoid the dead-code lint
    /// until a production caller lands.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_logical_index.is_empty()
    }

    /// The `ElementId` of the child built at `logical_index`, if any.
    pub(crate) fn get(&self, logical_index: usize) -> Option<ElementId> {
        self.by_logical_index.get(&logical_index).copied()
    }

    /// The logical indices of all currently-built children, ascending.
    ///
    /// Used in tests; suppressed in release builds to avoid the dead-code lint
    /// until a production caller lands.
    #[cfg(test)]
    pub(crate) fn logical_indices(&self) -> Keys<'_, usize, ElementId> {
        self.by_logical_index.keys()
    }

    /// Iterate over all currently-built `(logical_index, ElementId)` pairs.
    ///
    /// Used by the adaptor element's `on_unmount` (F3) to find and subtree-
    /// remove every lazy child: since the host's `child_ids` is empty (F4
    /// invariant) the generic tree-walk that covers dense children cannot
    /// reach them.
    pub(crate) fn iter_built(&self) -> impl Iterator<Item = (usize, ElementId)> + '_ {
        self.by_logical_index
            .iter()
            .map(|(&logical_index, &id)| (logical_index, id))
    }

    /// Ensure a child exists at `logical_index`, building it from `view` under
    /// `host` if absent. Returns the child's `ElementId` (existing or freshly
    /// mounted). A freshly-mounted child has its render node stamped with
    /// `SliverMultiBoxAdaptorParentData { index: logical_index }` so the lazy
    /// sliver can map it back to a dense slot regardless of attach order.
    ///
    /// Idempotent: a second call for an already-built index returns the existing
    /// id and does **not** rebuild (reconciling a changed `view` is a later
    /// concern — Flutter's `updateChild`).
    pub(crate) fn ensure(
        &mut self,
        logical_index: usize,
        view: &dyn View,
        host: ElementId,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &Arc<RwLock<PipelineOwner>>,
    ) -> ElementId {
        if let Some(&existing) = self.by_logical_index.get(&logical_index) {
            return existing;
        }
        let child = tree.insert(view, host, logical_index, owner);
        stamp_logical_index(tree, pipeline, child, logical_index);
        self.by_logical_index.insert(logical_index, child);

        // F1: `ElementTree::insert` (via `ElementCore::mount`) sets the child's
        // `dirty = true` but does NOT push it onto the build heap — only
        // `id_reconcile.rs` does that through `schedule_build_for`.  Without
        // this explicit push the second `build_scope` in
        // `BuildOwner::service_child_requests` drains an empty heap and the
        // child's own subtree (e.g. Padding(Text)) never expands.
        let child_depth = tree.get(child).map_or(0, ElementNode::depth);
        owner.schedule_build_for(child, child_depth);

        tracing::trace!(
            logical_index,
            ?child,
            ?host,
            "SparseChildren mounted lazy child"
        );
        child
    }

    /// Evict the child at `logical_index`, unmounting its element subtree (and
    /// thus its render nodes). Returns whether a child was removed; a `false`
    /// means no child was built at that index.
    pub(crate) fn evict(
        &mut self,
        logical_index: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
    ) -> bool {
        let Some(child) = self.by_logical_index.remove(&logical_index) else {
            return false;
        };
        // F2: use `remove_subtree` so the child's entire descendant subtree is
        // freed.  A single-node `tree.remove` only removes the top-level element
        // and leaks every descendant (e.g. the Padding and Text inside a
        // Container child stay as orphaned slab entries and dangling render nodes).
        tree.remove_subtree(child, owner);
        tracing::trace!(logical_index, ?child, "SparseChildren evicted lazy child");
        true
    }

    /// Evict every child whose logical index falls outside the half-open band
    /// `[first, last)` — the children that have scrolled out of the cache band.
    /// `O(K)` in the currently-built child count `K` (bounded by the band).
    ///
    /// Returns `true` if at least one child was evicted, `false` if all built
    /// children were already inside the band (no work done). Callers use this
    /// to decide whether to mark the sliver dirty for re-layout.
    pub(crate) fn retain_band(
        &mut self,
        first: usize,
        last: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
    ) -> bool {
        let out_of_band: Vec<usize> = self
            .by_logical_index
            .keys()
            .copied()
            .filter(|&logical_index| logical_index < first || logical_index >= last)
            .collect();
        let any_evicted = !out_of_band.is_empty();
        for logical_index in out_of_band {
            self.evict(logical_index, tree, owner);
        }
        any_evicted
    }
}

// Called from `SparseChildren::ensure` via the lazy-sliver adaptor element.
/// Stamp `child`'s render node with its sliver logical index, so the lazy sliver
/// can map `logical -> dense slot` from parent-data alone. Fresh render nodes
/// start with `parent_data = None`; this seeds a full
/// [`SliverMultiBoxAdaptorParentData`] carrying the index.
///
/// A direct sliver child always owns a render node by the time `insert` returns
/// (`RenderBehavior::on_mount` mints it); the debug assertion catches a future
/// regression where a non-render child is fed in by mistake.
fn stamp_logical_index(
    tree: &ElementTree,
    pipeline: &Arc<RwLock<PipelineOwner>>,
    child: ElementId,
    logical_index: usize,
) {
    let render_id: Option<RenderId> = tree.get(child).and_then(|node| node.element().render_id());
    let Some(render_id) = render_id else {
        debug_assert!(
            false,
            "a lazy sliver child must own a render node to carry its logical \
             index; logical_index={logical_index} produced no render id"
        );
        return;
    };
    let mut owner = pipeline.write();
    if let Some(node) = owner.render_tree_mut().get_mut(render_id) {
        node.set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(
            logical_index,
        )));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_foundation::ViewKey;
    use flui_objects::RenderSizedBox;
    use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_types::geometry::px;
    use parking_lot::RwLock;

    use super::SparseChildren;
    use crate::GlobalKey;
    use crate::view::{RenderView, View};
    use crate::{BuildOwner, ElementTree};

    /// A minimal render-bearing leaf view used as both host and child in these
    /// tests — mirrors the `SizedBoxView` in `view/render.rs` tests.
    #[derive(Clone)]
    struct LeafBox {
        side: f32,
    }

    impl RenderView for LeafBox {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(self.side)), Some(px(self.side)))
        }

        fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}
    }

    impl View for LeafBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// Like [`LeafBox`] but carries a [`GlobalKey`] so `tree.remove` soft-removes
    /// it into the inactive queue instead of freeing the slab entry immediately.
    /// Used to test the globally-keyed eviction → `finalize_tree` → slab-free path.
    #[derive(Clone)]
    struct GlobalKeyedLeafBox {
        side: f32,
        key: GlobalKey<Self>,
    }

    impl RenderView for GlobalKeyedLeafBox {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(self.side)), Some(px(self.side)))
        }

        fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}
    }

    impl View for GlobalKeyedLeafBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }

        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    /// Mount a render-bearing host root wired to a fresh `PipelineOwner`, and
    /// return everything the tests drive `SparseChildren` against.
    fn host_tree() -> (
        ElementTree,
        BuildOwner,
        Arc<RwLock<PipelineOwner>>,
        flui_foundation::ElementId,
    ) {
        let pipeline = Arc::new(RwLock::new(PipelineOwner::new()));
        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let host = tree.mount_root_with_pipeline_owner(
            &LeafBox { side: 10.0 },
            Some(Arc::clone(&pipeline)),
            &mut build_owner.element_owner_mut(),
        );
        (tree, build_owner, pipeline, host)
    }

    /// Read back the stamped logical index from a child's render node.
    fn stamped_index(
        tree: &ElementTree,
        pipeline: &Arc<RwLock<PipelineOwner>>,
        child: flui_foundation::ElementId,
    ) -> Option<usize> {
        let render_id = tree.get(child)?.element().render_id()?;
        let owner = pipeline.read();
        let node = owner.render_tree().get(render_id)?;
        node.parent_data()?
            .downcast_ref::<SliverMultiBoxAdaptorParentData>()
            .map(|pd| pd.index)
    }

    #[test]
    fn ensure_mounts_child_under_host_and_stamps_logical_index() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let host_render = tree.get(host).unwrap().element().render_id().unwrap();
        let mut children = SparseChildren::new();

        let child = children.ensure(
            5,
            &LeafBox { side: 4.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert_eq!(children.get(5), Some(child), "map records the built child");
        assert_eq!(children.len(), 1);

        // The child's render node attached under the host's render node.
        let child_render = tree.get(child).unwrap().element().render_id().unwrap();
        let owner = pipeline.read();
        assert_eq!(
            owner.render_tree().parent(child_render),
            Some(host_render),
            "the lazy child's render node attaches under the host",
        );
        drop(owner);

        // And carries the logical index in its parent data.
        assert_eq!(stamped_index(&tree, &pipeline, child), Some(5));
    }

    #[test]
    fn ensure_is_idempotent_for_a_built_index() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let mut children = SparseChildren::new();

        let first = children.ensure(
            2,
            &LeafBox { side: 4.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        let second = children.ensure(
            2,
            &LeafBox { side: 9.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert_eq!(first, second, "a built index is not rebuilt");
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn evict_unmounts_child_and_removes_its_render_node() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let mut children = SparseChildren::new();

        let child = children.ensure(
            3,
            &LeafBox { side: 4.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        let child_render = tree.get(child).unwrap().element().render_id().unwrap();

        let removed = children.evict(3, &mut tree, &mut build_owner.element_owner_mut());

        assert!(removed, "evict reports the child was removed");
        assert_eq!(children.get(3), None);
        assert!(children.is_empty());
        // The element is gone from the tree…
        assert!(tree.get(child).is_none(), "child element unmounted");
        // …and so is its render node.
        let owner = pipeline.read();
        assert!(
            owner.render_tree().get(child_render).is_none(),
            "the lazy child's render node is removed on evict",
        );
    }

    #[test]
    fn evict_absent_index_is_a_no_op() {
        let (mut tree, mut build_owner, _pipeline, _host) = host_tree();
        let mut children = SparseChildren::new();
        assert!(!children.evict(7, &mut tree, &mut build_owner.element_owner_mut()));
    }

    #[test]
    fn retain_band_drops_out_of_band_children_only() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let mut children = SparseChildren::new();

        for logical_index in 0..5 {
            children.ensure(
                logical_index,
                &LeafBox { side: 4.0 },
                host,
                &mut tree,
                &mut build_owner.element_owner_mut(),
                &pipeline,
            );
        }
        assert_eq!(children.len(), 5);

        // Keep only the band [2, 4): indices 2 and 3 survive.
        children.retain_band(2, 4, &mut tree, &mut build_owner.element_owner_mut());

        let surviving: Vec<usize> = children.logical_indices().copied().collect();
        assert_eq!(surviving, vec![2, 3], "only in-band children survive");
    }

    /// F1: `ensure` must push the freshly-mounted child onto the dirty heap so
    /// the second `build_scope` in `service_child_requests` can expand its
    /// subtree (e.g. Padding(Text)). Without `schedule_build_for` the heap is
    /// empty and child subtrees never grow past the top-level node.
    #[test]
    fn ensure_schedules_child_for_build() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        // Record how many elements are already scheduled by the root mount.
        let count_before = build_owner.dirty_count();

        let mut children = SparseChildren::new();
        children.ensure(
            0,
            &LeafBox { side: 4.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        // After `ensure`, the child must be on the dirty heap so the next
        // `build_scope` can expand its own subtree (F1).
        assert!(
            build_owner.dirty_count() > count_before,
            "ensure must schedule the freshly-mounted child for build (F1 — \
             without schedule_build_for, service_child_requests runs build_scope \
             over an empty heap and child subtrees never expand)",
        );
    }

    /// F2: `evict` must remove the child's *entire* descendant subtree, not
    /// only the top-level element. A single-node `tree.remove` leaks every
    /// descendant element (and their render nodes), which the slab retains as
    /// orphans forever.
    ///
    /// The test simulates a two-level view tree by:
    /// 1. `ensure`-mounting a top-level lazy child.
    /// 2. `tree.insert`-ing a grandchild and wiring it into the child's
    ///    `child_ids` via `set_child_ids` — exactly what the reconciler does
    ///    when it resolves a composite child view (e.g. Padding wrapping Text).
    /// 3. Evicting and asserting both nodes are gone.
    #[test]
    fn evict_subtree_cleans_descendants() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let mut children = SparseChildren::new();

        // Mount a top-level lazy child (the view-tree root of one list item).
        let child = children.ensure(
            0,
            &LeafBox { side: 4.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        // Insert a grandchild under `child` to simulate a composite view
        // subtree (e.g. Container → Padding → Text). `tree.insert` creates
        // the slab entry and runs `on_mount`, but does NOT automatically write
        // into `child.child_ids` — that only happens during reconciliation.
        // Wire it up explicitly so `remove_subtree`'s DFS finds it.
        let grandchild = tree.insert(
            &LeafBox { side: 2.0 },
            child,
            0,
            &mut build_owner.element_owner_mut(),
        );
        // Simulate the reconciler's `set_child_ids` call so the subtree-DFS
        // in `remove_subtree` can reach `grandchild` through `child.child_ids`.
        tree.get_mut(child).unwrap().set_child_ids(vec![grandchild]);

        // Both nodes live in the tree before eviction.
        assert!(tree.get(child).is_some(), "child present before evict");
        assert!(
            tree.get(grandchild).is_some(),
            "grandchild present before evict"
        );

        // Capture render IDs before eviction to verify render-tree cleanup.
        let child_render_id = tree.get(child).and_then(|n| n.element().render_id());
        let grandchild_render_id = tree.get(grandchild).and_then(|n| n.element().render_id());

        // Both render nodes must exist (pipeline is threaded through the parent
        // element into `tree.insert` via `pipeline_owner_any` propagation).
        assert!(
            child_render_id.is_some(),
            "child element must have a render node before evict"
        );
        assert!(
            grandchild_render_id.is_some(),
            "grandchild element must have a render node before evict"
        );

        // Evict the list item — the whole subtree must disappear (F2).
        let removed = children.evict(0, &mut tree, &mut build_owner.element_owner_mut());

        assert!(removed, "evict reports the child was present");
        assert!(
            tree.get(child).is_none(),
            "top-level lazy child must be removed on evict (F2)",
        );
        assert!(
            tree.get(grandchild).is_none(),
            "descendant element must also be removed (F2 — single-node remove \
             would leak this grandchild as an orphaned slab entry)",
        );

        // F2: render nodes must also be gone after subtree eviction.
        let owner = pipeline.read();
        if let Some(rid) = child_render_id {
            assert!(
                owner.render_tree().get(rid).is_none(),
                "child render node must be removed on subtree evict (F2)",
            );
        }
        if let Some(rid) = grandchild_render_id {
            assert!(
                owner.render_tree().get(rid).is_none(),
                "grandchild render node must also be removed on subtree evict (F2 — \
                 single-node remove leaks descendant render nodes)",
            );
        }
    }

    /// F5.key: a globally-keyed lazy child pushed to the inactive queue by
    /// eviction must be slab-freed by `finalize_tree` — not left dangling.
    ///
    /// A globally-keyed element is soft-removed by `tree.remove` (called inside
    /// `remove_subtree`): the slab entry stays alive, the element is placed into
    /// `BuildOwner::inactive_elements`, and `has_inactive_elements()` returns
    /// `true`. Only `finalize_tree` drains that queue and calls `remove_finalized`
    /// which actually frees the slab slot. Without `finalize_tree` the element
    /// would remain in the slab indefinitely.
    ///
    /// The test uses a leaf view so the globally-keyed root has no descendants —
    /// the non-keyed descendant-leak concern for composite subtrees is a separate,
    /// orthogonal investigation.
    #[test]
    fn evicted_globally_keyed_child_freed_by_finalize_tree() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let element_count_before = tree.len();

        let global_key = GlobalKey::<GlobalKeyedLeafBox>::new();
        let keyed_item = GlobalKeyedLeafBox {
            side: 4.0,
            key: global_key.clone(),
        };

        let mut children = SparseChildren::new();
        let child_id = children.ensure(
            0,
            &keyed_item,
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert_eq!(
            tree.len(),
            element_count_before + 1,
            "the globally-keyed child must occupy one slab slot after mount"
        );
        assert!(
            tree.get(child_id).is_some(),
            "child must be accessible in the tree before eviction"
        );

        // Evict: `remove_subtree` → `remove` → soft-removes because the element
        // has a `registered_global_key_hash` (GlobalKey). The slab entry survives.
        children.evict(0, &mut tree, &mut build_owner.element_owner_mut());

        assert_eq!(
            children.get(0),
            None,
            "evict must clear the SparseChildren map entry"
        );
        // The node is still in the slab (soft-removed), but pushed to inactive.
        assert_eq!(
            tree.len(),
            element_count_before + 1,
            "soft-remove must not free the slab slot immediately"
        );
        assert!(
            build_owner.has_inactive_elements(),
            "a globally-keyed eviction must push the element to the inactive queue, \
             not free it eagerly — this is what distinguishes soft-remove from eager-remove"
        );

        // `finalize_tree` drains the inactive queue and calls `remove_finalized`
        // on each entry, which frees the slab slot.
        build_owner.finalize_tree(&mut tree);

        assert!(
            !build_owner.has_inactive_elements(),
            "finalize_tree must drain the inactive queue completely"
        );
        assert_eq!(
            tree.len(),
            element_count_before,
            "the globally-keyed element must be slab-freed by finalize_tree (F5.key)"
        );
        assert!(
            tree.get(child_id).is_none(),
            "the element must no longer be accessible in the tree after finalize_tree"
        );
    }
}
