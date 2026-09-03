//! Sparse, on-demand child storage for lazy slivers — the FLUI analogue of the
//! child bookkeeping in Flutter's `SliverMultiBoxAdaptorElement`.
//!
//! A normal multi-child element keeps a *dense* `Vec<ElementId>` reconciled
//! top-down. A lazy sliver instead builds only the children whose logical
//! indices fall inside the viewport's visible-plus-cache band, in arbitrary
//! order, and disposes them when they scroll off. [`SparseChildren`] is that
//! bookkeeping: a `logical index -> ElementId` map plus mount/evict operations
//! that reuse [`ElementTree::insert`]/[`ElementTree::remove`] and stamp each
//! freshly-built child's render node with its [`SliverMultiBoxAdaptorParentData`](flui_rendering::parent_data::SliverMultiBoxAdaptorParentData)
//! index. Stamping is what lets the lazy sliver recover `logical -> dense slot`
//! from parent-data alone (ADR-0003), so children may be attached in any order —
//! FLUI has no equivalent of Flutter's `_currentBeforeChild` insertion cursor.

#[cfg(test)]
use std::collections::btree_map::Keys;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::AssertUnwindSafe;

use flui_foundation::{ElementId, RenderId, SaltedKey, ViewKey};
use flui_rendering::pipeline::PipelineCell;

use crate::BoxedView;
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
/// # Invariant: host `child_ids` stays empty
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
    /// Used by the adaptor element's `on_unmount` to find and subtree-remove
    /// every lazy child: since the host's `child_ids` stays empty by
    /// invariant, the generic tree-walk that covers dense children cannot
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
        pipeline: &PipelineCell,
    ) -> ElementId {
        if let Some(&existing) = self.by_logical_index.get(&logical_index) {
            return existing;
        }
        let child = mount_sparse_child(logical_index, view, host, tree, owner, pipeline);
        self.by_logical_index.insert(logical_index, child);
        child
    }

    /// Drop the bookkeeping for `child` without touching the tree — the
    /// child was grafted to another parent by a `GlobalKey` retake and is
    /// no longer this sliver's to evict or refresh (Flutter's
    /// `SliverMultiBoxAdaptorElement.forgetChild`). Returns the logical
    /// index it held, if it was resident.
    pub(crate) fn forget(&mut self, child: ElementId) -> Option<usize> {
        let index = self
            .by_logical_index
            .iter()
            .find_map(|(&index, &id)| (id == child).then_some(index))?;
        self.by_logical_index.remove(&index);
        tracing::trace!(
            logical_index = index,
            ?child,
            "SparseChildren forgot a grafted child"
        );
        Some(index)
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
        // Use `remove_subtree` so the child's entire descendant subtree is
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

    /// Reconcile the resident children against a (possibly changed) data
    /// source — the sparse counterpart of Flutter's
    /// `SliverMultiBoxAdaptorElement.performRebuild`
    /// (`widgets/sliver.dart`, tag `3.44.0`), and the mechanism behind
    /// `SliverChildBuilderDelegate.shouldRebuild => true`: a new delegate
    /// re-consults the builder for every resident index, not only the
    /// newly-visible ones.
    ///
    /// Two-phase, into a fresh map, so that a shift or swap of several keyed
    /// residents can never overwrite one of them (Flutter's separate
    /// `newChildren` map is load-bearing for the same reason):
    ///
    /// 1. **Snapshot and build.** Record every resident `(index, element,
    ///    key)`. The indices to build are the resident ones plus, for every
    ///    keyed resident, the index `find_index_by_key` reports for its key —
    ///    that is how a keyed child whose data moved *out of the resident
    ///    band* is still found (Flutter's `findChildIndexCallback`; a
    ///    `SliverChildListDelegate` derives the map from its children). Every
    ///    index is built through [`build_item_or_error`], so a panicking
    ///    builder yields an error child at that index and nothing else.
    /// 2. **Match and apply.** A built view with a key claims the first
    ///    unclaimed resident carrying an equal key (first wins on duplicate
    ///    keys, as the dense reconciler does) — wherever that resident sat,
    ///    so a keyed child moving *within* the band needs no callback at all;
    ///    a keyless view claims the resident at its own index when the types
    ///    agree. A claimed resident is updated in place, relocated first if
    ///    its index changed ([`ElementTree::relocate_sparse_child`] re-slots
    ///    it and re-derives the `sliver_slot` chain, then its render
    ///    descendants are re-stamped); an unclaimed resident is evicted; an
    ///    unclaimed view is mounted fresh. Keys are compared as the
    ///    residents carry them (a per-item wrapper carries the item's key
    ///    salted, and so does the freshly built wrapper); the callback sees
    ///    the item's own key through [`SaltedKey::unsalt`].
    ///
    /// `item_count` bounds the indices worth building. A builder answering
    /// `None` below it means the data source shrank: the index is left
    /// empty and reported in [`ReconcileOutcome::end_reached_at`] so the
    /// caller can clamp the render object's count.
    ///
    /// `host` is the adaptor element's own id (the parent for fresh mounts).
    pub(crate) fn reconcile(
        &mut self,
        builder: &dyn Fn(usize) -> Option<BoxedView>,
        find_index_by_key: Option<&dyn Fn(&dyn ViewKey) -> Option<usize>>,
        item_count: usize,
        host: ElementId,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &PipelineCell,
    ) -> ReconcileOutcome {
        // ── Phase 1: snapshot the residents, decide what to build, build ──
        struct Resident {
            index: usize,
            id: ElementId,
            key: Option<Box<dyn ViewKey>>,
            claimed: bool,
        }
        let mut residents: Vec<Resident> = self
            .by_logical_index
            .iter()
            .map(|(&index, &id)| Resident {
                index,
                id,
                key: tree
                    .get(id)
                    .and_then(|node| node.key().map(ViewKey::clone_key)),
                claimed: false,
            })
            .collect();
        let mut targets: BTreeSet<usize> = residents.iter().map(|r| r.index).collect();
        if let Some(find) = find_index_by_key {
            for resident in &residents {
                if let Some(key) = &resident.key
                    && let Some(new_index) = find_index_or_none(find, SaltedKey::unsalt(&**key))
                    && new_index < item_count
                {
                    targets.insert(new_index);
                }
            }
        }
        let mut end_reached_at: Option<usize> = None;
        let built: Vec<(usize, Option<BoxedView>)> = targets
            .into_iter()
            .map(|index| {
                let view = if index < item_count {
                    build_item_or_error(builder, index)
                } else {
                    None
                };
                if view.is_none() && index < item_count {
                    end_reached_at = Some(end_reached_at.map_or(index, |end| end.min(index)));
                }
                (index, view)
            })
            .collect();

        // ── Phase 2: match built views to residents ──
        // `matched[k] = Some(resident position)` for built entry `k`.
        let mut matched: Vec<Option<usize>> = vec![None; built.len()];
        for (k, (index, view)) in built.iter().enumerate() {
            let Some(view) = view else {
                continue;
            };
            let view: &dyn View = view.0.as_ref();
            let candidate = if let Some(key) = view.key() {
                residents
                    .iter()
                    .position(|r| !r.claimed && r.key.as_deref().is_some_and(|rk| rk.key_eq(key)))
            } else {
                residents
                    .iter()
                    .position(|r| !r.claimed && r.index == *index && r.key.is_none())
            };
            if let Some(pos) = candidate
                && resident_type_matches(tree, residents[pos].id, view)
            {
                residents[pos].claimed = true;
                matched[k] = Some(pos);
            }
        }

        // ── Apply into a fresh map: evict, relocate, update, mount ──
        let mut any_work = false;
        let mut next: BTreeMap<usize, ElementId> = BTreeMap::new();
        for resident in residents.iter().filter(|r| !r.claimed) {
            tree.remove_subtree(resident.id, owner);
            tracing::trace!(
                logical_index = resident.index,
                child = ?resident.id,
                "SparseChildren evicted an unclaimed resident"
            );
            any_work = true;
        }
        for (k, (index, view)) in built.iter().enumerate() {
            let Some(view) = view else {
                continue;
            };
            let view: &dyn View = view.0.as_ref();
            match matched[k] {
                Some(pos) => {
                    let resident = &residents[pos];
                    if resident.index != *index {
                        tree.relocate_sparse_child(resident.id, *index);
                        stamp_logical_index(tree, pipeline, resident.id, *index);
                        tracing::trace!(
                            from = resident.index,
                            to = *index,
                            child = ?resident.id,
                            "SparseChildren relocated a keyed resident"
                        );
                        any_work = true;
                    }
                    tree.update(resident.id, view, owner);
                    // Mirrors the dense reconciler's post-update scheduling
                    // (`tree/id_reconcile.rs`): an update that left the child
                    // clean (its own `should_skip_rebuild` memoization fired)
                    // must not be pushed onto the build heap.
                    if let Some(node) = tree.get(resident.id)
                        && node.element().is_dirty()
                    {
                        let depth = node.depth();
                        owner.schedule_build_for(
                            resident.id,
                            depth,
                            crate::RebuildReason::ParentUpdate,
                        );
                        any_work = true;
                    }
                    next.insert(*index, resident.id);
                }
                None => {
                    let child = mount_sparse_child(*index, view, host, tree, owner, pipeline);
                    next.insert(*index, child);
                    any_work = true;
                }
            }
        }
        self.by_logical_index = next;
        ReconcileOutcome {
            did_work: any_work,
            end_reached_at,
        }
    }
}

/// What [`SparseChildren::reconcile`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReconcileOutcome {
    /// Whether any child was evicted, relocated, remounted, or left dirty —
    /// callers mark the sliver for re-layout on `true`.
    pub(crate) did_work: bool,
    /// The lowest index below `item_count` for which the builder answered
    /// `None`: the data source shrank and the render object's count should
    /// be clamped to it.
    pub(crate) end_reached_at: Option<usize>,
}

/// Mount `view` at `logical_index` under `host` and stamp its render node(s).
fn mount_sparse_child(
    logical_index: usize,
    view: &dyn View,
    host: ElementId,
    tree: &mut ElementTree,
    owner: &mut ElementOwner<'_>,
    pipeline: &PipelineCell,
) -> ElementId {
    // Declare `host` as the parent being reconciled for the duration of
    // this insert. `ElementTree::insert` refuses to relocate an active
    // GlobalKey onto a parent that is not the one currently reconciling,
    // and its rejection arm panics — so without this, a keyed item
    // scrolling from one lazy list into another aborted the process
    // instead of moving. `service_child_requests` calls `service` (and so
    // this) outside any other reconcile, before its own `build_scope`,
    // so this never nests inside the guard `reconcile_children_by_id`
    // installs — which `begin_reconcile` asserts against.
    let child = {
        let _reconcile_guard = tree.begin_reconcile(host);
        tree.insert(view, host, logical_index, owner)
    };
    stamp_logical_index(tree, pipeline, child, logical_index);

    // `ElementTree::insert` (via `ElementCore::mount`) sets the child's
    // `dirty = true` but does NOT push it onto the build heap — only
    // `id_reconcile.rs` does that through `schedule_build_for`.  Without
    // this explicit push the follow-up `build_scope` in
    // `BuildOwner::service_child_requests` drains an empty heap and the
    // child's own subtree (e.g. Padding(Text)) never expands.
    let child_depth = tree.get(child).map_or(0, ElementNode::depth);
    owner.schedule_build_for(child, child_depth, crate::RebuildReason::ChildListChange);

    tracing::trace!(
        logical_index,
        ?child,
        ?host,
        "SparseChildren mounted lazy child"
    );
    child
}

/// Build the item at `index` through `builder`, substituting the registered
/// `ErrorView` when the builder panics — the lazy-sliver counterpart of
/// [`build_or_recover`](super::behavior_commons::build_or_recover), and the
/// port of `SliverChildBuilderDelegate.build`'s `try { builder(context,
/// index) } catch { _createErrorWidget(...) }`.
///
/// Recovery is per item, as in Flutter: the error child takes exactly the
/// panicking index, is unkeyed (so it can never be mistaken for a user's
/// keyed item by `find_index_by_key`), and updates in place while the panic
/// persists. Everything the caller had already done for other indices
/// stands.
///
/// The closure captures only the builder — the tree, the owner, and this
/// bookkeeping are all outside it — so a half-finished builder leaves no
/// shared state behind; `AssertUnwindSafe` restates that, it does not hide
/// anything.
pub(crate) fn build_item_or_error(
    builder: &dyn Fn(usize) -> Option<BoxedView>,
    index: usize,
) -> Option<BoxedView> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| builder(index))) {
        Ok(view) => view,
        Err(payload) => {
            let error = crate::view::FlutterError::from_panic(
                payload.as_ref(),
                format!("building lazy sliver child {index}"),
            );
            tracing::error!(
                index,
                "lazy sliver builder panicked; substituting ErrorView: {}",
                error.message
            );
            Some(BoxedView(crate::view::ErrorView::build_error_view(&error)))
        }
    }
}

/// Consult a user `find_index_by_key` callback under the same panic boundary
/// as the builder; a panicking callback answers `None` (no move) and is
/// reported once.
fn find_index_or_none(
    find: &dyn Fn(&dyn ViewKey) -> Option<usize>,
    key: &dyn ViewKey,
) -> Option<usize> {
    match std::panic::catch_unwind(AssertUnwindSafe(|| find(key))) {
        Ok(index) => index,
        Err(payload) => {
            let error = crate::view::FlutterError::from_panic(
                payload.as_ref(),
                "resolving a lazy sliver child index by key".to_string(),
            );
            tracing::error!(?key, "find_index_by_key panicked: {}", error.message);
            None
        }
    }
}

/// Whether `existing`'s live element can be updated in place by `new`.
///
/// Delegates to `tree/id_reconcile.rs`'s `can_update_by_id` — the same
/// type-then-key predicate the dense reconciler uses, so a keyed lazy child
/// (an item wrapper carrying the item's salted key, or an item answering
/// `View::key` itself) remounts on a key mismatch exactly as Flutter's
/// `Widget.canUpdate` demands, and a keyless one reconciles by type alone.
///
/// [`ViewKey`]: flui_foundation::ViewKey
fn resident_type_matches(tree: &ElementTree, existing: ElementId, new: &dyn View) -> bool {
    crate::tree::id_reconcile::can_update_by_id(tree, existing, new)
}

// Called from `SparseChildren::ensure` via the lazy-sliver adaptor element.
/// Stamp the render node(s) that carry `child`'s sliver logical index — the
/// first render descendants reachable from `child` without crossing another
/// render element — so the lazy sliver can map `logical -> dense slot` from
/// parent-data alone.
///
/// This is the *relocation* half of the stamp. A freshly-mounted render
/// object is stamped at adoption by `RenderBehavior::on_mount`, reading the
/// `sliver_slot` the slab seeded on insert (Flutter's `didAdoptChild`); a
/// subtree that arrives through GlobalKey relocation never re-mounts, so its
/// already-built render descendants are found here and stamped explicitly.
/// A fresh composite child (a bare `Text`, a `StatefulView`) has no render
/// descendant yet at `ensure` time — the walk stamps nothing, and the
/// adopt-time path covers it when the follow-up build expands the subtree.
fn stamp_logical_index(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    child: ElementId,
    logical_index: usize,
) {
    let render_ids = first_render_descendants(tree, child);
    if render_ids.is_empty() {
        return;
    }
    pipeline.with_mut(|owner| {
        for render_id in render_ids {
            crate::element::behavior_commons::stamp_sliver_logical_index(
                owner,
                render_id,
                logical_index,
            );
        }
    });
}

/// The render ids of the first render elements reachable from `root`
/// (inclusive) walking down through composite elements only. A render
/// element stops the walk on its branch: its own children attach under it,
/// not under the sliver.
fn first_render_descendants(tree: &ElementTree, root: ElementId) -> Vec<RenderId> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = tree.get(id) else {
            continue;
        };
        if let Some(render_id) = node.element().render_id() {
            found.push(render_id);
        } else {
            stack.extend(node.child_ids().iter().copied());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use flui_foundation::ViewKey;
    use flui_objects::RenderSizedBox;
    use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
    use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
    use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, RenderBox, Size};
    use flui_tree::Leaf;
    use flui_types::geometry::px;

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

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(self.side)), Some(px(self.side)))
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            render_object.set_size(Some(px(self.side)), Some(px(self.side)))
        }
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
        detach_count: Arc<AtomicUsize>,
    }

    #[derive(Debug)]
    struct DetachCountingBox {
        side: f32,
        detach_count: Arc<AtomicUsize>,
    }

    impl flui_foundation::Diagnosticable for DetachCountingBox {}

    impl RenderBox for DetachCountingBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            Size::new(px(self.side), px(self.side))
        }

        fn detach(&mut self) {
            self.detach_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl RenderView for GlobalKeyedLeafBox {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = DetachCountingBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            DetachCountingBox {
                side: self.side,
                detach_count: Arc::clone(&self.detach_count),
            }
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            render_object.side = self.side;
            flui_rendering::RenderUpdateImpact::LAYOUT
        }
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
        PipelineCell,
        flui_foundation::ElementId,
    ) {
        let pipeline = PipelineCell::new(PipelineOwner::new());
        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let host = tree.mount_root_with_pipeline_owner(
            &LeafBox { side: 10.0 },
            Some(pipeline.clone()),
            &mut build_owner.element_owner_mut(),
        );
        (tree, build_owner, pipeline, host)
    }

    /// Read back the stamped logical index from a child's render node.
    fn stamped_index(
        tree: &ElementTree,
        pipeline: &PipelineCell,
        child: flui_foundation::ElementId,
    ) -> Option<usize> {
        let render_id = tree.get(child)?.element().render_id()?;
        pipeline.with(|owner| {
            let node = owner.render_tree().get(render_id)?;
            node.parent_data()?
                .downcast_ref::<SliverMultiBoxAdaptorParentData>()
                .map(|pd| pd.index)
        })
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
        pipeline.with(|owner| {
            assert_eq!(
                owner.render_tree().parent(child_render),
                Some(host_render),
                "the lazy child's render node attaches under the host",
            );
        });

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
        pipeline.with(|owner| {
            assert!(
                owner.render_tree().get(child_render).is_none(),
                "the lazy child's render node is removed on evict",
            );
        });
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

    /// `ensure` must push the freshly-mounted child onto the dirty heap so
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
        // `build_scope` can expand its own subtree.
        assert!(
            build_owner.dirty_count() > count_before,
            "ensure must schedule the freshly-mounted child for build — \
             without schedule_build_for, service_child_requests runs build_scope \
             over an empty heap and child subtrees never expand",
        );
    }

    /// `evict` must remove the child's *entire* descendant subtree, not
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
        // element into `tree.insert` via `PipelineCell` propagation).
        assert!(
            child_render_id.is_some(),
            "child element must have a render node before evict"
        );
        assert!(
            grandchild_render_id.is_some(),
            "grandchild element must have a render node before evict"
        );

        // Evict the list item — the whole subtree must disappear.
        let removed = children.evict(0, &mut tree, &mut build_owner.element_owner_mut());

        assert!(removed, "evict reports the child was present");
        assert!(
            tree.get(child).is_none(),
            "top-level lazy child must be removed on evict",
        );
        assert!(
            tree.get(grandchild).is_none(),
            "descendant element must also be removed — single-node remove \
             would leak this grandchild as an orphaned slab entry",
        );

        // Render nodes must also be gone after subtree eviction.
        pipeline.with(|owner| {
            if let Some(rid) = child_render_id {
                assert!(
                    owner.render_tree().get(rid).is_none(),
                    "child render node must be removed on subtree evict",
                );
            }
            if let Some(rid) = grandchild_render_id {
                assert!(
                    owner.render_tree().get(rid).is_none(),
                    "grandchild render node must also be removed on subtree evict — \
                     single-node remove leaks descendant render nodes",
                );
            }
        });
    }

    /// A globally-keyed lazy child pushed to the inactive queue by eviction
    /// must be slab-freed by `finalize_tree` — not left dangling.
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
    /// A GlobalKey moving between two lazy hosts must relocate the existing
    /// element, not panic.
    ///
    /// TWO preconditions block it, and only the first is fixed:
    ///
    /// 1. `ensure` called `ElementTree::insert` with no reconcile guard, so
    ///    `retake_active_global_key`'s `is_reconciling_parent` check failed.
    ///    Fixed — `ensure` now declares `host` for the duration of the insert.
    /// 2. **Still open.** `retake_active_global_key` then verifies the
    ///    candidate is in `from_parent.child_ids`. A lazy host never populates
    ///    `child_ids` — resident children live in the `SparseChildren` map —
    ///    so that reverse-edge check fails, `try_retake_global_key` yields
    ///    `GlobalKeyRetake::Rejected`, and `insert`'s `Rejected` arm panics.
    ///
    /// Closing (2) is a design call, not a patch: either the membership check
    /// stops treating `child_ids` as authoritative, or the lazy path starts
    /// maintaining it. The latter has wider consequences — every walk that
    /// iterates `child_ids` (`collect_render_frontier`, `deactivate_subtree`,
    /// ancestry recompute) currently skips sparse children too.
    #[test]
    #[ignore = "known regression: a lazy host does not maintain child_ids, so \
                retake_active_global_key's reverse-edge membership check rejects \
                the relocation and insert's Rejected arm panics — see the test's \
                own doc comment"]
    fn a_global_key_moving_between_lazy_hosts_relocates_instead_of_panicking() {
        let (mut tree, mut build_owner, pipeline, host_a) = host_tree();
        let host_b = tree.insert(
            &LeafBox { side: 10.0 },
            host_a,
            1,
            &mut build_owner.element_owner_mut(),
        );

        let keyed_item = GlobalKeyedLeafBox {
            side: 4.0,
            key: GlobalKey::<GlobalKeyedLeafBox>::new(),
            detach_count: Arc::new(AtomicUsize::new(0)),
        };

        let mut list_a = SparseChildren::new();
        let first = list_a.ensure(
            0,
            &keyed_item,
            host_a,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        // The same key surfacing under a different lazy host — a keyed item
        // scrolled from one list into another.
        let mut list_b = SparseChildren::new();
        let moved = list_b.ensure(
            0,
            &keyed_item,
            host_b,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert_eq!(
            moved, first,
            "the keyed child must relocate, preserving element identity, not mount a duplicate"
        );
        assert_eq!(
            tree.get(moved).and_then(crate::ElementNode::parent),
            Some(host_b),
            "the relocated child must be reparented onto the new host"
        );
    }

    #[test]
    fn evicted_globally_keyed_child_freed_by_finalize_tree() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();
        let element_count_before = tree.len();

        let global_key = GlobalKey::<GlobalKeyedLeafBox>::new();
        let detach_count = Arc::new(AtomicUsize::new(0));
        let keyed_item = GlobalKeyedLeafBox {
            side: 4.0,
            key: global_key.clone(),
            detach_count: Arc::clone(&detach_count),
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
            detach_count.load(Ordering::SeqCst),
            1,
            "soft removal must detach the render subtree immediately",
        );

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

        assert_eq!(
            detach_count.load(Ordering::SeqCst),
            1,
            "finalization must not detach an already-detached render subtree twice",
        );

        assert!(
            !build_owner.has_inactive_elements(),
            "finalize_tree must drain the inactive queue completely"
        );
        assert_eq!(
            tree.len(),
            element_count_before,
            "the globally-keyed element must be slab-freed by finalize_tree"
        );
        assert!(
            tree.get(child_id).is_none(),
            "the element must no longer be accessible in the tree after finalize_tree"
        );
    }
}

#[cfg(test)]
mod reconcile_tests {
    //! The two-phase reconcile's bookkeeping, at the element tier: the
    //! scenarios the in-place remap could not survive (a shift of two keyed
    //! residents, a swap), plus the panic boundary.

    use std::rc::Rc;

    use flui_foundation::{ValueKey, ViewKey};
    use flui_objects::RenderSizedBox;
    use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
    use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
    use flui_types::geometry::px;

    use super::{SparseChildren, build_item_or_error};
    use crate::view::{RenderView, View};
    use crate::{BoxedView, BuildOwner, ElementTree};

    #[derive(Clone)]
    struct KeyedBox {
        key: ValueKey<u32>,
    }

    impl KeyedBox {
        fn new(id: u32) -> Self {
            Self {
                key: ValueKey::new(id),
            }
        }
    }

    impl RenderView for KeyedBox {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = RenderSizedBox;
        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(10.0)), Some(px(10.0)))
        }
        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for KeyedBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone)]
    struct HostBox;
    impl RenderView for HostBox {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = RenderSizedBox;
        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(100.0)), Some(px(100.0)))
        }
        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }
    impl View for HostBox {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    struct Fixture {
        tree: ElementTree,
        owner: BuildOwner,
        pipeline: PipelineCell,
        host: flui_foundation::ElementId,
        sparse: SparseChildren,
    }

    fn fixture() -> Fixture {
        let pipeline = PipelineCell::new(PipelineOwner::new());
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let host = tree.mount_root_with_pipeline_owner(
            &HostBox,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        Fixture {
            tree,
            owner,
            pipeline,
            host,
            sparse: SparseChildren::new(),
        }
    }

    fn index_of(fx: &Fixture, id: flui_foundation::ElementId) -> Option<usize> {
        let render_id = fx.tree.get(id)?.element().render_id()?;
        fx.pipeline.with(|owner| {
            owner
                .render_tree()
                .get(render_id)?
                .parent_data()?
                .downcast_ref::<SliverMultiBoxAdaptorParentData>()
                .map(|pd| pd.index)
        })
    }

    fn builder_over(ids: Vec<u32>) -> Rc<dyn Fn(usize) -> Option<BoxedView>> {
        Rc::new(move |i| ids.get(i).map(|&id| BoxedView(Box::new(KeyedBox::new(id)))))
    }

    fn seed(fx: &mut Fixture, ids: &[(usize, u32)]) -> Vec<flui_foundation::ElementId> {
        let mut out = Vec::new();
        for &(index, id) in ids {
            let mut element_owner = fx.owner.element_owner_mut();
            out.push(fx.sparse.ensure(
                index,
                &KeyedBox::new(id),
                fx.host,
                &mut fx.tree,
                &mut element_owner,
                &fx.pipeline,
            ));
        }
        out
    }

    /// Residents at 3 and 4 both shift to 4 and 5 (an insert at the head,
    /// reported by the callback): both elements survive, at their new
    /// indices, with their render parent data re-stamped — the in-place
    /// remap orphaned one of them here.
    #[test]
    fn shifting_two_keyed_residents_keeps_both_elements() {
        let mut fx = fixture();
        let seeded = seed(&mut fx, &[(3, 30), (4, 40)]);
        // New data: 99 inserted at the head → 30 is now index 4, 40 index 5.
        let data = vec![0, 1, 2, 99, 30, 40];
        let builder = builder_over(data.clone());
        let find = move |key: &dyn ViewKey| {
            key.as_any()
                .downcast_ref::<ValueKey<u32>>()
                .and_then(|k| data.iter().position(|id| id == k.value()))
        };
        let outcome = {
            let mut element_owner = fx.owner.element_owner_mut();
            fx.sparse.reconcile(
                &*builder,
                Some(&find),
                6,
                fx.host,
                &mut fx.tree,
                &mut element_owner,
                &fx.pipeline,
            )
        };
        assert!(outcome.did_work);
        assert_eq!(outcome.end_reached_at, None);
        assert_eq!(
            fx.sparse.get(4),
            Some(seeded[0]),
            "30 moved to 4, same element"
        );
        assert_eq!(
            fx.sparse.get(5),
            Some(seeded[1]),
            "40 moved to 5, same element"
        );
        assert!(
            fx.sparse
                .get(3)
                .is_some_and(|id| id != seeded[0] && id != seeded[1]),
            "99 mounted fresh at 3"
        );
        assert_eq!(
            index_of(&fx, seeded[0]),
            Some(4),
            "render parent data re-stamped"
        );
        assert_eq!(index_of(&fx, seeded[1]), Some(5));
        assert_eq!(fx.tree.get(seeded[0]).map(|n| n.slot()), Some(4));
    }

    /// A swap within the band, with no callback at all: matched by key.
    #[test]
    fn swapping_two_keyed_residents_needs_no_callback() {
        let mut fx = fixture();
        let seeded = seed(&mut fx, &[(1, 10), (2, 20), (3, 30)]);
        let builder = builder_over(vec![0, 30, 20, 10]);
        let outcome = {
            let mut element_owner = fx.owner.element_owner_mut();
            fx.sparse.reconcile(
                &*builder,
                None,
                4,
                fx.host,
                &mut fx.tree,
                &mut element_owner,
                &fx.pipeline,
            )
        };
        assert!(outcome.did_work);
        assert_eq!(fx.sparse.get(1), Some(seeded[2]));
        assert_eq!(fx.sparse.get(2), Some(seeded[1]));
        assert_eq!(fx.sparse.get(3), Some(seeded[0]));
        assert_eq!(index_of(&fx, seeded[0]), Some(3));
        assert_eq!(index_of(&fx, seeded[2]), Some(1));
        assert_eq!(fx.sparse.len(), 3);
    }

    /// A builder that declines an index below the count reports it, and the
    /// resident there is evicted.
    #[test]
    fn a_declined_index_is_reported_and_its_resident_evicted() {
        let mut fx = fixture();
        seed(&mut fx, &[(0, 10), (1, 20)]);
        let builder = builder_over(vec![10]);
        let outcome = {
            let mut element_owner = fx.owner.element_owner_mut();
            fx.sparse.reconcile(
                &*builder,
                None,
                2,
                fx.host,
                &mut fx.tree,
                &mut element_owner,
                &fx.pipeline,
            )
        };
        assert_eq!(outcome.end_reached_at, Some(1));
        assert_eq!(fx.sparse.len(), 1);
        assert!(fx.sparse.get(1).is_none());
    }

    #[test]
    fn a_panicking_builder_yields_the_error_view_for_that_index_only() {
        let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|i| {
            if i == 1 {
                panic!("boom");
            }
            Some(BoxedView(Box::new(KeyedBox::new(i as u32))))
        });
        assert!(build_item_or_error(&*builder, 0).is_some());
        let recovered = build_item_or_error(&*builder, 1).expect("an error view, not None");
        assert_eq!(
            recovered.0.view_type_id(),
            std::any::TypeId::of::<crate::view::ErrorView>()
        );
        assert!(recovered.0.key().is_none(), "the error view is unkeyed");
    }
}
