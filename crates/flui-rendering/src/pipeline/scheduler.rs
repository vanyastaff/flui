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
use smallvec::smallvec;

use crate::storage::RenderTree;

use super::{
    dirty::{DirtyNode, DirtySets, PaintEntry, PaintKind},
    notifier::VisualUpdateNotifier,
};

// ============================================================================
// PhaseKind — the three phase flags DirtyTracker tracks
// ============================================================================

/// Accepted layout-input changes, independent of queued-work transport.
#[derive(Debug, Clone, Copy)]
pub(super) enum LayoutStamp {
    Tracked(u64),
    Untrackable,
}

impl LayoutStamp {
    fn advance(&mut self) {
        *self = match *self {
            Self::Tracked(value) => value
                .checked_add(1)
                .map_or(Self::Untrackable, Self::Tracked),
            Self::Untrackable => Self::Untrackable,
        };
    }

    pub(super) fn compatible(self, saved: Self) -> bool {
        matches!((self, saved), (Self::Tracked(now), Self::Tracked(previous)) if now == previous)
    }
}

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
    /// layout pipeline; compositing roots selected by the canonical ancestor
    /// walk are routed here while layout is active.
    debug_doing_layout: bool,

    /// True while `run_paint` is iterating `dirty.needs_paint`.
    debug_doing_paint: bool,

    /// True while `run_semantics` is iterating `dirty.needs_semantics`.
    debug_doing_semantics: bool,
    /// Dirty layout entries drained by every `run_layout` so far — monotonic,
    /// so a caller measures a frame as the difference across it (`run_layout`
    /// can run several times per frame, so a per-run reset would report only
    /// the last, usually empty, pass; ADR-0074 §8 telemetry).
    layout_drained_total: u64,

    /// A continuation's premise changes on admission, even when queues dedup.
    layout_stamp: LayoutStamp,

    /// Rejected layout retains its work until a live layout invalidation.
    blocked_layout: Option<RenderId>,

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
            layout_drained_total: 0,
            layout_stamp: LayoutStamp::Tracked(0),
            blocked_layout: None,
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
    /// `on_invalidated` fires once per visited node (the mark target and
    /// every ancestor the walk reaches, boundary included). The owner uses
    /// it to lift layout poison: an invalidation mark is the signal that a
    /// node's properties, children, or tree position actually changed, so a
    /// previously poisoned node earns another layout attempt exactly here.
    ///
    /// The walk is idempotent — a stale call on an already-marked subtree
    /// short-circuits when the flag check repeats. Missing `RenderId`s
    /// (post-removal stale references) are silent no-ops.
    ///
    /// Fires `fire_need_visual_update` on the notifier **only when a new
    /// boundary entry is added** (the Phase-1 wake-on-mark fix).
    pub(super) fn mark_needs_layout(
        &mut self,
        tree: &mut RenderTree,
        id: RenderId,
        on_invalidated: &mut dyn FnMut(RenderId),
    ) {
        if self.commit_needs_layout(tree, id, on_invalidated) {
            self.notifier.read().fire_need_visual_update();
        }
    }

    /// Commit the complete invalidation walk without entering user code.
    pub(super) fn commit_needs_layout(
        &mut self,
        tree: &mut RenderTree,
        id: RenderId,
        on_invalidated: &mut dyn FnMut(RenderId),
    ) -> bool {
        let mut current = id;
        loop {
            // Snapshot the per-node decision under a short-lived borrow so
            // we can release before stepping to the parent in the next
            // iteration.
            let step = {
                let Some(node) = tree.get_mut(current) else {
                    // Stale reference (e.g. node removed mid-frame). Stop.
                    return false;
                };
                if current == id {
                    self.invalidate_layout_premise();
                }
                // Idempotent flag set — the AtomicRenderFlags fetch-or is a
                // no-op when the bit is already set. The walk does NOT
                // short-circuit on "already marked": even with the
                // `run_layout` → `layout_dirty_root` wiring (which clears
                // NEEDS_LAYOUT after each successful layout via
                // `layout_subtree_borrowed`), a stale flag can persist
                // briefly between phases. Always-walking preserves
                // correctness without depending on the precise clearing
                // schedule; idempotence keeps it cheap.
                node.mark_layout_flag();
                on_invalidated(current);
                // A non-empty layout cache means an
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
                // Always enqueue the boundary for this invalidation, with a
                // dedup check so multiple marks-in-same-frame don't push
                // duplicate entries.
                let resumed = self.blocked_layout.take().is_some();
                return self.dirty.needs_layout.push(DirtyNode::new(current, depth)) || resumed;
            }
            // `parent.is_none()` is folded into `is_boundary` above, so
            // reaching this branch guarantees `Some(_)`.
            current = parent.expect(
                "parent must be Some: parent.is_none() case is already handled as is_boundary",
            );
        }
    }

    // =========================================================================
    // mark_needs_paint — the repaint-boundary-walking dirty enqueue
    // =========================================================================

    /// Marks a node as needing paint and schedules the nearest established
    /// repaint boundary.
    ///
    /// Every visited node receives `NEEDS_PAINT`. The walk stops at the first
    /// node that is both a repaint boundary now and was one during the previous
    /// successful paint. A newly-created boundary has no retained layer yet, so
    /// invalidation continues to its parent; the existing ancestor boundary
    /// must repaint once to install it. A parentless node is the final paint
    /// root regardless of its boundary flag.
    ///
    /// This is the canonical paint invalidation path. Callers must not enqueue
    /// an arbitrary dirty render object: doing so loses the isolation contract
    /// that retained layer reuse depends on.
    pub(super) fn mark_needs_paint(&mut self, tree: &RenderTree, id: RenderId) {
        let mut current = id;
        loop {
            let Some(node) = tree.get(current) else {
                return;
            };

            // Idempotence rule: an already-dirty node proves that
            // an earlier walk has marked or reached the same paint owner.
            // Initial attachment uses `schedule_initial_paint` because fresh
            // RenderState deliberately starts dirty.
            if node.needs_paint() {
                return;
            }
            node.mark_paint_flag();
            let parent = node.links().parent();
            let owns_retained_layer =
                node.is_repaint_boundary_flag() && node.was_repaint_boundary();

            if owns_retained_layer || parent.is_none() {
                // Upgrades any composited-layer update already queued for this
                // boundary, in place, rather than withdrawing a separate
                // record — see `PaintQueue::enqueue`. A repaint subsumes an
                // update, and expressing that as a join on one entry is what
                // makes it hold no matter which mark arrived first or whether a
                // pass failed in between.
                self.schedule_paint_boundary(current, node.depth() as usize);
                return;
            }

            current = parent
                .expect("parent must be Some: parentless nodes are scheduled in the branch above");
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
        self.invalidate_layout_premise();
        let resumed = self.blocked_layout.take().is_some();
        let target = if self.debug_doing_layout {
            &mut self.mid_layout_marks.needs_layout
        } else {
            &mut self.dirty.needs_layout
        };
        if !target.push(DirtyNode::new(node_id, depth)) && !resumed {
            return; // already in set — frame already scheduled
        }
        self.notifier.read().fire_need_visual_update();
    }

    /// Schedules a render object that is already known to own the paint output
    /// being invalidated.
    ///
    /// Routes into `mid_layout_marks.needs_paint` when `debug_doing_paint`
    /// is true; otherwise into `dirty.needs_paint`.
    /// Fires the wake only on a new entry.
    ///
    /// This primitive deliberately does not walk the render tree. It is used
    /// only for initial attachment, before a node can have an established
    /// retained layer, and by scheduler-level tests. Runtime invalidation goes
    /// through [`Self::mark_needs_paint`].
    pub(super) fn schedule_paint_boundary(&mut self, node_id: RenderId, depth: usize) {
        self.enqueue_paint(node_id, depth, PaintKind::Repaint);
    }

    /// The ONE place a paint-queue entry is written.
    ///
    /// Routing comes FIRST, and the cross-queue join is deferred to
    /// [`PaintQueue::append`](super::dirty::PaintQueue::append). While a pass
    /// is running every mark goes to the
    /// side queue, even for a boundary already in the main one.
    ///
    /// The obvious-looking alternative — find the id in either queue and
    /// upgrade it in place — is the one that loses work, which is why it is
    /// named here rather than left to be rediscovered: an upgrade written to
    /// the main queue during a pass is invisible to that pass, and
    /// `clear_paint_queue` then deletes it before `exit_phase` drains the side
    /// queue.
    ///
    /// `PaintQueue::enqueue` never downgrades, so callers do not need to check
    /// what is already there — a repaint wins whatever order the marks arrive
    /// in, which is what makes the whole ordering family a non-issue rather
    /// than a set of guards.
    fn enqueue_paint(&mut self, node_id: RenderId, depth: usize, kind: PaintKind) {
        if self.debug_doing_paint {
            // ALWAYS the side queue while a pass is running, even when the id
            // is already in the main one. The running pass snapshotted its
            // dispositions before the walk, so an upgrade written to the main
            // queue cannot affect it — and `run_paint` ends with
            // `clear_paint_queue`, which would then discard that upgrade before
            // `exit_phase` drains the side queue. The mark would be lost
            // outright: the boundary grafts this frame and is not queued the
            // next.
            //
            // Routing here defers the join to `PaintQueue::append`, which
            // upgrades on collision precisely so a repaint raised mid-paint
            // survives being merged into whatever the main queue holds.
            if self
                .mid_layout_marks
                .needs_paint
                .enqueue(node_id, depth, kind)
            {
                self.notifier.read().fire_need_visual_update();
            }
            return;
        }
        // Outside a pass, raise the kind wherever the id already lives — the
        // side queue can still hold entries a previous pass left for the drain.
        if self.dirty.needs_paint.contains(&node_id) {
            self.dirty.needs_paint.enqueue(node_id, depth, kind);
            return;
        }
        if self.mid_layout_marks.needs_paint.contains(&node_id) {
            self.mid_layout_marks
                .needs_paint
                .enqueue(node_id, depth, kind);
            return;
        }
        if !self.dirty.needs_paint.enqueue(node_id, depth, kind) {
            return; // already queued — frame already scheduled
        }
        self.notifier.read().fire_need_visual_update();
    }

    /// Marks a node's own composited-layer properties dirty without dirtying
    /// anything for paint.
    ///
    /// This is the cheap arm of the paint phase: the node's alpha or transform
    /// changed, but nothing it or its children painted did, so the enclosing
    /// repaint boundary can replay its retained output and have just this
    /// node's effect layers rebuilt on the way through.
    ///
    /// Two cases refuse the shortcut:
    ///
    /// - **Paint wins.** A node already needing paint, or already carrying this
    ///   flag, returns immediately. A repaint
    ///   rebuilds the layer from current properties anyway, so an update on top
    ///   of it would be redundant work; and letting the update mark run would
    ///   enqueue a second, weaker entry for a node the walk is going to repaint.
    ///   Efficiency rather than correctness: `run_paint` filters a
    ///   boundary that needs paint out of the update set regardless, so a
    ///   mutation removing this line changes no output — it only lets pointless
    ///   marks through.
    /// - **No retained output, no shortcut.** If no ancestor boundary owns
    ///   output to patch, this degrades to [`Self::mark_needs_paint`]. The flag
    ///   is still left set, so a boundary that gains
    ///   retained output later can serve the node without a fresh mark.
    ///
    /// Unlike `mark_needs_paint` this does **not** flag the nodes it walks
    /// past: the whole point is that they are not dirty. It only finds the
    /// boundary that owns the retained output and enqueues THAT, unflagged, so
    /// `run_paint` reaches it and can tell an update-only entry from a repaint
    /// by asking each queued node whether it needs paint.
    pub(super) fn mark_needs_composited_layer_update(&mut self, tree: &RenderTree, id: RenderId) {
        let Some(node) = tree.get(id) else {
            return;
        };
        if node.needs_paint() || node.needs_composited_layer_update() {
            return;
        }
        node.mark_composited_layer_update_flag();

        // Find the boundary whose retained output contains this node. Same
        // `owns_retained_layer` predicate `mark_needs_paint` stops at, for the
        // same reason: a boundary that has not painted as one yet has nothing
        // to patch.
        //
        // `was_repaint_boundary` is parity and efficiency here rather than
        // correctness — a boundary queued without retained output finds no
        // capture and repaints, which is the same outcome by a longer route, so
        // a mutation dropping it changes no output. It stays because stopping
        // the walk at a boundary that cannot serve the request is the rule the
        // sibling `mark_needs_paint` already follows.
        let mut current = id;
        loop {
            let Some(node) = tree.get(current) else {
                return;
            };
            // `parent.is_some()` is part of the predicate, not an accident: the
            // paint root is never grafted. `run_paint` enters through
            // `paint_subtree(root)` rather than the child-boundary arm that
            // creates and replays captures, so a root boundary — `RenderView`
            // declares itself one — has no retained output an update could
            // patch and would repaint anyway. Queuing it would mean carrying a
            // request that can never be served and re-scanning every capture
            // for it once a frame. Falling through to the parentless arm below
            // degrades to a paint mark, which is what actually happens.
            if node.is_repaint_boundary_flag()
                && node.was_repaint_boundary()
                && node.links().parent().is_some()
            {
                // The requester is recorded in the entry, not just the fact
                // that the boundary has work. A node whose effect layers did
                // not exist when the boundary was captured has no slot to
                // patch, and a patch pass walking only existing slots cannot
                // see it — it would graft the old output and silently drop the
                // request. Knowing the targets lets the graft refuse instead.
                //
                // No "is a repaint already queued" guard is needed: the queue
                // never downgrades, so an entry that is already `Repaint` stays
                // one whatever order the marks arrive in — including a mark
                // that arrives AFTER a pass failed partway, which is where
                // every flag-based version of this guard went blind.
                let depth = node.depth() as usize;
                self.enqueue_paint(current, depth, PaintKind::LayerUpdate(smallvec![id]));
                return;
            }
            let Some(parent) = node.links().parent() else {
                // The paint root itself. There is no enclosing boundary to
                // patch, so this can only be served by repainting.
                self.mark_needs_paint(tree, id);
                return;
            };
            current = parent;
        }
    }

    /// Marks compositing bits dirty and schedules the node responsible for the
    /// update, preserving repaint-boundary transition behavior.
    ///
    /// Established boundaries stop at themselves, introduced or removed
    /// boundaries walk through non-boundary ancestors, and a boundary parent
    /// leaves its child as the responsible queued root.
    pub(super) fn mark_needs_compositing_bits_update(
        &mut self,
        tree: &RenderTree,
        node_id: RenderId,
    ) {
        let mut current = node_id;
        loop {
            let Some(node) = tree.get(current) else {
                return;
            };
            if node.needs_compositing_bits_update() {
                return;
            }
            node.mark_needs_compositing_bits_update();

            let Some(parent_id) = node.links().parent() else {
                self.schedule_compositing_root(current, node.depth() as usize);
                return;
            };
            let Some(parent) = tree.get(parent_id) else {
                self.schedule_compositing_root(current, node.depth() as usize);
                return;
            };
            if parent.needs_compositing_bits_update() {
                return;
            }
            if (!node.was_repaint_boundary() || !node.is_repaint_boundary_flag())
                && !parent.is_repaint_boundary_flag()
            {
                current = parent_id;
                continue;
            }

            self.schedule_compositing_root(current, node.depth() as usize);
            return;
        }
    }

    fn schedule_compositing_root(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_layout {
            &mut self.mid_layout_marks.needs_compositing
        } else {
            &mut self.dirty.needs_compositing
        };
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
        if self
            .blocked_layout
            .is_some_and(|id| removed_ids.contains(&id))
        {
            self.blocked_layout = None;
        }
    }

    /// Clears all dirty work without processing it. Use with caution.
    pub(super) fn clear_all(&mut self) {
        self.dirty.clear();
        self.mid_layout_marks.clear();
        self.blocked_layout = None;
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
        let batch: Vec<DirtyNode> = self.dirty.needs_layout.drain().collect();
        self.layout_drained_total += batch.len() as u64;
        batch
    }

    /// Retains an aborted batch. Authored rejection waits for changed input;
    /// stale measurement remains runnable against input already accepted.
    pub(super) fn retain_layout_batch(&mut self, batch: &[DirtyNode], wait_for_input: bool) {
        for &node in batch {
            self.dirty.needs_layout.push(node);
        }
        self.blocked_layout = if wait_for_input {
            batch.first().map(|node| node.id)
        } else {
            None
        };
    }

    /// Pending work remains observable, but unchanged rejected input is not runnable.
    pub(super) fn layout_waits_for_input(&self) -> bool {
        self.blocked_layout.is_some()
    }

    pub(super) fn layout_stamp(&self) -> LayoutStamp {
        self.layout_stamp
    }

    pub(super) fn invalidate_layout_premise(&mut self) {
        self.layout_stamp.advance();
    }

    /// Resume existing debt after readiness, without admitting new authored input.
    pub(super) fn resume_retained_layout(&mut self) {
        self.blocked_layout = None;
    }

    /// Retain queued layout without repeatedly measuring unavailable input.
    pub(super) fn defer_layout_until_input(&mut self) {
        self.blocked_layout = self
            .dirty
            .needs_layout
            .as_slice()
            .first()
            .map(|node| node.id);
    }

    /// Dirty layout entries drained by every `run_layout` so far.
    pub(super) fn layout_drained_total(&self) -> u64 {
        self.layout_drained_total
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

    /// Removes entries from the paint queue whose id is in `remove_ids`, and
    /// withdraws any composited-layer-update classification they carried.
    ///
    /// Used by the compositing walk's lost-boundary branch: a node that
    /// was a repaint boundary is removed from the paint queue (the old
    /// boundary-targeted entry) and re-enqueued at its new depth.
    ///
    /// The update record has to go with it. It addresses retained output the
    /// node no longer owns, and the re-enqueue that follows walks PAST the node
    /// to an ancestor boundary — so `mark_needs_paint`'s own withdrawal clears
    /// the ancestor's record, never this one. Leaving it behind puts the node
    /// in both classifications at once, which is the state `run_paint` asserts
    /// against and, worse, the one a failed pass turns into a lost repaint.
    pub(super) fn retain_paint_queue(&mut self, remove_ids: &rustc_hash::FxHashSet<RenderId>) {
        self.dirty.needs_paint.retain_not_in(remove_ids);
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
    /// Deepest-first ordering (leaves before ancestors) means boundary
    /// children emit their layers before
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
    /// assembled before children fold into it.
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
    pub(super) fn nodes_needing_paint(&self) -> &[PaintEntry] {
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
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {

    // Scheduler routing matrix: mid-paint repaint survival and mid-phase layout marks.
    #[test]
    fn scheduler_routing_matrix() {
        let cases: &[(&str, fn())] = &[
            (
                "a_repaint_raised_mid_paint_survives_a_boundary_already_queued_as_an_update",
                a_repaint_raised_mid_paint_survives_a_boundary_already_queued_as_an_update,
            ),
            (
                "mid_phase_layout_marks_route_to_side_queue_then_drain_back",
                mid_phase_layout_marks_route_to_side_queue_then_drain_back,
            ),
            (
                "exhausted_layout_premise_keeps_work_deliverable",
                exhausted_layout_premise_keeps_work_deliverable,
            ),
        ];
        for &(name, case) in cases {
            if let Err(payload) = std::panic::catch_unwind(case) {
                eprintln!("matrix case `{name}` failed");
                std::panic::resume_unwind(payload);
            }
        }
    }

    // A public consumer cannot reach the terminal counter in a finite test.
    // Only its initial stamp is seeded privately; real admission and transport
    // paths must continue delivering work while compatibility stays refused.
    fn exhausted_layout_premise_keeps_work_deliverable() {
        let (mut tracker, _) = super::DirtyTracker::new_test_pair();
        tracker.layout_stamp = super::LayoutStamp::Tracked(u64::MAX - 1);
        let first = flui_foundation::RenderId::new(1);
        let second = flui_foundation::RenderId::new(2);
        tracker.add_node_needing_layout(first, 0);
        let last_tracked = tracker.layout_stamp();
        assert!(tracker.layout_stamp().compatible(last_tracked));
        tracker.add_node_needing_layout(first, 0);
        let exhausted = tracker.layout_stamp();
        assert!(!tracker.layout_stamp().compatible(exhausted));
        assert!(!tracker.layout_stamp().compatible(last_tracked));
        let batch = tracker.take_layout_batch_shallow_first();
        assert_eq!(
            batch.len(),
            1,
            "duplicate admission keeps one deliverable root"
        );
        tracker.retain_layout_batch(&batch, true);
        tracker.resume_retained_layout();
        assert!(!tracker.layout_waits_for_input());
        assert!(!tracker.layout_stamp().compatible(exhausted));
        tracker.add_node_needing_layout(second, 1);
        let delivered = tracker.take_layout_batch_shallow_first();
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].id, first);
        assert_eq!(delivered[1].id, second);
        assert!(!tracker.layout_stamp().compatible(tracker.layout_stamp()));
    }

    /// A repaint raised mid-paint for a boundary ALREADY queued as an update
    /// must survive the pass.
    ///
    /// The trap is that the id is already in the MAIN queue, so an
    /// "upgrade wherever it lives" rule raises it there — where the running
    /// pass cannot see it (it snapshotted its dispositions before the walk) and
    /// where `clear_paint_queue` then deletes it, before `exit_phase` drains
    /// the side queue. The mark is lost outright: the boundary grafts this
    /// frame and is not queued the next.
    ///
    /// No production path marks during paint today — `run_paint` takes
    /// `&mut self` while the walk takes `&self` — so this drives the scheduler
    /// directly. `exit_phase` documents the window all the same, and this is
    /// what keeps the routing honest with that contract.
    fn a_repaint_raised_mid_paint_survives_a_boundary_already_queued_as_an_update() {
        let mut tracker = DirtyTracker::new(std::sync::Arc::new(parking_lot::RwLock::new(
            VisualUpdateNotifier::new(),
        )));
        let boundary = RenderId::new(7);

        tracker.enqueue_paint(
            boundary,
            1,
            PaintKind::LayerUpdate(smallvec![RenderId::new(8)]),
        );
        assert!(tracker.dirty.needs_paint.contains(&boundary));

        // The pass starts, snapshots its dispositions, and a repaint arrives.
        tracker.enter_phase(PhaseKind::Paint);
        tracker.enqueue_paint(boundary, 1, PaintKind::Repaint);
        assert!(
            tracker.mid_layout_marks.needs_paint.contains(&boundary),
            "a mark raised during the pass belongs in the side queue, whatever \
             the main queue already holds for that id",
        );

        // The pass ends the way `run_paint` ends: clear, then drain.
        tracker.clear_paint_queue();
        let drained = tracker.exit_phase(PhaseKind::Paint);
        assert_eq!(drained, 1, "the side queue's one entry must drain");

        assert_eq!(
            tracker.dirty.needs_paint.kind_for_test(boundary),
            Some(PaintKind::Repaint),
            "the repaint must reach the next frame's queue; upgrading the main \
             entry instead loses it to `clear_paint_queue`",
        );
    }

    use flui_foundation::RenderId;

    use super::*;

    // =========================================================================
    // Minimal render-object stub for tree-building tests.
    //
    // Implements `RenderObject<BoxProtocol>` with a zero-size leaf layout
    // and a no-op paint — does not panic. Used to build multi-node trees via
    // `PipelineOwner::insert` / `insert_child_render_object` in the
    // mark_needs_layout boundary-walk tests.
    // =========================================================================

    // =========================================================================
    // Wake-on-mark invariant tests (Phase-1 fix regression cluster)
    //
    // These tests exercise DirtyTracker directly via the (tracker, tree) pair
    // — no PipelineOwner, channel, or full Arc<RwLock> construction needed.
    // =========================================================================

    // =========================================================================
    // Mid-phase routing + drain tests
    // =========================================================================

    /// Direct test of the mid-phase routing → drain integration using a
    /// DirtyTracker pair directly (no PipelineOwner needed for this path).
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

    // =========================================================================
    // mark_needs_layout boundary-walk tests
    //
    // These tests need a tree with parent links, so we build one via
    // PipelineOwner's insert API. The walk logic lives in
    // DirtyTracker::mark_needs_layout; the tests exercise it through the thin
    // forwarder `owner.mark_needs_layout` and verify results on
    // `owner.scheduler.dirty`.
    // =========================================================================

    // =========================================================================
    // enter_phase / exit_phase
    // =========================================================================

    // =========================================================================
    // Eviction
    // =========================================================================

    // =========================================================================
    // Finding 2 — paint error-path mid-marks drain (intentional improvement)
    // =========================================================================
}
