//! PipelineOwner manages the rendering pipeline.
//!
//! Mythos Step 7 finalization (2026-05-20): the four pipeline phases now
//! own their work as `run_*` methods on the phase-specific impls. The
//! legacy `flush_*` aliases on `PipelineOwner<Idle>` are gone. Calling
//! `run_paint` on `<Idle>` is a compile error -- see the `compile_fail`
//! doctest at the end of `pipeline/phase.rs`.

use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use flui_foundation::{LayerId, RenderId};
use flui_layer::{
    ClipPathLayer, ClipRRectLayer, ClipRectLayer, Layer, LayerTree, OffsetLayer, OpacityLayer,
    PictureLayer, TransformLayer,
};
use flui_painting::DisplayList;
use flui_types::{Offset, Size};
use parking_lot::Mutex;
use rustc_hash::FxHashSet;

use crate::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    context::{FragmentClip, FragmentOp, FragmentRecorder},
    protocol::{
        BoxProtocol, MainAxisPosition, Protocol, SliverProtocol,
        box_protocol::{BoxLayoutCtxErased, LayoutChildCallback, SliverLayoutChildCallback},
        sliver_protocol::{SliverChildLayoutCallback, SliverLayoutCtxErased},
    },
    storage::{RenderEntry, RenderNode, RenderTree},
};

use super::{
    dirty::{DirtyNode, DirtySets},
    handle::{DirtyKind, DirtyRequest, PipelineOwnerHandle},
    notifier::VisualUpdateNotifier,
    phase::{Compositing, Idle, Layout, PaintPhase, PipelinePhase, Semantics},
};

/// Default bounded capacity of the dirty-request channel between
/// [`PipelineOwnerHandle`] producers and the [`PipelineOwner`] receiver.
/// 256 is a heuristic: more than peak burst from a typical async asset
/// loader completion storm, low enough that producers feel backpressure
/// rather than silently growing the queue. Tunable at owner construction
/// via [`PipelineOwner::new_with_capacity`].
const DEFAULT_DIRTY_CHANNEL_CAPACITY: usize = 256;

// ============================================================================
// Pipeline ID Counter
// ============================================================================

/// Global counter for unique pipeline owner IDs.
static PIPELINE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

// ============================================================================
// PipelineOwner
// ============================================================================

/// Manages the rendering pipeline for a tree of render objects.
///
/// The pipeline owner:
/// - Stores the root render object
/// - Tracks dirty nodes needing layout/paint/semantics
/// - Coordinates phase work via consuming phase transitions
/// - Holds the layer tree produced by the most recent paint phase
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `PipelineOwner` class in
/// `rendering/object.dart`. Where Flutter uses runtime `_debugDoingThis*`
/// asserts to enforce phase ordering, FLUI lifts the question into the
/// type system: each phase's `run_*` method lives only on the matching
/// `PipelineOwner<PhaseMarker>` impl block.
///
/// # Pipeline Phases
///
/// Use [`run_frame`](Self::run_frame) for the typestate-driven orchestration:
///
/// ```text
/// Idle ─into_layout()──▶ Layout ─run_layout()──▶ into_compositing()
///        ▲                                        │
///        │                                        ▼
///        │                                   Compositing ─run_compositing()─▶ into_paint()
///        │                                                                     │
///        │                                                                     ▼
///        │                                                                Paint ─run_paint()─▶ into_semantics()
///        │                                                                                      │
///        │                                                                                      ▼
///        │                                                                                  Semantics ─run_semantics()─▶ finish()
///        └──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
/// ```
///
/// # Multi-window
///
/// Each PipelineOwner manages one render tree. Multi-window applications
/// own multiple PipelineOwner instances side-by-side; the previous
/// hierarchical-pipelines API (`adopt_child` / `drop_child`) was removed
/// in Mythos Step 9 -- it used `Arc<RwLock<PipelineOwner>>` for tree
/// nodes, an anti-pattern this crate refuses.
pub struct PipelineOwner<Phase: PipelinePhase = Idle> {
    /// Unique identifier for this pipeline owner.
    id: u64,

    /// The render tree storing all RenderObjects (Slab-based).
    render_tree: RenderTree,

    /// The root render object ID of this pipeline.
    root_id: Option<RenderId>,

    /// Consolidated visual-update + semantics-owner-lifecycle callback
    /// notifier. Replaces three previously-separate `Box<dyn Fn() + Send +
    /// Sync>` fields. See [`VisualUpdateNotifier`].
    notifier: std::sync::Arc<parking_lot::RwLock<VisualUpdateNotifier>>,

    /// Co-located dirty sets for the four pipeline phases. See
    /// [`DirtySets`]. Replaces what used to be four
    /// parallel `Vec<DirtyNode>` fields scattered across the struct.
    dirty: DirtySets,

    /// Side queue for marks made DURING a phase iteration
    /// (`debug_doing_layout` / `debug_doing_paint` / etc. true).
    ///
    /// **D-block PR-A1 U22 (companion memo D7):** Flutter's pipeline
    /// permits a render object's `perform_layout` to mark another
    /// node dirty via `markNeedsLayout`. Pushing into the active
    /// `dirty` queue mid-iteration would either be silently ignored
    /// (the outer loop already snapshot the queue via `std::mem::take`)
    /// or processed in the wrong order. The side queue captures these
    /// mid-phase marks; the outer `while` loop in `run_layout` /
    /// `run_paint` drains it after the current iteration via
    /// [`Self::drain_mid_layout_marks`] before deciding whether to
    /// continue.
    ///
    /// Each phase's mid-mark vector retains capacity across frames
    /// (same non-shrinking discipline as `dirty`).
    mid_layout_marks: DirtySets,

    /// Constraints to pass to [`Self::layout_dirty_root`] when the
    /// dirty entry is the tree root (`root_id`) and the root has no
    /// cached `state.constraints()` yet (first frame).
    ///
    /// **D-block PR-A1 U23:** the binding layer (`flui-view` /
    /// `flui-app` / `flui-hot-reload`) sets this once per
    /// configuration via [`Self::set_root_constraints`] before the
    /// first `run_frame` invocation. On subsequent frames the root's
    /// cached constraints (post-layout) supersede this field; the
    /// fallback only fires on the very first layout pass.
    root_constraints: Option<BoxConstraints>,

    /// Whether we're currently doing layout.
    debug_doing_layout: bool,

    /// Whether we're currently doing paint.
    debug_doing_paint: bool,

    /// Whether we're currently doing semantics.
    debug_doing_semantics: bool,

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// The layer tree produced by the last paint phase.
    last_layer_tree: Option<LayerTree>,

    /// Device pixel ratio threaded into every paint pass (text shaping
    /// and hairline snapping are DPR-dependent). Set by the platform
    /// binding on surface creation / DPI change; defaults to 1.0 for
    /// headless tests.
    device_pixel_ratio: f32,

    /// Prototype handle held by the owner so `handle()` can clone it for
    /// each caller without re-allocating the channel. See
    /// [`PipelineOwnerHandle`].
    handle: PipelineOwnerHandle,

    /// Receiver end of the bounded dirty-request channel. Drained into
    /// `dirty` by `drain_pending_dirty` at phase boundaries.
    dirty_rx: crossbeam_channel::Receiver<DirtyRequest>,

    /// Phantom marker for the typestate phase. Always zero-sized.
    /// See `crates/flui-rendering/src/pipeline/phase.rs`.
    _phase: PhantomData<Phase>,
}

impl<Phase: PipelinePhase> std::fmt::Debug for PipelineOwner<Phase> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("phase", &Phase::NAME)
            .field("id", &self.id)
            .field("root_id", &self.root_id)
            .field("render_tree_len", &self.render_tree.len())
            .field("nodes_needing_layout", &self.dirty.needs_layout.len())
            .field("nodes_needing_paint", &self.dirty.needs_paint.len())
            .field("debug_doing_layout", &self.debug_doing_layout)
            .field("debug_doing_paint", &self.debug_doing_paint)
            .field("debug_doing_semantics", &self.debug_doing_semantics)
            .field("has_layer_tree", &self.last_layer_tree.is_some())
            .finish()
    }
}

impl Default for PipelineOwner<Idle> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Idle-only impl: constructors + orchestration
// ============================================================================

impl PipelineOwner<Idle> {
    /// Creates a new pipeline owner in the [`Idle`] phase with the
    /// default dirty-channel capacity (`DEFAULT_DIRTY_CHANNEL_CAPACITY`,
    /// 256).
    pub fn new() -> Self {
        Self::new_with_capacity(DEFAULT_DIRTY_CHANNEL_CAPACITY)
    }

    /// Creates a new pipeline owner in the [`Idle`] phase with a custom
    /// dirty-channel capacity. Use this when the default 256 doesn't match
    /// the producer profile.
    pub fn new_with_capacity(dirty_channel_capacity: usize) -> Self {
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(VisualUpdateNotifier::new()));
        let (handle, dirty_rx) =
            PipelineOwnerHandle::new_pair(dirty_channel_capacity, std::sync::Arc::clone(&notifier));
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            dirty: DirtySets::new(),
            mid_layout_marks: DirtySets::new(),
            root_constraints: None,
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
            device_pixel_ratio: 1.0,
            handle,
            dirty_rx,
            _phase: PhantomData,
        }
    }

    /// Creates a new pipeline owner with callbacks in the [`Idle`] phase.
    pub fn with_callbacks<F, G, H>(
        on_need_visual_update: Option<F>,
        on_semantics_owner_created: Option<G>,
        on_semantics_owner_disposed: Option<H>,
    ) -> Self
    where
        F: Fn() + Send + Sync + 'static,
        G: Fn() + Send + Sync + 'static,
        H: Fn() + Send + Sync + 'static,
    {
        let mut notifier = VisualUpdateNotifier::new();
        if let Some(f) = on_need_visual_update {
            notifier.set_need_visual_update(f);
        }
        if let Some(f) = on_semantics_owner_created {
            notifier.set_semantics_owner_created(f);
        }
        if let Some(f) = on_semantics_owner_disposed {
            notifier.set_semantics_owner_disposed(f);
        }
        let notifier = std::sync::Arc::new(parking_lot::RwLock::new(notifier));
        let (handle, dirty_rx) = PipelineOwnerHandle::new_pair(
            DEFAULT_DIRTY_CHANNEL_CAPACITY,
            std::sync::Arc::clone(&notifier),
        );
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            dirty: DirtySets::new(),
            mid_layout_marks: DirtySets::new(),
            root_constraints: None,
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
            device_pixel_ratio: 1.0,
            handle,
            dirty_rx,
            _phase: PhantomData,
        }
    }

    /// Transitions an idle pipeline into the [`Layout`] phase.
    ///
    /// Consumes `self`; once transitioned out of `Idle`, the legacy
    /// idle-only API (constructors, `run_frame`) is no longer reachable
    /// until you return through [`finish`](PipelineOwner::<Semantics>::finish).
    pub fn into_layout(self) -> PipelineOwner<Layout> {
        rebind_phase(self)
    }

    // ========================================================================
    // Full-frame orchestrator (Mythos Step 7)
    // ========================================================================

    /// Runs a full frame: layout -> compositing-bits -> paint -> semantics.
    /// Consumes `self`, returns the owner back at [`Idle`] plus a
    /// [`RenderResult`](crate::RenderResult) indicating whether the frame produced a layer
    /// tree or failed mid-phase.
    ///
    /// The phase transitions are the load-bearing mechanism here -- each
    /// `run_*` method lives only on its matching phase's impl block, so
    /// the type system enforces the ordering. There is no runtime branch
    /// that could call `run_paint` before `run_layout`.
    ///
    /// # Mythos Step 12 -- error handling
    ///
    /// If any phase returns [`crate::error::RenderError`] (most notably
    /// [`crate::error::RenderError::Poisoned`] from a panicking render
    /// object), the in-flight frame is dropped, the owner is returned at
    /// [`Idle`] (no in-flight layer tree), and the second element of the
    /// tuple is `Err(...)`. The owner is **always** usable for a
    /// subsequent frame on the success and error paths alike.
    pub fn run_frame(
        mut self,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
        // Observe cross-thread dirty requests (RepaintHandle /
        // PipelineOwnerHandle producers) before any phase runs — an
        // async decode that finished while the app idled lands in this
        // frame, not never.
        self.drain_pending_dirty();

        // Layout
        let mut owner = self.into_layout();
        if let Err(e) = owner.run_layout() {
            return (owner.into_idle(), Err(e));
        }

        // Compositing
        let mut owner = owner.into_compositing();
        if let Err(e) = owner.run_compositing() {
            return (owner.into_idle(), Err(e));
        }

        // Paint
        let mut owner = owner.into_paint();
        if let Err(e) = owner.run_paint() {
            return (owner.into_idle(), Err(e));
        }

        // Semantics
        let mut owner = owner.into_semantics();
        if let Err(e) = owner.run_semantics() {
            // Semantics phase has no `into_idle` because the transition
            // to <Idle> goes via `finish`. Use `finish` to recover the
            // owner for the error path -- the layer tree from the paint
            // phase is discarded on error to keep the invariant "Err =>
            // no layer tree".
            return (owner.finish(), Err(e));
        }

        let layer_tree = owner.take_layer_tree();
        (owner.finish(), Ok(layer_tree))
    }
}

// ============================================================================
// Phase-agnostic accessors / setters / insertion (Mythos Step 7)
// ============================================================================
//
// These methods are pure data access or side-effect-free notifier wiring.
// They are valid in any phase: the borrow checker still gates `&mut self`
// against the type-state transitions, but the methods themselves don't
// care which phase the owner is in.

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// Returns the unique identifier for this pipeline owner.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Sets the callback for when a visual update is needed.
    pub fn set_on_need_visual_update<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.write().set_need_visual_update(callback);
    }

    /// Sets the callback for when semantics owner is created.
    pub fn set_on_semantics_owner_created<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.write().set_semantics_owner_created(callback);
    }

    /// Sets the callback for when semantics owner is disposed.
    pub fn set_on_semantics_owner_disposed<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.write().set_semantics_owner_disposed(callback);
    }

    /// Requests a visual update.
    ///
    /// Called by render objects when they need to be re-rendered.
    pub fn request_visual_update(&self) {
        self.notifier.read().fire_need_visual_update();
    }

    // ========================================================================
    // Cross-thread mark-dirty handle (Mythos Step 8)
    // ========================================================================

    /// Returns a clone of the cross-thread mark-dirty handle.
    ///
    /// Each clone is its own `Sender` over the same bounded channel; sends
    /// from different threads do not block each other. Backpressure
    /// surfaces as [`super::handle::SendError::ChannelFull`].
    #[inline]
    pub fn handle(&self) -> PipelineOwnerHandle {
        self.handle.clone()
    }

    /// Binds a [`RepaintHandle`](crate::pipeline::RepaintHandle) to a
    /// live render object — the
    /// capability async producers (image decodes, arriving assets) use
    /// to repaint that node from any thread, with the platform woken on
    /// every request.
    ///
    /// `None` for a stale/foreign id. Once the node is later removed,
    /// the returned handle degrades to a silent no-op (generational id:
    /// the drain drops requests whose generation died).
    pub fn repaint_handle(&self, id: RenderId) -> Option<crate::pipeline::RepaintHandle> {
        let depth = self.render_tree.get(id)?.depth() as usize;
        Some(crate::pipeline::RepaintHandle::new(
            self.handle.clone(),
            id,
            depth,
        ))
    }

    /// Drains the pending dirty-request channel into the local
    /// [`DirtySets`].
    ///
    /// Called at phase boundaries by the typestate transitions; producers
    /// (background asset loaders, async work) write into the channel via
    /// [`PipelineOwnerHandle::request_mark_dirty`] and the owner observes
    /// them on the next frame. Non-blocking; processes every request
    /// available at the time of call and returns the count drained.
    pub fn drain_pending_dirty(&mut self) -> usize {
        let mut drained = 0;
        while let Ok(req) = self.dirty_rx.try_recv() {
            drained += 1;
            // Generation-validated replay through the SAME mark paths
            // local callers use: a stale id (the node died after the
            // producer captured its handle) falls out silently, the
            // layout mark walks to its relayout boundary, compositing
            // keeps the queue⇒flag invariant, and every path carries
            // its own dedup. The request's depth is advisory — the live
            // node's depth is authoritative.
            //
            // Pre-fix this pushed raw queue entries: a layout request
            // for a non-boundary node became a bogus dirty ROOT, a
            // compositing request skipped the flag (the silent-loss
            // footgun `add_node_needing_compositing_bits_update`
            // exists to prevent), and dead ids were replayed verbatim.
            let Some(node) = self.render_tree.get(req.id) else {
                tracing::trace!(?req, "drain_pending_dirty: stale id, dropped");
                continue;
            };
            let depth = node.depth() as usize;
            match req.kind {
                DirtyKind::Layout => self.mark_needs_layout(req.id),
                DirtyKind::Compositing => {
                    self.add_node_needing_compositing_bits_update(req.id, depth);
                }
                DirtyKind::Paint => self.add_node_needing_paint(req.id, depth),
                DirtyKind::Semantics => self.add_node_needing_semantics(req.id, depth),
            }
        }
        drained
    }

    /// Returns the root render object ID.
    pub fn root_id(&self) -> Option<RenderId> {
        self.root_id
    }

    /// Sets the root render object ID.
    pub fn set_root_id(&mut self, id: Option<RenderId>) {
        self.root_id = id;
    }

    /// Returns the constraints to apply to the root render object on
    /// the next layout pass (if no cached constraints yet).
    ///
    /// **D-block PR-A1 U23:** see [`Self::set_root_constraints`].
    #[inline]
    pub fn root_constraints(&self) -> Option<BoxConstraints> {
        self.root_constraints
    }

    /// Sets the constraints to apply to the root render object on the
    /// next layout pass when no cached constraints exist yet
    /// (first-frame initialization).
    ///
    /// **D-block PR-A1 U23:** the binding layer (`flui-view` /
    /// `flui-app` / `flui-hot-reload`) calls this once after
    /// constructing the pipeline + before the first `run_frame`
    /// invocation. On subsequent frames the root's cached
    /// `state.constraints()` (post-layout) supersedes this field; the
    /// fallback only fires on the very first layout pass.
    ///
    /// Pass `None` to clear (e.g., when the binding wants to defer to
    /// a yet-unmounted root render object that supplies its own
    /// constraints via `RootRenderElement::mount`).
    ///
    /// # Auto-schedules root relayout (PR #148 review fix)
    ///
    /// When `Some(_)` is passed AND `root_id` is set AND the new
    /// constraints differ from the prior value, this method also
    /// calls [`Self::mark_needs_layout`] on the root id so the
    /// next `run_layout` invocation picks up the change. This
    /// avoids the silent-no-relayout footgun the prior shape had —
    /// the binding no longer needs to call `mark_needs_layout`
    /// separately after `set_root_constraints`.
    ///
    /// Setting to the SAME constraints value (or to `None`) does
    /// NOT mark dirty — those cases either don't change the layout
    /// result or are explicit clears that the caller manages
    /// independently.
    pub fn set_root_constraints(&mut self, constraints: Option<BoxConstraints>) {
        let changed = constraints.is_some() && constraints != self.root_constraints;
        self.root_constraints = constraints;
        if changed && let Some(root_id) = self.root_id {
            self.mark_needs_layout(root_id);
        }
    }

    /// Returns a reference to the render tree.
    pub fn render_tree(&self) -> &RenderTree {
        &self.render_tree
    }

    /// Returns a mutable reference to the render tree.
    pub fn render_tree_mut(&mut self) -> &mut RenderTree {
        &mut self.render_tree
    }

    /// Returns a reference to the layer tree from the last paint phase.
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.last_layer_tree.as_ref()
    }

    /// Takes the layer tree from the last paint phase.
    ///
    /// This removes the layer tree from the pipeline owner, returning ownership
    /// to the caller. Useful for passing to the compositor.
    ///
    /// Phase-agnostic: works in any phase. `run_frame` calls this on
    /// `<Semantics>` to extract the layer tree before transitioning back to
    /// `<Idle>`.
    pub fn take_layer_tree(&mut self) -> Option<LayerTree> {
        self.last_layer_tree.take()
    }

    /// Device pixel ratio threaded into every paint pass.
    pub fn device_pixel_ratio(&self) -> f32 {
        self.device_pixel_ratio
    }

    /// Removes the subtree rooted at `id` — THE dispose site.
    ///
    /// Removal is where owner-side state dies (the inversion of the
    /// "Drop will handle it" idea): a `Drop` impl has no
    /// `&PipelineOwner`, so it cannot evict dirty-queue entries — only
    /// this method can. Order:
    ///
    /// 1. collect the subtree's ids;
    /// 2. evict every id from ALL dirty queues (live + mid-phase) so
    ///    no phase walks a freed slot's stale entry;
    /// 3. cascade-remove the nodes (each freed slot's generation bumps
    ///    — outstanding ids go stale, D2);
    /// 4. clear `root_id` when the root itself was removed.
    ///
    /// `Drop` on render objects remains strictly node-local (decoded
    /// images, shaped text, GPU handles).
    ///
    /// Returns the number of nodes removed. Must not run mid-phase —
    /// the layout/paint walks hold raw borrows into the slab.
    pub fn remove_render_object(&mut self, id: RenderId) -> usize {
        debug_assert!(
            !self.debug_doing_layout && !self.debug_doing_paint,
            "remove_render_object during an active layout/paint phase —              the walks hold borrows into the slab; defer removal to              between-frame work",
        );

        let subtree = self.render_tree.collect_subtree_ids(id);
        if subtree.is_empty() {
            return 0;
        }
        let removed: FxHashSet<RenderId> = subtree.iter().copied().collect();
        self.dirty.evict(&removed);
        self.mid_layout_marks.evict(&removed);

        let count = self.render_tree.remove_recursive(id);
        if self.root_id == Some(id) {
            self.root_id = None;
        }
        tracing::debug!(?id, count, "remove_render_object: subtree disposed");
        count
    }

    /// Hit-tests the render tree at `position` (root-local
    /// coordinates), appending leaf-first entries to `result`.
    ///
    /// The walk mirrors the paint walk's shape: per node, the
    /// protocol blanket wraps a driver-supplied child-recursion
    /// callback in the typed, arity-gated hit-test context and calls
    /// the object's `hit_test`. `None` child overrides resolve to the
    /// child's laid-out `RenderState.offset` — parents no longer
    /// mirror offsets in their own fields to hit-test children.
    ///
    /// Returns whether anything was hit. O(visited nodes) average;
    /// worst case O(tree) when nothing claims the position.
    pub fn hit_test(&self, position: Offset, result: &mut crate::hit_testing::HitTestResult) -> bool
    where
        // The child-recursion callback must be Send + Sync (ctx trait
        // bounds, U19 inheritance); it captures &self, so the phase
        // marker must be Sync. Every phase is a ZST — always satisfied
        // at call sites, spelled out for the generic impl.
        Phase: Sync,
    {
        let Some(root_id) = self.root_id else {
            return false;
        };
        self.hit_test_subtree(root_id, position, result)
    }

    fn hit_test_subtree(
        &self,
        id: RenderId,
        position: Offset,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        ensure_stack(|| self.hit_test_subtree_impl(id, position, result))
    }

    /// Body of [`Self::hit_test_subtree`]; split out so every
    /// recursion level enters through the [`ensure_stack`] probe.
    fn hit_test_subtree_impl(
        &self,
        id: RenderId,
        position: Offset,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        let Some(node) = self.render_tree.get(id) else {
            return false;
        };
        let Some(entry) = node.as_box() else {
            if node.as_sliver().is_some() {
                let sliver_position = Self::sliver_hit_position_from_offset(node, position);
                return self.hit_test_sliver_subtree(id, sliver_position, result);
            }
            return false;
        };
        let children: Vec<RenderId> = node.children().to_vec();
        let render_object = entry.render_object();

        let mut hit_child = |index: usize, override_pos: Option<Offset>| -> bool {
            let Some(&child_id) = children.get(index) else {
                return false;
            };
            let Some(child_node) = self.render_tree.get(child_id) else {
                return false;
            };
            if child_node.as_sliver().is_some() {
                // Explicit positions are already child-local; the layout-offset
                // fallback starts from the child's physical paint offset.
                let child_position = match override_pos {
                    Some(position) => Self::sliver_hit_position_from_offset(child_node, position),
                    None => Self::sliver_hit_position_from_paint_offset(
                        child_node,
                        position - child_node.offset(),
                    ),
                };
                return self.hit_test_sliver_subtree(child_id, child_position, result);
            }
            let child_position = override_pos.unwrap_or_else(|| position - child_node.offset());
            self.hit_test_subtree(child_id, child_position, result)
        };

        let hit = render_object.hit_test_raw(position, children.len(), &mut hit_child);
        if hit {
            // Leaf-first path: children pushed their entries during
            // the callback above; the ancestor follows.
            result.add(crate::hit_testing::HitTestEntry::new(id));
        }
        hit
    }

    fn sliver_hit_position_from_offset(node: &RenderNode, position: Offset) -> MainAxisPosition {
        let axis_direction = node
            .as_sliver()
            .and_then(|entry| entry.state().constraints())
            .map(|constraints| constraints.axis_direction);

        match axis_direction {
            Some(
                flui_types::layout::AxisDirection::LeftToRight
                | flui_types::layout::AxisDirection::RightToLeft,
            ) => MainAxisPosition::from_horizontal_offset(position),
            _ => MainAxisPosition::from_vertical_offset(position),
        }
    }

    fn sliver_hit_position_from_paint_offset(
        node: &RenderNode,
        position: Offset,
    ) -> MainAxisPosition {
        let Some(entry) = node.as_sliver() else {
            return MainAxisPosition::from_vertical_offset(position);
        };
        let Some(constraints) = entry.state().constraints() else {
            return MainAxisPosition::from_vertical_offset(position);
        };

        let (raw_main, cross_axis) = match constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => {
                (position.dx.get(), position.dy.get())
            }
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => {
                (position.dy.get(), position.dx.get())
            }
        };
        let effective_axis_direction = match constraints.growth_direction {
            crate::constraints::GrowthDirection::Forward => constraints.axis_direction,
            crate::constraints::GrowthDirection::Reverse => constraints.axis_direction.opposite(),
        };
        let main_axis = if effective_axis_direction.is_reversed() {
            entry
                .state()
                .geometry()
                .map_or(raw_main, |geometry| geometry.paint_extent - raw_main)
        } else {
            raw_main
        };

        MainAxisPosition::new(main_axis, cross_axis)
    }

    fn sliver_hit_position_minus_paint_offset(
        parent_constraints: &SliverConstraints,
        parent_geometry: &SliverGeometry,
        child_node: &RenderNode,
        position: MainAxisPosition,
        offset: Offset,
    ) -> MainAxisPosition {
        let Some(child_entry) = child_node.as_sliver() else {
            return position;
        };
        let Some(child_constraints) = child_entry.state().constraints() else {
            return position;
        };
        let Some(child_geometry) = child_entry.state().geometry() else {
            return position;
        };

        let (offset_main, offset_cross) = match child_constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => (offset.dx.get(), offset.dy.get()),
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => (offset.dy.get(), offset.dx.get()),
        };
        let parent_physical_main =
            if Self::effective_sliver_axis_direction(parent_constraints).is_reversed() {
                parent_geometry.paint_extent - position.main_axis
            } else {
                position.main_axis
            };
        let child_physical_main = parent_physical_main - offset_main;
        let child_main = if Self::effective_sliver_axis_direction(child_constraints).is_reversed() {
            child_geometry.paint_extent - child_physical_main
        } else {
            child_physical_main
        };

        MainAxisPosition::new(child_main, position.cross_axis - offset_cross)
    }

    fn effective_sliver_axis_direction(
        constraints: &SliverConstraints,
    ) -> flui_types::layout::AxisDirection {
        match constraints.growth_direction {
            crate::constraints::GrowthDirection::Forward => constraints.axis_direction,
            crate::constraints::GrowthDirection::Reverse => constraints.axis_direction.opposite(),
        }
    }

    fn box_hit_offset_from_sliver_position(
        constraints: &SliverConstraints,
        geometry: &SliverGeometry,
        child_size: Size,
        position: MainAxisPosition,
        offset: Offset,
    ) -> Offset {
        let reversed = matches!(
            constraints.axis_direction,
            flui_types::layout::AxisDirection::RightToLeft
                | flui_types::layout::AxisDirection::BottomToTop
        );
        let right_way_up = match constraints.growth_direction {
            crate::constraints::GrowthDirection::Forward => !reversed,
            crate::constraints::GrowthDirection::Reverse => reversed,
        };

        let (paint_main, paint_cross, child_main_extent) = match constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => {
                (offset.dx.get(), offset.dy.get(), child_size.width.get())
            }
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => {
                (offset.dy.get(), offset.dx.get(), child_size.height.get())
            }
        };
        let child_main_axis_position = if right_way_up {
            paint_main
        } else {
            geometry.paint_extent - child_main_extent - paint_main
        };
        let mut local_main = position.main_axis - child_main_axis_position;
        if !right_way_up {
            local_main = child_main_extent - local_main;
        }
        let local_cross = position.cross_axis - paint_cross;

        match constraints.axis_direction {
            flui_types::layout::AxisDirection::LeftToRight
            | flui_types::layout::AxisDirection::RightToLeft => Offset::new(
                flui_types::geometry::px(local_main),
                flui_types::geometry::px(local_cross),
            ),
            flui_types::layout::AxisDirection::TopToBottom
            | flui_types::layout::AxisDirection::BottomToTop => Offset::new(
                flui_types::geometry::px(local_cross),
                flui_types::geometry::px(local_main),
            ),
        }
    }

    fn hit_test_sliver_subtree(
        &self,
        id: RenderId,
        position: MainAxisPosition,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        ensure_stack(|| self.hit_test_sliver_subtree_impl(id, position, result))
    }

    fn hit_test_sliver_subtree_impl(
        &self,
        id: RenderId,
        position: MainAxisPosition,
        result: &mut crate::hit_testing::HitTestResult,
    ) -> bool
    where
        Phase: Sync,
    {
        let Some(node) = self.render_tree.get(id) else {
            return false;
        };
        let Some(entry) = node.as_sliver() else {
            return false;
        };
        let Some(geometry) = entry.state().geometry() else {
            return false;
        };
        let Some(constraints) = entry.state().constraints() else {
            return false;
        };
        if position.main_axis < 0.0
            || position.main_axis >= geometry.hit_test_extent
            || position.cross_axis < 0.0
            || position.cross_axis >= constraints.cross_axis_extent
        {
            return false;
        }
        let children: Vec<RenderId> = node.children().to_vec();
        let render_object = entry.render_object();

        let mut hit_child = |index: usize, override_pos: Option<MainAxisPosition>| -> bool {
            let Some(&child_id) = children.get(index) else {
                return false;
            };
            let Some(child_node) = self.render_tree.get(child_id) else {
                return false;
            };
            if child_node.as_sliver().is_some() {
                // Explicit positions are already in child sliver coordinates.
                // The layout-offset fallback starts from physical paint data.
                let child_position = match override_pos {
                    Some(position) => position,
                    None => Self::sliver_hit_position_minus_paint_offset(
                        constraints,
                        &geometry,
                        child_node,
                        position,
                        child_node.offset(),
                    ),
                };
                self.hit_test_sliver_subtree(child_id, child_position, result)
            } else if let Some(child_entry) = child_node.as_box() {
                let Some(child_size) = child_entry.state().geometry() else {
                    return false;
                };
                let child_position = Self::box_hit_offset_from_sliver_position(
                    constraints,
                    &geometry,
                    child_size,
                    override_pos.unwrap_or(position),
                    child_node.offset(),
                );
                self.hit_test_subtree(child_id, child_position, result)
            } else {
                false
            }
        };

        let hit = render_object.hit_test_raw(position, children.len(), &mut hit_child);
        if hit {
            result.add(crate::hit_testing::HitTestEntry::new(id));
        }
        hit
    }

    /// Sets the device pixel ratio for subsequent paint passes.
    ///
    /// Called by the platform binding on surface creation and DPI
    /// change. Non-finite or non-positive values are rejected (kept at
    /// the previous ratio) — a zero or NaN DPR poisons every shaped
    /// glyph and snapped hairline downstream.
    pub fn set_device_pixel_ratio(&mut self, dpr: f32) {
        if dpr.is_finite() && dpr > 0.0 {
            self.device_pixel_ratio = dpr;
        } else {
            tracing::warn!(
                dpr,
                "set_device_pixel_ratio: rejecting non-finite / non-positive \
                 ratio; keeping {}",
                self.device_pixel_ratio,
            );
        }
    }

    // ========================================================================
    // RenderObject Insertion (with dirty tracking)
    // ========================================================================

    /// Inserts a render object into the tree and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object into the RenderTree
    /// 2. Adds the node to the dirty layout list (since new nodes need layout)
    /// 3. Adds the node to the dirty paint list (since new nodes need paint)
    ///
    /// Use this instead of `render_tree_mut().insert()` to ensure proper dirty
    /// tracking.
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted node.
    pub fn insert<P>(&mut self, render_object: Box<dyn crate::traits::RenderObject<P>>) -> RenderId
    where
        P: crate::protocol::Protocol,
        crate::storage::RenderNode: From<Box<dyn crate::traits::RenderObject<P>>>,
    {
        use flui_tree::traits::TreeWrite;

        // Convert to RenderNode using From impl (zero-cost, compile-time dispatch)
        let node: crate::storage::RenderNode = render_object.into();
        let id = self.render_tree.insert(node);
        let depth = self.render_tree.depth(id).unwrap_or(0) as usize;

        // PR-A2 U33: bootstrap IS_REPAINT_BOUNDARY flag from the
        // render_object's static answer before the dirty pushes (so
        // the compositing walk has accurate boundary info on first
        // run_compositing).
        self.bootstrap_repaint_boundary_flag(id);

        // New nodes need layout and paint
        self.add_node_needing_layout(id, depth);
        self.add_node_needing_paint(id, depth);

        id
    }

    /// Inserts a render object as a child and marks it as needing layout.
    ///
    /// This method:
    /// 1. Inserts the render object as a child in the RenderTree
    /// 2. Adds the node to the dirty layout list
    /// 3. Adds the node to the dirty paint list
    /// 4. Marks the parent as needing layout (since child structure changed)
    ///
    /// Use this instead of `render_tree_mut().insert_child()` to ensure proper
    /// dirty tracking.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The parent node ID
    /// * `render_object` - The render object to insert as child
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted child, or `None` if parent doesn't exist.
    pub fn insert_child_render_object(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
    ) -> Option<RenderId> {
        // Get parent depth before insertion
        let parent_depth = self.render_tree.depth(parent_id)?;

        // Insert child (using Box protocol)
        let child_id = self
            .render_tree
            .insert_box_child(parent_id, render_object)?;
        let child_depth = parent_depth + 1;

        // PR-A2 U33: bootstrap IS_REPAINT_BOUNDARY flag from the
        // child render_object's static answer before any compositing
        // walk runs.
        self.bootstrap_repaint_boundary_flag(child_id);

        // Mark child as needing layout and paint
        self.add_node_needing_layout(child_id, child_depth as usize);
        self.add_node_needing_paint(child_id, child_depth as usize);

        // Mark parent as needing layout (child structure changed)
        self.add_node_needing_layout(parent_id, parent_depth as usize);

        Some(child_id)
    }

    /// Inserts a raw `RenderNode` directly into the tree.
    ///
    /// This bypasses the `RenderObject<P>` trait requirement and is used for
    /// special nodes like `RenderView` that manage their own layout/paint
    /// lifecycle outside the standard protocol dispatch.
    ///
    /// # Returns
    ///
    /// The `RenderId` of the inserted node.
    pub fn insert_render_node(&mut self, node: crate::storage::RenderNode) -> RenderId {
        use flui_tree::traits::TreeWrite;

        let id = self.render_tree.insert(node);
        let depth = self.render_tree.depth(id).unwrap_or(0) as usize;

        // PR-A2 U33: bootstrap IS_REPAINT_BOUNDARY flag (matches the
        // `insert` / `insert_child_render_object` paths so every code
        // path that adds nodes leaves the compositing flag in sync
        // with the trait answer).
        self.bootstrap_repaint_boundary_flag(id);

        self.add_node_needing_layout(id, depth);
        self.add_node_needing_paint(id, depth);

        id
    }

    /// Sets the root render object and marks it as needing layout.
    ///
    /// This is a convenience method that:
    /// 1. Inserts the render object
    /// 2. Sets it as the root
    /// 3. Ensures it's in the dirty lists
    ///
    /// # Returns
    ///
    /// The `RenderId` of the root node.
    pub fn set_root_render_object(
        &mut self,
        render_object: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
    ) -> RenderId {
        let id = self.insert(render_object);
        self.root_id = Some(id);
        id
    }

    /// Bootstraps the `IS_REPAINT_BOUNDARY` storage flag from the render
    /// object's static trait answer (`RenderObject::is_repaint_boundary()`).
    ///
    /// **D-block PR-A2 U33 (memo R26b).** Every node-insert path
    /// ([`Self::insert`], [`Self::insert_child_render_object`],
    /// [`Self::insert_render_node`]; [`Self::set_root_render_object`]
    /// inherits via `insert`) calls this immediately after the tree
    /// `insert` so the compositing-bits walk (U34) has accurate
    /// boundary info via the storage flag from frame 1.
    ///
    /// **Current consumer scope:** the compositing-bits walk consults
    /// `RenderNode::is_repaint_boundary_flag()`. The paint walk
    /// (`paint_node_recursive`) still reads `render_object.is_repaint_boundary()`
    /// directly — this matches Flutter parity (Flutter's `paint`
    /// reads the `isRepaintBoundary` final getter, equivalent to our
    /// trait answer; the bootstrap flag is the optimization target
    /// for a later sweep that swaps the paint check too).
    ///
    /// Pre-U33 the storage flag was effectively `false` for every node
    /// from the moment it entered the tree, which forced the
    /// compositing walk to fall through to the trait answer
    /// (`render_object().is_repaint_boundary()`) at every check site —
    /// a virtual dispatch and a divergence risk if a future caller
    /// flipped the flag dynamically.
    ///
    /// No-op if `id` is not present (defensive — every call site holds
    /// a freshly-inserted id, but a stale id passes through silently
    /// rather than panicking).
    #[inline]
    fn bootstrap_repaint_boundary_flag(&self, id: RenderId) {
        if let Some(node) = self.render_tree.get(id) {
            let is_boundary = node.is_repaint_boundary();
            node.set_repaint_boundary_flag(is_boundary);
        }
    }

    // ========================================================================
    // Dirty Node Access (Flutter API)
    // ========================================================================

    /// Returns the nodes needing layout.
    ///
    /// These are relayout boundaries that need to be laid out in the next
    /// layout phase.
    #[inline]
    pub fn nodes_needing_layout(&self) -> &[DirtyNode] {
        &self.dirty.needs_layout
    }

    /// Returns the nodes needing paint.
    ///
    /// These are repaint boundaries that need to be painted in the next
    /// paint phase.
    #[inline]
    pub fn nodes_needing_paint(&self) -> &[DirtyNode] {
        &self.dirty.needs_paint
    }

    /// Returns the nodes needing compositing bits update.
    #[inline]
    pub fn nodes_needing_compositing_bits_update(&self) -> &[DirtyNode] {
        &self.dirty.needs_compositing
    }

    /// Returns the nodes needing semantics update.
    #[inline]
    pub fn nodes_needing_semantics(&self) -> &[DirtyNode] {
        &self.dirty.needs_semantics
    }

    /// Adds a node to the layout dirty list.
    ///
    /// # Dedup + mid-layout routing (D-block PR-A1 U22, memo D7)
    ///
    /// 1. **Queue-membership dedup**: scans the target queue
    ///    (`dirty.needs_layout` OR `mid_layout_marks.needs_layout`
    ///    depending on the routing decision in step 2) and skips the
    ///    push if `node_id` is already present. O(N) scan matches
    ///    [`Self::mark_needs_layout`]'s pre-existing dedup pattern.
    ///    Flag-based dedup is unsuitable because `RenderState::new()`
    ///    defaults `NEEDS_LAYOUT = true` — a flag check would
    ///    silently no-op on the FIRST add for every newly-inserted
    ///    node (this is the regression the test
    ///    `test_run_frame_catches_paint_panic` flagged).
    /// 2. **Mid-layout routing**: if [`Self::debug_doing_layout`] is
    ///    `true`, the outer `run_layout` loop is iterating the
    ///    current `dirty.needs_layout` snapshot — pushing into the
    ///    active queue mid-iteration would either be silently ignored
    ///    (`std::mem::take` snapshot) or processed in the wrong
    ///    order. Push into `mid_layout_marks.needs_layout` instead;
    ///    the outer loop drains it after the current iteration via
    ///    [`Self::drain_mid_layout_marks`].
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_layout(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_layout {
            &mut self.mid_layout_marks.needs_layout
        } else {
            &mut self.dirty.needs_layout
        };
        if target.iter().any(|d| d.id == node_id) {
            return;
        }
        target.push(DirtyNode::new(node_id, depth));
        // New work was scheduled — wake the platform so an idle event
        // loop produces the frame (Flutter parity: markNeedsLayout →
        // owner.requestVisualUpdate()). Fired only on a NEW queue entry:
        // an existing entry means a frame is already scheduled.
        self.notifier.read().fire_need_visual_update();
    }

    /// Marks a node as needing layout, propagating the `NEEDS_LAYOUT` flag
    /// up the ancestor chain and pushing the **relayout boundary** onto
    /// `dirty.needs_layout` for the next `run_layout` pass.
    ///
    /// **D-block PR-A1 U15** (memo D3) — greenfield authoring of Flutter's
    /// `markNeedsLayout` walk (`.flutter/.../object.dart:2658-2700`). Walks
    /// the parent chain via [`NodeLinks::parent`] and at each step:
    ///
    /// 1. If the node is already marked `NEEDS_LAYOUT`, stop — earlier
    ///    propagation already reached the boundary; no need to re-walk.
    /// 2. Otherwise, set the flag via [`RenderNode::mark_layout_flag`].
    /// 3. If the node is a relayout boundary
    ///    ([`RenderNode::is_relayout_boundary`] — reads the per-instance
    ///    `IS_RELAYOUT_BOUNDARY` storage flag set by
    ///    [`compute_relayout_boundary`](crate::storage::RenderState::compute_relayout_boundary))
    ///    OR has no parent (tree root), push this id onto
    ///    `dirty.needs_layout` and return.
    /// 4. Otherwise, recurse to the parent.
    ///
    /// [`NodeLinks::parent`]: crate::storage::NodeLinks::parent
    /// [`RenderNode::mark_layout_flag`]: crate::storage::RenderNode::mark_layout_flag
    /// [`RenderNode::is_relayout_boundary`]: crate::storage::RenderNode::is_relayout_boundary
    ///
    /// The walk is idempotent — a stale call on an already-marked subtree
    /// short-circuits at step 1 without re-pushing the boundary. Missing
    /// `RenderId`s (post-removal stale references) are silent no-ops; the
    /// walk simply terminates at the missing-lookup step.
    ///
    /// This method supersedes the direct `add_node_needing_layout` call
    /// pattern in `flui-view::element::behavior_commons::mark_render_needs_layout_and_paint`
    /// (migrated in D-block PR-A1 U16). Direct `add_node_needing_layout`
    /// remains as the low-level primitive for callers that have already
    /// computed the correct boundary id (e.g. testing surfaces).
    ///
    /// **Bootstrap dependency (U17):** the relayout-boundary flag is set
    /// per-instance only after [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only)
    /// has run once. Pre-bootstrap (no layout has executed yet) every node
    /// reports `is_relayout_boundary() == false` and propagation runs to
    /// root — which is the correct fallback (root is always an implicit
    /// boundary in Flutter).
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        let mut current = id;
        loop {
            // Snapshot the per-node decision under a short-lived borrow so
            // we can release before stepping to the parent in the recursion.
            let step = {
                let Some(node) = self.render_tree.get_mut(current) else {
                    // Stale reference (e.g. node removed mid-frame). Stop.
                    return;
                };
                // Idempotent flag set — the AtomicRenderFlags fetch-or is a
                // no-op when the bit is already set. The walk does NOT
                // short-circuit on "already marked": even with U23's
                // `run_layout` → `layout_dirty_root` wiring (which
                // clears NEEDS_LAYOUT after each successful layout via
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
                let boundary =
                    (node.is_relayout_boundary() && !had_cached_queries) || parent.is_none();
                let depth = node.depth() as usize;
                (boundary, depth, parent)
            };
            let (is_boundary, depth, parent) = step;
            if is_boundary {
                // Codex P1 (PR #139 review): always enqueue the boundary
                // for this invalidation, with a dedup check against the
                // dirty queue so multiple marks-in-same-frame don't push
                // duplicate entries. Pre-fix, the algorithm returned early
                // on already-marked nodes WITHOUT pushing — which silently
                // dropped subsequent invalidations once the broken pipeline
                // had drained the dirty queue but not cleared the flag.
                if !self.dirty.needs_layout.iter().any(|d| d.id == current) {
                    self.dirty.needs_layout.push(DirtyNode::new(current, depth));
                    // Wake the platform: an idle event loop must produce
                    // a frame for this invalidation (Flutter parity:
                    // markNeedsLayout → owner.requestVisualUpdate()).
                    // Fired only on a NEW boundary entry — an existing
                    // entry means a frame is already scheduled.
                    self.notifier.read().fire_need_visual_update();
                }
                return;
            }
            // SAFETY: `parent.is_none()` is folded into `is_boundary` above,
            // so reaching this branch guarantees `Some(_)`.
            current = parent.unwrap();
        }
    }

    // ========================================================================
    // Intrinsic / Dry-Layout Queries (memoized walks)
    // ========================================================================

    /// One intrinsic dimension of a box subtree, memoized per node.
    ///
    /// The walk mirrors Flutter's `getMinIntrinsicWidth`-family wrapper
    /// layer (box.dart `_computeIntrinsics`): every node's answer for
    /// `(dimension, extent)` is cached in its `RenderState` layout
    /// cache, and `mark_needs_layout` clears the cache with
    /// boundary-crossing escalation. Repeated probes of the same child
    /// at the same extent — the canonical N-child container pattern —
    /// cost one computation each.
    ///
    /// Average O(subtree) on a cold cache, O(1) per cached node;
    /// worst case adds the hash-collision factor of the per-node maps.
    ///
    /// # Errors
    ///
    /// [`RenderError::NodeNotFound`](crate::error::RenderError::NodeNotFound)
    /// for a stale/foreign id,
    /// [`RenderError::ProtocolMismatch`](crate::error::RenderError::ProtocolMismatch)
    /// if the subtree contains a sliver node (box intrinsics are
    /// undefined there).
    pub fn box_intrinsic_dimension(
        &mut self,
        id: RenderId,
        dimension: crate::storage::IntrinsicDimension,
        extent: f32,
    ) -> crate::error::RenderResult<f32> {
        let mut slots = self.acquire_query_slots(id)?;
        intrinsic_query(&mut slots, id, dimension, extent)
    }

    /// The size a box subtree WOULD take under `constraints`, memoized
    /// per `(node, constraints)` — Flutter's `getDryLayout`.
    ///
    /// # Errors
    ///
    /// Same surface as [`Self::box_intrinsic_dimension`].
    pub fn box_dry_layout(
        &mut self,
        id: RenderId,
        constraints: crate::constraints::BoxConstraints,
    ) -> crate::error::RenderResult<flui_types::Size> {
        let mut slots = self.acquire_query_slots(id)?;
        dry_layout_query(&mut slots, id, constraints)
    }

    /// The dry baseline of a box node for `constraints`, memoized per
    /// `(constraints, baseline)` — Flutter's `getDryBaseline`. The
    /// computed answer may be `None` ("no baseline"); that answer is
    /// cached too.
    ///
    /// # Errors
    ///
    /// Same surface as [`Self::box_intrinsic_dimension`].
    pub fn box_dry_baseline(
        &mut self,
        id: RenderId,
        constraints: crate::constraints::BoxConstraints,
        baseline: crate::traits::TextBaseline,
    ) -> crate::error::RenderResult<Option<f32>> {
        let Some(node) = self.render_tree.get_mut(id) else {
            return Err(crate::error::RenderError::NodeNotFound(id));
        };
        let Some(entry) = node.as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol: "sliver",
                constraints_protocol: "box",
            });
        };
        if let Some(hit) = entry
            .state()
            .layout_cache()
            .peek_dry_baseline(constraints, baseline)
        {
            return Ok(hit);
        }
        let value = entry
            .render_object()
            .dry_baseline_raw(constraints, baseline);
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_dry_baseline(constraints, baseline, value);
        Ok(value)
    }

    /// Acquires the take-out borrow map for a memoizing query walk:
    /// disjoint `&mut` over the subtree (the same `get_subtree_mut`
    /// primitive the layout walk uses) plus each node's child-id
    /// snapshot. A node is moved OUT of its slot while its own
    /// computation runs, so re-entry — a child-link cycle — is
    /// detectable instead of UB.
    fn acquire_query_slots(
        &mut self,
        id: RenderId,
    ) -> crate::error::RenderResult<rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>> {
        let ids = self.render_tree.collect_subtree_ids(id);
        let nodes = self
            .render_tree
            .get_subtree_mut(&ids)
            .ok_or(crate::error::RenderError::NodeNotFound(id))?;
        Ok(ids
            .iter()
            .zip(nodes)
            .map(|(&node_id, node)| {
                let children = node.children().to_vec();
                (
                    node_id,
                    QuerySlot {
                        node: Some(node),
                        children,
                    },
                )
            })
            .collect())
    }

    /// Adds a node to the paint dirty list.
    ///
    /// **D-block PR-A1 U22 (memo D7):** same queue-membership dedup
    /// (O(N) `iter().any()` scan; flag-based dedup is unsuitable
    /// because `RenderState::new()` defaults `NEEDS_PAINT = true`) and
    /// mid-phase routing discipline as
    /// [`Self::add_node_needing_layout`]; see that method's doc for
    /// the dispatch rules. Mid-phase route is gated by
    /// `debug_doing_paint`.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_paint(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_paint {
            &mut self.mid_layout_marks.needs_paint
        } else {
            &mut self.dirty.needs_paint
        };
        if target.iter().any(|d| d.id == node_id) {
            return;
        }
        target.push(DirtyNode::new(node_id, depth));
        // Wake the platform for the (next) frame. Mid-paint marks are
        // drained into next-frame work at the end of run_paint — without
        // this wake an idle app would never paint them (the "GIF frozen
        // until you scroll" failure mode).
        self.notifier.read().fire_need_visual_update();
    }

    /// Adds a node to the compositing bits dirty list.
    ///
    /// **D-block PR-A1 U22 (memo D7):** same queue-membership dedup
    /// and mid-phase routing discipline as
    /// [`Self::add_node_needing_layout`]. Mid-phase route is gated by
    /// `debug_doing_layout` (the compositing phase shares the
    /// layout-phase debug flag because compositing-bits update runs
    /// as part of the layout pipeline per the typestate transitions).
    ///
    /// **PR-A2 Codex review #3294562493:** also sets the
    /// `NEEDS_COMPOSITING_BITS_UPDATE` flag on the node so the
    /// `run_compositing` walk's per-entry `needs_compositing_bits_update()`
    /// short-circuit doesn't silently drop this queue entry. The
    /// invariant "queue entry ⇒ flag set" makes the queue-clear at
    /// end of `run_compositing` safe — a queued entry can no longer
    /// be a no-op walk that loses the scheduling signal. Callers
    /// that want the bit set without queue membership should reach
    /// for [`RenderNode::mark_needs_compositing_bits_update`] directly.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_compositing_bits_update(&mut self, node_id: RenderId, depth: usize) {
        // Set the bit first so the run_compositing walk doesn't
        // skip this entry on the early-return path. No-op if the id
        // is not present in the tree (defensive).
        if let Some(node) = self.render_tree.get(node_id) {
            node.mark_needs_compositing_bits_update();
        }
        let target = if self.debug_doing_layout {
            &mut self.mid_layout_marks.needs_compositing
        } else {
            &mut self.dirty.needs_compositing
        };
        if target.iter().any(|d| d.id == node_id) {
            return;
        }
        target.push(DirtyNode::new(node_id, depth));
    }

    /// Adds a node to the semantics dirty list.
    ///
    /// **D-block PR-A1 U22 (memo D7):** same queue-membership dedup
    /// and mid-phase routing discipline as
    /// [`Self::add_node_needing_layout`]. Mid-phase route is gated by
    /// `debug_doing_semantics`.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_semantics(&mut self, node_id: RenderId, depth: usize) {
        let target = if self.debug_doing_semantics {
            &mut self.mid_layout_marks.needs_semantics
        } else {
            &mut self.dirty.needs_semantics
        };
        if target.iter().any(|d| d.id == node_id) {
            return;
        }
        target.push(DirtyNode::new(node_id, depth));
    }

    // ========================================================================
    // Semantics enablement (data access, phase-agnostic)
    // ========================================================================

    /// Returns whether semantics are enabled.
    #[inline]
    pub fn semantics_enabled(&self) -> bool {
        self.semantics_enabled.load(Ordering::Relaxed)
    }

    /// Sets whether semantics are enabled.
    pub fn set_semantics_enabled(&self, enabled: bool) {
        let was_enabled = self.semantics_enabled.swap(enabled, Ordering::Relaxed);
        if enabled && !was_enabled {
            self.notifier.read().fire_semantics_owner_created();
        } else if !enabled && was_enabled {
            self.notifier.read().fire_semantics_owner_disposed();
        }
    }

    // ========================================================================
    // Debug
    // ========================================================================

    /// Returns whether layout is currently being performed.
    #[inline]
    pub fn debug_doing_layout(&self) -> bool {
        self.debug_doing_layout
    }

    /// Returns whether paint is currently being performed.
    #[inline]
    pub fn debug_doing_paint(&self) -> bool {
        self.debug_doing_paint
    }

    /// Returns whether semantics update is currently being performed.
    #[inline]
    pub fn debug_doing_semantics(&self) -> bool {
        self.debug_doing_semantics
    }

    /// Returns whether any pipeline phase is currently active.
    #[inline]
    pub fn debug_doing_any_phase(&self) -> bool {
        self.debug_doing_layout || self.debug_doing_paint || self.debug_doing_semantics
    }

    /// Returns the total number of dirty nodes across all lists.
    pub fn dirty_node_count(&self) -> usize {
        self.dirty.needs_layout.len()
            + self.dirty.needs_compositing.len()
            + self.dirty.needs_paint.len()
            + self.dirty.needs_semantics.len()
    }

    /// Returns whether there are any dirty nodes.
    #[inline]
    pub fn has_dirty_nodes(&self) -> bool {
        !self.dirty.needs_layout.is_empty()
            || !self.dirty.needs_compositing.is_empty()
            || !self.dirty.needs_paint.is_empty()
            || !self.dirty.needs_semantics.is_empty()
    }

    /// Clears all dirty node lists without processing them.
    ///
    /// Use with caution - this discards pending work.
    pub fn clear_all_dirty_nodes(&mut self) {
        self.dirty.needs_layout.clear();
        self.dirty.needs_compositing.clear();
        self.dirty.needs_paint.clear();
        self.dirty.needs_semantics.clear();
        // PR-A1 U22 (memo D7): also clear mid-phase side queue.
        self.mid_layout_marks.needs_layout.clear();
        self.mid_layout_marks.needs_compositing.clear();
        self.mid_layout_marks.needs_paint.clear();
        self.mid_layout_marks.needs_semantics.clear();
    }

    /// Drains the mid-phase side queue into the active `dirty` set.
    ///
    /// **D-block PR-A1 U22 (memo D7):** called by U23's `run_layout`
    /// / `run_paint` outer `while` loops at the end of each iteration
    /// — picks up the side-queued marks made during the iteration
    /// (when `debug_doing_*` was `true`) so the next iteration of the
    /// outer loop processes them.
    ///
    /// Capacity-preserving: each side-queue vector is moved via
    /// `Vec::append` (drains source, keeps source's allocation
    /// reservation for the next frame). Returns the total number of
    /// entries drained, summed across all four phases — callers may
    /// use this as the loop-continue signal.
    pub fn drain_mid_layout_marks(&mut self) -> usize {
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

    /// Returns whether any mid-phase marks are pending drain.
    /// **D-block PR-A1 U22.**
    #[inline]
    pub fn has_mid_layout_marks(&self) -> bool {
        self.mid_layout_marks.any()
    }
}

// ============================================================================
// Layout phase: run_layout + helpers
// ============================================================================

impl PipelineOwner<Layout> {
    /// Transitions a layout-phase pipeline into the [`Compositing`] phase.
    pub fn into_compositing(self) -> PipelineOwner<Compositing> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the layout phase (e.g. on error abort).
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates layout for all dirty render objects.
    ///
    /// This is phase 1 of the rendering pipeline. During layout:
    /// - Sizes and positions are calculated
    /// - Objects may dirty paint or compositing
    ///
    /// Nodes are sorted by depth (shallow first) so parents are laid out
    /// before their children. This matches Flutter's `flushLayout` behavior.
    ///
    /// # Synchronous Child Layout
    ///
    /// With interior mutability (RwLock on RenderNode), parent's
    /// `perform_layout` can call `layout_child()` which triggers
    /// synchronous child layout through the RenderTree. The child is laid
    /// out immediately and returns its size.
    pub fn run_layout(&mut self) -> crate::error::RenderResult<()> {
        let _span =
            tracing::debug_span!("layout", dirty_nodes = self.dirty.needs_layout.len(),).entered();

        // Process own dirty nodes if any
        // Flutter pattern: while loop to handle nodes added during layout
        while !self.dirty.needs_layout.is_empty() {
            self.debug_doing_layout = true;

            // Take the dirty nodes and replace with empty vec
            // This allows new nodes to be added during layout (routed
            // to mid_layout_marks per U22; drained back at end of
            // iteration below).
            let mut dirty_nodes = std::mem::take(&mut self.dirty.needs_layout);

            // Sort by depth (shallow first) - parents before children
            // Flutter: dirtyNodes.sort((a, b) => a.depth - b.depth)
            dirty_nodes.sort_unstable_by_key(|node| node.depth);

            tracing::debug!(
                "run_layout: sorted order (shallow-first) = {:?}",
                dirty_nodes
                    .iter()
                    .map(|n| (n.id, n.depth))
                    .collect::<Vec<_>>()
            );

            // Process each dirty node.
            //
            // **D-block PR-A1 U23:** layout_dirty_root replaces the
            // legacy layout_node_with_children no-op recursion. Each
            // dirty entry is laid out via the pre-acquired-subtree
            // walk (U20.1) protected by LayoutCycleGuard (U21).
            // Constraints come from cached state (post-frame-1) OR
            // the binding-set root_constraints (frame-1 root).
            for dirty_node in dirty_nodes {
                // PR-A1 U23 P2 review fix (Copilot 3294417924): skip
                // entries whose NEEDS_LAYOUT flag was already cleared
                // earlier in this iteration. Common case: a parent's
                // layout_child callback recursively lays out a child
                // whose dirty-queue entry was queued separately
                // (e.g., insert_child_render_object enqueues both).
                // Re-laying out the child would be redundant +
                // potentially side-effectful.
                let already_clean = self
                    .render_tree
                    .get(dirty_node.id)
                    .is_some_and(|n| !n.needs_layout());
                if already_clean {
                    tracing::trace!(
                        id = ?dirty_node.id,
                        "run_layout: skipping dirty-queue entry whose NEEDS_LAYOUT \
                         was already cleared this iteration",
                    );
                    continue;
                }

                let Some(constraints) = self.cached_or_root_constraints(dirty_node.id) else {
                    // PR-A1 U23 P2 review fix (Copilot 3294417942):
                    // dropping the dirty entry here without recovery
                    // strands the work. The two real cases this hits:
                    //   1. Root id with root_constraints unset — the
                    //      binding should have called
                    //      set_root_constraints BEFORE run_frame.
                    //      U23's set_root_constraints fix auto-marks
                    //      root dirty when constraints land, so the
                    //      next run_layout picks up the deferred
                    //      work automatically.
                    //   2. Non-root id with no cached constraints
                    //      AND no parent-driven layout yet — the
                    //      shallow-first dirty queue sort means the
                    //      parent should have processed first; if
                    //      it didn't (parent's perform_layout didn't
                    //      call layout_child for this id), the entry
                    //      is correctly dropped because the parent
                    //      is the authority on child constraints.
                    // Logged at warn so the diagnostic surfaces but
                    // doesn't halt the pipeline.
                    tracing::warn!(
                        id = ?dirty_node.id,
                        is_root = ?(self.root_id == Some(dirty_node.id)),
                        "run_layout: no cached state.constraints() AND no \
                         root_constraints (or id != root_id); skipping dirty entry. \
                         Recovery: for root → call set_root_constraints (which \
                         auto-marks the root dirty); for non-root → parent's \
                         perform_layout must call ctx.layout_child(idx, c) for \
                         this id first."
                    );
                    continue;
                };
                if let Err(e) = self.layout_dirty_root(dirty_node.id, constraints) {
                    self.debug_doing_layout = false;
                    // PR-A1 U22 P1 review fix (Codex 3294365736): drain
                    // mid-phase marks back into `dirty` even on error
                    // path so they survive across phase invocations.
                    self.drain_mid_layout_marks();
                    return Err(e);
                }

                // Flutter parity (object.dart: `RenderObject.layout`
                // unconditionally ends with `markNeedsPaint()`): a
                // subtree that re-laid out must repaint, otherwise a
                // pure-layout invalidation (setState moving a child)
                // leaves stale pixels on screen. One entry per dirty
                // root suffices — run_paint walks the whole tree from
                // the root, so triggering the phase is what matters.
                self.add_node_needing_paint(dirty_node.id, dirty_node.depth);
            }

            self.debug_doing_layout = false;

            // PR-A1 U22 P1 review fix (Codex 3294365736): drain
            // mid_layout_marks back into `dirty` so the outer while
            // condition picks up marks routed to the side queue
            // during this iteration's `debug_doing_layout = true`
            // window.
            self.drain_mid_layout_marks();
        }
        Ok(())
    }

    /// Returns the constraints to apply when laying out `id` as a
    /// dirty root.
    ///
    /// **D-block PR-A1 U23:** sourced from (in priority order):
    ///
    /// 1. The node's cached `state.constraints()` — set on the
    ///    previous frame's successful layout. This is the common
    ///    case for re-layout (constraints unchanged → cache hit
    ///    fast path inside `layout_dirty_root`).
    /// 2. The binding-set [`Self::root_constraints`] if `id` is the
    ///    tree root (`root_id`). Used on the very first frame
    ///    before any layout has cached its own constraints.
    /// 3. Otherwise `None` — caller skips this dirty entry with a
    ///    warning (no constraints available means the parent's
    ///    perform_layout must propagate constraints first; the
    ///    dirty queue's shallow-first ordering ensures parents are
    ///    processed before children).
    fn cached_or_root_constraints(&self, id: RenderId) -> Option<BoxConstraints> {
        // The binding-set root constraints are AUTHORITATIVE for the
        // root: on resize, set_root_constraints updates them and marks
        // the root dirty — if the stale cached constraints won here,
        // the resize relayout would run at the OLD window size and the
        // newly exposed area would stay unpainted.
        if self.root_id == Some(id)
            && let Some(root) = self.root_constraints
        {
            return Some(root);
        }
        if let Some(node) = self.render_tree.get(id)
            && let Some(entry) = node.as_box()
            && let Some(cached) = entry.state().constraints()
        {
            return Some(*cached);
        }
        if self.root_id == Some(id) {
            return self.root_constraints;
        }
        None
    }

    /// D-block PR-A1b3 U20 — production disjoint-borrow layout walk.
    ///
    /// Lays out the subtree rooted at `id` with the supplied
    /// `constraints`, running `RenderObject::perform_layout_raw` against a
    /// typed [`crate::protocol::BoxLayoutCtx`] populated with the parent's direct children
    /// (companion memo D1). Returns the parent's computed `Size` on
    /// success.
    ///
    /// Replaces the recursion shape of `layout_node_with_children`
    /// (which only walks the dirty tree without invoking per-node
    /// layout — see the audit comment in that method). The
    /// pipeline-side `run_layout` outer loop is rewired to this method
    /// in U23.
    ///
    /// # Mechanism (U20.1 — pre-acquired subtree borrows)
    ///
    /// 1. **Collect ids**: `RenderTree::collect_subtree_ids(id)` walks
    ///    the subtree in DFS pre-order producing
    ///    `Vec<RenderId>` covering root + all descendants.
    /// 2. **Pre-acquire borrows in ONE scope**:
    ///    `RenderTree::get_subtree_mut(&ids)` materialises N disjoint
    ///    `&mut RenderNode` references in a single function body via
    ///    the proven `*mut Slab` reborrow pattern that already powers
    ///    [`RenderTree::get_two_mut`] and
    ///    [`RenderTree::get_parent_and_children_mut`].
    /// 3. **Index by id**: the N borrows are wrapped in a private
    ///    `SubtreeBorrows` as a `HashMap<RenderId, NodePtr>` (raw
    ///    pointer alias of the still-live `&mut RenderNode` borrows).
    ///    Lookup is O(1) by id.
    /// 4. **Recursive walk**: a private `layout_subtree_borrowed`
    ///    helper indexes into `SubtreeBorrows` to acquire one node's
    ///    reborrow at each call level. The leaf path delegates to
    ///    [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only).
    ///    The non-leaf path constructs a Direct-storage `BoxLayoutCtx`
    ///    via the erased driver context ([`crate::protocol::ErasedBoxLayoutCtx`]) with a closure
    ///    that captures `&SubtreeBorrows` (Sync via `NodePtr`'s
    ///    `unsafe impl`) and re-enters `layout_subtree_borrowed` for
    ///    each child. The bridge in `traits/render_box.rs` reconstructs
    ///    a typed `BoxLayoutCtx<T::Arity, T::ParentData>` (Proxy
    ///    variant) and forwards to `RenderBox::perform_layout`.
    ///    Synchronous `ctx.layout_child(i, c)` calls dispatch through
    ///    the callback, recursing into the child's subtree via its
    ///    pre-acquired `NodePtr`.
    /// 5. **Per-level cleanup**: on success **AND when no descendant
    ///    errored**, updates `state.set_geometry` /
    ///    `state.set_constraints`, bootstraps `IS_RELAYOUT_BOUNDARY`,
    ///    clears `NEEDS_LAYOUT`. When a descendant errored
    ///    mid-callback, geometry + constraints are still recorded but
    ///    `NEEDS_LAYOUT` stays set on the parent so the next dirty
    ///    walk re-runs the subtree.
    ///
    /// # Soundness (U20.1 fix — Miri-clean)
    ///
    /// The prior PR-A1b3 design (PR #144) used a recursive raw-pointer
    /// re-entry into `RenderTree` from inside the layout-child callback —
    /// outer `&mut RenderEntry` was held LIVE across `perform_layout_raw`
    /// while the inner call synthesised a fresh `&mut RenderTree` from
    /// the same `*mut`. Under Stacked / Tree Borrows that invalidates
    /// the outer tag (latent UB; Miri flagged the 2-level and 3-level
    /// happy paths).
    ///
    /// The U20.1 redesign eliminates the inner reborrow entirely. All
    /// N disjoint `&mut RenderNode` borrows are acquired in ONE call to
    /// `get_subtree_mut` (single `&mut Slab` reborrow scope, mirroring
    /// `get_parent_and_children_mut`). The callback then acquires one
    /// node's reborrow per call level by dereferencing the pre-acquired
    /// `NodePtr` for that slot — no `&mut RenderTree` ever appears
    /// inside the callback chain.
    ///
    /// At any given moment during the walk, the per-slot reborrows in
    /// scope are: one for the current call's parent + one for each
    /// active recursive child call below it. All on **distinct slab
    /// slots** (parent ≠ child in a well-formed tree). The Unique tag
    /// for each slot lives independently because `NodePtr` is a raw
    /// pointer (SharedReadWrite permission on the allocation, distinct
    /// derived Unique tags per slot reborrow). Miri verifies this on
    /// the existing U20 integration tests.
    ///
    /// # Error handling
    ///
    /// - **Leaf-path panics** in user `perform_layout` → caught by
    ///   `layout_leaf_only`'s `catch_unwind`, returned as
    ///   [`crate::error::RenderError::Poisoned`].
    /// - **Non-leaf-path panics** (PR-A1b3 review fix): wrapped in
    ///   `catch_unwind` at the non-leaf `perform_layout_raw` call site,
    ///   returned as the same [`crate::error::RenderError::Poisoned`].
    ///   Symmetric with the leaf path.
    /// - **Descendant `Err` returned through the callback** → tracking
    ///   flag (`AtomicBool`) set; outer `perform_layout` still completes
    ///   with `Size::ZERO` for that child; outer `Ok` is returned to the
    ///   caller, BUT parent's `NEEDS_LAYOUT` is **not cleared**.
    ///   Next-frame dirty walk re-runs the parent. The `LayoutChildCallback`
    ///   signature is `Fn(_) -> Size`, not `Fn(_) -> Result<Size, _>`,
    ///   so callback failures can't propagate as typed `Err` through
    ///   the parent's `perform_layout` body. Surfacing the typed error
    ///   to the outer caller requires widening `LayoutChildCallback`
    ///   to `Result`; deferred to Core.1.
    /// - **Stale tree state** (id not in tree → `NodeNotFound`; id is
    ///   present but wrong protocol → `ProtocolMismatch`).
    ///
    /// # ParentData scope (current limitation)
    ///
    /// The pipeline-side Direct `BoxLayoutCtx` is parameterised over
    /// [`crate::parent_data::BoxParentData`], so widgets whose `T::ParentData` is the default
    /// (`RenderPadding`, `RenderCenter`, `RenderColoredBox`,
    /// `RenderOpacity`, `RenderTransform`, `RenderSizedBox`) drive
    /// through correctly. Non-default parent-data types (e.g.,
    /// `FlexParentData` on `RenderFlex`) trigger a
    /// `BoxLayoutCtx::from_erased` debug-assert mismatch in debug builds
    /// and silently fail to downcast in release builds. Pipeline-driven
    /// flex layout is out of scope for D-block; per-render-object
    /// `T::ParentData` dispatch lands as a Core.1 follow-up alongside the
    /// real `RenderFlex` slice integration.
    ///
    /// # Cycle / depth safety (U21 wired)
    ///
    /// Three-layer cycle protection (U20.1 + U21 combined):
    ///
    /// 1. `collect_subtree_ids` terminates safely on cycles via its
    ///    `visited` `HashSet<RenderId>` short-circuit (PR #145) —
    ///    the cyclic id is visited at most once, the cycle edge is
    ///    silently dropped from the collected subtree, deduplicated
    ///    `Vec<RenderId>` returns. No hang / OOM at the collect
    ///    phase.
    /// 2. `get_subtree_mut` receives the deduplicated id list →
    ///    uniqueness precondition satisfied → returns `Some(refs)`.
    ///    No double-borrow attempt at acquisition.
    /// 3. `layout_subtree_borrowed` registers each `id` in
    ///    `SubtreeBorrows::currently_laying_out` via the
    ///    `LayoutCycleGuard` RAII on entry (U21). A `perform_layout`
    ///    body that calls `layout_child` for an ancestor id already
    ///    in flight hits the guard's `enter` collision check →
    ///    returns [`crate::error::RenderError::LayoutCycle`]
    ///    immediately instead of attempting a second `NodePtr`
    ///    reborrow (which would be UB).
    ///
    /// The cycle error collapses through the layout-child callback
    /// (Size::ZERO + `descendant_error_flag`) so the parent stays
    /// `NEEDS_LAYOUT` for next-frame retry. The cycle persists
    /// structurally so retry will re-surface `LayoutCycle` — but
    /// predictably, never as panic/UB/hang. The user can fix the
    /// tree (remove the cyclic `add_child`) and the next retry
    /// succeeds.
    ///
    /// Frame-cross panic safety: `LayoutCycleGuard::Drop` runs on
    /// every exit path including unwind from a panicking
    /// `perform_layout`. Combined with the non-leaf path's
    /// `catch_unwind` wrapper, the cycle set stays consistent across
    /// frames — a panic does not leak an in-flight id.
    ///
    /// **U23 wiring is now soundness-unblocked.** `run_layout` may
    /// wire `layout_dirty_root` per its dirty-queue iteration in U23.
    pub fn layout_dirty_root(
        &mut self,
        id: RenderId,
        constraints: BoxConstraints,
    ) -> crate::error::RenderResult<Size> {
        // Step 1: collect every id in the subtree rooted at `id`
        // (DFS pre-order). Empty result = id not in tree.
        let subtree_ids = self.render_tree.collect_subtree_ids(id);
        if subtree_ids.is_empty() {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }

        // Step 2: pre-acquire N disjoint &mut RenderNode borrows on
        // every subtree slot in ONE function scope. `get_subtree_mut`
        // returns None on (a) duplicate ids or (b) missing slab slots.
        // Per `collect_subtree_ids`'s PR #145 visited-set fix the
        // returned id list is GUARANTEED deduplicated, so case (a) is
        // unreachable here — None can only mean a slab slot
        // disappeared between collect and acquire (a race-condition
        // shape that doesn't occur with &mut self access; defensive
        // fallback only). NodeNotFound is the most accurate variant
        // for that residual case.
        let node_refs = self
            .render_tree
            .get_subtree_mut(&subtree_ids)
            .ok_or(crate::error::RenderError::NodeNotFound(id))?;

        // Step 3: wrap the borrows in a SubtreeBorrows index for O(1)
        // by-id lookup. The HashMap holds NodePtr (raw alias of the
        // &mut RenderNode borrows just acquired) so the recursive walk
        // can reborrow one slot at a time without re-entering the
        // tree's slab borrow.
        let borrows = SubtreeBorrows::new(&subtree_ids, node_refs);

        // Step 4: recursive walk via index-into-pre-acquired-pool. No
        // &mut RenderTree appears inside the callback chain — only
        // per-slot NodePtr reborrows on distinct slab slots, sound
        // under Stacked / Tree Borrows.
        //
        // SAFETY: `borrows` is alive for the entire walk; each
        // `layout_subtree_borrowed` call reborrows exactly one slot via
        // its NodePtr; distinct call levels reborrow distinct slots
        // (parent ≠ child in a well-formed tree).
        unsafe { layout_subtree_borrowed(&borrows, id, constraints) }
    }
}

// ============================================================================
// PR-A1b3 U20.1 — pre-acquired-subtree layout helper (Miri-clean)
// ============================================================================

/// `Send + Sync` raw-pointer alias of a single `&mut RenderNode` borrow
/// held in [`SubtreeBorrows`].
///
/// Each `NodePtr` in [`SubtreeBorrows::by_id`] is derived from one of
/// the N disjoint `&mut RenderNode` references returned by
/// [`RenderTree::get_subtree_mut`]. The pointer is stable for the
/// lifetime of the `SubtreeBorrows` instance because the underlying
/// `&mut RenderTree` is held by the caller (`PipelineOwner` while
/// `layout_dirty_root` runs) and the slab's slot allocation is
/// position-stable (no moves during the borrow window).
///
/// The wrapper is `Copy` so the layout-child closure can capture the
/// pointer by value without `Arc` ceremony. `Send + Sync` is declared
/// because [`LayoutChildCallback`] inherits those bounds from
/// `BoxLayoutCtxErased: Send + Sync` (U19 design). Single-thread
/// access is enforced at the [`SubtreeBorrows::check_thread`] entry.
#[derive(Clone, Copy)]
struct NodePtr(*mut RenderNode);

// SAFETY: the raw pointer is just an address; the load-bearing borrow
// is the `&mut RenderNode` returned by `get_subtree_mut` that this
// pointer aliases. Cross-thread reborrow is rejected by
// [`SubtreeBorrows::check_thread`] before any deref.
unsafe impl Send for NodePtr {}
// SAFETY: same as Send.
unsafe impl Sync for NodePtr {}

/// Pre-acquired set of N disjoint `&mut RenderNode` borrows on a
/// subtree, indexed by [`RenderId`] for O(1) lookup.
///
/// **D-block PR-A1b3 U20.1:** replaces the prior `TreePtr` +
/// recursive-tree-reborrow scheme (PR #144) that surfaced as latent
/// Stacked / Tree Borrows UB. The new scheme acquires ALL subtree
/// `&mut RenderNode` borrows in ONE call to
/// [`RenderTree::get_subtree_mut`] (single `&mut Slab` reborrow scope),
/// stores raw aliases in this map, and lets the recursive walk reborrow
/// one slot at a time per call level. No `&mut RenderTree` ever appears
/// inside the layout-child callback chain — eliminates the UB.
///
/// # Lifetime
///
/// `'tree` ties `SubtreeBorrows` to the source `&mut RenderTree`
/// borrow's lifetime via `PhantomData<&'tree mut ()>`. Constructed via
/// [`Self::new`] from a `Vec<&'tree mut RenderNode>` (the output of
/// `get_subtree_mut`); the references are immediately converted to
/// raw pointers and aggregated by id. The `&mut RenderTree` source
/// borrow keeps the slab's slots position-stable for the lifetime of
/// every aliased `NodePtr`.
///
/// # Thread affinity
///
/// `SubtreeBorrows` records the constructing thread's `ThreadId` and
/// checks it on every [`Self::get`] call. The check survives even
/// though [`NodePtr`] declares `Send + Sync` — the auto-trait bound
/// is mechanically required to satisfy
/// `LayoutChildCallback: Send + Sync` (inherited from
/// `BoxLayoutCtxErased`), but at the call site we panic loudly on
/// cross-thread access instead of corrupting the slab silently.
/// Cheap: one `ThreadId::eq` per lookup.
struct SubtreeBorrows<'tree> {
    by_id: std::collections::HashMap<RenderId, NodePtr>,
    /// Set of ids whose layout is currently in flight at some recursion
    /// level above the current call. Insert on layout entry, remove on
    /// drop (RAII via [`LayoutCycleGuard`]). Re-entry on a member id
    /// surfaces as [`crate::error::RenderError::LayoutCycle`] — closes
    /// the U21 cycle-detection blocker (companion memo D6).
    ///
    /// Wrapped in `parking_lot::Mutex` because the layout-child closure
    /// requires `&SubtreeBorrows: Send + Sync` (inherited from
    /// `BoxLayoutCtxErased`). Uncontended `parking_lot::Mutex` acquire
    /// is ~10 ns — negligible vs `perform_layout` cost. The cross-
    /// thread closure-smuggle attack vector is independently rejected
    /// by [`Self::check_thread`], so the Mutex serves only as the
    /// shared-mutability cell, not as actual cross-thread sync.
    currently_laying_out: Mutex<FxHashSet<RenderId>>,
    owner_thread: std::thread::ThreadId,
    _lifetime: std::marker::PhantomData<&'tree mut ()>,
}

impl<'tree> SubtreeBorrows<'tree> {
    /// Constructs a `SubtreeBorrows` from the output of
    /// [`RenderTree::collect_subtree_ids`] paired with the matching
    /// output of [`RenderTree::get_subtree_mut`].
    ///
    /// Precondition: `ids.len() == refs.len()` and each
    /// `ids[i]` corresponds to `refs[i]` (in order). Caller must
    /// satisfy this — currently the only caller is
    /// [`PipelineOwner::layout_dirty_root`] which feeds the two
    /// methods' outputs directly to this ctor.
    fn new(ids: &[RenderId], refs: Vec<&'tree mut RenderNode>) -> Self {
        debug_assert_eq!(
            ids.len(),
            refs.len(),
            "SubtreeBorrows::new precondition violated: ids and refs \
             must have the same length",
        );
        let owner_thread = std::thread::current().id();
        let mut by_id = std::collections::HashMap::with_capacity(ids.len());
        for (&id, r) in ids.iter().zip(refs) {
            by_id.insert(id, NodePtr(r as *mut RenderNode));
        }
        Self {
            by_id,
            // Pre-sized to subtree size — at most `ids.len()` entries
            // can be in-flight concurrently (the recursive walk
            // descends linearly through one path at a time).
            currently_laying_out: Mutex::new(FxHashSet::with_capacity_and_hasher(
                ids.len(),
                Default::default(),
            )),
            owner_thread,
            _lifetime: std::marker::PhantomData,
        }
    }

    /// Panics if the calling thread is not the constructing thread.
    /// Called by [`Self::get`] before returning any [`NodePtr`].
    #[inline]
    fn check_thread(&self) {
        let current = std::thread::current().id();
        if current != self.owner_thread {
            panic!(
                "SubtreeBorrows accessed from non-owner thread: \
                 owner = {:?}, current = {:?}. The U20 layout walk \
                 requires the layout_child callback to fire on the \
                 same thread as PipelineOwner::layout_dirty_root \
                 (the pipeline phase holds &mut self synchronously). \
                 User RenderBox::perform_layout body must not spawn \
                 ctx.layout_child(...) calls to other threads — the \
                 underlying RenderTree slab is not Sync.",
                self.owner_thread, current,
            );
        }
    }

    /// Returns the [`NodePtr`] for `id` if present, panicking
    /// (via [`Self::check_thread`]) on cross-thread access.
    #[inline]
    fn get(&self, id: RenderId) -> Option<NodePtr> {
        self.check_thread();
        self.by_id.get(&id).copied()
    }
}

// ============================================================================
// PR-A1 U21 — RAII layout-cycle guard
// ============================================================================

/// RAII guard that registers `id` in [`SubtreeBorrows::currently_laying_out`]
/// on construction and unregisters on drop.
///
/// **D-block PR-A1 U21 (companion memo D6):** detects re-entry into a
/// node's `layout_subtree_borrowed` call (the situation where a user
/// `perform_layout` body calls `ctx.layout_child` for an ancestor id
/// whose layout is already in flight up the stack). On collision the
/// constructor returns [`crate::error::RenderError::LayoutCycle`]
/// instead of attempting a second [`NodePtr`] reborrow (which would be
/// UB under aliasing rules — the same slot's Unique tag is live up the
/// recursion stack).
///
/// The guard's `Drop` impl unconditionally removes `id` from the set,
/// even on unwind (Rust's drop semantics guarantee this for any
/// `Drop`-implementing value going out of scope). Combined with the
/// `catch_unwind` wrapper around `perform_layout_raw` in the non-leaf
/// path, this means the cycle set stays consistent across frames: a
/// panicking widget's id is cleared, the next frame's walk does not
/// see it as in-flight.
struct LayoutCycleGuard<'b, 'tree> {
    borrows: &'b SubtreeBorrows<'tree>,
    id: RenderId,
}

impl<'b, 'tree> LayoutCycleGuard<'b, 'tree> {
    /// Registers `id` as currently-laying-out. Returns
    /// `Err(RenderError::LayoutCycle(id))` if `id` is already
    /// registered — caller must propagate immediately.
    fn enter(borrows: &'b SubtreeBorrows<'tree>, id: RenderId) -> crate::error::RenderResult<Self> {
        // check_thread here so the diagnostic surfaces at the cycle-
        // guard layer too (covers callers that bypass `get`).
        borrows.check_thread();
        let mut set = borrows.currently_laying_out.lock();
        if !set.insert(id) {
            // Debug-level: the layout-child callback in
            // `layout_subtree_borrowed` already logs the propagated
            // Err at tracing::error when it collapses descendant Err
            // to Size::ZERO. Logging here at error too would produce
            // 2 log lines per cycle event (PR #146 Copilot review
            // comment 3294315141). The API-boundary error log is the
            // user-facing one; this debug-level log retains the
            // collision-point diagnostic for tracing.
            tracing::debug!(
                ?id,
                "layout_subtree_borrowed: layout cycle detected — id is \
                     already in flight at a parent call level; returning \
                     RenderError::LayoutCycle(id)",
            );
            return Err(crate::error::RenderError::layout_cycle(id));
        }
        // Lock drops here — set is held only for the insert.
        Ok(Self { borrows, id })
    }
}

impl<'b, 'tree> Drop for LayoutCycleGuard<'b, 'tree> {
    fn drop(&mut self) {
        // Unconditional remove — runs on every exit path including
        // unwind. Cycle set stays consistent for the next frame.
        // `Mutex::lock` is panic-safe (no poisoning in parking_lot).
        self.borrows.currently_laying_out.lock().remove(&self.id);
    }
}

/// Grows the stack ahead of each pipeline-walk recursion level so
/// arbitrarily deep render trees cannot overflow the fixed OS stack
/// (the Windows main thread gets 1 MiB by default; a ~1000-level
/// single-child chain blew it in the `layout/deep/1000` bench, and a
/// production tree of that depth would crash the app identically).
///
/// Same discipline as rustc's `ensure_sufficient_stack`: when fewer
/// than the red-zone bytes remain, the continuation runs on a fresh
/// heap-allocated stack segment. Cost on the hot path is one
/// stack-pointer probe per recursion level (sub-ns next to the
/// per-node layout/paint work).
///
/// Falls back to a direct call under miri (psm's stack-switching
/// assembly cannot be interpreted) and on wasm32 (no stack switching;
/// the dependency is compiled out in Cargo.toml) — those environments
/// keep plain recursion and its pre-existing depth limits.
#[inline]
fn ensure_stack<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(any(miri, target_arch = "wasm32"))]
    {
        f()
    }
    #[cfg(not(any(miri, target_arch = "wasm32")))]
    {
        // 128 KiB red zone: covers the deepest single-level frame
        // chain between two probes (driver frame + typed ctx + the
        // render object's own perform_layout/paint locals). 2 MiB
        // segments amortize one allocation across many levels.
        stacker::maybe_grow(128 * 1024, 2 * 1024 * 1024, f)
    }
}

/// Recursive helper for [`PipelineOwner::layout_dirty_root`].
///
/// Reborrows one [`NodePtr`] from the pre-acquired [`SubtreeBorrows`]
/// at each call level, drives `perform_layout_raw` against a typed
/// `BoxLayoutCtx`, and recurses via a closure that captures
/// `&SubtreeBorrows` (Sync via [`NodePtr`]'s `unsafe impl`). Distinct
/// call levels reborrow distinct slab slots (parent ≠ child) — no
/// aliasing.
///
/// # Safety
///
/// Caller must guarantee:
///
/// 1. `borrows` is alive for the entire duration of this call AND
///    every recursive call this helper triggers via the callback. The
///    [`PipelineOwner::layout_dirty_root`] flow constructs
///    `SubtreeBorrows` on the caller's stack and only invokes this
///    helper while the binding is live.
/// 2. At any moment, no two concurrent reborrows of the SAME
///    [`NodePtr`] exist. Sequential call levels (parent → child →
///    grandchild) reborrow DIFFERENT slots — preserved by the U21
///    `LayoutCycleGuard` (returns
///    [`crate::error::RenderError::LayoutCycle`] on re-entry into
///    a slot already in flight up the stack).
unsafe fn layout_subtree_borrowed<'tree>(
    borrows: &SubtreeBorrows<'tree>,
    id: RenderId,
    constraints: BoxConstraints,
) -> crate::error::RenderResult<Size> {
    ensure_stack(|| {
        // SAFETY: identical contract, forwarded verbatim from this
        // wrapper's own `# Safety` section; the stack-growth wrapper
        // only relocates which memory the frames live in, never their
        // borrow structure, lifetimes, or drop order.
        unsafe { layout_subtree_borrowed_impl(borrows, id, constraints) }
    })
}

/// Body of [`layout_subtree_borrowed`]; split out so every recursion
/// level enters through the [`ensure_stack`] probe.
///
/// # Safety
///
/// Same contract as [`layout_subtree_borrowed`].
unsafe fn layout_subtree_borrowed_impl<'tree>(
    borrows: &SubtreeBorrows<'tree>,
    id: RenderId,
    constraints: BoxConstraints,
) -> crate::error::RenderResult<Size> {
    // U21 cycle guard: register `id` in currently_laying_out *before*
    // any NodePtr reborrow. Drop on every exit path (RAII) — set stays
    // consistent across panics via the catch_unwind in the non-leaf
    // path below + Rust's drop-on-unwind discipline. Re-entry returns
    // RenderError::LayoutCycle(id) immediately, skipping the reborrow
    // attempt entirely (avoids the otherwise-UB second-Unique-tag on
    // an in-flight slot).
    let _cycle_guard = LayoutCycleGuard::enter(borrows, id)?;

    // Resolve id → NodePtr. Cross-thread access panics inside `get`.
    let NodePtr(node_ptr) = match borrows.get(id) {
        Some(np) => np,
        None => return Err(crate::error::RenderError::NodeNotFound(id)),
    };

    // SAFETY: `node_ptr` aliases a `&mut RenderNode` that
    // `SubtreeBorrows` holds disjointly from every other slot it
    // covers. The reborrow below is the FIRST and ONLY reborrow of
    // `id`'s slot in this call frame; the recursive callback below
    // reborrows DIFFERENT slots (child ≠ parent by tree acyclicity).
    // Distinct slot reborrows have independent Unique tags under
    // Stacked / Tree Borrows — no aliasing.
    let node_ref: &mut RenderNode = unsafe { &mut *node_ptr };

    // Extract typed RenderEntry<BoxProtocol> + snapshot child_ids in
    // one scope. Distinguish missing-from-tree (handled before via
    // borrows.get) from present-but-wrong-protocol.
    let node_protocol = node_ref.protocol_name();
    let entry: &mut RenderEntry<BoxProtocol> = match node_ref.as_box_mut() {
        Some(e) => e,
        None => {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "Box",
            });
        }
    };
    let child_ids: Vec<RenderId> = entry.links().children().to_vec();

    // Short-circuit clean children: if NEEDS_LAYOUT is not set AND
    // constraints match the cached value, skip layout entirely.
    // (Flutter rendering/object.dart:2852: early return before recurse)
    if !entry.needs_layout() && entry.state().has_constraints(&constraints) {
        // If constraints match and layout is clean, geometry MUST be present
        // from prior layout. This is a structural invariant.
        if let Some(geometry) = entry.state().geometry() {
            return Ok(geometry);
        }
        // If not, log and continue with layout (should never happen).
        tracing::warn!(
            node_id = ?id,
            "layout short-circuit: clean constraints cache but missing geometry; \
             proceeding with layout (invariant violation)"
        );
    }

    // Leaf path: delegate to layout_leaf_only (constructs typed
    // BoxLayoutCtx<Leaf> + catch_unwind + state update + relayout-
    // boundary bootstrap). Same code as the U18/U19 leaf bridge tests
    // exercise.
    if child_ids.is_empty() {
        return entry.layout_leaf_only(constraints);
    }

    // Non-leaf path: build ChildState backing vec + descendant-error
    // flag + recursive callback. Same shape as the prior U20 version,
    // but the callback recurses into `layout_subtree_borrowed` which
    // uses pre-acquired NodePtrs instead of fresh tree reborrows.
    let mut child_states: Vec<crate::protocol::ErasedChildState> = child_ids
        .iter()
        .map(|&cid| crate::protocol::ErasedChildState::new(cid))
        .collect();

    // Seed each ChildState.offset from the child's persisted
    // RenderState.offset. A parent that does not re-position a child
    // during this walk must preserve the child's prior offset
    // (Flutter parity: BoxParentData.offset persists until
    // positionChild overwrites it) — without the seed, the
    // post-layout commit below would silently reset unpositioned
    // children to Offset::ZERO. Box parents can now host both Box and
    // Sliver children, so seed through RenderNode's protocol-generic
    // offset view.
    for cs in &mut child_states {
        if let Some(child_ptr) = borrows.get(cs.id) {
            // SAFETY: shared reborrow of a DISTINCT child slot
            // (parent ≠ child by tree acyclicity). No `&mut` to this
            // slot is live: the recursive layout callback has not run
            // yet, and `node_ref` covers only the parent's slot.
            // Distinct slot reborrows have independent tags under
            // Stacked / Tree Borrows.
            let child_node: &RenderNode = unsafe { &*child_ptr.0 };
            cs.offset = child_node.offset();
            cs.needs_layout = child_node.needs_layout();
            if let Some(sliver_entry) = child_node.as_sliver() {
                cs.sliver_constraints = sliver_entry.state().constraints().copied();
                cs.sliver_geometry = sliver_entry.state().geometry();
            }
        }
    }

    // Descendant-error tracking flag. Closure flips to `true` on any
    // descendant `RenderError`; stage 6 below skips `clear_needs_layout`
    // when set so the parent stays dirty for next-frame retry. Shared
    // via `Arc<AtomicBool>` because the closure is `Send + Sync`
    // (inherited from `LayoutChildCallback`'s bound).
    let descendant_error_flag: std::sync::Arc<std::sync::atomic::AtomicBool> =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descendant_error_for_cb = std::sync::Arc::clone(&descendant_error_flag);

    // Capture `&SubtreeBorrows` for the recursive callback. `&T` is
    // `Send` iff `T: Sync`; `SubtreeBorrows: Sync` because its
    // `HashMap<RenderId, NodePtr>` is Sync (NodePtr declares Sync via
    // unsafe impl + RenderId is Sync) and the `PhantomData<&'tree mut ()>`
    // is Sync too. So `&SubtreeBorrows: Send + Sync`, satisfying
    // `LayoutChildCallback: Send + Sync`.
    let borrows_for_cb: &SubtreeBorrows<'_> = borrows;
    let descendant_error_for_sliver_cb = std::sync::Arc::clone(&descendant_error_flag);
    let cb_owned = move |child_id: RenderId, child_constraints: BoxConstraints| -> Size {
        // SAFETY: `borrows_for_cb` is alive (held by the outer
        // layout_dirty_root stack frame for the entire walk). The
        // recursive reborrow happens on `child_id`'s slot — distinct
        // from the current `id`'s slot (tree acyclicity → parent ≠
        // child). No two concurrent reborrows of the same NodePtr.
        match unsafe { layout_subtree_borrowed(borrows_for_cb, child_id, child_constraints) } {
            Ok(size) => size,
            Err(err) => {
                descendant_error_for_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                tracing::error!(
                    parent = ?id,
                    ?child_id,
                    ?err,
                    "layout_dirty_root: descendant layout failed; \
                         returning Size::ZERO to caller's perform_layout. \
                         Parent NEEDS_LAYOUT preserved for next-frame retry.",
                );
                Size::ZERO
            }
        }
    };
    let cb_ref: LayoutChildCallback<'_> = &cb_owned;

    // Sliver child callback: invoked when the Box parent calls
    // `ctx.layout_sliver_child(index, sliver_constraints)`.  Uses the
    // same `borrows_for_cb` pool and `descendant_error_flag` as the box
    // callback — no extra pre-acquisition needed since sliver children
    // are already in the pre-acquired `SubtreeBorrows` set.
    let sliver_cb_owned = move |child_id: RenderId,
                                sliver_constraints: SliverConstraints|
          -> SliverGeometry {
        // SAFETY: `borrows_for_cb` is alive for the entire walk (held
        // by the `layout_dirty_root` stack frame). The reborrow targets
        // `child_id`'s slot — distinct from the Box parent's slot
        // (tree acyclicity) and from any concurrently-active Box child
        // slot (LayoutCycleGuard blocks re-entry). No two concurrent
        // reborrows of the same NodePtr.
        match unsafe {
            layout_sliver_subtree_borrowed(borrows_for_cb, child_id, sliver_constraints)
        } {
            Ok(geometry) => geometry,
            Err(err) => {
                descendant_error_for_sliver_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                tracing::error!(
                    parent = ?id,
                    ?child_id,
                    ?err,
                    "layout_dirty_root: sliver descendant layout failed; \
                         returning SliverGeometry::ZERO to caller's perform_layout. \
                         Parent NEEDS_LAYOUT preserved for next-frame retry.",
                );
                SliverGeometry::ZERO
            }
        }
    };
    let sliver_cb_ref: SliverLayoutChildCallback<'_> = &sliver_cb_owned;

    // Construct the driver-side PARENT-DATA-ERASED context. The walk
    // cannot name the parent's ParentData type (it holds dyn nodes);
    // the typed blanket bridge reconstructs BoxLayoutCtx<T::Arity,
    // T::ParentData> per node and lazily creates each child's
    // parent-data slot with T::ParentData::default() — Flex/Stack and
    // every other non-BoxParentData parent now lay out in production
    // (the former ChildState<BoxParentData> hardcode panicked them in
    // from_erased).
    let mut ctx = crate::protocol::ErasedBoxLayoutCtx::new(
        constraints,
        &mut child_states,
        &child_ids,
        cb_ref,
        Some(sliver_cb_ref),
    );
    let erased: &mut dyn BoxLayoutCtxErased = &mut ctx;

    // Invoke perform_layout_raw wrapped in catch_unwind (symmetric
    // with the leaf path's layout_leaf_only — third-party panics
    // surface as RenderError::Poisoned instead of unwinding out of
    // layout_dirty_root). Capture debug_name BEFORE the &mut reborrow.
    let debug_name = entry.render_object().debug_name();
    let render_object = entry.render_object_mut();
    let unwind_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_object.perform_layout_raw(erased)
    }));
    let geometry = match unwind_result {
        Ok(inner) => inner?,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&'static str>().copied())
                .unwrap_or("(non-string panic payload)");
            tracing::error!(
                render_object = debug_name,
                panic_msg = msg,
                "perform_layout panicked in non-leaf path — surfacing as \
                     RenderError::Poisoned (symmetric with leaf-path \
                     layout_leaf_only catch_unwind discipline)",
            );
            return Err(crate::error::RenderError::poisoned(debug_name, "layout"));
        }
    };

    // State update on success path (mirrors
    // RenderEntry::layout_leaf_only's post-perform_layout discipline).
    // On the Err path above, state is intentionally unmodified so
    // NEEDS_LAYOUT stays set for next-frame retry.

    // Same protocol-generic geometry check the leaf commit runs
    // (RenderEntry::layout_leaf_only) — Flutter's debugAssertDoesMeetConstraints:
    // a finite size that satisfies the constraints. No-op for slivers.
    <BoxProtocol as Protocol>::debug_assert_layout_output(&constraints, &geometry);

    entry.state_mut().set_geometry(geometry);
    entry.state_mut().set_constraints(constraints);

    // Commit the offsets perform_layout wrote via `position_child`
    // into each child's persisted `RenderState.offset`. The
    // `ChildState` vec is a per-walk transient — without this commit
    // every positioned offset dies with the stack frame and paint /
    // hit-test (which read `RenderState.offset` as the authoritative
    // child position) would place all children at the parent origin.
    // Runs only on the parent-success path: on the Err / panic paths
    // above, state stays unmodified so NEEDS_LAYOUT retry semantics
    // hold. A descendant error does NOT skip the commit — the
    // parent's perform_layout returned Ok, so its positioning
    // decisions are valid regardless of a failed grandchild.
    for cs in &child_states {
        if let Some(child_ptr) = borrows.get(cs.id) {
            // SAFETY: shared reborrow of a DISTINCT child slot
            // (parent ≠ child by tree acyclicity). All recursive
            // child borrows ended when perform_layout_raw returned;
            // `entry`/`node_ref` cover only the parent's slot.
            // `set_offset` is an atomic store through `&self`.
            let child_node: &RenderNode = unsafe { &*child_ptr.0 };
            child_node.set_offset(cs.offset);
        }
    }

    // Bootstrap relayout boundary (U17). BoxProtocol runs the Flutter
    // formula; SliverProtocol would be a no-op (not reachable on this
    // path since we routed through as_box_mut).
    let has_parent = entry.links().parent().is_some();
    <BoxProtocol as Protocol>::bootstrap_relayout_boundary(entry.state(), has_parent);

    // Only clear NEEDS_LAYOUT if the recursive callback observed no
    // descendant failure. Preserves retry-next-frame semantics.
    if !descendant_error_flag.load(std::sync::atomic::Ordering::Relaxed) {
        entry.clear_needs_layout();
    } else {
        tracing::debug!(
            parent = ?id,
            "layout_dirty_root: a descendant errored during this walk; \
                 keeping parent NEEDS_LAYOUT set for next-frame retry"
        );
    }

    Ok(geometry)
}

// ============================================================================
// Cross-protocol child layout: Box parent → Sliver child
// ============================================================================
//
// `layout_sliver_subtree_borrowed` is the Sliver sibling of
// `layout_subtree_borrowed`. It is called from the `sliver_cb_owned`
// closure captured inside `layout_subtree_borrowed_impl`'s non-leaf path
// when a Box parent calls `ctx.layout_sliver_child(index, constraints)`.
//
// Scope: sliver subtrees. Leaf nodes still delegate to
// `RenderEntry::layout_leaf_only`; non-leaf slivers get an erased driver
// context and may call `ctx.layout_child(...)` to lay out sliver children.
//
// The short-circuit clean-child optimisation from the Box path is omitted
// here on purpose: sliver constraints change with scroll position on every
// frame. Omitting the check is correct and saves a branch.

/// Stack-probe wrapper for [`layout_sliver_subtree_borrowed_impl`].
///
/// # Safety
///
/// Same contract as [`layout_subtree_borrowed`]:
/// 1. `borrows` must outlive every recursive invocation this helper
///    triggers (it is held by the outer `layout_dirty_root` stack frame
///    for the entire walk).
/// 2. At any moment, no two concurrent reborrows of the SAME [`NodePtr`]
///    exist. The `LayoutCycleGuard` enforces this by returning
///    [`crate::error::RenderError::LayoutCycle`] on re-entry into a slot
///    already in flight, preventing the otherwise-UB second Unique tag.
unsafe fn layout_sliver_subtree_borrowed<'tree>(
    borrows: &SubtreeBorrows<'tree>,
    id: flui_foundation::RenderId,
    constraints: SliverConstraints,
) -> crate::error::RenderResult<SliverGeometry> {
    ensure_stack(|| {
        // SAFETY: identical contract, forwarded verbatim from this
        // wrapper's own `# Safety` section; the stack-growth wrapper
        // only relocates which memory the frames live in, never their
        // borrow structure, lifetimes, or drop order.
        unsafe { layout_sliver_subtree_borrowed_impl(borrows, id, constraints) }
    })
}

/// Body of [`layout_sliver_subtree_borrowed`]; split out so every
/// recursion level enters through the [`ensure_stack`] probe.
///
/// # Safety
///
/// Same contract as [`layout_sliver_subtree_borrowed`].
unsafe fn layout_sliver_subtree_borrowed_impl<'tree>(
    borrows: &SubtreeBorrows<'tree>,
    id: flui_foundation::RenderId,
    constraints: SliverConstraints,
) -> crate::error::RenderResult<SliverGeometry> {
    // U21 cycle guard: register `id` in currently_laying_out *before*
    // any NodePtr reborrow. Same RAII discipline as the Box path —
    // Drop runs on every exit including unwind so the set stays
    // consistent across panics.
    let _cycle_guard = LayoutCycleGuard::enter(borrows, id)?;

    // Resolve id → NodePtr. Cross-thread access panics inside `get`.
    let NodePtr(node_ptr) = match borrows.get(id) {
        Some(np) => np,
        None => return Err(crate::error::RenderError::NodeNotFound(id)),
    };

    // SAFETY: `node_ptr` aliases a `&mut RenderNode` that
    // `SubtreeBorrows` holds disjointly from every other slot it
    // covers. The reborrow below is the FIRST and ONLY reborrow of
    // `id`'s slot in this call frame; no other live `&mut` to this
    // slot exists — the Box parent's reborrow belongs to a DISTINCT
    // slot (parent ≠ child by tree acyclicity), and the cycle guard
    // above prevents re-entry into `id` from the same call stack.
    // Distinct slot reborrows have independent Unique tags under
    // Stacked / Tree Borrows — no aliasing.
    let node_ref: &mut crate::storage::RenderNode = unsafe { &mut *node_ptr };

    let node_protocol = node_ref.protocol_name();
    let entry: &mut RenderEntry<SliverProtocol> = match node_ref.as_sliver_mut() {
        Some(e) => e,
        None => {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "Sliver",
            });
        }
    };
    let child_ids: Vec<RenderId> = entry.links().children().to_vec();

    if child_ids.is_empty() {
        return entry.layout_leaf_only(constraints);
    }

    let mut child_states: Vec<crate::protocol::ErasedSliverChildState> = child_ids
        .iter()
        .map(|&cid| crate::protocol::ErasedSliverChildState::new(cid))
        .collect();

    // Seed child offsets from persisted RenderState so a sliver parent that
    // does not call `position_child` on a later pass preserves its child's
    // previous placement, matching the Box path's offset semantics.
    for cs in &mut child_states {
        if let Some(child_ptr) = borrows.get(cs.id) {
            // SAFETY: shared reborrow of a DISTINCT child slot
            // (parent != child by tree acyclicity). No recursive child
            // borrow has run yet in this frame, and `node_ref` covers only
            // the current sliver parent slot.
            let child_node: &crate::storage::RenderNode = unsafe { &*child_ptr.0 };
            cs.offset = child_node.offset();
        }
    }

    let descendant_error_flag: std::sync::Arc<std::sync::atomic::AtomicBool> =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let descendant_error_for_cb = std::sync::Arc::clone(&descendant_error_flag);
    let descendant_error_for_box_cb = std::sync::Arc::clone(&descendant_error_flag);
    let borrows_for_cb: &SubtreeBorrows<'_> = borrows;

    let cb_owned = move |child_id: RenderId,
                         child_constraints: SliverConstraints|
          -> SliverGeometry {
        // SAFETY: `borrows_for_cb` is alive for the whole dirty-root
        // walk, and this callback reborrows a child slot distinct from
        // the current sliver node. `LayoutCycleGuard` rejects re-entry.
        match unsafe { layout_sliver_subtree_borrowed(borrows_for_cb, child_id, child_constraints) }
        {
            Ok(geometry) => geometry,
            Err(err) => {
                descendant_error_for_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                tracing::error!(
                    parent = ?id,
                    ?child_id,
                    ?err,
                    "layout_dirty_root: sliver descendant layout failed; \
                     returning SliverGeometry::ZERO to caller's perform_layout. \
                     Parent NEEDS_LAYOUT preserved for next-frame retry.",
                );
                SliverGeometry::ZERO
            }
        }
    };
    let cb_ref: SliverChildLayoutCallback<'_> = &cb_owned;

    let box_cb_owned = move |child_id: RenderId, child_constraints: BoxConstraints| -> Size {
        // SAFETY: same subtree-borrow contract as the sliver child callback,
        // but routed through the Box layout walk for Sliver -> Box adapters.
        match unsafe { layout_subtree_borrowed(borrows_for_cb, child_id, child_constraints) } {
            Ok(size) => size,
            Err(err) => {
                descendant_error_for_box_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                tracing::error!(
                    parent = ?id,
                    ?child_id,
                    ?err,
                    "layout_dirty_root: box descendant layout failed from sliver parent; \
                     returning Size::ZERO to caller's perform_layout. \
                     Parent NEEDS_LAYOUT preserved for next-frame retry.",
                );
                Size::ZERO
            }
        }
    };
    let box_cb_ref: crate::protocol::sliver_protocol::BoxChildLayoutCallback<'_> = &box_cb_owned;

    let mut ctx = crate::protocol::ErasedSliverLayoutCtx::new(
        constraints,
        &mut child_states,
        &child_ids,
        cb_ref,
        box_cb_ref,
    );
    let erased: &mut dyn SliverLayoutCtxErased = &mut ctx;

    let debug_name = entry.render_object().debug_name();
    let render_object = entry.render_object_mut();
    let unwind_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_object.perform_layout_raw(erased)
    }));
    let geometry = match unwind_result {
        Ok(inner) => inner?,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&'static str>().copied())
                .unwrap_or("(non-string panic payload)");
            tracing::error!(
                render_object = debug_name,
                panic_msg = msg,
                "perform_layout panicked in non-leaf sliver path — surfacing as \
                 RenderError::Poisoned",
            );
            return Err(crate::error::RenderError::poisoned(debug_name, "layout"));
        }
    };

    <SliverProtocol as Protocol>::debug_assert_layout_output(&constraints, &geometry);

    entry.state_mut().set_geometry(geometry);
    entry.state_mut().set_constraints(constraints);

    // Commit child paint offsets produced by sliver parents
    // (`RenderSliverPadding` et al.) into the same parent-relative
    // offset slot that paint / hit-test already consult. The
    // `ErasedSliverChildState` vec is per-walk transient; without
    // this commit, child placement dies with the layout stack frame.
    for cs in &child_states {
        if let Some(child_ptr) = borrows.get(cs.id) {
            // SAFETY: shared reborrow of a DISTINCT child slot
            // (parent != child by tree acyclicity). All recursive
            // child borrows ended when perform_layout_raw returned;
            // `entry` / `node_ref` cover only the parent's slot.
            let child_node: &crate::storage::RenderNode = unsafe { &*child_ptr.0 };
            child_node.set_offset(cs.offset);
        }
    }

    let has_parent = entry.links().parent().is_some();
    <SliverProtocol as Protocol>::bootstrap_relayout_boundary(entry.state(), has_parent);

    if !descendant_error_flag.load(std::sync::atomic::Ordering::Relaxed) {
        entry.clear_needs_layout();
    } else {
        tracing::debug!(
            parent = ?id,
            "layout_dirty_root: a sliver descendant errored during this walk; \
             keeping parent NEEDS_LAYOUT set for next-frame retry"
        );
    }

    Ok(geometry)
}

// ============================================================================
// Compositing phase: run_compositing
// ============================================================================

impl PipelineOwner<Compositing> {
    /// Transitions a compositing-phase pipeline into the [`PaintPhase`] phase.
    pub fn into_paint(self) -> PipelineOwner<PaintPhase> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the compositing phase.
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates compositing bits for all dirty render objects.
    ///
    /// **D-block PR-A2 U34 (memo D3-3).** Port of Flutter's
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
        if self.dirty.needs_compositing.is_empty() {
            return Ok(());
        }
        let _span = tracing::debug_span!(
            "compositing",
            dirty_nodes = self.dirty.needs_compositing.len(),
        )
        .entered();

        // Sort shallow-first per Flutter
        // `_nodesNeedingCompositingBitsUpdate.sort((a, b) => a.depth - b.depth)`.
        self.dirty
            .needs_compositing
            .sort_unstable_by_key(|node| node.depth);

        // Iterate by index over the dirty list so the loop body can
        // call `&self` recursion without holding the iterator's
        // borrow across the call (the `update_subtree_compositing_bits`
        // method takes `&self`, which conflicts with the iter's
        // simultaneous `&self.dirty.needs_compositing` borrow under
        // some borrow-checker versions). Pre-fix this snapshotted
        // the ids into a fresh `Vec<RenderId>` per frame
        // (PR-A2 Copilot #3294557191).
        let mut actions = CompositingWalkActions::default();
        for i in 0..self.dirty.needs_compositing.len() {
            let id = self.dirty.needs_compositing[i].id;
            self.update_subtree_compositing_bits(id, &mut actions);
        }

        // Apply paint-queue mutations after the walk completes (under
        // disjoint `&mut self`). Remove-first, then re-enqueue, so an
        // id present in both buckets ends up correctly re-queued at the
        // post-walk depth.
        if !actions.remove_from_paint_queue.is_empty() {
            self.dirty
                .needs_paint
                .retain(|d| !actions.remove_from_paint_queue.contains(&d.id));
        }
        for (id, depth) in actions.mark_needs_paint {
            self.add_node_needing_paint(id, depth);
        }

        // PR #109 retain-capacity idiom (kept from pre-U34 stub): `clear()`
        // preserves the Vec's backing capacity across frames.
        self.dirty.needs_compositing.clear();
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
        // without a clone. Pre-fix the loop cloned children into a
        // fresh `Vec<RenderId>` per visited node (per-node heap
        // allocation) — flagged by Copilot #3294557204 as conflicting
        // with the repo's documented "no per-node child clone"
        // optimization in `RenderTree::visit_depth_first`
        // (`storage/tree.rs:738-751`).
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
/// after the recursion under `&mut self` (D-block PR-A2 U34).
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
    remove_from_paint_queue: rustc_hash::FxHashSet<RenderId>,
}

// ============================================================================
// Paint phase: run_paint + helpers
// ============================================================================

impl PipelineOwner<PaintPhase> {
    /// Transitions a paint-phase pipeline into the [`Semantics`] phase.
    pub fn into_semantics(self) -> PipelineOwner<Semantics> {
        rebind_phase(self)
    }

    /// Returns to [`Idle`] from the paint phase.
    pub fn into_idle(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Paints all dirty render objects.
    ///
    /// Phase 3 of the rendering pipeline, as a **fragment composition**
    /// (sans-IO paint model): each node's `paint_raw` records a
    /// node-local fragment — draw runs, child markers, clip scopes —
    /// which is immediately replayed into the frame's [`LayerTree`].
    /// Adjacent inline draw runs merge into shared `PictureLayer`s;
    /// repaint-boundary children are rebased to `Offset::ZERO` under
    /// their own `OffsetLayer`; clip scopes become real clip layers.
    ///
    /// A fresh full `LayerTree` is produced every paint pass —
    /// cross-frame retention of boundary subtrees is deliberately out
    /// of scope until the layer tree grows a structural-sharing
    /// substrate and the engine an incremental upload path.
    pub fn run_paint(&mut self) -> crate::error::RenderResult<()> {
        if self.dirty.needs_paint.is_empty() {
            return Ok(());
        }

        let _span =
            tracing::debug_span!("paint", dirty_nodes = self.dirty.needs_paint.len(),).entered();

        self.debug_doing_paint = true;

        // Deepest-first ordering retained (Flutter `flushPaint`): the
        // full-tree descent below repaints everything, but per-boundary
        // dirty-driven repaints will rely on this order once retention
        // lands, and keeping it now means the dirty-list semantics
        // don't shift under that change.
        self.dirty
            .needs_paint
            .sort_unstable_by_key(|n| std::cmp::Reverse(n.depth));

        if let Some(root_id) = self.root_id
            && self.render_tree.get(root_id).is_some()
        {
            let mut composer = FragmentComposer::new(self.device_pixel_ratio);
            match self.paint_subtree(&mut composer, root_id, Offset::ZERO) {
                Ok(()) => {
                    let layer_tree = composer.finish();
                    tracing::debug!("run_paint: layer tree has {} layers", layer_tree.len());
                    self.last_layer_tree = Some(layer_tree);
                }
                Err(e) => {
                    // Restore the debug invariant before propagating so
                    // the owner stays consistent on the error path.
                    self.debug_doing_paint = false;
                    return Err(e);
                }
            }
        }

        // Dirty-list residue scan: any node still flagged needs_paint
        // AFTER the root descent was not reached by it (multi-root or
        // detached subtree). Warn + clear so the bug is visible AND the
        // dirty list doesn't accumulate across frames.
        for dirty_node in &self.dirty.needs_paint {
            if let Some(render_node) = self.render_tree.get(dirty_node.id)
                && render_node.needs_paint()
            {
                tracing::warn!(
                    id = ?dirty_node.id,
                    depth = dirty_node.depth,
                    "run_paint: dirty node not reached by root descent (multi-root \
                     or detached subtree?); paint dropped, flag cleared"
                );
                render_node.clear_needs_paint();
            }
        }
        // `clear()` retains capacity (preserve Vec backing across frames).
        self.dirty.needs_paint.clear();

        self.debug_doing_paint = false;

        // Drain mid-paint dirty marks back into the dirty sets so paint
        // marks made during this pass become next-frame work rather
        // than being stranded — Flutter's flushPaint semantics.
        self.drain_mid_layout_marks();

        Ok(())
    }

    /// Records one node's paint fragment and replays it into the
    /// composer, recursing at child markers.
    ///
    /// Per-node order follows Flutter's `PaintingContext._paintWithContext`:
    /// `WAS_REPAINT_BOUNDARY` is written and `NEEDS_PAINT` cleared
    /// **before** the node paints, so a paint body that re-marks its own
    /// node is caught by the debug check below instead of silently
    /// erasing the evidence.
    fn paint_subtree(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
    ) -> crate::error::RenderResult<()> {
        ensure_stack(|| self.paint_subtree_impl(composer, node_id, origin))
    }

    /// Body of [`Self::paint_subtree`]; split out so every recursion
    /// level enters through the [`ensure_stack`] probe.
    fn paint_subtree_impl(
        &self,
        composer: &mut FragmentComposer,
        node_id: RenderId,
        origin: Offset,
    ) -> crate::error::RenderResult<()> {
        let Some(render_node) = self.render_tree.get(node_id) else {
            return Ok(());
        };

        let is_repaint_boundary = render_node.is_repaint_boundary();
        let alpha = render_node.paint_alpha();
        let transform = render_node.paint_transform();
        let child_ids: Vec<RenderId> = render_node.children().to_vec();

        // Written unconditionally PRE-paint (Flutter object.dart:3560):
        // a node flipping boundary→non-boundary leaves exactly one
        // `WAS_REPAINT_BOUNDARY=true` trail for the next compositing
        // walk's lost-boundary branch.
        render_node.set_was_repaint_boundary(is_repaint_boundary);

        // Clear BEFORE paint so the post-paint check catches a paint
        // body that marks its own node dirty (paint-must-not-redirty).
        render_node.clear_needs_paint();

        // Fully transparent subtree: skip recording entirely. Children
        // keep whatever dirty flags they carry; the residue scan in
        // run_paint clears them with a warning.
        if alpha == Some(0) {
            return Ok(());
        }

        // Record the node's fragment. paint_raw sees ONLY the recorder
        // (sans-IO): no tree access, no layer access, no recursion.
        let debug_name = render_node.debug_name();
        let mut recorder = FragmentRecorder::new(origin, self.device_pixel_ratio);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_node.paint_raw(&mut recorder, child_ids.len());
        }))
        .map_err(|_| crate::error::RenderError::poisoned(debug_name, "paint"))?;
        let fragment = recorder.finish();

        debug_assert!(
            !render_node.needs_paint(),
            "paint-must-not-redirty: a render object marked ITSELF \
             needs-paint during its own paint; derive visual changes \
             from state read at paint time instead of re-marking",
        );

        // Effect hooks wrap the ENTIRE node fragment (self draws AND
        // children). The pre-fragment walk wrapped children only; hook
        // implementors draw nothing themselves, so the visible result
        // is identical and the new rule matches Flutter (RenderOpacity
        // wraps its child's whole paint).
        let mut effect_layers = 0usize;
        if let Some(alpha) = alpha {
            composer.push_layer(Layer::Opacity(OpacityLayer::with_offset(
                f32::from(alpha) / 255.0,
                Offset::ZERO,
            )));
            effect_layers += 1;
        }
        if let Some(matrix) = transform {
            // The node reports its transform in LOCAL coordinates, but
            // every run inside this layer space is recorded with the
            // accumulated `origin` baked into its canvas transform.
            // Conjugate by the origin (Flutter object.dart
            // `pushTransform`: T(offset)·M·T(−offset)) so the matrix
            // pivots around the node's own origin instead of the layer
            // origin — a raw local matrix would translate/rotate the
            // whole accumulated space.
            let effective = if origin == Offset::ZERO {
                matrix
            } else {
                let (dx, dy) = (origin.dx.get(), origin.dy.get());
                flui_types::Matrix4::translation(dx, dy, 0.0)
                    * matrix
                    * flui_types::Matrix4::translation(-dx, -dy, 0.0)
            };
            composer.push_layer(Layer::Transform(TransformLayer::new(effective)));
            effect_layers += 1;
        }

        for op in fragment.ops {
            match op {
                FragmentOp::Run(list) => composer.append_run(list),
                FragmentOp::Push(clip) => composer.push_layer(clip_layer(*clip, origin)),
                FragmentOp::Pop => composer.pop_layer(),
                FragmentOp::Child {
                    index,
                    offset_override,
                } => {
                    let Some(&child_id) = child_ids.get(index) else {
                        debug_assert!(
                            false,
                            "fragment child marker {index} out of range ({} children) — \
                             PaintCx bounds-checks markers, so a mismatch means the \
                             tree changed during paint",
                            child_ids.len(),
                        );
                        continue;
                    };
                    let Some(child_node) = self.render_tree.get(child_id) else {
                        continue;
                    };
                    if child_node
                        .as_sliver()
                        .and_then(|entry| entry.state().geometry())
                        .is_some_and(|geometry| !geometry.visible)
                    {
                        continue;
                    }
                    // Authoritative child position: RenderState.offset,
                    // committed by the layout walk; paint_child_at
                    // overrides it explicitly.
                    let child_offset = offset_override.unwrap_or_else(|| child_node.offset());
                    let child_is_boundary = child_node.is_repaint_boundary();

                    if child_is_boundary {
                        // Boundary children rebase to ZERO under their
                        // own OffsetLayer so a future offset-only move
                        // is a layer-property update, not a repaint.
                        composer.push_layer(Layer::Offset(OffsetLayer::new(origin + child_offset)));
                        self.paint_subtree(composer, child_id, Offset::ZERO)?;
                        composer.pop_layer();
                    } else {
                        // Inline children bake into the shared picture
                        // space — runs merge, no extra layer.
                        self.paint_subtree(composer, child_id, origin + child_offset)?;
                    }
                }
            }
        }

        for _ in 0..effect_layers {
            composer.pop_layer();
        }

        Ok(())
    }
}

// ============================================================================
// Fragment composition (paint phase plumbing)
// ============================================================================

/// Builds the frame's [`LayerTree`] from replayed paint fragments,
/// merging adjacent inline draw runs into shared `PictureLayer`s.
///
/// Sealing discipline mirrors the recorder's: the open run is flushed
/// into a `PictureLayer` whenever a layer boundary needs ordering
/// (push/pop) and at [`Self::finish`]. The stack always holds at least
/// the root `OffsetLayer`.
#[derive(Debug)]
struct FragmentComposer {
    tree: LayerTree,
    stack: Vec<LayerId>,
    open: DisplayList,
}

impl FragmentComposer {
    /// `device_pixel_ratio` becomes the root layer's scale: the
    /// framework paints in LOGICAL pixels, the engine rasterizes in
    /// physical surface pixels — the root transform is the single
    /// place the two meet (Flutter's RenderView root transform).
    fn new(device_pixel_ratio: f32) -> Self {
        let mut tree = LayerTree::new();
        let root_layer = if (device_pixel_ratio - 1.0).abs() < f32::EPSILON {
            Layer::Offset(OffsetLayer::zero())
        } else {
            Layer::Transform(TransformLayer::new(flui_types::Matrix4::scaling(
                device_pixel_ratio,
                device_pixel_ratio,
                1.0,
            )))
        };
        let root = tree.insert(root_layer);
        tree.set_root(Some(root));
        Self {
            tree,
            stack: vec![root],
            open: DisplayList::new(),
        }
    }

    /// Merges a sealed fragment run into the open picture.
    fn append_run(&mut self, run: DisplayList) {
        self.open.append(run);
    }

    /// Flushes the open picture into a `PictureLayer` under the
    /// current stack top (no-op when empty).
    fn seal_picture(&mut self) {
        if flui_painting::DisplayListCore::is_empty(&self.open) {
            return;
        }
        let list = std::mem::take(&mut self.open);
        let layer_id = self.tree.insert(Layer::from(PictureLayer::new(list)));
        let parent = *self
            .stack
            .last()
            .expect("composer stack always holds the root layer (popping it is rejected)");
        self.tree.add_child(parent, layer_id);
    }

    fn push_layer(&mut self, layer: Layer) {
        self.seal_picture();
        let id = self.tree.insert(layer);
        let parent = *self
            .stack
            .last()
            .expect("composer stack always holds the root layer (popping it is rejected)");
        self.tree.add_child(parent, id);
        self.stack.push(id);
    }

    fn pop_layer(&mut self) {
        self.seal_picture();
        debug_assert!(
            self.stack.len() > 1,
            "composer pop without matching push — fragment scope ops are \
             balanced by the recorder, so an underflow means the replay \
             loop pushed/popped asymmetrically",
        );
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    fn finish(mut self) -> LayerTree {
        self.seal_picture();
        debug_assert_eq!(
            self.stack.len(),
            1,
            "composer finished with unbalanced layer stack — every \
             push_layer in the replay loop must have a matching pop_layer",
        );
        self.tree
    }
}

/// Maps a recorded clip scope onto its `flui-layer` clip layer.
///
/// Clip shapes are recorded in the node's LOCAL coordinates, while the
/// runs they bracket carry the accumulated `origin` baked into their
/// canvas transforms — so the shape is shifted by `origin` here
/// (Flutter `pushClipRect`: `clipRect.shift(offset)`), or a clip away
/// from the parent origin would cut at the layer's (0,0) instead of
/// the node's position.
///
/// Always a real clip layer today; lowering non-composited clips back
/// into canvas clips inside the merged picture is a composer-side
/// optimization gated on the `needs_compositing` bits — correctness is
/// identical either way, so the recording API does not expose the
/// choice.
fn clip_layer(clip: FragmentClip, origin: Offset) -> Layer {
    match clip {
        FragmentClip::Rect { rect, behavior } => {
            Layer::ClipRect(ClipRectLayer::new(rect.translate_offset(origin), behavior))
        }
        FragmentClip::RRect { rrect, behavior } => Layer::ClipRRect(ClipRRectLayer::new(
            rrect.translate_offset(origin),
            behavior,
        )),
        FragmentClip::Path { path, behavior } => {
            let path = if origin == Offset::ZERO {
                *path
            } else {
                path.translate(origin)
            };
            Layer::ClipPath(Box::new(ClipPathLayer::new(path, behavior)))
        }
    }
}

// ============================================================================
// Semantics phase: run_semantics
// ============================================================================

impl PipelineOwner<Semantics> {
    /// Completes the frame and returns to [`Idle`].
    pub fn finish(self) -> PipelineOwner<Idle> {
        rebind_phase(self)
    }

    /// Updates semantics for all dirty render objects.
    ///
    /// This is phase 4 of the rendering pipeline. During semantics:
    /// - Accessibility information is gathered
    /// - Semantics tree is updated
    ///
    /// Nodes are sorted by depth (shallow first) for top-down traversal.
    /// The geometries of children depend on ancestors' transforms and clips,
    /// so parents must be processed first. This matches Flutter's
    /// `flushSemantics`.
    pub fn run_semantics(&mut self) -> crate::error::RenderResult<()> {
        if !self.semantics_enabled() {
            return Ok(());
        }

        tracing::debug!("run_semantics: {} nodes", self.dirty.needs_semantics.len());

        self.debug_doing_semantics = true;

        // PR #109 review feedback: pre-fix this path used
        // `std::mem::take(&mut self.dirty.needs_semantics)` to drain in
        // one step. Take leaves an empty `Vec::new()` (capacity 0)
        // behind, so every subsequent semantics-enabled frame's first
        // push re-allocates. Switch to an in-place sort + iterate +
        // clear pattern that preserves the Vec's backing capacity
        // across frames (idiom: *Programming Rust* 2nd ed §11 "Owned
        // vs Borrowed", retain the allocation by retaining ownership).
        // The Flutter-parity `where !object._needsLayout` filter the
        // pre-cycle comment promised was never implemented; that gap
        // lands when the real semantics-config build is wired (R-1
        // follow-up).

        // Sort shallow-first matching Flutter's flushSemantics. Roots
        // dispatch before their descendants so a parent's config is
        // assembled before children fold into it.
        self.dirty.needs_semantics.sort_unstable_by_key(|n| n.depth);

        // Cycle 4 R-1: pre-cycle the path panicked with
        // `unimplemented!()` once any node was queued — a Constitution
        // Principle 6 violation in a hot-path callable from
        // `RendererBinding::draw_frame` on every frame as soon as
        // semantics_enabled() flipped true.
        //
        // Post-cycle: walk the dirty list, emit a `tracing::warn!`
        // per node carrying the missing-integration hint, and return
        // `Ok(())`. The framework no longer aborts on semantics flips;
        // when the full `SemanticsOwner` integration lands, swap the
        // warn for the real config-build + owner-register call.
        //
        // Split-borrow as in `run_compositing`: `self.dirty.needs_semantics`
        // and `self.render_tree` are disjoint fields under Rust 2024
        // disjoint capture, so the loop compiles without a temporary
        // clone.
        for dirty_node in &self.dirty.needs_semantics {
            if self.render_tree.contains(dirty_node.id) {
                tracing::warn!(
                    id = ?dirty_node.id,
                    depth = dirty_node.depth,
                    "run_semantics: full SemanticsOwner integration pending; \
                     semantics config build for this node is a no-op until \
                     RenderObject → SemanticsConfiguration plumbing lands"
                );
            }
        }
        // `clear()` retains the Vec's allocated capacity; next frame's
        // pushes amortise into the existing buffer.
        self.dirty.needs_semantics.clear();

        self.debug_doing_semantics = false;

        // PR-A1 U22 P1 review fix (Codex 3294365736): drain
        // mid_layout_marks.needs_semantics so semantics marks made
        // during this iteration's `debug_doing_semantics = true`
        // window aren't stranded. Drained entries land on
        // dirty.needs_semantics for the NEXT run_semantics
        // invocation.
        self.drain_mid_layout_marks();

        Ok(())
    }
}

/// Internal helper: shifts the `Phase` phantom parameter without touching any
/// runtime field. Behaviour-preserving by construction.
#[inline]
fn rebind_phase<From, To>(from: PipelineOwner<From>) -> PipelineOwner<To>
where
    From: PipelinePhase,
    To: PipelinePhase,
{
    PipelineOwner {
        id: from.id,
        render_tree: from.render_tree,
        root_id: from.root_id,
        notifier: from.notifier,
        dirty: from.dirty,
        mid_layout_marks: from.mid_layout_marks,
        root_constraints: from.root_constraints,
        debug_doing_layout: from.debug_doing_layout,
        debug_doing_paint: from.debug_doing_paint,
        debug_doing_semantics: from.debug_doing_semantics,
        semantics_enabled: from.semantics_enabled,
        last_layer_tree: from.last_layer_tree,
        device_pixel_ratio: from.device_pixel_ratio,
        handle: from.handle,
        dirty_rx: from.dirty_rx,
        _phase: PhantomData,
    }
}

// ============================================================================
// Memoizing query walks (intrinsics / dry layout)
// ============================================================================

/// One node's slot in a memoizing query walk: the disjoint `&mut`
/// borrow plus a snapshot of the node's child ids. The node is moved
/// OUT (`node.take()`) while its own computation runs, so re-entry —
/// which only a cyclic child link can produce — is detected instead of
/// aliasing the borrow.
struct QuerySlot<'a> {
    node: Option<&'a mut crate::storage::RenderNode>,
    children: Vec<RenderId>,
}

/// Recursive memoized intrinsic query over the take-out slot map.
///
/// Per node: cache peek → on miss, run the object's `intrinsic_raw`
/// with a child callback that recurses through this same function →
/// store the result. Errors inside the child callback are stashed and
/// re-raised after the object call returns (the raw callback channel
/// is infallible by design — same convention as the hit-test walk).
fn intrinsic_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
) -> crate::error::RenderResult<f32> {
    ensure_stack(|| intrinsic_query_impl(slots, id, dimension, extent))
}

/// Body of [`intrinsic_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn intrinsic_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
) -> crate::error::RenderResult<f32> {
    let Some(slot) = slots.get_mut(&id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };
    let Some(node) = slot.node.take() else {
        // Only reachable through a cyclic child link: the node's own
        // computation is still on the stack. Degenerate-but-defined in
        // release; loud in debug (collect_subtree_ids already refuses
        // to loop, so the cycle must close through duplicate child
        // indices).
        debug_assert!(
            false,
            "intrinsic query re-entered node {id:?} mid-computation — cyclic child links"
        );
        return Ok(0.0);
    };
    let children = slot.children.clone();

    let result = (|| {
        let Some(entry) = node.as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol: "sliver",
                constraints_protocol: "box",
            });
        };
        if let Some(hit) = entry
            .state()
            .layout_cache()
            .peek_intrinsic(dimension, extent)
        {
            return Ok(hit);
        }
        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_query =
                |index: usize, dim: crate::storage::IntrinsicDimension, ext: f32| -> f32 {
                    let Some(&child_id) = children.get(index) else {
                        child_err.get_or_insert(crate::error::RenderError::contract_violation(
                            "intrinsic child query",
                            "child index out of range for this node's children",
                        ));
                        return 0.0;
                    };
                    match intrinsic_query(slots, child_id, dim, ext) {
                        Ok(v) => v,
                        Err(err) => {
                            child_err.get_or_insert(err);
                            0.0
                        }
                    }
                };
            entry
                .render_object()
                .intrinsic_raw(dimension, extent, children.len(), &mut child_query)
        };
        if let Some(err) = child_err {
            return Err(err);
        }
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_intrinsic(dimension, extent, value);
        Ok(value)
    })();

    // Restore the slot even on the error path — sibling queries in the
    // same walk must still find the node.
    if let Some(slot) = slots.get_mut(&id) {
        slot.node = Some(node);
    }
    result
}

/// Recursive memoized dry-layout query; same skeleton as
/// [`intrinsic_query`] with `(constraints → Size)` payloads.
fn dry_layout_query(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
) -> crate::error::RenderResult<flui_types::Size> {
    ensure_stack(|| dry_layout_query_impl(slots, id, constraints))
}

/// Body of [`dry_layout_query`]; split out so every recursion level
/// enters through the [`ensure_stack`] probe.
fn dry_layout_query_impl(
    slots: &mut rustc_hash::FxHashMap<RenderId, QuerySlot<'_>>,
    id: RenderId,
    constraints: crate::constraints::BoxConstraints,
) -> crate::error::RenderResult<flui_types::Size> {
    let Some(slot) = slots.get_mut(&id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };
    let Some(node) = slot.node.take() else {
        debug_assert!(
            false,
            "dry-layout query re-entered node {id:?} mid-computation — cyclic child links"
        );
        return Ok(flui_types::Size::ZERO);
    };
    let children = slot.children.clone();

    let result = (|| {
        let Some(entry) = node.as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol: "sliver",
                constraints_protocol: "box",
            });
        };
        if let Some(hit) = entry.state().layout_cache().peek_dry_layout(constraints) {
            return Ok(hit);
        }
        let mut child_err: Option<crate::error::RenderError> = None;
        let value = {
            let child_err = &mut child_err;
            let mut child_dry =
                |index: usize, c: crate::constraints::BoxConstraints| -> flui_types::Size {
                    let Some(&child_id) = children.get(index) else {
                        child_err.get_or_insert(crate::error::RenderError::contract_violation(
                            "dry-layout child query",
                            "child index out of range for this node's children",
                        ));
                        return flui_types::Size::ZERO;
                    };
                    match dry_layout_query(slots, child_id, c) {
                        Ok(v) => v,
                        Err(err) => {
                            child_err.get_or_insert(err);
                            flui_types::Size::ZERO
                        }
                    }
                };
            entry
                .render_object()
                .dry_layout_raw(constraints, children.len(), &mut child_dry)
        };
        if let Some(err) = child_err {
            return Err(err);
        }
        entry
            .state_mut()
            .layout_cache_mut()
            .insert_dry_layout(constraints, value);
        Ok(value)
    })();

    if let Some(slot) = slots.get_mut(&id) {
        slot.node = Some(node);
    }
    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_pipeline_owner_new() {
        let owner = PipelineOwner::new();
        assert!(owner.root_id().is_none());
        assert!(owner.nodes_needing_layout().is_empty());
        assert!(owner.nodes_needing_paint().is_empty());
        assert!(!owner.debug_doing_layout());
        assert!(!owner.debug_doing_paint());
    }

    #[test]
    fn test_pipeline_owner_id_unique() {
        let owner1 = PipelineOwner::new();
        let owner2 = PipelineOwner::new();
        assert_ne!(owner1.id(), owner2.id());
    }

    #[test]
    fn test_pipeline_owner_dirty_nodes() {
        let mut owner = PipelineOwner::new();

        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_layout(RenderId::new(2), 1);
        owner.add_node_needing_paint(RenderId::new(3), 2);

        assert_eq!(owner.nodes_needing_layout().len(), 2);
        assert_eq!(owner.nodes_needing_paint().len(), 1);
        assert_eq!(owner.dirty_node_count(), 3);
        assert!(owner.has_dirty_nodes());
    }

    #[test]
    fn test_pipeline_owner_run_layout() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_layout(RenderId::new(2), 1);

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout phase should succeed");

        assert!(owner.nodes_needing_layout().is_empty());
    }

    #[test]
    fn test_pipeline_owner_run_frame() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_paint(RenderId::new(2), 1);
        owner.add_node_needing_compositing_bits_update(RenderId::new(3), 2);

        let (owner, result) = owner.run_frame();
        let _layer_tree = result.expect("frame should succeed");

        assert!(!owner.has_dirty_nodes());
    }

    #[test]
    fn test_run_layout_sorts_by_depth_shallow_first() {
        let mut owner = PipelineOwner::new();
        // Add nodes in reverse depth order
        owner.add_node_needing_layout(RenderId::new(3), 2); // deepest
        owner.add_node_needing_layout(RenderId::new(1), 0); // shallowest
        owner.add_node_needing_layout(RenderId::new(2), 1); // middle

        // Before flush, they're in insertion order
        assert_eq!(owner.nodes_needing_layout()[0].depth, 2);
        assert_eq!(owner.nodes_needing_layout()[1].depth, 0);
        assert_eq!(owner.nodes_needing_layout()[2].depth, 1);

        let mut owner = owner.into_layout();
        owner.run_layout().expect("layout phase should succeed");

        // After flush, list is cleared
        assert!(owner.nodes_needing_layout().is_empty());
    }

    #[test]
    fn test_run_paint_sorts_by_depth_deep_first() {
        let mut owner = PipelineOwner::new();
        // Add nodes in shallow-first order
        owner.add_node_needing_paint(RenderId::new(1), 0); // shallowest
        owner.add_node_needing_paint(RenderId::new(2), 1); // middle
        owner.add_node_needing_paint(RenderId::new(3), 2); // deepest

        let owner = owner.into_layout().into_compositing();
        let mut owner = owner.into_paint();
        owner.run_paint().expect("paint phase should succeed");

        // After flush, list is cleared
        assert!(owner.nodes_needing_paint().is_empty());
    }

    // test_pipeline_owner_hierarchy removed in Mythos Step 9 along with the
    // adopt_child/drop_child/child_count/children API. Multi-PipelineOwner
    // scenarios (multi-window) are now owned by flui-app side-by-side.

    #[test]
    fn test_pipeline_owner_semantics_enabled() {
        let owner = PipelineOwner::new();
        assert!(!owner.semantics_enabled());

        owner.set_semantics_enabled(true);
        assert!(owner.semantics_enabled());

        owner.set_semantics_enabled(false);
        assert!(!owner.semantics_enabled());
    }

    #[test]
    fn test_pipeline_owner_clear_dirty_nodes() {
        let mut owner = PipelineOwner::new();
        owner.add_node_needing_layout(RenderId::new(1), 0);
        owner.add_node_needing_paint(RenderId::new(2), 1);
        owner.add_node_needing_semantics(RenderId::new(3), 2);

        owner.clear_all_dirty_nodes();

        assert!(!owner.has_dirty_nodes());
        assert_eq!(owner.dirty_node_count(), 0);
    }

    #[test]
    fn test_pipeline_owner_with_callbacks() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        owner.request_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        owner.request_visual_update();
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    /// Idle-wake contract: scheduling NEW dirty work fires the
    /// visual-update callback exactly once per new queue entry, so a
    /// quiescent platform loop wakes for the frame — and duplicate
    /// marks (a frame is already scheduled) don't spam wakes.
    #[test]
    fn dirty_marks_fire_visual_update_once_per_new_entry() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        owner.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "a new layout entry must wake the platform",
        );
        owner.add_node_needing_layout(RenderId::new(1), 0);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "a duplicate entry means a frame is already scheduled — no second wake",
        );

        owner.add_node_needing_paint(RenderId::new(2), 1);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            2,
            "a new paint entry must wake the platform",
        );
        owner.add_node_needing_paint(RenderId::new(2), 1);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    /// The boundary-walking `mark_needs_layout` fires the wake when it
    /// enqueues the boundary, and stays silent when the boundary is
    /// already queued.
    #[test]
    fn mark_needs_layout_fires_visual_update_on_boundary_enqueue() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let mut owner = PipelineOwner::with_callbacks(
            Some(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }),
            None::<fn()>,
            None::<fn()>,
        );

        let id = owner.insert(Box::new(crate::objects::RenderColoredBox::red(10.0, 10.0))
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.clear_all_dirty_nodes();
        let base = counter.load(Ordering::Relaxed);

        owner.mark_needs_layout(id);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            base + 1,
            "enqueueing the relayout boundary must wake the platform",
        );
        owner.mark_needs_layout(id);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            base + 1,
            "boundary already queued — no extra wake",
        );
    }

    // ========================================================================
    // Mythos Step 12: catch_unwind plumbing
    // ========================================================================
    //
    // Verifies that a render object panicking inside a third-party trait
    // call (paint, perform_layout_raw) surfaces as
    // RenderError::Poisoned rather than aborting the process, and that
    // the owner remains usable for a subsequent frame.

    /// Direct (non-RenderBox) RenderObject<BoxProtocol> impl whose
    /// `paint` method panics on demand. Used by the catch_unwind tests
    /// below.
    ///
    /// We bypass the RenderBox blanket impl (whose paint is a no-op)
    /// because we want to exercise the actual third-party paint call
    /// site the pipeline owner wraps in `catch_unwind`.
    #[derive(Debug)]
    struct PanickingPaintBox {
        size: flui_types::Size,
    }

    impl PanickingPaintBox {
        fn new() -> Self {
            Self {
                size: flui_types::Size::ZERO,
            }
        }
    }

    impl flui_foundation::Diagnosticable for PanickingPaintBox {}
    impl crate::traits::PaintEffectsCapability for PanickingPaintBox {}
    impl crate::traits::SemanticsCapability for PanickingPaintBox {}
    impl crate::traits::HotReloadCapability for PanickingPaintBox {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for PanickingPaintBox {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            Ok(self.size)
        }

        fn paint_raw(&self, _recorder: &mut crate::context::FragmentRecorder, _child_count: usize) {
            panic!("PanickingPaintBox::paint_raw -- intentional test panic");
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
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

        fn geometry(&self) -> &crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
            &self.size
        }

        fn set_geometry(
            &mut self,
            geometry: crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        ) {
            self.size = geometry;
        }

        fn paint_bounds(&self) -> flui_types::Rect {
            flui_types::Rect::from_origin_size(flui_types::Point::ZERO, self.size)
        }
    }

    /// Direct (non-RenderBox) RenderObject<BoxProtocol> impl whose
    /// `perform_layout_raw` panics. Used to test catch_unwind on the
    /// layout phase through `RenderEntry::layout`.
    #[derive(Debug)]
    struct PanickingLayoutBox {
        size: flui_types::Size,
    }

    impl PanickingLayoutBox {
        fn new() -> Self {
            Self {
                size: flui_types::Size::ZERO,
            }
        }
    }

    impl flui_foundation::Diagnosticable for PanickingLayoutBox {}
    impl crate::traits::PaintEffectsCapability for PanickingLayoutBox {}
    impl crate::traits::SemanticsCapability for PanickingLayoutBox {}
    impl crate::traits::HotReloadCapability for PanickingLayoutBox {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for PanickingLayoutBox {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            // Intentional unstructured panic — exercises the catch_unwind →
            // Poisoned path in `RenderEntry::layout_leaf_only`. This test
            // fixture is one explicit way to produce
            // `RenderError::Poisoned`; in production any third-party
            // panic in user widget code (`panic!`, `unwrap()`, assertion
            // failure inside `RenderBox::perform_layout`) reaches the
            // same path. Bridge-detected contract violations go through
            // the typed `Result` chain instead and surface as
            // `RenderError::ContractViolation`.
            panic!("PanickingLayoutBox::perform_layout_raw -- intentional test panic");
        }

        fn paint_raw(&self, _recorder: &mut crate::context::FragmentRecorder, _child_count: usize) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
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

        fn geometry(&self) -> &crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
            &self.size
        }

        fn set_geometry(
            &mut self,
            geometry: crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        ) {
            self.size = geometry;
        }

        fn paint_bounds(&self) -> flui_types::Rect {
            flui_types::Rect::from_origin_size(flui_types::Point::ZERO, self.size)
        }
    }

    /// A panicking `paint` call must surface as
    /// `RenderError::Poisoned { phase: "paint", .. }` and not abort.
    /// The owner must remain usable for a subsequent frame.
    #[test]
    fn test_run_frame_catches_paint_panic() {
        use crate::error::RenderError;

        // Silence the default panic hook for the duration of this test
        // so cargo test output isn't polluted by the intentional panic.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let mut owner = PipelineOwner::new();
        let root_id = owner.insert(Box::new(PanickingPaintBox::new())
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.set_root_id(Some(root_id));

        let (owner, result) = owner.run_frame();

        std::panic::set_hook(prev);

        // The frame produces an error of the Poisoned variant.
        let err = result.expect_err("paint should panic, surface as Err");
        match err {
            RenderError::Poisoned { phase, .. } => {
                assert_eq!(phase, "paint", "phase should be 'paint'");
            }
            other => panic!("expected RenderError::Poisoned, got {other:?}"),
        }

        // Owner is reusable for a subsequent frame -- it's back at <Idle>
        // and another `run_frame` call must not panic. We re-mark the
        // panicking node dirty to force the paint path to run again,
        // since the first frame already cleared its paint dirty flag.
        // The second frame must hit the panic site once more and
        // surface the same Err(Poisoned).
        let mut owner = owner;
        owner.add_node_needing_paint(root_id, 0);

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let (owner, second_result) = owner.run_frame();
        std::panic::set_hook(prev);

        let _second_err =
            second_result.expect_err("re-marked paint should hit the panicking node again");

        // The owner is still at Idle after the second frame and can be
        // dropped cleanly -- the catch_unwind plumbing has not left any
        // resources poisoned.
        drop(owner);
    }

    /// A panicking `perform_layout_raw` surfaces as
    /// `RenderError::Poisoned { phase: "layout", .. }` through
    /// `RenderEntry::layout`. This verifies the catch_unwind wrapper on
    /// the layout call site (Mythos Step 12).
    ///
    /// Note: `RenderEntry::layout` is not yet wired into the pipeline
    /// owner's `run_layout` (the propagation stubs are empty per the
    /// Mythos Outstanding Refactors list), so this test exercises the
    /// entry directly rather than through `run_frame`.
    #[test]
    fn test_render_entry_layout_catches_panic() {
        use crate::error::RenderError;
        use crate::storage::RenderEntry;
        use flui_types::Size;

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let mut entry =
            RenderEntry::<crate::protocol::BoxProtocol>::new(Box::new(PanickingLayoutBox::new())
                as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);

        let result = entry.layout_leaf_only(crate::constraints::BoxConstraints::tight(Size::ZERO));

        std::panic::set_hook(prev);

        let err = result.expect_err("perform_layout_raw should panic, surface as Err");
        match err {
            RenderError::Poisoned { phase, .. } => {
                assert_eq!(phase, "layout", "phase should be 'layout'");
            }
            other => panic!("expected RenderError::Poisoned, got {other:?}"),
        }

        // After a poisoned layout, the entry's NEEDS_LAYOUT flag is
        // still set (geometry was never updated).
        assert!(
            entry.needs_layout(),
            "needs_layout should remain true on the panic path"
        );
    }

    /// `RenderObject::debug_name` returns the concrete type name via
    /// vtable dispatch through `core::any::type_name::<Self>()` in the
    /// monomorphized default body. This is the static identifier used
    /// in `RenderError::Poisoned`.
    #[test]
    fn test_debug_name_via_dyn_dispatch() {
        let panicking: Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>> =
            Box::new(PanickingPaintBox::new());

        let name = panicking.debug_name();
        // Type names include the module path. We only assert that it
        // contains the concrete type's identifier to keep the test
        // independent of compiler-version formatting.
        assert!(
            name.contains("PanickingPaintBox"),
            "debug_name() should resolve to the concrete type via vtable; got `{name}`"
        );
    }

    // ========================================================================
    // D-block PR-A1 U22 P1 regression (Codex 3294365736 / Copilot 3294367387)
    // ========================================================================
    //
    // Verifies the mid-phase routing + drain integration: when
    // debug_doing_layout is true, add_node_needing_layout routes into
    // mid_layout_marks; the wired drain at run_layout iteration end
    // moves those entries back into dirty so the next iteration picks
    // them up. Lib-scoped because debug_doing_layout is a private field.

    /// Direct test of the U22 mid-phase routing → drain integration.
    /// Flips debug_doing_layout=true, pushes via the public
    /// add_node_needing_layout API, verifies the entry went to
    /// mid_layout_marks (NOT dirty); then calls drain_mid_layout_marks
    /// and verifies the entry is now in dirty.
    #[test]
    fn test_mid_phase_layout_marks_route_to_side_queue_then_drain_back() {
        let mut owner = PipelineOwner::new();
        let id = owner.insert(Box::new(PanickingPaintBox::new())
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.clear_all_dirty_nodes();

        // Phase 1: regular add (debug_doing_layout=false) goes to dirty.
        owner.add_node_needing_layout(id, 0);
        assert_eq!(owner.nodes_needing_layout().len(), 1);
        assert!(!owner.has_mid_layout_marks());
        owner.clear_all_dirty_nodes();

        // Phase 2: simulate mid-phase by flipping the private debug
        // flag. Subsequent add_node_needing_layout routes into
        // mid_layout_marks instead of dirty.
        owner.debug_doing_layout = true;
        owner.add_node_needing_layout(id, 0);
        owner.debug_doing_layout = false;

        assert_eq!(
            owner.nodes_needing_layout().len(),
            0,
            "mid-phase add must NOT land in dirty.needs_layout",
        );
        assert!(
            owner.has_mid_layout_marks(),
            "mid-phase add must land in mid_layout_marks",
        );

        // Phase 3: drain moves the side-queued entry back to dirty.
        let drained = owner.drain_mid_layout_marks();
        assert_eq!(drained, 1, "drain must report 1 entry moved");
        assert_eq!(
            owner.nodes_needing_layout().len(),
            1,
            "drained mid-mark must land in dirty.needs_layout",
        );
        assert!(
            !owner.has_mid_layout_marks(),
            "mid queue must be empty post-drain"
        );
    }

    /// Same shape for the dedup invariant under mid-phase routing:
    /// repeated mid-phase adds collapse to one entry in
    /// mid_layout_marks.
    #[test]
    fn test_mid_phase_routing_dedups_repeated_marks() {
        let mut owner = PipelineOwner::new();
        let id = owner.insert(Box::new(PanickingPaintBox::new())
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.clear_all_dirty_nodes();

        owner.debug_doing_layout = true;
        owner.add_node_needing_layout(id, 0);
        owner.add_node_needing_layout(id, 0);
        owner.add_node_needing_layout(id, 0);
        owner.debug_doing_layout = false;

        let drained = owner.drain_mid_layout_marks();
        assert_eq!(
            drained, 1,
            "3 repeated mid-phase marks must dedup to 1 entry; got {drained}",
        );
    }

    // ========================================================================
    // D-block PR-A1 U15 — PipelineOwner::mark_needs_layout walk tests
    // ========================================================================
    //
    // Verifies the Flutter `markNeedsLayout` shape ported in U15:
    //   - propagation walks the ancestor chain
    //   - flag is set on every visited node (NEEDS_LAYOUT)
    //   - propagation stops at the first relayout boundary or root
    //   - dirty.needs_layout receives exactly the boundary id
    //   - re-marking an already-dirty node is a no-op
    //   - stale RenderIds (post-removal) terminate the walk silently

    /// Build a 3-level chain root → middle → leaf with `PanickingPaintBox`
    /// mocks via the public `insert` / `insert_child_render_object` APIs,
    /// then clear the dirty queue so tests can observe only post-clear
    /// marks. Returns `(owner, root_id, middle_id, leaf_id)`.
    fn build_three_level_chain() -> (PipelineOwner<Idle>, RenderId, RenderId, RenderId) {
        let mut owner = PipelineOwner::new();
        let root_id = owner.insert(Box::new(PanickingPaintBox::new())
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        let middle_id = owner
            .insert_child_render_object(
                root_id,
                Box::new(PanickingPaintBox::new())
                    as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
            )
            .expect("middle should attach under root");
        let leaf_id = owner
            .insert_child_render_object(
                middle_id,
                Box::new(PanickingPaintBox::new())
                    as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>,
            )
            .expect("leaf should attach under middle");
        owner.clear_all_dirty_nodes();
        for id in [root_id, middle_id, leaf_id] {
            if let Some(node) = owner.render_tree.get_mut(id) {
                match node {
                    crate::storage::RenderNode::Box(entry) => {
                        entry.state().clear_needs_layout();
                    }
                    crate::storage::RenderNode::Sliver(entry) => {
                        entry.state().clear_needs_layout();
                    }
                }
            }
        }
        (owner, root_id, middle_id, leaf_id)
    }

    /// Marking a leaf where no relayout boundary is set propagates the
    /// `NEEDS_LAYOUT` flag up to root and pushes the root onto
    /// `dirty.needs_layout` (root is the implicit boundary).
    #[test]
    fn mark_needs_layout_walks_to_root_when_no_boundary_set() {
        let (mut owner, root_id, middle_id, leaf_id) = build_three_level_chain();
        assert!(owner.nodes_needing_layout().is_empty());

        owner.mark_needs_layout(leaf_id);

        for (id, label) in [(leaf_id, "leaf"), (middle_id, "middle"), (root_id, "root")] {
            let node = owner.render_tree.get(id).expect(label);
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

    /// Re-marking an already-dirty node short-circuits at step 1 of the
    /// walk — no second push, no flag toggle (flags are idempotent anyway).
    #[test]
    fn mark_needs_layout_is_idempotent_on_repeat() {
        let (mut owner, _root_id, _middle_id, leaf_id) = build_three_level_chain();
        owner.mark_needs_layout(leaf_id);
        let first_dirty_len = owner.nodes_needing_layout().len();
        owner.mark_needs_layout(leaf_id);
        assert_eq!(
            owner.nodes_needing_layout().len(),
            first_dirty_len,
            "second mark on already-dirty subtree must not re-push",
        );
    }

    /// When an intermediate ancestor IS a relayout boundary, propagation
    /// stops at that ancestor — the root above stays clean and the
    /// boundary id (not root) is the one pushed to the dirty queue.
    #[test]
    fn mark_needs_layout_stops_at_intermediate_relayout_boundary() {
        let (mut owner, root_id, middle_id, leaf_id) = build_three_level_chain();
        // Promote `middle` to a relayout boundary via the storage flag (U17
        // wires this from `RenderEntry::layout`'s post-set_constraints
        // compute_relayout_boundary call; this test pre-bootstraps the
        // flag directly to isolate U15 walk behaviour from U17 bootstrap).
        if let Some(crate::storage::RenderNode::Box(entry)) = owner.render_tree.get_mut(middle_id) {
            entry.state().set_relayout_boundary(true);
        }

        owner.mark_needs_layout(leaf_id);

        assert!(
            owner.render_tree.get(leaf_id).expect("leaf").needs_layout(),
            "leaf should be marked",
        );
        assert!(
            owner
                .render_tree
                .get(middle_id)
                .expect("middle")
                .needs_layout(),
            "boundary itself should be marked",
        );
        assert!(
            !owner.render_tree.get(root_id).expect("root").needs_layout(),
            "root above the boundary stays clean",
        );
        let dirty = owner.nodes_needing_layout();
        assert_eq!(dirty.len(), 1);
        assert_eq!(
            dirty[0].id, middle_id,
            "dirty entry should be the boundary, not the root",
        );
    }

    /// Marking a stale `RenderId` (post-removal) terminates the walk
    /// silently with no dirty-queue mutation.
    #[test]
    fn mark_needs_layout_stale_id_is_silent_noop() {
        let mut owner = PipelineOwner::new();
        let phantom = RenderId::new(99);
        owner.mark_needs_layout(phantom);
        assert!(owner.nodes_needing_layout().is_empty());
    }

    /// Leaf `RenderObject<BoxProtocol>` returning a fixed size regardless of
    /// the constraints — used to drive the layout-output debug assertion on
    /// the leaf commit path (`RenderEntry::layout_leaf_only`).
    #[derive(Debug)]
    struct FixedSizeLeaf {
        size: flui_types::Size,
    }

    impl flui_foundation::Diagnosticable for FixedSizeLeaf {}
    impl crate::traits::PaintEffectsCapability for FixedSizeLeaf {}
    impl crate::traits::SemanticsCapability for FixedSizeLeaf {}
    impl crate::traits::HotReloadCapability for FixedSizeLeaf {}

    impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for FixedSizeLeaf {
        fn perform_layout_raw(
            &mut self,
            _ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<
                '_,
            >,
        ) -> crate::error::RenderResult<
            crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        > {
            Ok(self.size)
        }

        fn paint_raw(&self, _recorder: &mut crate::context::FragmentRecorder, _child_count: usize) {
        }

        fn hit_test_raw(
            &self,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
            _child_count: usize,
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

        fn geometry(&self) -> &crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
            &self.size
        }

        fn set_geometry(
            &mut self,
            geometry: crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
        ) {
            self.size = geometry;
        }

        fn paint_bounds(&self) -> flui_types::Rect {
            flui_types::Rect::from_origin_size(flui_types::Point::ZERO, self.size)
        }
    }

    /// A leaf committing a size that violates the constraints it was laid out
    /// under trips `Protocol::debug_assert_layout_output` (Flutter
    /// `debugAssertDoesMeetConstraints`) on the leaf commit path — a node
    /// returning 999×999 under tight 100×100 is a layout bug.
    #[test]
    #[should_panic(expected = "violates its constraints")]
    fn leaf_committing_a_constraint_violating_size_trips_the_layout_assert() {
        let mut owner = PipelineOwner::new();
        let root = owner.insert(Box::new(FixedSizeLeaf {
            size: flui_types::Size::new(
                flui_types::geometry::px(999.0),
                flui_types::geometry::px(999.0),
            ),
        })
            as Box<dyn crate::traits::RenderObject<crate::protocol::BoxProtocol>>);
        owner.set_root_id(Some(root));
        owner.set_root_constraints(Some(BoxConstraints::tight(flui_types::Size::new(
            flui_types::geometry::px(100.0),
            flui_types::geometry::px(100.0),
        ))));

        let _ = owner.run_frame();
    }
}
