//! Scheduling of layout, paint, compositing and semantics work for nodes, and reads of what is
//! still dirty.

use flui_foundation::RenderId;

use crate::RenderUpdateImpact;

use super::PipelineOwner;

impl PipelineOwner {
    /// Adds a node to the layout dirty list.
    ///
    /// Routes into the mid-phase side queue when layout is active, otherwise
    /// into `dirty.needs_layout`. See `DirtyTracker::add_node_needing_layout`
    /// for the full routing and dedup contract.
    pub(crate) fn add_node_needing_layout(&mut self, node_id: RenderId, depth: usize) {
        self.scheduler.add_node_needing_layout(node_id, depth);
    }

    /// Marks a node as needing layout, propagating the `NEEDS_LAYOUT` flag
    /// up the ancestor chain and pushing the **relayout boundary** onto
    /// `dirty.needs_layout` for the next `run_layout` pass.
    ///
    /// Thin forwarder: the walk logic lives in `DirtyTracker::mark_needs_layout` so it is
    /// unit-testable without a full owner. The `scheduler`, `render_tree`,
    /// and `layout_poison` fields are disjoint, so the split borrow
    /// compiles.
    ///
    /// Every node the walk visits is also un-poisoned: an invalidation mark
    /// means the node's inputs actually changed, which is the one signal
    /// that re-arms a layout-poisoned node for another attempt.
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        self.scheduler
            .mark_needs_layout(&mut self.render_tree, id, &mut |visited| {
                self.layout_poison.unpoison(visited);
            });
    }

    /// Marks a node as needing paint and schedules the nearest established
    /// repaint boundary.
    ///
    /// The invalidation flag propagates toward the root until an existing repaint boundary owns the
    /// affected retained layer. A boundary introduced this frame has no layer
    /// to update yet, so the walk continues to the nearest established owner.
    pub fn mark_needs_paint(&mut self, id: RenderId) {
        self.scheduler.mark_needs_paint(&self.render_tree, id);
    }

    /// Schedules first paint for a freshly attached render object.
    ///
    /// New render state starts with `NEEDS_PAINT` without a queue entry.
    /// Reattachment also enrolls its retained boundaries directly because
    /// detach withdrew their old entries. Ordinary invalidation within an
    /// open attachment interval goes through [`Self::mark_needs_paint`].
    pub(super) fn schedule_initial_paint(&mut self, id: RenderId) {
        let Some(node) = self.render_tree.get(id) else {
            return;
        };
        self.scheduler
            .schedule_paint_boundary(id, node.depth() as usize);
    }

    /// Marks a node's own composited-layer properties dirty without dirtying
    /// anything for paint.
    ///
    /// Ports `RenderObject.markNeedsCompositedLayerUpdate`. Degrades to
    /// [`Self::mark_needs_paint`] when no ancestor boundary owns retained
    /// output to patch, and refuses itself when the node already needs paint —
    /// see `Scheduler::mark_needs_composited_layer_update` for both rules.
    ///
    /// A stale ID is a no-op.
    pub fn mark_needs_composited_layer_update(&mut self, id: RenderId) {
        self.scheduler
            .mark_needs_composited_layer_update(&self.render_tree, id);
    }

    /// Marks compositing bits dirty using the repaint-boundary walk.
    ///
    /// A stale ID is a no-op. The walk marks the necessary ancestor chain and
    /// queues exactly the node responsible for recomputing the affected bits.
    pub fn mark_needs_compositing_bits_update(&mut self, node_id: RenderId) {
        self.scheduler
            .mark_needs_compositing_bits_update(&self.render_tree, node_id);
    }

    /// Marks a live node as needing semantics when semantics are enabled.
    ///
    /// Semantics dirtiness is currently queue-authoritative: the legacy
    /// `NEEDS_SEMANTICS` render-state flag is not set by this method because
    /// the semantics pass does not consume or clear it. Disabled semantics and
    /// stale IDs are no-ops.
    pub fn mark_needs_semantics(&mut self, node_id: RenderId) {
        if !self.semantics_enabled() {
            return;
        }
        let Some(node) = self.render_tree.get(node_id) else {
            return;
        };
        self.scheduler
            .add_node_needing_semantics(node_id, node.depth() as usize);
    }

    /// Marks a live node whose subtree's semantics moved without any
    /// configuration changing, when semantics are enabled: the pass then
    /// re-places the semantics nodes inside the subtree instead of
    /// re-assembling the one above it. Disabled semantics and stale IDs are
    /// no-ops.
    pub fn mark_needs_semantics_geometry(&mut self, node_id: RenderId) {
        if !self.semantics_enabled() {
            return;
        }
        let Some(node) = self.render_tree.get(node_id) else {
            return;
        };
        if self
            .scheduler
            .commit_node_needing_semantics_geometry(node_id, node.depth() as usize)
        {
            self.scheduler.notify_visual_update();
        }
    }

    /// Applies the pipeline work returned by a render-object update.
    ///
    /// Work is scheduled in layout, compositing, paint, then semantics order.
    /// Layout owns its eventual paint, so an impact containing layout never
    /// adds a redundant immediate paint mark.
    pub fn apply_render_update_impact(&mut self, node_id: RenderId, impact: RenderUpdateImpact) {
        if impact.needs_layout() {
            self.mark_needs_layout(node_id);
        }
        if impact.needs_compositing_bits_update() {
            self.mark_needs_compositing_bits_update(node_id);
        }
        if impact.needs_paint() && !impact.needs_layout() {
            self.mark_needs_paint(node_id);
        }
        // "Paint wins" is expressed as a CONDITION, not as ordering. Relying on
        // the marks above having run first is wrong for `LAYOUT`, which
        // contains `PAINT_BIT` but is deliberately not marked here (layout owns
        // its own eventual paint) — so a setter reporting `LAYOUT |
        // COMPOSITED_LAYER_UPDATE` would reach this line with nothing marked
        // and classify its boundary as update-only. It happens to be rescued
        // today by `run_layout` marking every boundary it lays out, but that
        // does not hold for a node whose parent stops laying it out.
        if impact.needs_composited_layer_update() && !impact.needs_paint() {
            self.mark_needs_composited_layer_update(node_id);
        }
        if impact.needs_semantics_update() {
            self.mark_needs_semantics(node_id);
        } else if impact.needs_semantics_geometry_update() {
            self.mark_needs_semantics_geometry(node_id);
        }
    }

    /// Returns the total number of nodes in the canonical phase lists.
    /// Pending invalidations are counted after [`Self::drain_pending_dirty`].
    #[inline]
    pub fn dirty_node_count(&self) -> usize {
        self.scheduler.dirty_node_count()
    }

    /// Returns whether there are any dirty nodes.
    #[inline]
    pub fn has_dirty_nodes(&self) -> bool {
        self.scheduler.has_dirty_nodes() || self.local_dirty.has_pending()
    }

    /// Whether accepted invalidation still awaits owner-thread application.
    ///
    /// This observation never drains requests, marks nodes, wakes observers or
    /// retires ownership. Queued stale requests can conservatively contribute
    /// until the next [`Self::drain_pending_dirty`]. Font changes are observed
    /// after the text context's current measurement loan returns, as in
    /// [`Self::apply_font_change`]; a busy context alone invents no work.
    #[must_use]
    pub fn has_pending_invalidations(&self) -> bool {
        !self.dirty_rx.is_empty()
            || self.local_dirty.has_pending()
            || self
                .text
                .fonts_generation()
                .is_some_and(|generation| generation != self.fonts_seen)
    }

    /// Whether retained layout work needs a fresh layout invalidation before
    /// an automatic frame retry can make progress. Explicit `run_frame` calls
    /// may still inspect the rejection; a changed input resumes ordinary scheduling.
    #[must_use]
    pub fn layout_waits_for_input(&self) -> bool {
        self.scheduler.layout_waits_for_input()
    }

    /// Clears all dirty node lists without processing them.
    ///
    /// Use with caution — this discards pending work.
    pub fn clear_all_dirty_nodes(&mut self) {
        self.scheduler.clear_all();
    }

    /// Drains the mid-phase side queue into the active `dirty` set.
    ///
    /// Thin forwarder to `DirtyTracker::drain_mid_marks`. Available for
    /// external callers (e.g. `flui-app`, integration tests) that need to
    /// drain mid-marks between phase invocations without going through the
    /// full phase-exit flow. The `run_*` phase methods drain via
    /// `DirtyTracker::exit_phase` internally and do NOT call this method.
    #[must_use]
    pub fn drain_mid_layout_marks(&mut self) -> usize {
        self.scheduler.drain_mid_marks()
    }

    /// Directly seed a pending child-build request for wiring tests.
    ///
    /// Mirrors the internal push that `SubtreeArena::request_child_build`
    /// performs during layout, so a cross-crate test (e.g. `flui-app`) can
    /// verify `service_child_requests` wiring without building a full
    /// lazy-sliver tree. Gated behind the `testing` feature (plus this crate's
    /// own `test` cfg) so it never lands in a normal/release build; downstream
    /// crates opt in via `flui-rendering = { features = ["testing"] }` in their
    /// dev-dependencies, exactly as the render-object `testing` harness does.
    #[cfg(any(test, feature = "testing"))]
    pub fn push_pending_child_request_for_test(
        &mut self,
        sliver_id: flui_foundation::RenderId,
        index: usize,
    ) {
        self.pending_child_requests.push((sliver_id, index));
    }

    /// Whether `id` is currently bounded-retry layout-poisoned (see the
    /// crate-internal `LayoutPoison` bounded-retry state).
    ///
    /// Test-only oracle for the bounded-retry suite
    /// (`tests/layout_poison.rs`): no production reader exists. The layout
    /// walk itself consults `LayoutPoison` directly (`subtree_arena.rs`),
    /// never through this accessor. Gated the same way as
    /// [`push_pending_child_request_for_test`](Self::push_pending_child_request_for_test)
    /// so it never lands in a normal/release build.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn is_layout_poisoned(&self, id: RenderId) -> bool {
        self.layout_poison.is_poisoned(id)
    }
}
