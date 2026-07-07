//! RenderNode - Type-erased render node for heterogeneous tree storage.
//!
//! This module provides `RenderNode`, an enum that wraps protocol-specific
//! `RenderEntry<P>` variants for storage in `RenderTree`.

use flui_foundation::RenderId;

use super::{entry::RenderEntry, links::NodeLinks};
use crate::protocol::{BoxProtocol, RenderObject, SliverProtocol};

/// Render node enum for heterogeneous tree storage.
///
/// This enum wraps protocol-specific `RenderEntry<P>` variants, allowing
/// a single `RenderTree` to store both Box and Sliver nodes.
///
/// # Protocols
///
/// - `Box`: 2D cartesian layout (most widgets)
/// - `Sliver`: Scrollable content layout (lists, grids)
///
/// # Usage
///
/// Most operations work through enum matching or convenience methods:
///
/// ```rust,ignore
/// let node: &RenderNode = tree.get(id)?;
///
/// // Common operations work on any variant
/// let parent = node.parent();
/// let needs_layout = node.needs_layout();
///
/// // Protocol-specific access
/// if let Some(box_entry) = node.as_box() {
///     let size = box_entry.state().geometry();
/// }
/// ```
#[derive(Debug)]
pub enum RenderNode {
    /// Box protocol node (2D cartesian layout).
    Box(RenderEntry<BoxProtocol>),

    /// Sliver protocol node (scrollable layout).
    Sliver(RenderEntry<SliverProtocol>),
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl RenderNode {
    /// Creates a new Box protocol node.
    pub fn new_box(render_object: Box<dyn RenderObject<BoxProtocol>>) -> Self {
        Self::Box(RenderEntry::new(render_object))
    }

    /// Creates a new Box protocol node with a parent.
    pub fn new_box_with_parent(
        render_object: Box<dyn RenderObject<BoxProtocol>>,
        parent: RenderId,
        depth: u16,
    ) -> Self {
        Self::Box(RenderEntry::with_parent(render_object, parent, depth))
    }

    /// Creates a new Sliver protocol node.
    pub fn new_sliver(render_object: Box<dyn RenderObject<SliverProtocol>>) -> Self {
        Self::Sliver(RenderEntry::new(render_object))
    }

    /// Creates a new Sliver protocol node with a parent.
    pub fn new_sliver_with_parent(
        render_object: Box<dyn RenderObject<SliverProtocol>>,
        parent: RenderId,
        depth: u16,
    ) -> Self {
        Self::Sliver(RenderEntry::with_parent(render_object, parent, depth))
    }
}

// ============================================================================
// FROM CONVERSIONS
// ============================================================================

impl From<Box<dyn RenderObject<BoxProtocol>>> for RenderNode {
    fn from(render_object: Box<dyn RenderObject<BoxProtocol>>) -> Self {
        Self::new_box(render_object)
    }
}

impl From<Box<dyn RenderObject<SliverProtocol>>> for RenderNode {
    fn from(render_object: Box<dyn RenderObject<SliverProtocol>>) -> Self {
        Self::new_sliver(render_object)
    }
}

// ============================================================================
// PROTOCOL CHECK
// ============================================================================

impl RenderNode {
    /// Returns true if this is a Box protocol node.
    #[inline]
    pub fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Returns true if this is a Sliver protocol node.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }

    /// Returns the protocol name.
    pub fn protocol_name(&self) -> &'static str {
        match self {
            Self::Box(_) => "Box",
            Self::Sliver(_) => "Sliver",
        }
    }

    /// Hot-reload hook: mark this render object for reprocessing.
    ///
    /// Dispatches to [`RenderObject::reassemble`] on the underlying object.
    pub fn reassemble(&mut self) {
        match self {
            Self::Box(entry) => entry.render_object_mut().reassemble(),
            Self::Sliver(entry) => entry.render_object_mut().reassemble(),
        }
    }

    /// Tree-lifecycle hook (ADR-0013): hands the freshly-inserted render
    /// object a self-dirty handle bound to its own node.
    ///
    /// Dispatches to [`RenderObject::attach`] on the underlying object.
    pub fn attach(&mut self, handle: crate::pipeline::RepaintHandle) {
        match self {
            Self::Box(entry) => entry.render_object_mut().attach(handle),
            Self::Sliver(entry) => entry.render_object_mut().attach(handle),
        }
    }

    /// Tree-lifecycle hook (ADR-0013): tears down whatever `attach`
    /// subscribed to, called before this node is removed from the tree.
    ///
    /// Dispatches to [`RenderObject::detach`] on the underlying object.
    pub fn detach(&mut self) {
        match self {
            Self::Box(entry) => entry.render_object_mut().detach(),
            Self::Sliver(entry) => entry.render_object_mut().detach(),
        }
    }
}

// ============================================================================
// TYPED ACCESS
// ============================================================================

impl RenderNode {
    /// Returns a reference to the Box entry, if this is a Box node.
    #[inline]
    pub fn as_box(&self) -> Option<&RenderEntry<BoxProtocol>> {
        match self {
            Self::Box(entry) => Some(entry),
            Self::Sliver(_) => None,
        }
    }

    /// Returns a mutable reference to the Box entry, if this is a Box node.
    #[inline]
    pub fn as_box_mut(&mut self) -> Option<&mut RenderEntry<BoxProtocol>> {
        match self {
            Self::Box(entry) => Some(entry),
            Self::Sliver(_) => None,
        }
    }

    /// Returns a reference to the Box entry, panics if this is not a Box node.
    ///
    /// Use this when you know the node is Box protocol (e.g., in PipelineOwner
    /// which only works with Box nodes currently).
    #[inline]
    pub fn as_box_unchecked(&self) -> &RenderEntry<BoxProtocol> {
        self.as_box().expect("Expected Box protocol node")
    }

    /// Returns a mutable reference to the Box entry, panics if this is not a
    /// Box node.
    #[inline]
    pub fn as_box_unchecked_mut(&mut self) -> &mut RenderEntry<BoxProtocol> {
        self.as_box_mut().expect("Expected Box protocol node")
    }

    /// Returns a reference to the Sliver entry, if this is a Sliver node.
    #[inline]
    pub fn as_sliver(&self) -> Option<&RenderEntry<SliverProtocol>> {
        match self {
            Self::Sliver(entry) => Some(entry),
            Self::Box(_) => None,
        }
    }

    /// Returns a mutable reference to the Sliver entry, if this is a Sliver
    /// node.
    #[inline]
    pub fn as_sliver_mut(&mut self) -> Option<&mut RenderEntry<SliverProtocol>> {
        match self {
            Self::Sliver(entry) => Some(entry),
            Self::Box(_) => None,
        }
    }
}

// ============================================================================
// LINKS ACCESS (Common across all protocols)
// ============================================================================

impl RenderNode {
    /// Returns a reference to the tree links.
    #[inline]
    pub fn links(&self) -> &NodeLinks {
        match self {
            Self::Box(entry) => entry.links(),
            Self::Sliver(entry) => entry.links(),
        }
    }

    /// Returns a mutable reference to the tree links.
    #[inline]
    pub fn links_mut(&mut self) -> &mut NodeLinks {
        match self {
            Self::Box(entry) => entry.links_mut(),
            Self::Sliver(entry) => entry.links_mut(),
        }
    }

    // Convenience methods

    /// Returns the parent ID.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.links().parent()
    }

    /// Sets the parent ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<RenderId>) {
        self.links_mut().set_parent(parent);
    }

    /// Returns the children IDs.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        self.links().children()
    }

    /// Returns the depth in the tree.
    #[inline]
    pub fn depth(&self) -> u16 {
        self.links().depth()
    }

    /// Sets the depth.
    #[inline]
    pub fn set_depth(&mut self, depth: u16) {
        self.links_mut().set_depth(depth);
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.links().child_count()
    }

    /// Adds a child.
    #[inline]
    pub fn add_child(&mut self, child: RenderId) {
        self.links_mut().add_child(child);
    }

    /// Inserts a child at `index`, shifting later siblings right.
    ///
    /// # Panics
    ///
    /// Panics if `index > self.child_count()`.
    #[inline]
    pub fn insert_child(&mut self, index: usize, child: RenderId) {
        self.links_mut().insert_child(index, child);
    }

    /// Removes a child.
    #[inline]
    pub fn remove_child(&mut self, child: RenderId) -> bool {
        self.links_mut().remove_child(child)
    }

    /// Returns true if this is a root node.
    #[inline]
    pub fn is_root(&self) -> bool {
        self.links().is_root()
    }

    /// Returns true if this is a leaf node.
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.links().is_leaf()
    }
}

// ============================================================================
// STATE ACCESS (Common across all protocols)
// ============================================================================

impl RenderNode {
    /// Returns true if layout is needed.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        match self {
            Self::Box(entry) => entry.needs_layout(),
            Self::Sliver(entry) => entry.needs_layout(),
        }
    }

    /// Returns true if paint is needed.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        match self {
            Self::Box(entry) => entry.needs_paint(),
            Self::Sliver(entry) => entry.needs_paint(),
        }
    }

    /// Sets the `NEEDS_LAYOUT` flag on this node's state — **flag-only**, no
    /// propagation. Added in D-block PR-A1 U15 to support the
    /// [`PipelineOwner::mark_needs_layout`](crate::pipeline::PipelineOwner::mark_needs_layout)
    /// ancestor-walk: each step of the walk flips one node's flag, and the
    /// owner is responsible for the ancestor traversal and dirty-queue push
    /// at the boundary.
    ///
    /// The previously-removed `RenderNode::mark_needs_layout()` did
    /// propagation; the new owner-side walk supersedes it. Direct callers
    /// should still use `PipelineOwner::mark_needs_layout` for correct
    /// Flutter-parity boundary semantics.
    #[inline]
    pub fn mark_layout_flag(&self) {
        match self {
            Self::Box(entry) => entry.state().mark_needs_layout(),
            Self::Sliver(entry) => entry.state().mark_needs_layout(),
        }
    }

    /// Clears this node's layout calculation cache (memoized intrinsics /
    /// dry layout / dry baselines), returning whether anything WAS cached.
    ///
    /// `true` means an ancestor's layout consumed this node's intrinsic
    /// queries: the invalidation walk must escalate to the parent even
    /// across a relayout boundary (Flutter `RenderBox.markNeedsLayout`,
    /// box.dart:2840). Sliver nodes carry no cache yet and always return
    /// `false`.
    #[inline]
    pub fn clear_layout_cache(&mut self) -> bool {
        match self {
            Self::Box(entry) => entry.state_mut().clear_layout_cache(),
            Self::Sliver(entry) => entry.state_mut().clear_layout_cache(),
        }
    }

    /// Sets the `NEEDS_PAINT` flag on this node's state — flag-only,
    /// no propagation.
    ///
    /// **D-block PR-A1 U22 (memo D7):** additive helper mirroring
    /// [`Self::mark_layout_flag`]. NOT used by U22's dedup path —
    /// `PipelineOwner::add_node_needing_paint` uses queue-membership
    /// scanning instead of flag-based dedup (flag-based dedup is
    /// unsuitable because `RenderState::new()` defaults
    /// `NEEDS_PAINT = true` and would silently no-op on first add
    /// for fresh nodes). Kept available for future callers that need
    /// direct flag manipulation outside the dirty-queue scheduling
    /// path.
    #[inline]
    pub fn mark_paint_flag(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().mark_needs_paint(),
            Self::Sliver(entry) => entry.state().flags().mark_needs_paint(),
        }
    }

    /// Sets the `NEEDS_COMPOSITING` flag on this node's state —
    /// flag-only, no propagation. **D-block PR-A1 U22:** additive
    /// helper; not used by the queue-scan dedup path (see
    /// [`Self::mark_paint_flag`] doc for rationale).
    #[inline]
    pub fn mark_compositing_flag(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().mark_needs_compositing(),
            Self::Sliver(entry) => entry.state().flags().mark_needs_compositing(),
        }
    }

    /// Sets the `NEEDS_SEMANTICS` flag on this node's state —
    /// flag-only, no propagation. **D-block PR-A1 U22:** additive
    /// helper; not used by the queue-scan dedup path (see
    /// [`Self::mark_paint_flag`] doc for rationale).
    #[inline]
    pub fn mark_semantics_flag(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().mark_needs_semantics(),
            Self::Sliver(entry) => entry.state().flags().mark_needs_semantics(),
        }
    }

    /// Returns true if `NEEDS_SEMANTICS` is set on this node's state.
    /// **D-block PR-A1 U22:** additive accessor for future flag-based
    /// callers (current `add_node_needing_semantics` uses queue-scan
    /// dedup, not this flag).
    #[inline]
    pub fn needs_semantics(&self) -> bool {
        match self {
            Self::Box(entry) => entry
                .state()
                .flags()
                .contains(crate::storage::flags::RenderFlags::NEEDS_SEMANTICS),
            Self::Sliver(entry) => entry
                .state()
                .flags()
                .contains(crate::storage::flags::RenderFlags::NEEDS_SEMANTICS),
        }
    }

    /// Returns true if `NEEDS_COMPOSITING` is set on this node's
    /// state. **D-block PR-A1 U22:** additive accessor (see
    /// [`Self::needs_semantics`] doc for usage notes).
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        match self {
            Self::Box(entry) => entry.state().needs_compositing(),
            Self::Sliver(entry) => entry.state().needs_compositing(),
        }
    }

    /// Protocol-erased **leaf-mode** layout dispatch.
    ///
    /// Matches the inner `RenderEntry<P>` against the supplied
    /// [`ErasedConstraints`](crate::storage::ErasedConstraints) variant
    /// and forwards to
    /// [`RenderEntry::<P>::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only),
    /// returning the result as
    /// [`ErasedGeometry`](crate::storage::ErasedGeometry).
    ///
    /// # ⚠ LEAF-ONLY — DO NOT CALL FOR NON-LEAF RENDER OBJECTS
    ///
    /// This method delegates to `RenderEntry::layout_leaf_only`, which
    /// builds a `BoxLayoutCtx::<Leaf, BoxParentData>` with **no
    /// children**. Non-leaf render objects routed through this method
    /// observe `ctx.child_count() == 0` and silently produce wrong
    /// geometry. The name `layout_leaf_erased` (PR #141 Codex review
    /// comment 3293746309 P1) makes the constraint compile-time
    /// obvious at every callsite.
    ///
    /// # Background
    ///
    /// **D-block PR-A1b U18 (companion memo D4):** the pipeline operates on
    /// protocol-erased `RenderNode`s, but
    /// `RenderEntry::layout_leaf_only` is generic over `P: Protocol`.
    /// This method bridges the seam — variant mismatch (e.g., `Box`
    /// constraints handed to a `Sliver` entry) returns
    /// [`RenderError::ProtocolMismatch`](crate::error::RenderError::ProtocolMismatch).
    ///
    /// Pipeline-side callers lift the protocol-typed root constraints
    /// to `ErasedConstraints` via the `From<BoxConstraints>` /
    /// `From<SliverConstraints>` impls before invoking. Per-protocol-typed
    /// callers (the `RenderBox` bridge and `RenderSliver` bridge in
    /// U19) downcast the returned geometry via
    /// `TryFrom<ErasedGeometry>`.
    ///
    /// U20's `PipelineOwner::layout_dirty_root` does NOT route through
    /// this method — it builds typed `BoxLayoutCtx` with children via
    /// disjoint borrows and calls `render_object.perform_layout_raw`
    /// directly against an erased view, bypassing the leaf-mode
    /// constraint entirely.
    pub fn layout_leaf_erased(
        &mut self,
        constraints: crate::storage::ErasedConstraints,
    ) -> crate::error::RenderResult<crate::storage::ErasedGeometry> {
        use crate::storage::ErasedConstraints;
        match (self, constraints) {
            (Self::Box(entry), ErasedConstraints::Box(c)) => {
                entry.layout_leaf_only(c).map(Into::into)
            }
            (Self::Sliver(entry), ErasedConstraints::Sliver(c)) => {
                entry.layout_leaf_only(c).map(Into::into)
            }
            (Self::Box(_), ErasedConstraints::Sliver(_)) => {
                Err(crate::error::RenderError::ProtocolMismatch {
                    node_protocol: "Box",
                    constraints_protocol: "Sliver",
                })
            }
            (Self::Sliver(_), ErasedConstraints::Box(_)) => {
                Err(crate::error::RenderError::ProtocolMismatch {
                    node_protocol: "Sliver",
                    constraints_protocol: "Box",
                })
            }
        }
    }

    /// Returns true if this is a repaint boundary.
    ///
    /// Reads the static `RenderObject::is_repaint_boundary()` trait answer.
    /// This is the per-instance, type-level value (constant for a given
    /// render object). For the runtime per-state flag value (as bootstrapped
    /// by `PipelineOwner::bootstrap_repaint_boundary_flag` on insert), use
    /// [`Self::is_repaint_boundary_flag`].
    pub fn is_repaint_boundary(&self) -> bool {
        match self {
            Self::Box(entry) => entry.render_object().is_repaint_boundary(),
            Self::Sliver(entry) => entry.render_object().is_repaint_boundary(),
        }
    }

    /// Reads the `IS_REPAINT_BOUNDARY` storage flag (D-block PR-A2 U33).
    ///
    /// The flag is bootstrapped on insert from the trait answer via
    /// `PipelineOwner::bootstrap_repaint_boundary_flag`. The compositing-bits
    /// walk consults the flag (not the trait answer directly) so that future
    /// dynamic repaint-boundary scenarios — and the
    /// `WAS_REPAINT_BOUNDARY` paint-phase write-back — share a single
    /// source of truth.
    #[inline]
    pub fn is_repaint_boundary_flag(&self) -> bool {
        match self {
            Self::Box(entry) => entry.state().flags().is_repaint_boundary(),
            Self::Sliver(entry) => entry.state().flags().is_repaint_boundary(),
        }
    }

    /// Sets the `IS_REPAINT_BOUNDARY` storage flag (D-block PR-A2 U33).
    ///
    /// Called by `PipelineOwner::bootstrap_repaint_boundary_flag` at insert
    /// time after reading the trait answer; not called from layout/paint
    /// hot paths (the flag is configuration, not dirty state).
    #[inline]
    pub fn set_repaint_boundary_flag(&self, is_boundary: bool) {
        match self {
            Self::Box(entry) => entry.state().flags().set_repaint_boundary(is_boundary),
            Self::Sliver(entry) => entry.state().flags().set_repaint_boundary(is_boundary),
        }
    }

    /// Returns true if this node is a relayout boundary.
    ///
    /// **D-block PR-A1 U15**: reads the per-instance `IS_RELAYOUT_BOUNDARY`
    /// storage flag (set by [`RenderState::compute_relayout_boundary`] during
    /// layout per Flutter `!parentUsesSize || sizedByParent || constraints.isTight() || !hasParent`).
    /// Prior behaviour returned the hardcoded `RenderObject::is_relayout_boundary()`
    /// trait answer; that value was never consulted in production (zero
    /// callers via grep) and reflected the type-level default rather than
    /// runtime layout context.
    ///
    /// The trait-level static answer is still available via
    /// `entry.render_object().is_relayout_boundary()` for the rare callers
    /// that genuinely want the type-default.
    ///
    /// [`RenderState::compute_relayout_boundary`]: crate::storage::RenderState::compute_relayout_boundary
    #[inline]
    pub fn is_relayout_boundary(&self) -> bool {
        match self {
            Self::Box(entry) => entry.state().is_relayout_boundary(),
            Self::Sliver(entry) => entry.state().is_relayout_boundary(),
        }
    }

    /// Returns the persistent parent data for this node, if set.
    ///
    /// Parent data is stored on the child's `RenderState` and persists
    /// across frames. Returns `None` if no parent data has been set yet.
    #[inline]
    pub fn parent_data(&self) -> Option<&dyn crate::parent_data::ParentData> {
        match self {
            Self::Box(entry) => entry.state().parent_data(),
            Self::Sliver(entry) => entry.state().parent_data(),
        }
    }

    /// Mutable access to the persistent parent data for this node.
    ///
    /// Returns `None` if no parent data has been set yet.
    #[inline]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn crate::parent_data::ParentData> {
        match self {
            Self::Box(entry) => entry.state_mut().parent_data_mut(),
            Self::Sliver(entry) => entry.state_mut().parent_data_mut(),
        }
    }

    /// Installs `data` as this node's parent data, replacing any previously
    /// stored value.
    ///
    /// Used by the deferred-insert path to seed a freshly-inserted node with
    /// the parent-data box that the build backend pre-built (e.g.
    /// `SliverMultiBoxAdaptorParentData { index: logical_index }`).  Prefer
    /// [`Self::parent_data_mut`] when the node already has parent-data and
    /// only a field needs updating.
    #[inline]
    pub fn set_parent_data(&mut self, data: Box<dyn crate::parent_data::ParentData>) {
        match self {
            Self::Box(entry) => entry.state_mut().set_parent_data(data),
            Self::Sliver(entry) => entry.state_mut().set_parent_data(data),
        }
    }

    // 2B field dedup: `RenderNode::paint_bounds` was deleted. It had zero
    // call sites workspace-wide (a dead producer — the paint pipeline does
    // no bounds-based culling yet) and, once geometry moved to
    // `RenderState`, derived an *untransformed* rect that silently dropped
    // `RenderTransform`'s corner-mapped bounds. A future culling consumer
    // must reintroduce it transform-aware (apply `paint_transform()` to the
    // committed `RenderState` geometry), not resurrect a half-correct
    // producer. Root paint bounds for the engine live on
    // `RenderView::physical_paint_bounds`.

    /// Stable debug name for the stored render object.
    #[inline]
    pub fn debug_name(&self) -> &'static str {
        match self {
            Self::Box(entry) => entry.render_object().debug_name(),
            Self::Sliver(entry) => entry.render_object().debug_name(),
        }
    }

    /// Optional paint opacity effect for this render object.
    #[inline]
    pub fn paint_alpha(&self) -> Option<u8> {
        match self {
            Self::Box(entry) => entry.render_object().paint_alpha(),
            Self::Sliver(entry) => entry.render_object().paint_alpha(),
        }
    }

    /// Whether this node's render object requests that child paint be skipped.
    ///
    /// Returns `true` when the node is fully transparent (e.g. `RenderOpacity`
    /// at alpha=0 without the `always_needs_compositing` flag). The pipeline
    /// owner calls this before recording child fragments to avoid invisible GPU
    /// draws.
    #[inline]
    pub fn skip_paint(&self) -> bool {
        match self {
            Self::Box(entry) => entry.render_object().skip_paint(),
            Self::Sliver(entry) => entry.render_object().skip_paint(),
        }
    }

    /// Optional blend mode for the opacity layer wrapping children.
    ///
    /// Returns the blend mode set by `paint_layer_blend`, or `None` when the
    /// object uses the default `SrcOver` compositing (most objects).
    #[inline]
    pub fn paint_layer_blend(&self) -> Option<flui_types::painting::BlendMode> {
        match self {
            Self::Box(entry) => entry.render_object().paint_layer_blend(),
            Self::Sliver(entry) => entry.render_object().paint_layer_blend(),
        }
    }

    /// Optional paint transform effect for this render object.
    ///
    /// The laid-out size is resolved from
    /// [`RenderState`](crate::storage::RenderState) (geometry's sole
    /// owner) and threaded in so an alignment-relative transform reads it
    /// instead of caching its own size (channel for the `&self`
    /// `paint_transform` hook). Box → committed `Size`; sliver → absolute
    /// paint size.
    #[inline]
    pub fn paint_transform(&self) -> Option<flui_types::Matrix4> {
        match self {
            Self::Box(entry) => {
                let size = entry.state().geometry().unwrap_or(flui_types::Size::ZERO);
                entry.render_object().paint_transform(size)
            }
            Self::Sliver(entry) => {
                let size = entry.state().absolute_paint_size();
                entry.render_object().paint_transform(size)
            }
        }
    }

    /// Records this node's paint fragment through the protocol blanket.
    ///
    /// The node's laid-out paint size is resolved from
    /// [`RenderState`](crate::storage::RenderState) (geometry's sole
    /// owner) and threaded into `paint_raw` so the render object reads
    /// `ctx.size()` instead of caching its own `size` field (2B field
    /// dedup). Box → committed `Size`; sliver → absolute paint size.
    #[inline]
    pub fn paint_raw(&self, recorder: &mut crate::context::FragmentRecorder, child_count: usize) {
        match self {
            Self::Box(entry) => {
                let size = entry.state().geometry().unwrap_or(flui_types::Size::ZERO);
                entry.render_object().paint_raw(recorder, child_count, size);
            }
            Self::Sliver(entry) => {
                let size = entry.state().absolute_paint_size();
                entry.render_object().paint_raw(recorder, child_count, size);
            }
        }
    }

    /// Returns this node's parent-relative offset.
    #[inline]
    pub fn offset(&self) -> flui_types::Offset {
        match self {
            Self::Box(entry) => entry.state().offset(),
            Self::Sliver(entry) => entry.state().offset(),
        }
    }

    /// Sets this node's parent-relative offset.
    #[inline]
    pub fn set_offset(&self, offset: flui_types::Offset) {
        match self {
            Self::Box(entry) => entry.state().set_offset(offset),
            Self::Sliver(entry) => entry.state().set_offset(offset),
        }
    }

    /// Returns the size for Box protocol nodes (None for Sliver nodes).
    pub fn size(&self) -> Option<flui_types::Size> {
        match self {
            Self::Box(entry) => entry.state().geometry(),
            Self::Sliver(_) => None,
        }
    }

    /// Returns the geometry for this node (Size for Box, SliverGeometry for
    /// Sliver).
    pub fn geometry_box(&self) -> Option<flui_types::Size> {
        self.as_box().and_then(|entry| entry.state().geometry())
    }

    /// Returns the sliver geometry for Sliver protocol nodes (None for Box
    /// nodes).
    pub fn geometry_sliver(&self) -> Option<crate::constraints::SliverGeometry> {
        self.as_sliver().and_then(|entry| entry.state().geometry())
    }

    /// Returns an immutable reference to the Box render object.
    ///
    /// Panics if this is not a Box node.
    pub fn box_render_object(&self) -> &dyn RenderObject<BoxProtocol> {
        self.as_box_unchecked().render_object()
    }

    /// Returns a mutable reference to the Box render object.
    ///
    /// Panics if this is not a Box node. Requires `&mut self`; pipeline
    /// phases obtain this through `&mut RenderTree`. See the U2 exemplar
    /// refactor docstring on `RenderEntry`.
    pub fn box_render_object_mut(&mut self) -> &mut dyn RenderObject<BoxProtocol> {
        self.as_box_unchecked_mut().render_object_mut()
    }

    /// Downcasts this node's render object to a concrete type `T`, regardless
    /// of protocol (Box or Sliver). Returns `None` if the stored object is not
    /// a `T`.
    ///
    /// This is the View layer's hook for `RenderObjectElement`'s update path:
    /// when a `RenderObjectWidget` updates, the framework downcasts the live
    /// render object to the widget's concrete `RenderObject` type and calls
    /// `RenderView::update_render_object` to apply the new configuration in
    /// place (Flutter's `Widget.updateRenderObject`).
    pub fn downcast_render_object_mut<T: std::any::Any>(&mut self) -> Option<&mut T> {
        match self {
            Self::Box(entry) => entry.render_object_mut().as_any_mut().downcast_mut::<T>(),
            Self::Sliver(entry) => entry.render_object_mut().as_any_mut().downcast_mut::<T>(),
        }
    }

    /// Clears the needs_paint flag.
    #[inline]
    pub fn clear_needs_paint(&self) {
        match self {
            Self::Box(entry) => entry.clear_needs_paint(),
            Self::Sliver(entry) => entry.clear_needs_paint(),
        }
    }

    /// Clears the needs_layout flag.
    #[inline]
    pub fn clear_needs_layout(&self) {
        match self {
            Self::Box(entry) => entry.clear_needs_layout(),
            Self::Sliver(entry) => entry.clear_needs_layout(),
        }
    }

    /// Reads the `RenderObject::always_needs_compositing()` static trait
    /// answer (D-block PR-A2 U34 / memo R26b).
    ///
    /// Consulted by the compositing-bits walk to force `NEEDS_COMPOSITING`
    /// regardless of subtree state — used by render objects that apply
    /// per-frame compositor effects (e.g., shader masks, backdrop filters).
    #[inline]
    pub fn always_needs_compositing(&self) -> bool {
        match self {
            Self::Box(entry) => entry.render_object().always_needs_compositing(),
            Self::Sliver(entry) => entry.render_object().always_needs_compositing(),
        }
    }

    /// Reads the `WAS_REPAINT_BOUNDARY` storage flag (D-block PR-A2 U34).
    ///
    /// Set by the paint phase after a node was painted as a repaint
    /// boundary. The compositing-bits walk consults this to detect the
    /// "lost-boundary-status" transition (`!is_repaint_boundary &&
    /// was_repaint_boundary`) per Flutter `_updateCompositingBits`
    /// (object.dart:3246-3251).
    #[inline]
    pub fn was_repaint_boundary(&self) -> bool {
        match self {
            Self::Box(entry) => entry.state().flags().was_repaint_boundary(),
            Self::Sliver(entry) => entry.state().flags().was_repaint_boundary(),
        }
    }

    /// Writes the `WAS_REPAINT_BOUNDARY` storage flag (D-block PR-A2 U35).
    ///
    /// Called by the paint phase after a node is painted so subsequent
    /// compositing-bits walks can detect boundary-status transitions.
    #[inline]
    pub fn set_was_repaint_boundary(&self, was_boundary: bool) {
        match self {
            Self::Box(entry) => entry.state().flags().set_was_repaint_boundary(was_boundary),
            Self::Sliver(entry) => entry.state().flags().set_was_repaint_boundary(was_boundary),
        }
    }

    /// Sets the `NEEDS_COMPOSITING` flag (D-block PR-A2 U34).
    ///
    /// Distinct from [`Self::mark_compositing_flag`] only in that this
    /// method documents its role in the per-frame compositing-bits walk
    /// (`_updateCompositingBits`); `mark_compositing_flag` was added in
    /// U22 as an additive helper.
    #[inline]
    pub fn mark_needs_compositing(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().mark_needs_compositing(),
            Self::Sliver(entry) => entry.state().flags().mark_needs_compositing(),
        }
    }

    /// Clears the `NEEDS_COMPOSITING` flag (D-block PR-A2 U34).
    #[inline]
    pub fn clear_needs_compositing(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().clear_needs_compositing(),
            Self::Sliver(entry) => entry.state().flags().clear_needs_compositing(),
        }
    }

    /// Reads the `NEEDS_COMPOSITING_BITS_UPDATE` flag (D-block PR-A2 U32).
    ///
    /// Set by `markNeedsCompositingBitsUpdate` (parent chain walks up to
    /// the first repaint boundary or already-dirty ancestor) and consulted
    /// by `_updateCompositingBits` to short-circuit subtrees that have
    /// nothing to do.
    #[inline]
    pub fn needs_compositing_bits_update(&self) -> bool {
        match self {
            Self::Box(entry) => entry.state().flags().needs_compositing_bits_update(),
            Self::Sliver(entry) => entry.state().flags().needs_compositing_bits_update(),
        }
    }

    /// Sets the `NEEDS_COMPOSITING_BITS_UPDATE` flag (D-block PR-A2 U32).
    #[inline]
    pub fn mark_needs_compositing_bits_update(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().mark_needs_compositing_bits_update(),
            Self::Sliver(entry) => entry.state().flags().mark_needs_compositing_bits_update(),
        }
    }

    /// Clears the `NEEDS_COMPOSITING_BITS_UPDATE` flag (D-block PR-A2 U32).
    #[inline]
    pub fn clear_needs_compositing_bits_update(&self) {
        match self {
            Self::Box(entry) => entry.state().flags().clear_needs_compositing_bits_update(),
            Self::Sliver(entry) => entry.state().flags().clear_needs_compositing_bits_update(),
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_tree::Leaf;
    use flui_types::{Size, geometry::px};

    use super::*;
    use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

    /// Minimal leaf box used only to exercise node-wiring logic.
    /// Concrete objects live in `flui_objects`; the node API is object-agnostic.
    #[derive(Debug, Default)]
    struct TestBox;

    impl flui_foundation::Diagnosticable for TestBox {}

    impl RenderBox for TestBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            Size::new(px(100.0), px(50.0))
        }

        fn paint(&self, _ctx: &mut crate::context::PaintCx<'_, Leaf>) {}
    }

    #[test]
    fn test_render_node_box_creation() {
        let node = RenderNode::new_box(Box::new(TestBox));

        assert!(node.is_box());
        assert!(!node.is_sliver());
        assert_eq!(node.protocol_name(), "Box");
    }

    #[test]
    fn test_render_node_links() {
        let node = RenderNode::new_box(Box::new(TestBox));

        // New nodes have no parent
        assert!(node.parent().is_none());
        assert!(node.children().is_empty());
        assert_eq!(node.depth(), 0);
    }

    #[test]
    fn test_render_node_with_parent() {
        let parent_id = RenderId::new(1);
        let node = RenderNode::new_box_with_parent(Box::new(TestBox), parent_id, 1);

        assert_eq!(node.parent(), Some(parent_id));
        assert_eq!(node.depth(), 1);
    }

    #[test]
    fn test_render_node_as_box() {
        let node = RenderNode::new_box(Box::new(TestBox));

        assert!(node.as_box().is_some());
        assert!(node.as_sliver().is_none());
    }

    #[test]
    fn test_render_node_state_access() {
        let node = RenderNode::new_box(Box::new(TestBox));

        // Check state access through the node
        if let Some(entry) = node.as_box() {
            // Entry should have default state (None before layout)
            assert!(entry.state().geometry().is_none());
        }
    }

    #[test]
    fn test_render_node_needs_layout() {
        let node = RenderNode::new_box(Box::new(TestBox));

        // New nodes need layout by default
        assert!(node.needs_layout());

        // Clear the flag
        node.clear_needs_layout();
        assert!(!node.needs_layout());
    }
}
