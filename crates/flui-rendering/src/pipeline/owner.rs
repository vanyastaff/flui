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

use flui_foundation::RenderId;
use flui_layer::LayerTree;
use flui_types::{Offset, Size};
use parking_lot::Mutex;
use rustc_hash::FxHashSet;

use crate::{
    constraints::BoxConstraints,
    context::CanvasContext,
    parent_data::BoxParentData,
    protocol::{
        BoxLayoutCtx, BoxProtocol, ChildState, Protocol,
        box_protocol::{BoxLayoutCtxErased, LayoutChildCallback},
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

/// Maximum render-tree recursion depth during layout. Going deeper means
/// the parent-child layout has cycled or the tree is pathologically
/// deep. The pipeline aborts the offending subtree, surfaces
/// [`RenderError::LayoutDepthExceeded`](crate::RenderError::LayoutDepthExceeded) via tracing, and continues with
/// the next dirty root. Mythos Step 12 (2026-05-20).
pub const LAYOUT_DEPTH_LIMIT: usize = 1024;

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
    notifier: VisualUpdateNotifier,

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
        let (handle, dirty_rx) = PipelineOwnerHandle::new_pair(dirty_channel_capacity);
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier: VisualUpdateNotifier::new(),
            dirty: DirtySets::new(),
            mid_layout_marks: DirtySets::new(),
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
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
        let (handle, dirty_rx) = PipelineOwnerHandle::new_pair(DEFAULT_DIRTY_CHANNEL_CAPACITY);
        Self {
            id: PIPELINE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            render_tree: RenderTree::new(),
            root_id: None,
            notifier,
            dirty: DirtySets::new(),
            mid_layout_marks: DirtySets::new(),
            debug_doing_layout: false,
            debug_doing_paint: false,
            debug_doing_semantics: false,
            semantics_enabled: AtomicBool::new(false),
            last_layer_tree: None,
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
        self,
    ) -> (
        PipelineOwner<Idle>,
        crate::error::RenderResult<Option<LayerTree>>,
    ) {
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
        self.notifier.set_need_visual_update(callback);
    }

    /// Sets the callback for when semantics owner is created.
    pub fn set_on_semantics_owner_created<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.set_semantics_owner_created(callback);
    }

    /// Sets the callback for when semantics owner is disposed.
    pub fn set_on_semantics_owner_disposed<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.notifier.set_semantics_owner_disposed(callback);
    }

    /// Requests a visual update.
    ///
    /// Called by render objects when they need to be re-rendered.
    pub fn request_visual_update(&self) {
        self.notifier.fire_need_visual_update();
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
            match req.kind {
                DirtyKind::Layout => {
                    self.dirty
                        .needs_layout
                        .push(DirtyNode::new(req.id, req.depth));
                }
                DirtyKind::Compositing => {
                    self.dirty
                        .needs_compositing
                        .push(DirtyNode::new(req.id, req.depth));
                }
                DirtyKind::Paint => {
                    self.dirty
                        .needs_paint
                        .push(DirtyNode::new(req.id, req.depth));
                }
                DirtyKind::Semantics => {
                    self.dirty
                        .needs_semantics
                        .push(DirtyNode::new(req.id, req.depth));
                }
            }
            drained += 1;
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
                // short-circuit on "already marked"; the prior mark's
                // dirty-queue entry may have been drained by `run_layout`
                // without clearing the flag (the current run_layout still
                // contains the `layout_node_with_children` no-op walk that
                // doesn't invoke `RenderEntry::layout` — once PR-A1b lands
                // the walk rewrite, layout clears `NEEDS_LAYOUT` and the
                // walk could short-circuit, but the always-walk shape
                // remains correct).
                node.mark_layout_flag();
                let parent = node.links().parent();
                let boundary = node.is_relayout_boundary() || parent.is_none();
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
                }
                return;
            }
            // SAFETY: `parent.is_none()` is folded into `is_boundary` above,
            // so reaching this branch guarantees `Some(_)`.
            current = parent.unwrap();
        }
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
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_compositing_bits_update(&mut self, node_id: RenderId, depth: usize) {
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
            self.notifier.fire_semantics_owner_created();
        } else if !enabled && was_enabled {
            self.notifier.fire_semantics_owner_disposed();
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
        tracing::debug!("run_layout: {} nodes", self.dirty.needs_layout.len());

        // Process own dirty nodes if any
        // Flutter pattern: while loop to handle nodes added during layout
        while !self.dirty.needs_layout.is_empty() {
            self.debug_doing_layout = true;

            // Take the dirty nodes and replace with empty vec
            // This allows new nodes to be added during layout
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

            // Process each dirty node
            for dirty_node in dirty_nodes {
                // Layout this node with synchronous child layout support.
                // Depth starts at 0; recursion limit enforced inside.
                //
                // Mythos Step 12: layout_node_with_children returns
                // RenderResult<()>. On error we restore
                // debug_doing_layout and bail.
                if let Err(e) = self.layout_node_with_children(dirty_node.id, 0) {
                    self.debug_doing_layout = false;
                    // PR-A1 U22 P1 review fix (Codex 3294365736): drain
                    // mid-phase marks back into `dirty` even on error
                    // path so they survive across phase invocations.
                    // Without this, a panic / Err mid-iteration would
                    // strand mid-phase marks indefinitely.
                    self.drain_mid_layout_marks();
                    return Err(e);
                }
            }

            self.debug_doing_layout = false;

            // PR-A1 U22 P1 review fix (Codex 3294365736): drain
            // mid_layout_marks back into `dirty` so the outer while
            // condition `!self.dirty.needs_layout.is_empty()` picks up
            // marks that were routed to the side queue during this
            // iteration's `debug_doing_layout = true` window. Without
            // this drain, mid-phase marks accumulate in
            // `mid_layout_marks` and are never processed.
            self.drain_mid_layout_marks();
        }
        Ok(())
    }

    /// Lays out a single node with depth-first child layout.
    ///
    /// This method follows Flutter's layout model:
    /// 1. Propagate constraints to children
    /// 2. Layout children first (depth-first) so their sizes are available
    /// 3. Sync child sizes to parent's ChildState
    /// 4. Layout parent using child sizes via `layout_child()` calls
    ///
    /// This ensures that when parent's `perform_layout` calls `layout_child()`,
    /// the child's size is already cached and available.
    fn layout_node_with_children(
        &mut self,
        render_id: RenderId,
        depth: usize,
    ) -> crate::error::RenderResult<()> {
        // Mythos Step 12: bound recursion depth to detect infinite
        // parent-child cycles. Going past LAYOUT_DEPTH_LIMIT surfaces
        // RenderError::LayoutDepthExceeded; the caller (run_layout)
        // propagates the error.
        if depth > LAYOUT_DEPTH_LIMIT {
            tracing::error!(
                ?render_id,
                "layout_node_with_children: depth limit exceeded ({})",
                LAYOUT_DEPTH_LIMIT
            );
            return Err(crate::error::RenderError::layout_depth_exceeded(
                LAYOUT_DEPTH_LIMIT,
            ));
        }

        // Check if node exists and needs layout
        let needs_layout = {
            if let Some(render_node) = self.render_tree.get(render_id) {
                render_node.needs_layout()
            } else {
                return Ok(());
            }
        };

        if !needs_layout {
            return Ok(());
        }

        tracing::trace!(
            "layout_node_with_children: laying out node id={:?}",
            render_id
        );

        // STEP 1: Get children IDs and propagate constraints
        let children: Vec<RenderId> = {
            if let Some(render_node) = self.render_tree.get(render_id) {
                render_node.children().to_vec()
            } else {
                Vec::new()
            }
        };

        // STEP 2: Layout children FIRST (depth-first)
        // This ensures child sizes are available when parent's perform_layout runs.
        //
        // Cycle 4 R-13: the previous `propagate_constraints_to_child` and
        // `sync_child_size_to_parent` calls bracketed each recursive child
        // walk. Both were empty-body stubs (`fn ..(_) {}`) -- no constraints
        // propagated, no sizes synced. Workspace audit:
        //
        //   - `RenderEntry::layout(constraints)` is the canonical per-node
        //     layout entry point at `storage/entry.rs:252`. It accepts the
        //     constraints as a parameter and is the only path that runs
        //     `RenderObject::perform_layout_raw`.
        //   - `layout_node_with_children` itself never calls
        //     `entry.layout_leaf_only(...)` -- it only walks the tree
        //     marking `needs_layout` checks and recursing. The actual
        //     per-node layout happens nowhere in production today; the
        //     only `entry.layout_leaf_only()` callsite in the file is
        //     inside a `#[test]` block.
        //
        // So both stubs were dead code embedded in a larger half-implemented
        // walk. They were called every layout pass at zero cost (empty body)
        // but produced no behavior; deletion is strictly subtractive.
        //
        // The remaining `needs_layout()` walk preserves the depth-first
        // recursion shape so that when the per-node layout call lands, the
        // post-order traversal is already in place. The post-order ordering
        // is what Flutter's `performLayout` relies on so child sizes are
        // available when the parent's `performLayout` runs -- structurally
        // correct even though no node-level layout call exists yet.
        for child_id in &children {
            let child_needs_layout = {
                if let Some(child_node) = self.render_tree.get(*child_id) {
                    child_node.needs_layout()
                } else {
                    false
                }
            };

            if child_needs_layout {
                // Recursively layout the child (depth-first), incrementing
                // the recursion-depth counter so LAYOUT_DEPTH_LIMIT can
                // catch infinite cycles. Errors propagate up the
                // recursion via `?`.
                self.layout_node_with_children(*child_id, depth + 1)?;
            }
        }

        Ok(())
    }

    /// D-block PR-A1b3 U20 — production disjoint-borrow layout walk.
    ///
    /// Lays out the subtree rooted at `id` with the supplied
    /// `constraints`, running `RenderObject::perform_layout_raw` against a
    /// typed [`BoxLayoutCtx`] populated with the parent's direct children
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
    ///    via [`BoxLayoutCtx::with_layout_callback`] with a closure
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
    /// [`BoxParentData`], so widgets whose `T::ParentData` is the default
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
    let mut child_states: Vec<ChildState<BoxParentData>> =
        child_ids.iter().map(|&cid| ChildState::new(cid)).collect();

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

    // Construct pipeline-side Direct BoxLayoutCtx (Variable arity is
    // most permissive — Proxy bridge picks T::Arity from the user's
    // RenderBox impl regardless; BoxParentData per ParentData scope
    // limitation on `layout_dirty_root`).
    let mut ctx = BoxLayoutCtx::<flui_tree::Variable, BoxParentData>::with_layout_callback(
        constraints,
        &mut child_states,
        &child_ids,
        cb_ref,
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
    entry.state_mut().set_geometry(geometry);
    entry.state_mut().set_constraints(constraints);

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
    /// This is phase 2 of the rendering pipeline. During this phase:
    /// - Each object determines if it needs a compositing layer
    /// - This information is used during paint
    ///
    /// Nodes are sorted by depth (shallow first). This matches Flutter's
    /// `flushCompositingBits` behavior.
    pub fn run_compositing(&mut self) -> crate::error::RenderResult<()> {
        // PR #109 review feedback: pre-fix this path used
        // `std::mem::take(&mut self.dirty.needs_compositing)` to drain in
        // one step. Take leaves an empty `Vec::new()` (capacity 0) behind,
        // so every subsequent frame's first compositing push re-allocates.
        // The compositing dirty list churns per-frame in any animated
        // scene, so the realloc cost is hot-path. Switch to an in-place
        // sort + iterate + clear pattern that preserves the Vec's backing
        // capacity across frames (idiom: *Programming Rust* 2nd ed §11
        // "Owned vs Borrowed", retain the allocation by retaining
        // ownership).
        if self.dirty.needs_compositing.is_empty() {
            return Ok(());
        }
        tracing::debug!(
            "run_compositing: {} nodes",
            self.dirty.needs_compositing.len()
        );

        // Sort by depth (shallow first). Flutter:
        // `_nodesNeedingCompositingBitsUpdate.sort((a, b) => a.depth - b.depth)`.
        self.dirty
            .needs_compositing
            .sort_unstable_by_key(|node| node.depth);

        // Cycle 4 R-4: pre-cycle this path emitted a `tracing::trace!`
        // per dirty node and returned `Ok(())` without actually
        // updating any compositing bit — a SILENT half-impl flagged
        // as P0 "worse than R-1 because R-1 panics loudly; this just
        // returns success with no work done."
        //
        // The honest stub: keep the walk (so callers see the dirty
        // node ids in logs), but UPGRADE the per-node log to
        // `tracing::warn!` so the missing-impl is visible in any
        // production log scrape. The full Flutter parity
        // (`_updateSubtreeCompositingBits` recursion + repaint-
        // boundary check) is its own follow-up that needs the
        // `RenderObject::always_needs_compositing` + `is_repaint_boundary`
        // bool accessors plumbed through the dyn surface.
        //
        // Split-borrow: `self.dirty.needs_compositing` (immutable) and
        // `self.render_tree` (immutable) are disjoint fields under
        // Rust 2024's disjoint capture, so this loop compiles without
        // a temporary clone.
        for node in &self.dirty.needs_compositing {
            if self.render_tree.contains(node.id) {
                tracing::warn!(
                    id = ?node.id,
                    depth = node.depth,
                    "run_compositing: compositing-bits update is a no-op until \
                     `_updateSubtreeCompositingBits` recursion + repaint-boundary \
                     dispatch land; this node's compositing flags are unchanged"
                );
            }
        }
        // `clear()` retains the Vec's allocated capacity; next frame's
        // pushes amortise into the existing buffer.
        self.dirty.needs_compositing.clear();
        Ok(())
    }
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
    /// This is phase 3 of the rendering pipeline. During paint:
    /// - Render objects record paint commands
    /// - Compositing layers are built
    ///
    /// Nodes are sorted by depth (deep first) so children are painted before
    /// their parents. This matches Flutter's `flushPaint` behavior.
    pub fn run_paint(&mut self) -> crate::error::RenderResult<()> {
        tracing::debug!("run_paint: {} nodes", self.dirty.needs_paint.len());

        if self.dirty.needs_paint.is_empty() {
            return Ok(());
        }

        self.debug_doing_paint = true;

        // Cycle 4 R-15: pre-fix this method
        //   1. drained `dirty.needs_paint` via `std::mem::take` (capacity-
        //      dropping),
        //   2. did NOT sort the dirty list by depth (comment said
        //      "we don't need to sort since we paint from root"),
        //   3. cleared every dirty node's `needs_paint` flag in a
        //      separate loop BEFORE the paint walk,
        //   4. painted via root descent (`paint_node_recursive`),
        //   5. silently dropped any dirty node not reached by the
        //      descent (its flag was already cleared, its paint command
        //      never recorded).
        //
        // Audit R-15 flagged steps 2/3/5 as a half-impl: Flutter's
        // `flushPaint` sorts dirty deep-first AND paints each node
        // (paint clears the flag, no separate pass). Dropping paints
        // for unreached nodes is silent bug-bait for any future
        // multi-root or detached-subtree design.
        //
        // Post-fix:
        //   1. Sort dirty deep-first (Reverse depth) so repaint-
        //      boundary subtrees process before their ancestors when
        //      the per-node dirty-driven paint path lands.
        //   2. Walk via root descent (unchanged).
        //   3. `paint_node_recursive` clears `needs_paint` per node it
        //      visits (folded into the recursion).
        //   4. After the walk, scan the dirty list for nodes whose
        //      flag is still set -- those are the unreached cases.
        //   5. Emit `tracing::warn!` for each unreached dirty node,
        //      then clear (so the dirty list doesn't accumulate
        //      across frames).

        self.dirty
            .needs_paint
            .sort_unstable_by_key(|n| std::cmp::Reverse(n.depth));

        // Paint render tree recursively starting from root.
        // Each parent paints itself, then paints children with
        // accumulated offset.
        //
        // Mythos Step 12: paint_node_recursive returns RenderResult<()>;
        // a panicking render object surfaces as Err(Poisoned). We must
        // restore debug_doing_paint before `?`-propagating so the
        // owner's debug invariants stay consistent on the error path.
        if let Some(root_id) = self.root_id
            && let Some(root_node) = self.render_tree.get(root_id)
        {
            let paint_bounds = root_node.paint_bounds();
            tracing::debug!("run_paint: painting root with bounds {:?}", paint_bounds);

            // Create CanvasContext
            let mut context = CanvasContext::new(paint_bounds);

            // Paint recursively from root with offset accumulation.
            // `paint_node_recursive` clears `needs_paint` on every
            // node it visits (R-15 fold), so the dirty-list scan
            // below only fires for the unreached cases.
            let paint_result = self.paint_node_recursive(&mut context, root_id, Offset::ZERO);

            match paint_result {
                Ok(()) => {
                    // Store the resulting layer tree
                    self.last_layer_tree = Some(context.into_layer_tree());
                    tracing::debug!(
                        "run_paint: layer tree has {} layers",
                        self.last_layer_tree.as_ref().map(|t| t.len()).unwrap_or(0)
                    );
                }
                Err(e) => {
                    self.debug_doing_paint = false;
                    return Err(e);
                }
            }
        }

        // R-15: dirty-list residue scan. Any node still flagged
        // needs_paint AFTER the root descent is the unreached case
        // the pre-fix loop silently swallowed. Warn + clear so the
        // bug is visible AND the dirty list doesn't accumulate
        // across frames.
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
        // `clear()` retains capacity per cycle 4 R-1/R-4 PR #109
        // review feedback (preserve Vec backing across frames).
        self.dirty.needs_paint.clear();

        self.debug_doing_paint = false;

        // PR-A1 U22 P1 review fix (Codex 3294365736): drain
        // mid_layout_marks.needs_paint back into dirty so paint marks
        // made during this iteration's `debug_doing_paint = true`
        // window aren't stranded. The current run_paint is single-
        // pass (no outer while loop), so drained entries land on
        // dirty.needs_paint for the NEXT run_paint invocation rather
        // than this one — matches Flutter's flushPaint semantics
        // where mid-paint marks become next-frame work.
        self.drain_mid_layout_marks();

        Ok(())
    }

    /// Recursively paints a node and its children with accumulated offset.
    ///
    /// This follows Flutter's approach where each parent:
    /// 1. Paints itself at the given offset
    /// 2. For each child, adds child's offset and recursively paints
    ///
    /// # Repaint Boundaries
    ///
    /// When a child is a repaint boundary (`is_repaint_boundary() == true`),
    /// it creates its own `OffsetLayer` to isolate its painting. The offset
    /// is stored in the layer rather than accumulated, allowing the subtree
    /// to be cached and reused when only the offset changes.
    ///
    /// The tree structure (parent-child relationships) is stored in RenderTree,
    /// while child offsets are stored in each render object's internal state
    /// (set during layout via position_child).
    fn paint_node_recursive(
        &self,
        context: &mut CanvasContext,
        node_id: RenderId,
        offset: Offset,
    ) -> crate::error::RenderResult<()> {
        // Get the render node and collect info for painting
        let (is_repaint_boundary, children_with_offsets, paint_alpha, paint_transform): (
            bool,
            Vec<(RenderId, Offset)>,
            Option<u8>,
            Option<flui_types::Matrix4>,
        ) = {
            if let Some(render_node) = self.render_tree.get(node_id) {
                let render_object = render_node.box_render_object();

                // Get children from tree structure (RenderNode stores parent-child
                // relationships)
                let tree_children = render_node.children();

                let is_boundary = render_object.is_repaint_boundary();
                let alpha = render_object.paint_alpha();
                let transform = render_object.paint_transform();

                tracing::debug!(
                    "paint_node_recursive: node_id={:?}, offset=({}, {}), is_repaint_boundary={}, tree_children={}, ro_child_count={}, alpha={:?}",
                    node_id,
                    offset.dx,
                    offset.dy,
                    is_boundary,
                    tree_children.len(),
                    render_object.child_count(),
                    alpha
                );

                // Paint this node at the accumulated offset.
                //
                // Mythos Step 12: the paint call is third-party code. Wrap
                // in catch_unwind so a panicking render object surfaces as
                // RenderError::Poisoned rather than aborting the process.
                // AssertUnwindSafe is justified because (a) the canvas
                // context's drawing primitives are themselves panic-safe
                // (they record commands into Vec, no mid-state torn
                // invariants) and (b) the render object's internal state
                // is opaque to us -- on panic we treat the node as torn
                // and let the caller decide whether to drop it.
                let debug_name = render_object.debug_name();
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render_object.paint(context, offset);
                }))
                .map_err(|_| crate::error::RenderError::poisoned(debug_name, "paint"))?;

                // Cycle 4 R-15: clear the needs_paint flag now that
                // this node has been painted. Pre-fix the flag was
                // cleared in a separate up-front loop on `dirty.needs_paint`,
                // which silently dropped paints for nodes not reachable
                // from `root_id`. Folding the clear into the recursive
                // visit ensures the flag-clear and the paint walk stay
                // in lockstep -- Flutter's `flushPaint` model.
                render_node.clear_needs_paint();

                // For each child in the tree, get its offset from the render object
                // The render object stores offsets via position_child during layout
                let children: Vec<_> = tree_children
                    .iter()
                    .enumerate()
                    .map(|(i, &child_id)| {
                        // Get offset from render object (set during layout)
                        let child_offset = render_object.child_offset(i);
                        tracing::debug!(
                            "  child[{}]: id={:?}, offset=({}, {})",
                            i,
                            child_id,
                            child_offset.dx,
                            child_offset.dy
                        );
                        (child_id, child_offset)
                    })
                    .collect();

                (is_boundary, children, alpha, transform)
            } else {
                return Ok(());
            }
        };

        // Mythos Step 12: paint_children captures a mutable error slot.
        // The push_opacity / push_transform callbacks are FnOnce; if a
        // recursive paint surfaces a Poisoned error, we stash it and the
        // outer function returns it once the closure has unwound.
        let mut child_error: Option<crate::error::RenderError> = None;
        let mut paint_children = |ctx: &mut CanvasContext, base_offset: Offset| {
            for (child_id, child_offset) in &children_with_offsets {
                if child_error.is_some() {
                    return;
                }
                // Check if child is a repaint boundary
                let child_is_repaint_boundary = {
                    if let Some(child_node) = self.render_tree.get(*child_id) {
                        child_node.box_render_object().is_repaint_boundary()
                    } else {
                        false
                    }
                };

                let result = if child_is_repaint_boundary {
                    // For repaint boundaries, create a new OffsetLayer
                    let child_accumulated_offset = base_offset + *child_offset;
                    let mut inner_result: crate::error::RenderResult<()> = Ok(());
                    ctx.paint_child_with_offset(child_accumulated_offset, |child_ctx| {
                        inner_result =
                            self.paint_node_recursive(child_ctx, *child_id, Offset::ZERO);
                    });
                    inner_result
                } else {
                    // Normal case: accumulate offset and paint directly
                    let child_accumulated_offset = base_offset + *child_offset;
                    self.paint_node_recursive(ctx, *child_id, child_accumulated_offset)
                };

                if let Err(e) = result {
                    child_error = Some(e);
                    return;
                }
            }
        };

        // Apply effect layers (opacity, transform) around children
        if let Some(alpha) = paint_alpha {
            // Skip painting children entirely if fully transparent
            if alpha == 0 {
                // Don't paint children at all
            } else {
                // Wrap children in opacity layer
                // The offset is where this node is positioned. Children are painted
                // relative to this node, so we pass Offset::ZERO for children's base,
                // but the OpacityLayer itself is positioned at `offset`.
                context.push_opacity(offset, alpha, |opacity_ctx| {
                    if let Some(transform) = paint_transform {
                        // Apply transform layer inside opacity
                        opacity_ctx.push_transform(
                            true,
                            Offset::ZERO,
                            &transform,
                            |transform_ctx| {
                                paint_children(transform_ctx, Offset::ZERO);
                            },
                            None,
                        );
                    } else {
                        // Children paint relative to the opacity layer's origin
                        paint_children(opacity_ctx, Offset::ZERO);
                    }
                });
            }
        } else if let Some(transform) = paint_transform {
            // Apply transform layer
            context.push_transform(
                true,
                offset,
                &transform,
                |transform_ctx| {
                    paint_children(transform_ctx, Offset::ZERO);
                },
                None,
            );
        } else {
            // No effect layers - paint children directly
            paint_children(context, offset);
        }

        // Propagate any child error captured during recursive paint.
        if let Some(err) = child_error {
            return Err(err);
        }

        // Track that this was a repaint boundary for future reference.
        //
        // U2 exemplar refactor: the previous shape took a write lock on the
        // trait object (`render_node.box_render_object_mut()`) to flip a single
        // bool via `set_was_repaint_boundary`. The bit lives on `RenderState`
        // as `WAS_REPAINT_BOUNDARY` (see `storage/flags.rs`); the paint phase
        // now flips an atomic without touching the trait object. The trait
        // method has been removed. See `docs/PORT.md` Refusal trigger 1 and
        // `crates/flui-rendering/ARCHITECTURE.md`.
        if is_repaint_boundary
            && let Some(render_node) = self.render_tree.get(node_id)
            && let Some(entry) = render_node.as_box()
        {
            entry.state().set_was_repaint_boundary(true);
        }

        Ok(())
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
        debug_doing_layout: from.debug_doing_layout,
        debug_doing_paint: from.debug_doing_paint,
        debug_doing_semantics: from.debug_doing_semantics,
        semantics_enabled: from.semantics_enabled,
        last_layer_tree: from.last_layer_tree,
        handle: from.handle,
        dirty_rx: from.dirty_rx,
        _phase: PhantomData,
    }
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

        fn paint(&self, _context: &mut crate::context::CanvasContext, _offset: flui_types::Offset) {
            panic!("PanickingPaintBox::paint -- intentional test panic");
        }

        fn hit_test_raw(
            &self,
            _result: &mut crate::protocol::ProtocolHitResult<crate::protocol::BoxProtocol>,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
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

        fn paint(&self, _context: &mut crate::context::CanvasContext, _offset: flui_types::Offset) {
        }

        fn hit_test_raw(
            &self,
            _result: &mut crate::protocol::ProtocolHitResult<crate::protocol::BoxProtocol>,
            _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
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
        assert!(!owner.has_mid_layout_marks(), "mid queue must be empty post-drain");
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
}
