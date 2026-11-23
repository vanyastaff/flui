//! Deactivated elements awaiting unmount, and the finalize pass that unmounts them.

use super::BuildOwner;
use crate::owner::global_key_reservations;
use crate::tree::ElementTree;
use flui_foundation::ElementId;
use flui_rendering::pipeline::DetachedRenderSubtrees;

/// An element that has been deactivated and is pending unmount.
///
/// Made `pub(crate)` so [`ElementOwner`](super::super::ElementOwner) can hold a
/// `&mut Vec<InactiveElement>` split-borrow reference. End-of-frame
/// finalization (`BuildOwner::finalize_tree`) drains the queue
/// deepest-first using the recorded `depth`.
#[derive(Debug)]
pub(crate) struct InactiveElement {
    id: ElementId,
    depth: usize,
    detached_render_subtrees: Option<DetachedRenderSubtrees>,
}

impl InactiveElement {
    /// Construct a new inactive-element record.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self {
            id,
            depth,
            detached_render_subtrees: None,
        }
    }

    /// Construct a record that also owns the element's render-relocation token.
    pub(crate) fn with_detached_render_subtrees(
        id: ElementId,
        depth: usize,
        detached_render_subtrees: DetachedRenderSubtrees,
    ) -> Self {
        Self {
            id,
            depth,
            detached_render_subtrees: Some(detached_render_subtrees),
        }
    }

    /// The element id queued for end-of-frame unmount.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order finalization (deepest first).
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Whether this record still owns a render-relocation token.
    ///
    /// Lets a retake reject *before* dequeueing: removing an entry it cannot
    /// use would strand the element with no finalization record and no right
    /// to reattach its detached render subtree.
    pub(crate) fn holds_detached_render_subtrees(&self) -> bool {
        self.detached_render_subtrees.is_some()
    }

    /// Take the record's render-relocation token, leaving the record behind.
    pub(crate) fn take_detached_render_subtrees(&mut self) -> Option<DetachedRenderSubtrees> {
        self.detached_render_subtrees.take()
    }
}

impl BuildOwner {
    /// Add an element to the inactive list.
    ///
    /// Called when an element is deactivated (e.g., its parent rebuilds without
    /// it). The element will be unmounted in `finalize_tree()`.
    pub fn add_to_inactive(&mut self, id: ElementId, depth: usize) {
        self.inactive_elements.push(InactiveElement::new(id, depth));
    }

    /// Remove an element from the inactive list and return its relocation token.
    ///
    /// Called when an element is reactivated (e.g., moved via GlobalKey).
    /// A returned token must be reattached or released for finalization.
    #[must_use]
    pub fn remove_from_inactive(&mut self, id: ElementId) -> Option<DetachedRenderSubtrees> {
        let index = self
            .inactive_elements
            .iter()
            .position(|inactive| inactive.id() == id)?;
        self.inactive_elements
            .remove(index)
            .take_detached_render_subtrees()
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
        self.unmount_inactive_elements(tree);
        self.verify_global_key_reservations(tree);
    }

    /// Verify this frame's `GlobalKey` declarations, then clear them.
    ///
    /// Runs at the very end of [`Self::finalize_tree`], AFTER the inactive
    /// sweep, so a key whose only remaining claimant was unmounted this
    /// frame is not reported.
    ///
    /// Reports are appended to the diagnostic drain rather than raised, and
    /// each one has already had its losing parent's dangling child edge
    /// repaired — see [`super::super::global_key_reservations`].
    fn verify_global_key_reservations(&mut self, tree: &mut ElementTree) {
        if self.global_key_reservations.is_empty() {
            return;
        }
        let reports = global_key_reservations::verify(&mut self.global_key_reservations, tree);
        self.global_key_diagnostics.extend(reports);
    }

    /// The inactive-element unmount sweep — [`Self::finalize_tree`]'s first
    /// half, split out so the reservation verification after it runs on
    /// every frame, including the frames with nothing to unmount.
    pub(super) fn unmount_inactive_elements(&mut self, tree: &mut ElementTree) {
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

        let mut elements_to_unmount = Vec::new();
        for inactive in &self.inactive_elements {
            self.collect_elements_to_unmount(tree, inactive.id(), &mut elements_to_unmount);
        }
        tree.preflight_unmount(&elements_to_unmount);

        // Take ownership of inactive elements to avoid borrow conflicts.
        // `mem::take` snapshots the queue before the unmount sweep so
        // mid-iteration `ElementOwner::push_inactive` calls (e.g. children
        // deactivating as a parent unmounts) land in the *next* frame's
        // queue rather than re-entering this drain — same snapshot-then-fire
        // discipline as `ChangeNotifier::notify_listeners` (foundation
        // notifier.rs:158-163).
        let inactive_elements: Vec<_> = std::mem::take(&mut self.inactive_elements);
        let mut inactive_elements = inactive_elements;

        // Consume every linear relocation token before ordinary unmount begins.
        // This authorizes finalization without deleting render nodes or firing
        // callbacks; the existing deepest-first unmount below remains the sole
        // lifecycle and render-disposal path.
        for inactive in &mut inactive_elements {
            let Some(token) = inactive.take_detached_render_subtrees() else {
                continue;
            };
            let pipeline = tree
                .get(inactive.id())
                .and_then(|node| node.element().pipeline_owner())
                .expect(
                    "BUG: an inactive element with a detached render token must retain its pipeline owner",
                );
            pipeline.with_mut(|pipeline_owner| {
                pipeline_owner
                    .release_detached_render_subtrees_for_finalization(token)
                    .expect(
                        "BUG: an inactive element's detached render token must remain valid until finalization",
                    );
            });
        }

        tree.retire_sparse_children(&elements_to_unmount, &self.child_manager_registry);

        // Build the split-borrow handle once for the entire unmount sweep.
        // The handle survives `tree.get_mut` borrows because it points into
        // disjoint `BuildOwner` fields. No live build runs here, so the
        // build-time tree handle is absent.
        let partitioned_dirty_count = self.partitioned_dirty_count();
        let mut element_owner = super::super::ElementOwner {
            global_keys: &mut self.global_keys,
            global_key_reservations: &mut self.global_key_reservations,
            dirty_elements: &mut self.dirty_elements,
            partitioned_dirty_count,
            dirty_reasons: &mut self.dirty_reasons,
            inactive_elements: &mut self.inactive_elements,
            pending_dependency_changes: &mut self.pending_dependency_changes,
            inherited_dependencies: &mut self.inherited_dependencies,
            keep_alive: self.keep_alive.clone(),
            unmount_hooks: self.unmount_hooks.clone(),
            frame_wake: super::super::element_owner::FrameWake::Now(
                self.on_build_scheduled.as_deref(),
            ),
            external_inbox: &self.external_inbox,
            external_request_frame: self.on_build_scheduled.as_ref(),
            build_view: None,
            child_manager_registry: &self.child_manager_registry,
            layout_builder_registry: &self.layout_builder_registry,
            focus_manager: &self.focus_manager,
            lifecycle_handle: &self.lifecycle_handle,
            window_handle: &self.window_handle,
            async_driver: &self.async_driver,
            post_frame_handle: &self.post_frame_handle,
            capabilities: &self.capabilities,
            flush_registry: &self.flush_registry,
            interaction_dispatch: &self.interaction_dispatch,
            collects_semantics: self.collects_semantics,
            hit_test_handle: &self.hit_test_handle,
            global_key_scope: &mut self.global_key_scope,
            owner_tag: self.owner_tag,
            tree_observer: &mut self.tree_observer,
            recovered_panics: &mut self.recovered_panics,
            build_recovered: None,
            reactive: &self.reactive,
            lifecycle_panic_handoff: &self.lifecycle_panic_handoff,
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

        tracing::debug!("Inactive-element sweep complete");
    }

    /// Iteratively collect all element IDs to unmount, parent before
    /// children (pre-order DFS, children in
    /// [`ElementNode::child_ids`](crate::tree::ElementNode) slot order).
    ///
    /// Children come from the slab-resident `child_ids` list — the
    /// single element graph. `finalize_tree` reverses the collected order
    /// so `remove_finalized` runs deepest-first; pre-order guarantees a
    /// parent always precedes every one of its descendants, so the
    /// reversed sweep never frees a parent slot before its children.
    ///
    /// The walk is driven by an explicit `Vec` work-stack instead of
    /// recursion: the element tree nests several times deeper than the
    /// render tree, and a recursive shape overflowed the 1 MiB Windows
    /// main-thread stack on deep chains (a failure class the render-tree walks
    /// also had). To preserve the recursive
    /// shape's visit order on a LIFO stack, children are pushed in
    /// reverse slot order so the leftmost child is popped next — same
    /// discipline as `WidgetsBinding::collect_all_elements`.
    ///
    /// Complexity: O(n) time over the n reachable nodes, average and
    /// worst case (each node pushed/popped exactly once); the work-stack
    /// peaks at O(n) heap in the degenerate all-siblings case and O(tree
    /// height) for a chain. Call-stack usage is constant.
    fn collect_elements_to_unmount(
        &self,
        tree: &ElementTree,
        id: ElementId,
        result: &mut Vec<ElementId>,
    ) {
        let mut stack: Vec<ElementId> = vec![id];
        while let Some(id) = stack.pop() {
            result.push(id);
            // The `tree.get` shared borrow ends with the statement; the
            // extend writes only into the local stack, never the slab.
            tree.stack_owned_children(id, &self.child_manager_registry, &mut stack);
        }
    }
}
