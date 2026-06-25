//! `DirtyTracker` — the dirty-work scheduling subsystem for the rendering
//! pipeline.
//!
//! This module owns:
//!
//! - The two [`DirtySets`] pairs (`dirty` and `mid_layout_marks`).
//! - The three `debug_doing_*` phase-guard flags.
//! - A clone of the [`VisualUpdateNotifier`] Arc (the wake sink — the owner
//!   holds its own Arc clone for the callback setters; both point at the same
//!   allocation).
//!
//! **Correctness contract (the wake-on-mark invariant):**
//! Every *new* queue entry fires exactly one `fire_need_visual_update` so a
//! quiescent platform event loop wakes to produce the frame. A duplicate mark
//! (the frame is already scheduled) fires *no* second wake. Mid-phase marks
//! (when a `debug_doing_*` flag is true) route into the matching
//! `mid_layout_marks` side queue and are drained back into `dirty` at phase
//! exit via [`DirtyTracker::drain_mid_marks`].
//!
//! # Why a subsystem
//!
//! The Phase-1 wake-on-mark bug lived exactly at the boundary of dirty-queue
//! membership and wake dispatch — a bug with its own regression test cluster
//! is a subsystem with a contract, not a field group. Moving it here makes the
//! invariant auditable in one place and allows a `(DirtyTracker, RenderTree)`
//! pair to be constructed in tests without a full `PipelineOwner`, channel, or
//! `Arc<RwLock>` setup.
//!
//! See the chief-architect design document §1a for the full rationale.

use flui_foundation::RenderId;
use rustc_hash::FxHashSet;

use crate::storage::RenderTree;

use super::{
    dirty::{DirtyNode, DirtySets},
    notifier::VisualUpdateNotifier,
};

// ============================================================================
// PhaseKind — the three phase flags DirtyTracker tracks
// ============================================================================

/// Identifies one of the three pipeline phases that set a `debug_doing_*`
/// flag.
///
/// Compositing does NOT have its own flag — mid-compositing marks route via
/// [`PhaseKind::Layout`] (the compositing pass runs as part of the layout
/// pipeline per the typestate transitions). This enum models the cross-product
/// without hiding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PhaseKind {
    /// The layout phase — sets `debug_doing_layout`.
    /// Also governs mid-compositing mark routing.
    Layout,
    /// The paint phase — sets `debug_doing_paint`.
    Paint,
    /// The semantics phase — sets `debug_doing_semantics`.
    Semantics,
}

// ============================================================================
// DirtyTracker
// ============================================================================

/// The dirty-work scheduling subsystem for the rendering pipeline.
///
/// Owns the two [`DirtySets`] (active work + mid-phase side queue), the three
/// phase-guard flags, and a clone of the [`VisualUpdateNotifier`] Arc for
/// wake-on-mark.
#[derive(Debug)]
pub(super) struct DirtyTracker {
    /// Active dirty work for the next pipeline frame.
    dirty: DirtySets,

    /// Side queue for marks made WHILE a phase is running.
    ///
    /// When a `debug_doing_*` flag is true the outer phase loop is iterating
    /// its dirty snapshot — pushing into `dirty` mid-iteration would either be
    /// silently ignored (the loop already snapshot via `std::mem::take`) or
    /// processed in the wrong order. Marks made in that window route here;
    /// [`DirtyTracker::drain_mid_marks`] moves them back into `dirty` at phase
    /// exit so the outer `while` condition picks them up.
    mid_layout_marks: DirtySets,

    /// True while `run_layout` is iterating `dirty.needs_layout`.
    ///
    /// Also gates mid-compositing mark routing (compositing runs inside the
    /// layout pipeline; see [`DirtyTracker::add_node_needing_compositing_bits_update`]).
    debug_doing_layout: bool,

    /// True while `run_paint` is iterating `dirty.needs_paint`.
    debug_doing_paint: bool,

    /// True while `run_semantics` is iterating `dirty.needs_semantics`.
    debug_doing_semantics: bool,

    /// Shared wake sink — the same `Arc` that `PipelineOwner` holds for its
    /// callback setters. Both clones point at the same
    /// `RwLock<VisualUpdateNotifier>`.
    notifier: std::sync::Arc<parking_lot::RwLock<VisualUpdateNotifier>>,
}

impl DirtyTracker {
    /// Creates a new `DirtyTracker` sharing `notifier` with the owner.
    pub(super) fn new(notifier: std::sync::Arc<parking_lot::RwLock<VisualUpdateNotifier>>) -> Self {
        Self {
            dirty: DirtySets::new(),
            mid_layout_marks: DirtySets::new(),
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            notifier,
        }
    }

    // =========================================================================
    // Phase entry / exit
    // =========================================================================

    /// Sets the `debug_doing_*` flag for `phase`.
    ///
    /// Call at the top of a `run_*` method, before the phase loop starts.
    /// Pair with [`Self::exit_phase`] to clear the flag and drain mid-phase
    /// marks.
    pub(super) fn enter_phase(&mut self, phase: PhaseKind) {
        match phase {
            PhaseKind::Layout => self.debug_doing_layout = true,
            PhaseKind::Paint => self.debug_doing_paint = true,
            PhaseKind::Semantics => self.debug_doing_semantics = true,
        }
    }

    /// Clears the `debug_doing_*` flag for `phase` and drains mid-phase marks
    /// back into `dirty`.
    ///
    /// Returns the number of mid-phase entries moved (informational). All
    /// callers currently use `let _ =`; the outer loop re-checks the queue
    /// via `has_layout_work()` / `has_paint_work()` etc. rather than
    /// branching on this count.
    ///
    /// Always drains mid-marks regardless of whether the phase completed
    /// successfully or exited early on error — marks routed to the side
    /// queue during the phase window survive into the next frame rather
    /// than being lost.
    #[must_use]
    pub(super) fn exit_phase(&mut self, phase: PhaseKind) -> usize {
        match phase {
            PhaseKind::Layout => self.debug_doing_layout = false,
            PhaseKind::Paint => self.debug_doing_paint = false,
            PhaseKind::Semantics => self.debug_doing_semantics = false,
        }
        self.drain_mid_marks()
    }

    // =========================================================================
    // mark_needs_layout — the boundary-walking dirty enqueue
    // =========================================================================

    /// Marks a node as needing layout, propagating the `NEEDS_LAYOUT` flag
    /// up the ancestor chain and pushing the **relayout boundary** onto
    /// `dirty.needs_layout` for the next `run_layout` pass.
    ///
    /// Takes `&mut RenderTree` because the boundary walk calls
    /// `node.clear_layout_cache()` (a mutable operation) at each visited
    /// ancestor so that any ancestor whose layout consumed this node's
    /// intrinsics re-invalidates. The borrow is disjoint from `&mut self`
    /// (separate fields on `PipelineOwner`) so the split borrow compiles at
    /// the call site.
    ///
    /// The walk is idempotent — a stale call on an already-marked subtree
    /// short-circuits when the flag check repeats. Missing `RenderId`s
    /// (post-removal stale references) are silent no-ops.
    ///
    /// Fires `fire_need_visual_update` on the notifier **only when a new
    /// boundary entry is added** (the Phase-1 wake-on-mark fix).
    pub(super) fn mark_needs_layout(&mut self, tree: &mut RenderTree, id: RenderId) {
        let mut current = id;
        loop {
            // Snapshot the per-node decision under a short-lived borrow so
            // we can release before stepping to the parent in the next
            // iteration.
            let step = {
                let Some(node) = tree.get_mut(current) else {
                    // Stale reference (e.g. node removed mid-frame). Stop.
                    return;
                };
                // Idempotent flag set — the AtomicRenderFlags fetch-or is a
                // no-op when the bit is already set. The walk does NOT
                // short-circuit on "already marked": even with U23's
                // `run_layout` → `layout_dirty_root` wiring (which clears
                // NEEDS_LAYOUT after each successful layout via
                // `layout_subtree_borrowed`), a stale flag can persist
                // briefly between phases. Always-walking preserves
                // correctness without depending on the precise clearing
                // schedule; idempotence keeps it cheap.
                node.mark_layout_flag();
                // Flutter box.dart:2840 — a non-empty layout cache means an
                // ANCESTOR's layout consumed this node's intrinsics/dry
                // layout/baseline, so the invalidation must reach that
                // ancestor: keep walking past a relayout boundary (the
                // boundary only isolates constraint-driven layout, not
                // intrinsic queries). Each ancestor visited clears its own
                // cache the same way, so the escalation chains exactly as
                // far as the cached dependencies go and no further.
                let had_cached_queries = node.clear_layout_cache();
                let parent = node.links().parent();
                let is_boundary =
                    (node.is_relayout_boundary() && !had_cached_queries) || parent.is_none();
                let depth = node.depth() as usize;
                (is_boundary, depth, parent)
            };
            let (is_boundary, depth, parent) = step;
            if is_boundary {
                // Codex P1 (PR #139 review): always enqueue the boundary for
                // this invalidation with a dedup check so multiple
                // marks-in-same-frame don't push duplicate entries.
                if self.dirty.needs_layout.push(DirtyNode::new(current, depth)) {
                    // Wake the platform: an idle event loop must produce a
                    // frame for this invalidation (Flutter parity:
                    // markNeedsLayout → owner.requestVisualUpdate()).
                    // Fired only on a NEW boundary entry — an existing entry
                    // means a frame is already scheduled.
                    self.notifier.read().fire_need_visual_update();
                }
                return;
            }
            // `parent.is_none()` is folded into `is_boundary` above, so
            // reaching this branch guarantees `Some(_)`.
            current = parent.expect(
                "parent must be Some: parent.is_none() case is already handled as is_boundary",
            );
        }
    }

    // =========================================================================
    // Low-level enqueue + wake — one per dirty list
    // =========================================================================

    /// Adds a node to the layout dirty list.
    ///
    /// Routes into `mid_layout_marks.needs_layout` when `debug_doing_layout`
    /// is true (mid-phase routing); otherwise into `dirty.needs_layout`.
    /// Fires the wake only on a new entry.
    pub(super) fn add_node_needing_layout(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_layout {
            &mut self.mid_layout_marks.needs_layout
        } else {
            &mut self.dirty.needs_layout
        };
        if !target.push(DirtyNode::new(node_id, depth)) {
            return; // already in set — frame already scheduled
        }
        self.notifier.read().fire_need_visual_update();
    }

    /// Adds a node to the paint dirty list.
    ///
    /// Routes into `mid_layout_marks.needs_paint` when `debug_doing_paint`
    /// is true; otherwise into `dirty.needs_paint`.
    /// Fires the wake only on a new entry.
    pub(super) fn add_node_needing_paint(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_paint {
            &mut self.mid_layout_marks.needs_paint
        } else {
            &mut self.dirty.needs_paint
        };
        if !target.push(DirtyNode::new(node_id, depth)) {
            return; // already in set — frame already scheduled
        }
        self.notifier.read().fire_need_visual_update();
    }

    /// Adds a node to the compositing bits dirty list.
    ///
    /// Also sets `NEEDS_COMPOSITING_BITS_UPDATE` on the node (via atomic) so
    /// the `run_compositing` walk's per-entry short-circuit cannot silently
    /// skip this entry. Routes via `debug_doing_layout` because compositing
    /// runs as part of the layout pipeline per the typestate transitions —
    /// this is the cross-product: compositing marks share the layout flag.
    ///
    /// Takes `&RenderTree` (shared) because `mark_needs_compositing_bits_update`
    /// is an atomic operation and does not require exclusive tree access.
    pub(super) fn add_node_needing_compositing_bits_update(
        &mut self,
        tree: &RenderTree,
        node_id: RenderId,
        depth: usize,
    ) {
        // Set the bit first so the run_compositing walk doesn't skip this
        // entry on the early-return path. No-op if the id is not present.
        if let Some(node) = tree.get(node_id) {
            node.mark_needs_compositing_bits_update();
        }
        let target = if self.debug_doing_layout {
            &mut self.mid_layout_marks.needs_compositing
        } else {
            &mut self.dirty.needs_compositing
        };
        // The bit is set BEFORE the push so the run_compositing walk's
        // per-entry `needs_compositing_bits_update()` short-circuit cannot
        // silently skip this entry even if `push` returns false (duplicate).
        // Wake fires only on a genuinely-new entry.
        if target.push(DirtyNode::new(node_id, depth)) {
            self.notifier.read().fire_need_visual_update();
        }
    }

    /// Adds a node to the semantics dirty list.
    ///
    /// Routes via `debug_doing_semantics`. Fires the wake on a new entry only.
    pub(super) fn add_node_needing_semantics(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_semantics {
            &mut self.mid_layout_marks.needs_semantics
        } else {
            &mut self.dirty.needs_semantics
        };
        if target.push(DirtyNode::new(node_id, depth)) {
            self.notifier.read().fire_need_visual_update();
        }
    }

    // =========================================================================
    // Mid-marks drain
    // =========================================================================

    /// Drains the mid-phase side queue into the active `dirty` set.
    ///
    /// Called by [`Self::exit_phase`] at the end of every phase (success and
    /// error paths alike). Also available for external callers that need to
    /// drain mid-marks between phase invocations without going through the full
    /// phase-exit flow (e.g., the `PipelineOwner::drain_mid_layout_marks` API
    /// used by `flui-app` and integration tests).
    ///
    /// Returns the total entries moved across all four sets (informational).
    ///
    /// Capacity-preserving: `DirtySet::append` drains the source but keeps its
    /// allocation for the next frame.
    #[must_use]
    pub(super) fn drain_mid_marks(&mut self) -> usize {
        let drained = self.mid_layout_marks.total();
        self.dirty
            .needs_layout
            .append(&mut self.mid_layout_marks.needs_layout);
        self.dirty
            .needs_compositing
            .append(&mut self.mid_layout_marks.needs_compositing);
        self.dirty
            .needs_paint
            .append(&mut self.mid_layout_marks.needs_paint);
        self.dirty
            .needs_semantics
            .append(&mut self.mid_layout_marks.needs_semantics);
        drained
    }

    // =========================================================================
    // Eviction and bulk clear
    // =========================================================================

    /// Evicts all entries for `removed_ids` from both `dirty` and
    /// `mid_layout_marks`. Called by `remove_render_object` before the slab
    /// slots are freed so no phase walks a freed id.
    pub(super) fn evict(&mut self, removed_ids: &FxHashSet<RenderId>) {
        self.dirty.evict(removed_ids);
        self.mid_layout_marks.evict(removed_ids);
    }

    /// Clears all dirty work without processing it. Use with caution.
    pub(super) fn clear_all(&mut self) {
        self.dirty.clear();
        self.mid_layout_marks.clear();
    }

    // =========================================================================
    // Counts / predicates
    // =========================================================================

    /// Returns the total number of entries in `dirty` (excluding mid-marks).
    #[inline]
    pub(super) fn dirty_node_count(&self) -> usize {
        self.dirty.needs_layout.len()
            + self.dirty.needs_compositing.len()
            + self.dirty.needs_paint.len()
            + self.dirty.needs_semantics.len()
    }

    /// Returns `true` when `dirty` has at least one entry in any queue.
    #[inline]
    pub(super) fn has_dirty_nodes(&self) -> bool {
        !self.dirty.needs_layout.is_empty()
            || !self.dirty.needs_compositing.is_empty()
            || !self.dirty.needs_paint.is_empty()
            || !self.dirty.needs_semantics.is_empty()
    }

    /// Returns `true` when any mid-phase marks are pending drain.
    #[inline]
    pub(super) fn has_mid_marks(&self) -> bool {
        self.mid_layout_marks.any()
    }

    // =========================================================================
    // Per-queue counts (for Debug impl and span fields)
    // =========================================================================

    /// Returns the number of entries in the layout dirty queue.
    #[inline]
    pub(super) fn layout_queue_len(&self) -> usize {
        self.dirty.needs_layout.len()
    }

    /// Returns the number of entries in the paint dirty queue.
    #[inline]
    pub(super) fn paint_queue_len(&self) -> usize {
        self.dirty.needs_paint.len()
    }

    /// Returns the number of entries in the compositing dirty queue.
    #[inline]
    pub(super) fn compositing_queue_len(&self) -> usize {
        self.dirty.needs_compositing.len()
    }

    /// Returns the number of entries in the semantics dirty queue.
    #[inline]
    pub(super) fn semantics_queue_len(&self) -> usize {
        self.dirty.needs_semantics.len()
    }

    // =========================================================================
    // Phase-flag accessors
    // =========================================================================

    /// Returns whether the layout phase is currently active.
    #[inline]
    pub(super) fn debug_doing_layout(&self) -> bool {
        self.debug_doing_layout
    }

    /// Returns whether the paint phase is currently active.
    #[inline]
    pub(super) fn debug_doing_paint(&self) -> bool {
        self.debug_doing_paint
    }

    /// Returns whether the semantics phase is currently active.
    #[inline]
    pub(super) fn debug_doing_semantics(&self) -> bool {
        self.debug_doing_semantics
    }

    /// Returns whether any pipeline phase is currently active.
    #[inline]
    pub(super) fn debug_doing_any_phase(&self) -> bool {
        self.debug_doing_layout || self.debug_doing_paint || self.debug_doing_semantics
    }

    // =========================================================================
    // Querying phase work (predicates — for outer while/if conditions)
    // =========================================================================

    /// Returns `true` when the layout queue has at least one entry.
    #[inline]
    pub(super) fn has_layout_work(&self) -> bool {
        !self.dirty.needs_layout.is_empty()
    }

    /// Returns `true` when the compositing queue has at least one entry.
    #[inline]
    pub(super) fn has_compositing_work(&self) -> bool {
        !self.dirty.needs_compositing.is_empty()
    }

    /// Returns `true` when the paint queue has at least one entry.
    #[inline]
    pub(super) fn has_paint_work(&self) -> bool {
        !self.dirty.needs_paint.is_empty()
    }

    // =========================================================================
    // Batch-take methods (sort + drain into a caller-owned Vec)
    // =========================================================================

    /// Sorts the layout queue shallow-first and drains it into a
    /// caller-owned `Vec<DirtyNode>`.
    ///
    /// After this call the layout queue is empty; new marks made during
    /// the iteration (routed to `mid_layout_marks` while
    /// `debug_doing_layout` is true) are drained back by
    /// [`Self::exit_phase`] so the outer `while has_layout_work()`
    /// loop picks them up in the next iteration.
    pub(super) fn take_layout_batch_shallow_first(&mut self) -> Vec<DirtyNode> {
        self.dirty.needs_layout.sort_shallow_first();
        self.dirty.needs_layout.drain().collect()
    }

    // =========================================================================
    // Sort-in-place + slice accessors (for compositing / paint / semantics)
    // =========================================================================

    /// Sorts the compositing queue shallow-first.
    ///
    /// Call before borrowing the queue as a slice via
    /// [`Self::compositing_queue_as_slice`]. Two-step idiom because the
    /// compositing walk iterates a shared slice while deferring mutations
    /// — sort cannot happen mid-iteration.
    pub(super) fn sort_compositing_shallow_first(&mut self) {
        self.dirty.needs_compositing.sort_shallow_first();
    }

    /// Returns a shared slice of the compositing queue.
    ///
    /// Call after [`Self::sort_compositing_shallow_first`]. The slice is
    /// valid until the next `&mut self` operation on this tracker.
    #[inline]
    pub(super) fn compositing_queue_as_slice(&self) -> &[DirtyNode] {
        self.dirty.needs_compositing.as_slice()
    }

    /// Removes entries from the paint queue whose id is in `remove_ids`.
    ///
    /// Used by the compositing walk's lost-boundary branch: a node that
    /// was a repaint boundary is removed from the paint queue (the old
    /// boundary-targeted entry) and re-enqueued at its new depth.
    pub(super) fn retain_paint_queue(&mut self, remove_ids: &rustc_hash::FxHashSet<RenderId>) {
        self.dirty
            .needs_paint
            .retain(|d| !remove_ids.contains(&d.id));
    }

    /// Clears the compositing queue without processing it.
    ///
    /// Retains the Vec's backing capacity for the next frame.
    #[inline]
    pub(super) fn clear_compositing_queue(&mut self) {
        self.dirty.needs_compositing.clear();
    }

    /// Sorts the paint queue deep-first.
    ///
    /// Deepest-first ordering (leaves before ancestors) is the Flutter
    /// `flushPaint` discipline: boundary children emit their layers before
    /// ancestor compositing decisions are resolved.
    pub(super) fn sort_paint_deep_first(&mut self) {
        self.dirty.needs_paint.sort_deep_first();
    }

    /// Clears the paint queue without processing it.
    ///
    /// Retains the Vec's backing capacity for the next frame.
    #[inline]
    pub(super) fn clear_paint_queue(&mut self) {
        self.dirty.needs_paint.clear();
    }

    /// Sorts the semantics queue shallow-first.
    ///
    /// Roots dispatch before their descendants so a parent's config is
    /// assembled before children fold into it (Flutter's `flushSemantics`).
    pub(super) fn sort_semantics_shallow_first(&mut self) {
        self.dirty.needs_semantics.sort_shallow_first();
    }

    /// Clears the semantics queue without processing it.
    ///
    /// Retains the Vec's backing capacity for the next frame.
    #[inline]
    pub(super) fn clear_semantics_queue(&mut self) {
        self.dirty.needs_semantics.clear();
    }

    // =========================================================================
    // Slice accessors (thin views onto dirty sets)
    // =========================================================================

    /// Returns the nodes needing layout.
    #[inline]
    pub(super) fn nodes_needing_layout(&self) -> &[DirtyNode] {
        self.dirty.needs_layout.as_slice()
    }

    /// Returns the nodes needing paint.
    #[inline]
    pub(super) fn nodes_needing_paint(&self) -> &[DirtyNode] {
        self.dirty.needs_paint.as_slice()
    }

    /// Returns the nodes needing compositing bits update.
    #[inline]
    pub(super) fn nodes_needing_compositing_bits_update(&self) -> &[DirtyNode] {
        self.dirty.needs_compositing.as_slice()
    }

    /// Returns the nodes needing semantics update.
    #[inline]
    pub(super) fn nodes_needing_semantics(&self) -> &[DirtyNode] {
        self.dirty.needs_semantics.as_slice()
    }

    // =========================================================================
    // Test-only construction helpers
    // =========================================================================

    /// Constructs a `(DirtyTracker, RenderTree)` pair for unit tests.
    ///
    /// The notifier has no callbacks set — use
    /// [`Self::new_test_pair_with_wake_counter`] to wire a wake counter.
    #[cfg(test)]
    pub(crate) fn new_test_pair() -> (Self, RenderTree) {
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(VisualUpdateNotifier::new()));
        (Self::new(notifier), RenderTree::new())
    }

    /// Constructs a `(DirtyTracker, RenderTree)` pair where the wake callback
    /// increments `wake_counter` on every `fire_need_visual_update` call.
    #[cfg(test)]
    pub(crate) fn new_test_pair_with_wake_counter(
        wake_counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> (Self, RenderTree) {
        let mut notifier = VisualUpdateNotifier::new();
        notifier.set_need_visual_update(move || {
            wake_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(notifier));
        (Self::new(notifier), RenderTree::new())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use flui_foundation::RenderId;
    use rustc_hash::FxHashSet;

    use crate::{
        pipeline::{Idle, owner::PipelineOwner},
        storage::RenderNode,
    };

    use super::*;

    // =========================================================================
    // Minimal render-object stub for tree-building tests.
    //
    // Implements `RenderObject<BoxProtocol>` with a zero-size leaf layout
    // and a no-op paint — does not panic. Used to build multi-node trees via
    // `PipelineOwner::insert` / `insert_child_render_object` in the
    // mark_needs_layout boundary-walk tests.
    // =========================================================================

    #[derive(Debug)]
    struct ZeroSizeLeaf;

    impl flui_foundation::Diagnosticable for ZeroSizeLeaf {}
    impl crate::traits::PaintEffectsCapability for ZeroSizeLeaf {}
    impl crate::traits::SemanticsCapability for ZeroSizeLeaf {}
    impl crate::traits::HotReloadCapability for ZeroSizeLeaf {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for ZeroSizeLeaf {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            Ok(flui_types::Size::ZERO)
        }

        fn paint_raw(
            &self,
            _recorder: &mut crate::context::FragmentRecorder,
            _child_count: usize,
            _size: flui_types::Size,
        ) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
            _size: flui_types::Size,
            _hit_child: &mut (
                     dyn FnMut(
                usize,
                Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
            ) -> bool
                         + Send
                         + Sync
                 ),
        ) -> bool {
            false
        }
    }

    fn insert_zero_size_leaf(owner: &mut PipelineOwner<Idle>) -> RenderId {
        owner.insert(Box::new(ZeroSizeLeaf)
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>)
    }

    // =========================================================================
    // Wake-on-mark invariant tests (Phase-1 fix regression cluster)
    //
    // These tests exercise DirtyTracker directly via the (tracker, tree) pair
    // — no PipelineOwner, channel, or full Arc<RwLock> construction needed.
    // =========================================================================

    /// New dirty work fires the wake exactly once per new queue entry. Duplicate
    /// marks (frame already scheduled) fire no second wake.
    #[test]
    fn dirty_marks_fire_visual_update_once_per_new_entry() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let (mut tracker, _tree) =
            DirtyTracker::new_test_pair_with_wake_counter(Arc::clone(&wake_count));

        tracker.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "a new layout entry must wake the platform",
        );
        tracker.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "a duplicate entry means a frame is already scheduled — no second wake",
        );

        tracker.add_node_needing_paint(RenderId::new(2), 1);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            2,
            "a new paint entry must wake the platform",
        );
        tracker.add_node_needing_paint(RenderId::new(2), 1);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            2,
            "a duplicate paint entry must not fire a second wake",
        );
    }

    /// Compositing marks fire the wake exactly once per new entry (Flutter:
    /// markNeedsCompositingBitsUpdate → owner.requestVisualUpdate).
    #[test]
    fn compositing_mark_fires_visual_update_on_new_entry_and_deduplicates() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let (mut tracker, tree) =
            DirtyTracker::new_test_pair_with_wake_counter(Arc::clone(&wake_count));

        let baseline = wake_count.load(Ordering::Relaxed);

        tracker.add_node_needing_compositing_bits_update(&tree, RenderId::new(10), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_compositing_bits_update: first entry must fire \
             fire_need_visual_update (the GIF-frozen-until-you-scroll bug)"
        );

        tracker.add_node_needing_compositing_bits_update(&tree, RenderId::new(10), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_compositing_bits_update: duplicate entry must not \
             fire a second wake"
        );
    }

    /// Semantics marks fire the wake exactly once per new entry (Flutter:
    /// markNeedsSemanticsUpdate → owner.requestVisualUpdate).
    #[test]
    fn semantics_mark_fires_visual_update_on_new_entry_and_deduplicates() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let (mut tracker, _tree) =
            DirtyTracker::new_test_pair_with_wake_counter(Arc::clone(&wake_count));

        let baseline = wake_count.load(Ordering::Relaxed);

        tracker.add_node_needing_semantics(RenderId::new(20), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_semantics: first entry must fire fire_need_visual_update"
        );

        tracker.add_node_needing_semantics(RenderId::new(20), 0);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "add_node_needing_semantics: duplicate entry must not fire a second wake"
        );
    }

    /// The boundary-walking `mark_needs_layout` fires the wake when it enqueues
    /// the boundary, and stays silent when the boundary is already queued.
    #[test]
    fn mark_needs_layout_fires_visual_update_on_boundary_enqueue() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_clone = Arc::clone(&wake_count);
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                wake_count_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        let id = insert_zero_size_leaf(&mut owner);
        owner.clear_all_dirty_nodes();
        let baseline = wake_count.load(Ordering::Relaxed);

        owner.mark_needs_layout(id);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "enqueueing the relayout boundary must wake the platform",
        );
        owner.mark_needs_layout(id);
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            baseline + 1,
            "boundary already queued — no extra wake",
        );
    }

    // =========================================================================
    // Mid-phase routing + drain tests
    // =========================================================================

    /// Direct test of the U22 mid-phase routing → drain integration using a
    /// DirtyTracker pair directly (no PipelineOwner needed for this path).
    #[test]
    fn mid_phase_layout_marks_route_to_side_queue_then_drain_back() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();

        // Before any phase flag: add goes straight to dirty.
        tracker.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(tracker.nodes_needing_layout().len(), 1);
        assert!(!tracker.has_mid_marks());
        tracker.clear_all();

        // Simulate mid-phase by flipping the flag directly.
        tracker.debug_doing_layout = true;
        tracker.add_node_needing_layout(RenderId::new(1), 0);
        tracker.debug_doing_layout = false;

        assert_eq!(
            tracker.nodes_needing_layout().len(),
            0,
            "mid-phase add must NOT land in dirty.needs_layout",
        );
        assert!(
            tracker.has_mid_marks(),
            "mid-phase add must land in mid_layout_marks",
        );

        // Drain moves the side-queued entry back to dirty.
        let drained = tracker.drain_mid_marks();
        assert_eq!(drained, 1, "drain must report 1 entry moved");
        assert_eq!(
            tracker.nodes_needing_layout().len(),
            1,
            "drained mid-mark must land in dirty.needs_layout",
        );
        assert!(
            !tracker.has_mid_marks(),
            "mid queue must be empty post-drain"
        );
    }

    /// Repeated mid-phase adds for the same id collapse to one entry.
    #[test]
    fn mid_phase_routing_dedups_repeated_marks() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();

        tracker.debug_doing_layout = true;
        tracker.add_node_needing_layout(RenderId::new(1), 0);
        tracker.add_node_needing_layout(RenderId::new(1), 0);
        tracker.add_node_needing_layout(RenderId::new(1), 0);
        tracker.debug_doing_layout = false;

        let drained = tracker.drain_mid_marks();
        assert_eq!(
            drained, 1,
            "3 repeated mid-phase marks must dedup to 1 entry; got {drained}",
        );
    }

    // =========================================================================
    // mark_needs_layout boundary-walk tests
    //
    // These tests need a tree with parent links, so we build one via
    // PipelineOwner's insert API. The walk logic lives in
    // DirtyTracker::mark_needs_layout; the tests exercise it through the thin
    // forwarder `owner.mark_needs_layout` and verify results on
    // `owner.scheduler.dirty`.
    // =========================================================================

    /// Builds a 3-level chain root → middle → leaf and clears the dirty
    /// queues + NEEDS_LAYOUT flags so tests observe only the marks they
    /// explicitly trigger.
    fn build_three_level_chain() -> (PipelineOwner<Idle>, RenderId, RenderId, RenderId) {
        let mut owner = PipelineOwner::new();
        let root_id = insert_zero_size_leaf(&mut owner);
        let middle_id = owner
            .insert_child_render_object(
                root_id,
                Box::new(ZeroSizeLeaf)
                    as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
            )
            .expect("middle should attach under root");
        let leaf_id = owner
            .insert_child_render_object(
                middle_id,
                Box::new(ZeroSizeLeaf)
                    as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
            )
            .expect("leaf should attach under middle");
        owner.clear_all_dirty_nodes();
        for id in [root_id, middle_id, leaf_id] {
            if let Some(node) = owner.render_tree_mut().get_mut(id) {
                match node {
                    RenderNode::Box(entry) => entry.state().clear_needs_layout(),
                    RenderNode::Sliver(entry) => entry.state().clear_needs_layout(),
                }
            }
        }
        (owner, root_id, middle_id, leaf_id)
    }

    /// Marking a leaf where no relayout boundary is set propagates
    /// `NEEDS_LAYOUT` up to root and pushes root onto `dirty.needs_layout`
    /// (root is the implicit boundary).
    #[test]
    fn mark_needs_layout_walks_to_root_when_no_boundary_set() {
        let (mut owner, root_id, middle_id, leaf_id) = build_three_level_chain();
        assert!(owner.nodes_needing_layout().is_empty());

        owner.mark_needs_layout(leaf_id);

        for (id, label) in [(leaf_id, "leaf"), (middle_id, "middle"), (root_id, "root")] {
            let node = owner.render_tree().get(id).expect(label);
            assert!(
                node.needs_layout(),
                "{label} should have NEEDS_LAYOUT set after walk",
            );
        }
        let dirty = owner.nodes_needing_layout();
        assert_eq!(
            dirty.len(),
            1,
            "exactly one boundary should land on dirty queue, got {dirty:?}",
        );
        assert_eq!(dirty[0].id, root_id, "boundary should be the root id");
    }

    /// Re-marking an already-dirty node produces no second push.
    #[test]
    fn mark_needs_layout_is_idempotent_on_repeat() {
        let (mut owner, _root_id, _middle_id, leaf_id) = build_three_level_chain();
        owner.mark_needs_layout(leaf_id);
        let first_count = owner.nodes_needing_layout().len();
        owner.mark_needs_layout(leaf_id);
        assert_eq!(
            owner.nodes_needing_layout().len(),
            first_count,
            "second mark on already-dirty subtree must not re-push",
        );
    }

    /// When an intermediate ancestor is a relayout boundary, propagation stops
    /// there — the root above stays clean and the boundary id is queued.
    #[test]
    fn mark_needs_layout_stops_at_intermediate_relayout_boundary() {
        let (mut owner, root_id, middle_id, leaf_id) = build_three_level_chain();
        if let Some(RenderNode::Box(entry)) = owner.render_tree_mut().get_mut(middle_id) {
            entry.state().set_relayout_boundary(true);
        }

        owner.mark_needs_layout(leaf_id);

        assert!(
            owner
                .render_tree()
                .get(leaf_id)
                .expect("leaf")
                .needs_layout(),
            "leaf should be marked",
        );
        assert!(
            owner
                .render_tree()
                .get(middle_id)
                .expect("middle")
                .needs_layout(),
            "boundary itself should be marked",
        );
        assert!(
            !owner
                .render_tree()
                .get(root_id)
                .expect("root")
                .needs_layout(),
            "root above the boundary stays clean",
        );
        let dirty = owner.nodes_needing_layout();
        assert_eq!(dirty.len(), 1);
        assert_eq!(
            dirty[0].id, middle_id,
            "dirty entry should be the boundary, not the root",
        );
    }

    /// Marking a stale `RenderId` terminates the walk silently with no
    /// dirty-queue mutation.
    #[test]
    fn mark_needs_layout_stale_id_is_silent_noop() {
        let (mut tracker, mut tree) = DirtyTracker::new_test_pair();
        tracker.mark_needs_layout(&mut tree, RenderId::new(99));
        assert!(tracker.nodes_needing_layout().is_empty());
    }

    // =========================================================================
    // enter_phase / exit_phase
    // =========================================================================

    #[test]
    fn enter_exit_phase_layout_sets_and_clears_flag_then_drains() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();
        assert!(!tracker.debug_doing_layout());

        tracker.enter_phase(PhaseKind::Layout);
        assert!(tracker.debug_doing_layout(), "flag must be set after enter");

        tracker.add_node_needing_layout(RenderId::new(5), 0);
        assert!(
            tracker.has_mid_marks(),
            "mid-phase mark lands in side queue"
        );

        let drained = tracker.exit_phase(PhaseKind::Layout);
        assert!(
            !tracker.debug_doing_layout(),
            "flag must be cleared after exit"
        );
        assert_eq!(drained, 1, "exit_phase must drain mid-marks");
        assert_eq!(tracker.nodes_needing_layout().len(), 1);
    }

    #[test]
    fn enter_exit_phase_paint_sets_and_clears_flag_then_drains() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();
        tracker.enter_phase(PhaseKind::Paint);
        assert!(tracker.debug_doing_paint());
        tracker.add_node_needing_paint(RenderId::new(7), 1);
        assert!(tracker.has_mid_marks());
        let drained = tracker.exit_phase(PhaseKind::Paint);
        assert!(!tracker.debug_doing_paint());
        assert_eq!(drained, 1);
        assert_eq!(tracker.nodes_needing_paint().len(), 1);
    }

    #[test]
    fn enter_exit_phase_semantics_sets_and_clears_flag_then_drains() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();
        tracker.enter_phase(PhaseKind::Semantics);
        assert!(tracker.debug_doing_semantics());
        tracker.add_node_needing_semantics(RenderId::new(8), 2);
        assert!(tracker.has_mid_marks());
        let drained = tracker.exit_phase(PhaseKind::Semantics);
        assert!(!tracker.debug_doing_semantics());
        assert_eq!(drained, 1);
        assert_eq!(tracker.nodes_needing_semantics().len(), 1);
    }

    // =========================================================================
    // Eviction
    // =========================================================================

    /// evict removes entries from both dirty and mid_layout_marks.
    #[test]
    fn evict_removes_from_both_queues() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();
        let id = RenderId::new(42);

        tracker.add_node_needing_layout(id, 0);
        // Force a mid-mark by flipping the flag.
        tracker.debug_doing_layout = true;
        tracker.add_node_needing_paint(id, 0);
        tracker.debug_doing_layout = false;

        let mut removed = FxHashSet::default();
        removed.insert(id);
        tracker.evict(&removed);

        assert_eq!(
            tracker.dirty_node_count(),
            0,
            "dirty must be empty after evict"
        );
        assert!(
            !tracker.has_mid_marks(),
            "mid-marks must be empty after evict"
        );
    }

    // =========================================================================
    // Finding 2 — paint error-path mid-marks drain (intentional improvement)
    // =========================================================================

    /// `exit_phase(Paint)` drains mid-paint marks into `dirty.needs_paint`
    /// even when called on the error path.
    ///
    /// Pre-refactor, the `run_paint` error path only cleared
    /// `debug_doing_paint` and did NOT drain `mid_layout_marks`; marks made
    /// between `enter_phase(Paint)` and the error were silently lost. The
    /// always-drain contract of `exit_phase` is the correct behavior: a
    /// mid-paint mark (e.g. a child requesting repaint during a sibling's
    /// paint) survives into the next frame's retry.
    #[test]
    fn exit_phase_paint_drains_mid_marks_on_error_path() {
        let (mut tracker, _tree) = DirtyTracker::new_test_pair();

        // Simulate: enter paint phase.
        tracker.enter_phase(PhaseKind::Paint);
        assert!(tracker.debug_doing_paint(), "flag must be set after enter");

        // Mid-paint mark arrives (routes to side queue because debug_doing_paint is true).
        tracker.add_node_needing_paint(RenderId::new(42), 3);
        assert!(
            tracker.has_mid_marks(),
            "mid-paint mark must land in side queue while debug_doing_paint",
        );
        assert_eq!(
            tracker.nodes_needing_paint().len(),
            0,
            "mid-paint mark must NOT land in dirty.needs_paint while the phase is active",
        );

        // Simulate error path: exit_phase is called before returning Err.
        let drained = tracker.exit_phase(PhaseKind::Paint);

        assert!(
            !tracker.debug_doing_paint(),
            "debug_doing_paint must be cleared after exit_phase",
        );
        assert_eq!(
            drained, 1,
            "exit_phase must drain 1 mid-paint mark even on the error path",
        );
        assert_eq!(
            tracker.nodes_needing_paint().len(),
            1,
            "mid-paint mark must land in dirty.needs_paint after exit_phase \
             so it survives into the next frame's retry",
        );
        assert!(
            !tracker.has_mid_marks(),
            "side queue must be empty after drain",
        );
    }
}
