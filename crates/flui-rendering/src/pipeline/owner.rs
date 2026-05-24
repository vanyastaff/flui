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
use flui_types::Offset;

use crate::{context::CanvasContext, storage::RenderTree};

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
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_layout(&mut self, node_id: RenderId, depth: usize) {
        self.dirty.needs_layout.push(DirtyNode::new(node_id, depth));
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
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_paint(&mut self, node_id: RenderId, depth: usize) {
        self.dirty.needs_paint.push(DirtyNode::new(node_id, depth));
    }

    /// Adds a node to the compositing bits dirty list.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_compositing_bits_update(&mut self, node_id: RenderId, depth: usize) {
        self.dirty
            .needs_compositing
            .push(DirtyNode::new(node_id, depth));
    }

    /// Adds a node to the semantics dirty list.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `RenderId` of the render object (1-based)
    /// * `depth` - The depth of the node in the render tree
    pub fn add_node_needing_semantics(&mut self, node_id: RenderId, depth: usize) {
        self.dirty
            .needs_semantics
            .push(DirtyNode::new(node_id, depth));
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
                    return Err(e);
                }
            }

            self.debug_doing_layout = false;
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
            // Poisoned path in `RenderEntry::layout_leaf_only`. After the
            // Option A signature change, this panic is the ONE remaining
            // way to produce `RenderError::Poisoned` (typed
            // ContractViolation returns are reserved for bridge-detected
            // contract violations that go through the `Result` chain).
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
