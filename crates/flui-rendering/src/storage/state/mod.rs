//! The per-node state a render object's protocol keeps beside it.
//!
//! - Dirty and boundary flags, in a [`RenderFlagsCell`] changed through `&self`.
//! - The constraints and geometry of the last layout, in the protocol's
//!   [`LayoutRecord`](crate::protocol::Protocol::LayoutRecord), overwritten
//!   by each layout through `&mut self`.
//! - The offset the parent assigned, and the layout generations that tell
//!   paint, hit testing and semantics whether the parent still places it.
//!
//! Everything changed through `&self` is a `Cell`: the render tree is
//! `!Send + !Sync` and one thread runs every pipeline pass (ADR-0323 §1).
//!
//! Dirty marking does not live here. Render-object setters return
//! `RenderUpdateImpact`; `flui-view` forwards it to
//! [`PipelineOwner::apply_render_update_impact`](crate::pipeline::PipelineOwner::apply_render_update_impact),
//! whose layout branch walks `mark_needs_layout`.

use std::cell::Cell;
use std::marker::PhantomData;

use flui_foundation::RenderId;

use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::storage::flags::{RenderFlags, RenderFlagsCell};

mod constraints;
mod flag_accessors;
mod geometry;
mod layout_cache;
mod layout_record;
mod offset;

pub use layout_cache::{BoxLayoutCache, IntrinsicDimension, ProtocolLayoutCache};
pub use layout_record::{BoxLayoutRecord, LayoutRecord, SliverLayoutRecord};

use offset::OffsetCell;

/// Render state for Box protocol (uses Size and BoxConstraints).
pub type BoxRenderState = RenderState<BoxProtocol>;

/// Render state for Sliver protocol (uses SliverGeometry and
/// SliverConstraints).
pub type SliverRenderState = RenderState<SliverProtocol>;

/// The state the pipeline keeps for one render object of protocol `P`.
///
/// Every node occupies a render-slab slot as large as the larger of the
/// two protocols' entries, so this struct is sized for the box protocol:
/// the box record (`Size`, `BoxConstraints`) is inline and the sliver
/// record is one pointer to a record allocated on the sliver's first
/// layout. `size_of::<RenderNode>()` is pinned by
/// `render_node_slot_stays_within_its_budget`.
#[derive(Debug)]
pub struct RenderState<P: Protocol> {
    /// Dirty and boundary flags.
    flags: RenderFlagsCell,

    /// The constraints of the last layout and the geometry it produced
    /// (`Size` and `BoxConstraints` for a box, `SliverGeometry` and
    /// `SliverConstraints` for a sliver), each overwritten by the next
    /// layout. The constraints are what the relayout-boundary short-circuit
    /// compares against. The protocol chooses where the record lives: a box
    /// keeps it inline, a sliver behind one allocation made on first write,
    /// so the slab slot every node occupies is sized by the box record.
    layout: P::LayoutRecord,

    /// Offset relative to parent.
    ///
    /// Set by parent during layout, read during paint and hit testing.
    offset: OffsetCell,

    /// Bumped every time this node is laid out for real — past the
    /// short-circuit, so a node whose cached geometry was reused does not
    /// advance.
    ///
    /// Paired with [`placed_generation`](Self::placed_generation) on this
    /// node's children: a child is part of this pass's layout exactly when its
    /// `placed_generation` equals this value. **Paint and hit-test** read that
    /// comparison and skip the rest.
    ///
    /// Starts at 1, the same value [`placed_generation`](Self::placed_generation)
    /// starts at, so an untouched parent and its untouched children agree —
    /// see that field for why "no evidence" has to mean "placed". The first
    /// real layout advances this to 2 and stamps the children it laid out,
    /// which is the first moment the two can differ.
    layout_generation: Cell<u64>,

    /// The parent's [`layout_generation`](Self::layout_generation) at the last
    /// pass that laid this node out as one of its children.
    ///
    /// Starts at 1, matching a parent's initial `layout_generation`, so a node
    /// whose parent has not laid anything out **yet** counts as placed. The
    /// gate must only ever remove a child a parent demonstrably *stopped*
    /// laying out; absent evidence it paints, which is the behaviour that
    /// preceded it. A parent that never lays out its children at all — served
    /// entirely from cache, or laying out through some path of its own — would
    /// otherwise take its whole subtree off the screen.
    ///
    /// Meaningful only together with [`placed_by`](Self::placed_by): the
    /// counter is per-parent, so the same number is issued by every parent
    /// that has laid out the same number of times, and a child reparented
    /// between two of them would match on the number alone.
    ///
    /// **Semantics is gated on this too**, decided separately from paint and
    /// hit-test because the trade differs: the same rule there drops a child's
    /// *content*, not merely its stale rect — a merge folds a descendant's
    /// configuration regardless of geometry, so excluding an unplaced child
    /// loses its label and role along with its position. That is the correct
    /// outcome, since a node the parent stopped laying out has no place on
    /// screen to announce; the alternative is an assistive technology reading
    /// a row at coordinates nothing occupies. See `ARCHITECTURE.md`'s
    /// `## Mapping decisions` entry for the reasoning.
    placed_generation: Cell<u64>,

    /// The parent that issued [`placed_generation`](Self::placed_generation);
    /// `None` when nothing has stamped this node.
    ///
    /// Without it the comparison is a bare number against a per-parent
    /// counter, and numbers collide across parents. A `GlobalKey` relocation
    /// makes that reachable rather than theoretical: a child stamped `2` by
    /// parent A, reparented onto a B that has never laid out, passes B's first
    /// real layout — which also reaches `2` — even though B laid out nothing.
    /// It would then paint and hit-test at A's offset.
    placed_by: Cell<Option<RenderId>>,

    /// Per-node layout calculation cache:
    /// memoized intrinsic dimensions / dry layout / dry baselines.
    /// Cleared by `mark_needs_layout`; a non-empty clear escalates the
    /// invalidation past relayout boundaries.
    layout_cache: P::LayoutCache,

    /// Persistent parent data for this node, set by the parent during
    /// layout. Survives across frames — the parent writes it once, and
    /// subsequent reads return the cached value.
    ///
    /// This replaces the transient `Option<Box<dyn ParentData>>` in
    /// `ErasedChildState` which was rebuilt every frame with `None`.
    ///
    /// # Type erasure
    ///
    /// The parent data type is determined by the PARENT's
    /// `RenderBox::ParentData` associated type, not this node's protocol.
    /// We store it as `Option<Box<dyn ParentData>>` (type-erased) because
    /// `RenderState<P>` cannot name the parent's concrete type.
    /// The parent downcasts via `downcast_ref::<T>()` when reading.
    parent_data: Option<Box<dyn crate::parent_data::ParentData>>,

    /// Protocol marker (zero-sized).
    _phantom: PhantomData<P>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Creates a new render state with default dirty flags.
    ///
    /// Initial state:
    /// - NEEDS_LAYOUT flag set (requires initial layout)
    /// - NEEDS_PAINT flag set (requires initial paint)
    /// - No geometry or constraints (will be set during first layout)
    /// - Offset at origin (will be set by parent during layout)
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::storage::BoxRenderState;
    ///
    /// let state = BoxRenderState::new();
    /// assert!(state.needs_layout());
    /// assert!(state.needs_paint());
    /// ```
    pub fn new() -> Self {
        Self {
            flags: RenderFlagsCell::new(RenderFlags::NEEDS_LAYOUT | RenderFlags::NEEDS_PAINT),
            layout: P::LayoutRecord::default(),
            offset: OffsetCell::new(flui_foundation::geometry::Offset::ZERO),
            layout_generation: Cell::new(1),
            placed_generation: Cell::new(1),
            placed_by: Cell::new(None),
            layout_cache: P::LayoutCache::default(),
            parent_data: None,
            _phantom: PhantomData,
        }
    }

    /// Creates a render state with custom initial flags.
    ///
    /// Use this when you need specific initial flags (e.g., for testing
    /// or special initialization scenarios).
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::storage::{BoxRenderState, RenderFlags};
    ///
    /// // A state that does not need an initial layout.
    /// let state = BoxRenderState::with_flags(RenderFlags::empty());
    /// assert!(!state.needs_layout());
    /// ```
    pub fn with_flags(flags: RenderFlags) -> Self {
        Self {
            flags: RenderFlagsCell::new(flags),
            layout: P::LayoutRecord::default(),
            offset: OffsetCell::new(flui_foundation::geometry::Offset::ZERO),
            layout_generation: Cell::new(1),
            placed_generation: Cell::new(1),
            placed_by: Cell::new(None),
            layout_cache: P::LayoutCache::default(),
            parent_data: None,
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// PARENT DATA ACCESS
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Returns a reference to the persistent parent data, if set.
    ///
    /// Parent data is set by the parent during layout and survives
    /// across frames. Returns `None` if no parent data has been set yet.
    #[inline]
    pub fn parent_data(&self) -> Option<&dyn crate::parent_data::ParentData> {
        self.parent_data.as_deref()
    }

    /// Returns a mutable reference to the persistent parent data, if set.
    #[inline]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn crate::parent_data::ParentData> {
        self.parent_data.as_deref_mut()
    }

    pub(super) fn take_parent_data(&mut self) -> Option<Box<dyn crate::parent_data::ParentData>> {
        self.parent_data.take()
    }

    /// Sets (or replaces) the persistent parent data.
    ///
    /// Called by the parent during layout to store metadata about this
    /// child (flex factor, stack position, etc.). The data persists
    /// across frames until the parent replaces it.
    #[inline]
    pub fn set_parent_data(&mut self, data: Box<dyn crate::parent_data::ParentData>) {
        self.parent_data = Some(data);
    }

    pub(super) fn clear_parent_data(&mut self) -> bool {
        let Some(mut parent_data) = self.parent_data.take() else {
            return false;
        };
        parent_data.detach();
        true
    }
}

// ============================================================================
// LAYOUT CACHE ACCESS
// ============================================================================

impl<P: Protocol> RenderState<P> {
    /// Read access to the layout calculation cache (peek path of the
    /// pipeline's memoizing walks).
    pub fn layout_cache(&self) -> &P::LayoutCache {
        &self.layout_cache
    }

    /// Mutable access to the layout calculation cache. The pipeline
    /// memoizes intrinsic/dry-layout queries here; render objects never
    /// touch it.
    pub fn layout_cache_mut(&mut self) -> &mut P::LayoutCache {
        &mut self.layout_cache
    }

    /// Clears the layout cache, returning whether anything WAS cached.
    ///
    /// `true` means an ancestor's layout consumed this node's
    /// intrinsics/baseline, so the caller must escalate the invalidation
    /// to the parent even across a relayout boundary.
    pub fn clear_layout_cache(&mut self) -> bool {
        self.layout_cache.clear()
    }
}

impl<P: Protocol> Default for RenderState<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Clone for RenderState<P>
where
    P::Geometry: Clone,
    P::Constraints: Clone,
{
    fn clone(&self) -> Self {
        Self {
            flags: RenderFlagsCell::new(self.flags.load()),
            layout: self.layout.clone(),
            offset: OffsetCell::new(self.offset.load()),
            layout_generation: Cell::new(self.layout_generation.get()),
            placed_generation: Cell::new(self.placed_generation.get()),
            placed_by: Cell::new(self.placed_by.get()),
            // Memoized results are node-local; a cloned state starts cold.
            layout_cache: P::LayoutCache::default(),
            parent_data: self.parent_data.clone(),
            _phantom: PhantomData,
        }
    }
}
