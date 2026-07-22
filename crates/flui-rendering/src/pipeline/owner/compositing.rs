//! Compositing phase implementation for `PipelineOwner<Compositing>`.

use flui_foundation::RenderId;
use rustc_hash::FxHashSet;

use crate::pipeline::phase::{Compositing, Idle, PaintPhase};

use super::{PipelineOwner, rebind_phase, subtree_arena::ensure_stack};

// ============================================================================
// Compositing phase: run_compositing
// ============================================================================

impl PipelineOwner<Compositing> {
    /// Transitions a compositing-phase pipeline into the [`PaintPhase`] phase.
    #[must_use]
    pub fn into_paint(self) -> PipelineOwner<PaintPhase> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the compositing phase.
    #[must_use]
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates compositing bits for all dirty render objects.
    ///
    /// Port of Flutter's
    /// `PipelineOwner.flushCompositingBits` + per-object
    /// `RenderObject._updateCompositingBits`
    /// (`.flutter/.../object.dart:3226-3258`). For each entry in
    /// `dirty.needs_compositing` (sorted shallow-first to match
    /// Flutter's `_nodesNeedingCompositingBitsUpdate.sort`), this
    /// method recursively walks the subtree, recomputing
    /// `NEEDS_COMPOSITING` bottom-up:
    ///
    /// 1. If a node does NOT have `NEEDS_COMPOSITING_BITS_UPDATE`
    ///    set, skip (parent walk already covered, or no work to do).
    /// 2. Otherwise: save `old_needs_compositing`, clear current
    ///    `NEEDS_COMPOSITING`, recurse into each child, OR-in each
    ///    child's `NEEDS_COMPOSITING` result.
    /// 3. Force `NEEDS_COMPOSITING = true` if the node is a repaint
    ///    boundary (`IS_REPAINT_BOUNDARY`) or always needs compositing
    ///    (`RenderObject::always_needs_compositing()`).
    /// 4. Three transition cases:
    ///    - **Lost-boundary**: if the node previously was a repaint
    ///      boundary (`WAS_REPAINT_BOUNDARY`) but no longer is, clear
    ///      its accumulated paint state and re-enqueue for paint so
    ///      a new boundary owner picks it up (Flutter object.dart:3246).
    ///    - **Compositing changed**: if `old_needs_compositing !=
    ///      new_needs_compositing`, mark dirty for paint so the
    ///      compositor sees the new shape (Flutter object.dart:3252).
    ///    - **No change**: clear `NEEDS_COMPOSITING_BITS_UPDATE` and
    ///      leave paint state untouched (Flutter object.dart:3255).
    ///
    /// The walk is staged via a private `CompositingWalkActions`
    /// accumulator so that post-walk paint-queue mutations don't
    /// fight the recursive borrows: the recursion reads
    /// `&self.render_tree` (shared) and accumulates actions, then
    /// we apply them under `&mut self`.
    pub fn run_compositing(&mut self) -> crate::error::RenderResult<()> {
        // Empty fast-path: no allocation, no logging churn for the
        // common "nothing changed" frame.
        if !self.scheduler.has_compositing_work() {
            return Ok(());
        }
        let _span = tracing::debug_span!(
            "compositing",
            dirty_nodes = self.scheduler.compositing_queue_len(),
        )
        .entered();

        // Sort shallow-first per Flutter
        // `_nodesNeedingCompositingBitsUpdate.sort((a, b) => a.depth - b.depth)`.
        self.scheduler.sort_compositing_shallow_first();

        // Iterate the dirty list by shared reference: the recursion takes
        // `&self`, which coexists with the slice's shared borrow of
        // the compositing queue (both shared). Queue mutations are deferred
        // into `actions` and applied after the walk, so nothing mutates the
        // list mid-iteration.
        let mut actions = CompositingWalkActions::default();
        let compositing_slice = self.scheduler.compositing_queue_as_slice();
        for node in compositing_slice {
            self.update_subtree_compositing_bits(node.id, &mut actions);
        }

        // Apply paint-queue mutations after the walk completes (under
        // disjoint `&mut self`). Remove-first, then re-enqueue, so an
        // id present in both buckets ends up correctly re-queued at the
        // post-walk depth.
        if !actions.remove_from_paint_queue.is_empty() {
            self.scheduler
                .retain_paint_queue(&actions.remove_from_paint_queue);
        }
        for (id, depth) in actions.mark_needs_paint {
            self.add_node_needing_paint(id, depth);
        }

        // Retain-capacity idiom: `clear()` preserves the Vec's backing
        // capacity across frames.
        self.scheduler.clear_compositing_queue();
        Ok(())
    }

    /// Recursive helper for [`Self::run_compositing`] — see that method's
    /// doc for the algorithm. Operates on shared `&self.render_tree` via
    /// interior-mutability flag accessors; paint-queue side effects are
    /// staged via `actions` for post-walk application.
    fn update_subtree_compositing_bits(&self, id: RenderId, actions: &mut CompositingWalkActions) {
        ensure_stack(|| self.update_subtree_compositing_bits_impl(id, actions));
    }

    /// Body of [`Self::update_subtree_compositing_bits`]; split out so
    /// every recursion level enters through the [`ensure_stack`] probe.
    /// (Its frames are smaller than the layout walk's, so it survived
    /// deeper trees by luck — same crash class, just a later threshold.)
    fn update_subtree_compositing_bits_impl(
        &self,
        id: RenderId,
        actions: &mut CompositingWalkActions,
    ) {
        let Some(node) = self.render_tree.get(id) else {
            return;
        };
        if !node.needs_compositing_bits_update() {
            return;
        }

        let old_needs_compositing = node.needs_compositing();
        node.clear_needs_compositing();

        // Iterate the child slice in-place — both `tree.children(id)`
        // and the recursive `update_subtree_compositing_bits` call are
        // shared borrows of `&self.render_tree`, so they coexist
        // without a clone. An earlier version of this loop cloned
        // children into a fresh `Vec<RenderId>` per visited node (a
        // per-node heap allocation), conflicting with the repo's
        // documented "no per-node child clone" optimization in
        // `RenderTree::visit_depth_first` (`storage/tree.rs:738-751`).
        //
        // Index loop (not iterator) so the loop body can call `&self`
        // recursion without holding the slice iterator across the
        // call (slice iter would borrow `&self.render_tree.children(id)`,
        // which transitively borrows `&self`).
        let child_count = self.render_tree.children(id).len();
        for i in 0..child_count {
            let child_id = self.render_tree.children(id)[i];
            self.update_subtree_compositing_bits(child_id, actions);
            if let Some(child) = self.render_tree.get(child_id)
                && child.needs_compositing()
            {
                node.mark_needs_compositing();
            }
        }

        if node.is_repaint_boundary_flag() || node.always_needs_compositing() {
            node.mark_needs_compositing();
        }

        let new_needs_compositing = node.needs_compositing();
        let is_boundary = node.is_repaint_boundary_flag();
        let was_boundary = node.was_repaint_boundary();

        // Flutter object.dart:3246 — lost-boundary status: drop the
        // accumulated paint state so a NEW boundary parent picks this
        // node up for paint. The id is removed from the dirty paint
        // queue (since the queued paint targeted us-as-a-boundary)
        // and re-enqueued at our current depth so the walk in
        // `mark_needs_paint`'s spirit re-locates the responsible
        // boundary owner.
        if !is_boundary && was_boundary {
            node.clear_needs_paint();
            actions.remove_from_paint_queue.insert(id);
            node.clear_needs_compositing_bits_update();
            let depth = self.render_tree.depth(id).unwrap_or(0) as usize;
            actions.mark_needs_paint.push((id, depth));
        } else if old_needs_compositing != new_needs_compositing {
            // Flutter object.dart:3252 — compositing shape changed:
            // mark paint dirty so the compositor sees the new shape.
            node.clear_needs_compositing_bits_update();
            let depth = self.render_tree.depth(id).unwrap_or(0) as usize;
            actions.mark_needs_paint.push((id, depth));
        } else {
            // Flutter object.dart:3255 — no shape change: just clear
            // the bits-update flag.
            node.clear_needs_compositing_bits_update();
        }
    }
}

/// Side-effects staged during a compositing-bits walk and applied
/// after the recursion under `&mut self`.
///
/// The recursive walk in
/// [`PipelineOwner::update_subtree_compositing_bits`] runs under
/// `&self` (interior-mutability flag access only). Paint-queue
/// mutations (remove-from / push-to `dirty.needs_paint`) can't
/// happen mid-recursion without re-borrowing `&mut self`, so they
/// are recorded here and replayed by [`PipelineOwner::run_compositing`]
/// post-walk.
#[derive(Default)]
struct CompositingWalkActions {
    /// `(id, depth)` pairs to enqueue via `add_node_needing_paint`
    /// after the walk. Either the lost-boundary or compositing-shape-
    /// changed branch may push here.
    mark_needs_paint: Vec<(RenderId, usize)>,
    /// Ids to drop from `dirty.needs_paint` before the re-enqueue
    /// (lost-boundary branch only — a queued paint targeted the node
    /// as-a-boundary, which it no longer is).
    remove_from_paint_queue: FxHashSet<RenderId>,
}
