//! The recursive layout walks over a leased render tree, and the lease the query walks share.
//!
//! The pipeline takes one lease on the render tree ([`crate::storage::RenderTree`]'s `lease`, see
//! ADR-0252) and a walk enters each node as it reaches it, so the cost of a walk follows the nodes
//! it touches, not the size of the dirty subtree. [`SubtreeArena`] holds that lease for the layout
//! walk, with the sinks the walk records into; [`QueryWalk`] holds one for the memoizing query
//! walks of `query.rs`.
//!
//! A walk borrows a node only through an [`EnteredNode`] (`storage/node_slab.rs`, the one module
//! that turns a slot into a reference), which holds the node's in-flight bit while it lives. Each
//! level of the layout walk is a [`LayoutFrame`] that owns the entry of its node; every other
//! node a frame touches (a child it seeds, lends parent data to or places, a baseline chain, a
//! child's parent data for an intrinsic query) it enters for that access alone. A cyclic child
//! link that leads back to a node entered up the stack is refused there: a frame reports
//! [`crate::error::RenderError::LayoutCycle`], a short access skips the node.
//!
//! The walk runs on the thread that took the lease: [`SubtreeArena`] is `!Send + !Sync` (pinned
//! in `tests`), so no callback it hands a render object can carry it elsewhere.

use crate::pipeline::TextLender;
use crate::storage::{EnterRefusal, EnteredNode, TreeLease};

use std::marker::PhantomData;

use flui_foundation::RenderId;
use flui_foundation::panic::payload_text;
#[cfg(any(test, feature = "testing"))]
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::cell::{Cell, RefCell};

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    parent_data::ParentData,
    protocol::{
        ActualBaselineChildCallback, BoxProtocol, ErasedBoxLayoutContext,
        ErasedSliverLayoutContext, LayoutChildCallback, Protocol, SliverChildLayoutCallback,
        SliverLayoutChildCallback, SliverProtocol,
    },
    storage::{ErasedConstraints, RenderNode, RenderTree},
};

use super::buffer_pool::{BufferPool, Pooled};
use super::poison::{LayoutFailureKind, LayoutPoison};

/// The child lists a layout walk checks out for each node it lays out, kept
/// by the owner across frames so a pass over a settled tree allocates none.
#[derive(Default)]
pub(super) struct LayoutBuffers {
    child_ids: BufferPool<RenderId>,
    box_children: BufferPool<crate::protocol::ErasedChildState>,
    sliver_children: BufferPool<crate::protocol::ErasedSliverChildState>,
}

// ============================================================================
// Child states of a layout frame
// ============================================================================

/// A layout walk's state for one child: the parent data it holds while its parent lays it out,
/// and the placement the parent decided.
trait WalkedChild {
    fn child(&self) -> RenderId;
    fn parent_data_slot(&mut self) -> &mut Option<Box<dyn ParentData>>;
    fn offset(&self) -> flui_foundation::geometry::Offset;
    fn laid_out_this_pass(&self) -> bool;
}

impl WalkedChild for crate::protocol::ErasedChildState {
    fn child(&self) -> RenderId {
        self.id
    }

    fn parent_data_slot(&mut self) -> &mut Option<Box<dyn ParentData>> {
        &mut self.parent_data
    }

    fn offset(&self) -> flui_foundation::geometry::Offset {
        self.offset
    }

    fn laid_out_this_pass(&self) -> bool {
        self.laid_out_this_pass
    }
}

impl WalkedChild for crate::protocol::ErasedSliverChildState {
    fn child(&self) -> RenderId {
        self.id
    }

    fn parent_data_slot(&mut self) -> &mut Option<Box<dyn ParentData>> {
        &mut self.parent_data
    }

    fn offset(&self) -> flui_foundation::geometry::Offset {
        self.offset
    }

    fn laid_out_this_pass(&self) -> bool {
        self.laid_out_this_pass
    }
}

/// The child states of one layout frame, holding the parent data the frame borrowed from its
/// children.
///
/// [`Self::seed`] moves a child's parent data out of its node into the child's state, so the
/// parent reads and writes it there without a copy; dropping this value moves it back, on every
/// exit of the frame: success, an error and an unwind. What the parent wrote during its layout
/// (a sliver's `layout_offset`, a default it created on first access) is what the child keeps,
/// which is the parent data's contract: the parent owns it.
///
/// Only the child's own parent borrows it: a torn list that names a child another node owns gets
/// a copy of what the node holds (none while the owner's frame, above it, has it lent), as does a
/// harness seed, and neither goes back. A child listed twice is lent to its first
/// slot; the second reads none this pass, and whatever it creates never replaces the original.
/// A child entered up the stack (a cyclic link) is neither seeded nor lent nor placed: it keeps
/// the state's defaults, and the cycle already degrades it to a stand-in.
///
/// The render tree cannot change while the walk runs (the lease holds its `&mut RenderTree`, and
/// child builds are deferred until the walk ends), so every lent value goes back to the node it
/// came from.
struct LentChildren<'a, 'tree, T: WalkedChild> {
    arena: &'a SubtreeArena<'tree>,
    /// The node laying these children out.
    parent: RenderId,
    states: Pooled<'a, T>,
}

impl<'a, 'tree, T: WalkedChild> LentChildren<'a, 'tree, T> {
    fn new(arena: &'a SubtreeArena<'tree>, parent: RenderId, states: Pooled<'a, T>) -> Self {
        Self {
            arena,
            parent,
            states,
        }
    }

    /// Fills each child's state from its node before the parent lays it out: `read` copies what
    /// the protocol carries over from the last pass (a parent that does not position a child
    /// again keeps its prior offset), then the child's parent data is lent, or a harness seed
    /// stands in for it unlent.
    fn seed(&mut self, mut read: impl FnMut(&mut T, &RenderNode)) {
        for state in self.states.iter_mut() {
            let child = state.child();
            let entered = match self.arena.lease.enter(child) {
                Err(EnterRefusal::InFlight) => continue,
                entered => entered.ok(),
            };
            let seeded = self
                .arena
                .seed_child_parent_data(child, state.parent_data_slot());
            let Some(mut entered) = entered else {
                continue;
            };
            read(state, entered.node());
            if !seeded {
                *state.parent_data_slot() = if lends_to(self.arena, self.parent, child) {
                    entered.node_mut().take_parent_data()
                } else {
                    entered.node().parent_data().map(dyn_clone::clone_box)
                };
            }
        }
    }

    /// Commits the placement the parent decided into each child's node, after the parent's
    /// layout succeeded: the offset paint and hit-test read, and, for the children this pass
    /// laid out, the parent's fresh layout generation. A child the parent skipped keeps its
    /// previous stamp, so paint and hit-test leave it alone (a lazy sliver's out-of-band
    /// residents are not painted at an offset that no longer describes the tree).
    ///
    /// The generation advances here, beside the stamps: advancing it at layout entry lets an
    /// early return in between (a protocol error, a poisoned descendant) move the parent forward
    /// while stamping nobody, which unplaces every child and stops the whole subtree painting.
    fn commit_placement(&self, frame: &LayoutFrame<'_, '_>) {
        let generation = frame.node().advance_layout_generation();
        for state in self.states.iter() {
            let Ok(entered) = self.arena.lease.enter(state.child()) else {
                continue;
            };
            let node = entered.node();
            node.set_offset(state.offset());
            if state.laid_out_this_pass() {
                node.set_placed_by(frame.id(), generation);
            }
        }
    }
}

/// Whether `child` lends its parent data to `parent`'s layout frame: it has no harness seed and
/// names `parent` as its parent. The caller has entered `child`, so it is not in flight up the
/// stack.
fn lends_to(arena: &SubtreeArena<'_>, parent: RenderId, child: RenderId) -> bool {
    #[cfg(any(test, feature = "testing"))]
    if arena.parent_data_seeds.contains_key(&child) {
        return false;
    }
    arena.lease.is_child_of(child, parent)
}

impl<T: WalkedChild> std::ops::Deref for LentChildren<'_, '_, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.states
    }
}

impl<T: WalkedChild> std::ops::DerefMut for LentChildren<'_, '_, T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.states
    }
}

impl<T: WalkedChild> Drop for LentChildren<'_, '_, T> {
    fn drop(&mut self) {
        for state in self.states.iter_mut() {
            let child = state.child();
            // A child entered up the stack, seeded, or owned by another node holds a copy or the
            // parent's own default, which dies with the state.
            let Ok(mut entered) = self.arena.lease.enter(child) else {
                continue;
            };
            if !lends_to(self.arena, self.parent, child) {
                continue;
            }
            let Some(data) = state.parent_data_slot().take() else {
                continue;
            };
            let node = entered.node_mut();
            // A child listed twice was lent once; its second state never holds the original,
            // so it must not replace what the first one returned.
            if node.parent_data().is_none() {
                node.set_parent_data(data);
            }
        }
    }
}

// ============================================================================
// SubtreeArena — the layout walk's leased view of the tree
// ============================================================================

/// The layout walk's view of the render tree: a lease that enters nodes by [`RenderId`] as the
/// walk reaches them, plus the sinks the walk records into. No `&mut RenderTree` appears inside
/// the layout-child callback chain; the lease holds the one borrow for the whole walk.
///
/// `'tree` is the source `&mut RenderTree` borrow, which the lease keeps; constructed via
/// [`Self::from_tree`].
///
/// `SubtreeArena` is `!Send + !Sync` (pinned by `static_assertions::assert_not_impl_any!` in
/// `tests`, below): the lease's slot pointer, the `RefCell` sinks and the `Cell` counters each
/// block the auto-traits, so safe code cannot move an arena to another thread.
pub(super) struct SubtreeArena<'tree> {
    /// The walk's lease on the render tree: nodes are entered as the walk reaches them, with the
    /// in-flight bit and the parent link in the slot headers, which the walk reads while the
    /// node itself is entered up the stack.
    lease: TreeLease<'tree>,
    /// The node whose walk frame is innermost, the parent of whatever the next
    /// [`LayoutFrame::enter`] is asked to enter. `None` before the dirty root.
    current: Cell<Option<RenderId>>,
    #[cfg(any(test, feature = "testing"))]
    parent_data_seeds: &'tree FxHashMap<RenderId, ParentDataSeed>,
    /// Child-build requests from `RenderSliverList`: `(sliver_id,
    /// logical_index)` pairs recorded when an absent in-band child is
    /// encountered.  No render object is pre-built here — the element tree
    /// decides what to build.  Drained in `layout_dirty_root` and moved into
    /// `PipelineOwner::pending_child_requests` for the binding layer, which
    /// services it between layout passes of the same frame's fixpoint.
    /// A `RefCell`, like every sink here: the arena is `!Send + !Sync`
    /// (see the pin in `tests`), and no borrow is held across user code.
    pending_child_requests: RefCell<Vec<(flui_foundation::RenderId, usize)>>,
    /// Retain-band signals from element-owned slivers.
    ///
    /// Each entry is `(sliver_id, cache_first, cache_last)` — the `[first,
    /// last)` band the sliver retained this pass.  Drained after the walk
    /// (after `pending_child_requests`, after `drop(arena)`) and moved into
    /// `PipelineOwner::pending_retain_bands` for the binding layer.  Same
    /// `RefCell` discipline as the other sinks.  Empty unless an element-owned
    /// sliver (`RenderSliverList`) completed layout this frame.
    pending_retain_bands: RefCell<Vec<(flui_foundation::RenderId, usize, usize)>>,
    /// Ids of the repaint BOUNDARIES this walk actually laid out.
    ///
    /// Recorded once the short-circuit has declined to fire, so a boundary
    /// that kept its cached geometry — not dirty, same constraints — is
    /// absent. Non-boundaries are filtered at the record site rather than on
    /// drain: only boundaries are ever used from this list, and pushing every
    /// laid-out node instead cost a measurable ~5% on `layout/flat/1000`,
    /// where the tree holds no boundaries at all.
    /// `run_layout` marks the repaint boundaries among these for paint, a
    /// per-object needs-paint mark expressed as one drain instead of one call
    /// per object.
    ///
    /// The alternative it replaced was a downward sweep over the dirty root's
    /// subtree, which had to queue every boundary it found because it could
    /// not tell which ones layout had skipped. Over-queueing is safe but costs
    /// exactly the repaints a retaining paint pass exists to avoid.
    ///
    /// Same `RefCell` discipline and post-walk drain as the sinks above.
    laid_out: RefCell<Vec<flui_foundation::RenderId>>,
    /// How many nodes this walk actually laid out, boundary or not.
    ///
    /// Bumped at the same record sites as [`Self::laid_out`], so a node the
    /// short-circuit served from its cache is not counted. A plain `Cell`: the
    /// arena is `!Send + !Sync`, and this is one integer add per laid-out node
    /// rather than the vector push `laid_out` avoids for non-boundaries.
    /// Drained into `PipelineCounters::nodes_laid_out` after the walk.
    laid_out_count: Cell<u64>,
    /// How many frames this walk entered (a node served from its cache counts; the short entries
    /// a frame makes to seed or place its children do not). Drained into
    /// `PipelineCounters::nodes_acquired` after the walk.
    acquired_count: Cell<u64>,
    /// Read-only view of the owner's layout-poison table for the whole
    /// walk.  Consulted at the top of each recursion level: a node
    /// poisoned under the *same* constraints it is now offered is
    /// skipped (its last committed geometry stands in) instead of
    /// re-running a `perform_layout` that deterministically fails
    /// again.  Also gates the success sink below so a skip's stand-in
    /// `Ok` is never mistaken for a recovery.
    layout_poison: &'tree LayoutPoison,
    /// The owner's child-list buffers, checked out per node and given back.
    buffers: &'tree LayoutBuffers,
    /// The UI runtime's text context, lent to each node the walk lays out or
    /// measures: box leaves, box parents, and box intrinsic queries, whether
    /// a box or a sliver parent asked for them. Each loan records the node
    /// it was made for.
    text: TextLender<'tree>,
    /// Nodes this walk re-attempted despite being poisoned because the
    /// incoming constraints differ from the failed attempt's.  A success
    /// for such a node IS a real recovery, so
    /// [`Self::note_layout_success`] accepts it alongside nodes with
    /// open (not yet poisoned) failure records.  Per-walk scratch: dies
    /// with the arena, no drain.
    poison_retries: RefCell<FxHashSet<flui_foundation::RenderId>>,
    /// Descendant layout AND intrinsic-measurement failures swallowed by
    /// the layout-child / intrinsic-child callbacks during this walk:
    /// `(direct layout parent, failed node, kind, attempt)` tuples,
    /// where `attempt` carries the constraints of the failed layout
    /// (`None` for intrinsic-query failures, which have no constraints).
    /// The callbacks must return a geometry / measured value, so the
    /// typed error cannot propagate through `perform_layout`; this sink
    /// carries the failure identity to `layout_dirty_root`, which feeds
    /// the poison counters after the walk.  Same `RefCell` discipline as
    /// the other sinks.  Empty unless a descendant errored this walk.
    #[expect(
        clippy::type_complexity,
        reason = "a private sink whose tuple is pushed and drained in this module only"
    )]
    layout_failures: RefCell<
        Vec<(
            flui_foundation::RenderId,
            flui_foundation::RenderId,
            LayoutFailureKind,
            Option<ErasedConstraints>,
        )>,
    >,
    /// Descendants whose own layout succeeded this walk AND which had open
    /// failure records — the only successes the poison table needs (a
    /// success clears the record).  Filtered at record time so the sink
    /// stays empty in the common all-healthy case.
    layout_successes: RefCell<Vec<flui_foundation::RenderId>>,
    /// Every degradation event of this walk: a layout that failed and handed
    /// its caller a stand-in, or a poisoned node that served one.
    ///
    /// A layout context takes a `DegradationProbe` over this counter when it
    /// is built and reports whether the count moved during its node's pass.
    /// The walk is depth-first and synchronous, so a count that moved between
    /// a node's context creation and its query moved inside that node's
    /// subtree, at any depth. A parent that must not act on a stand-in — a
    /// viewport about to publish scroll dimensions — asks before it commits.
    degradation_events: Cell<u64>,
    _lifetime: PhantomData<&'tree mut ()>,
}

impl<'tree> SubtreeArena<'tree> {
    /// Constructs a `SubtreeArena` over a lease on the render tree, taken by
    /// [`super::PipelineOwner::layout_dirty_root`], the only caller. No node is entered here:
    /// the walk enters each one when it reaches it.
    pub(super) fn new(
        lease: TreeLease<'tree>,
        layout_poison: &'tree LayoutPoison,
        buffers: &'tree LayoutBuffers,
        text: TextLender<'tree>,
        #[cfg(any(test, feature = "testing"))] parent_data_seeds: &'tree FxHashMap<
            RenderId,
            ParentDataSeed,
        >,
    ) -> Self {
        Self {
            lease,
            current: Cell::new(None),
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds,
            pending_child_requests: RefCell::new(Vec::new()),
            pending_retain_bands: RefCell::new(Vec::new()),
            laid_out: RefCell::new(Vec::new()),
            laid_out_count: Cell::new(0),
            acquired_count: Cell::new(0),
            layout_poison,
            buffers,
            text,
            poison_retries: RefCell::new(FxHashSet::default()),
            layout_failures: RefCell::new(Vec::new()),
            layout_successes: RefCell::new(Vec::new()),
            degradation_events: Cell::new(0),
            _lifetime: PhantomData,
        }
    }

    /// Puts the harness seed for `child_id` in `slot`, and says whether there was one.
    #[cfg(any(test, feature = "testing"))]
    pub(super) fn seed_child_parent_data(
        &self,
        child_id: RenderId,
        slot: &mut Option<Box<dyn crate::parent_data::ParentData>>,
    ) -> bool {
        let Some(seed) = self.parent_data_seeds.get(&child_id) else {
            return false;
        };
        *slot = Some(seed.to_box());
        true
    }

    /// A build without the harness seeds none.
    #[cfg(not(any(test, feature = "testing")))]
    #[expect(
        clippy::unused_self,
        reason = "the twin of the testing build's seed lookup, so both walks read alike"
    )]
    pub(super) fn seed_child_parent_data(
        &self,
        _child_id: RenderId,
        _slot: &mut Option<Box<dyn crate::parent_data::ParentData>>,
    ) -> bool {
        false
    }

    /// Takes the child-build requests recorded by request-strategy slivers
    /// during this walk.  Returns `(sliver_id, logical_index)` pairs
    /// for the binding layer to service after the frame.  Called in
    /// `layout_dirty_root` once the walk has returned.
    pub(super) fn take_pending_child_requests(&self) -> Vec<(flui_foundation::RenderId, usize)> {
        std::mem::take(&mut *self.pending_child_requests.borrow_mut())
    }

    /// Takes the retain-band signals recorded by element-owned slivers
    /// (`RenderSliverList`) during this walk.  Returns `(sliver_id,
    /// cache_first, cache_last)` triples — the `[first, last)` band each
    /// element-owned sliver retained this frame.  Called in `layout_dirty_root`
    /// after `take_pending_child_requests`, once the walk has returned.
    ///
    /// Symmetric to [`Self::take_pending_child_requests`].  The binding layer
    /// drives `SparseChildren::retain_band` from these entries.
    pub(super) fn take_pending_retain_bands(
        &self,
    ) -> Vec<(flui_foundation::RenderId, usize, usize)> {
        std::mem::take(&mut *self.pending_retain_bands.borrow_mut())
    }

    /// Drains the ids this walk actually laid out — see [`Self::laid_out`].
    pub(super) fn take_laid_out(&self) -> Vec<flui_foundation::RenderId> {
        std::mem::take(&mut *self.laid_out.borrow_mut())
    }

    /// Drains how many nodes this walk entered — see [`Self::acquired_count`].
    pub(super) fn take_acquired_count(&self) -> u64 {
        self.acquired_count.replace(0)
    }

    /// Drains how many nodes this walk laid out — see [`Self::laid_out_count`].
    pub(super) fn take_laid_out_count(&self) -> u64 {
        self.laid_out_count.replace(0)
    }

    /// Counts one node past the short-circuit, i.e. actually laid out.
    #[inline]
    fn note_laid_out(&self) {
        self.laid_out_count.set(self.laid_out_count.get() + 1);
    }

    /// The walk's degradation count so far (see [`Self::degradation_events`]).
    fn degradation_count(&self) -> u64 {
        self.degradation_events.get()
    }

    /// Count one degradation event (see [`Self::degradation_events`]).
    fn note_degradation(&self) {
        self.degradation_events
            .set(self.degradation_events.get().saturating_add(1));
    }

    /// Records a descendant layout failure swallowed at a layout-child
    /// callback.  `parent` is the node whose `perform_layout` invoked the
    /// callback; `failed` is the direct child whose own layout returned
    /// `err`; `attempt` carries the constraints of the failed layout
    /// (`None` for intrinsic-query failures).  Read by `layout_dirty_root`
    /// after the walk to feed the poison counters.
    fn note_layout_failure(
        &self,
        parent: RenderId,
        failed: RenderId,
        err: &crate::error::RenderError,
        attempt: Option<ErasedConstraints>,
    ) {
        if err.is_recoverable_layout_error() {
            return;
        }
        self.layout_failures.borrow_mut().push((
            parent,
            failed,
            LayoutFailureKind::of(err),
            attempt,
        ));
        // Every failure recorded here handed its caller a stand-in size,
        // geometry or intrinsic — the walk continues on a value this pass
        // did not compute.
        self.note_degradation();
    }

    /// Records a descendant layout success, but only for a node the
    /// poison table has a record for AND which actually ran layout this
    /// walk: either a node with open (not yet poisoned) failures, or a
    /// poisoned node re-attempted under changed constraints (registered
    /// in `poison_retries` at the walk's skip site).  A poisoned node
    /// that was merely *skipped* returns its stand-in geometry through
    /// the same callback `Ok` arm, and this filter is what keeps that
    /// stand-in from being misread as a recovery.
    fn note_layout_success(&self, id: RenderId) {
        let retried = self.poison_retries.borrow().contains(&id);
        if retried || self.layout_poison.has_open_failures(id) {
            self.layout_successes.borrow_mut().push(id);
        }
    }

    /// Takes the descendant layout failures recorded during this walk:
    /// `(direct layout parent, failed node, kind, attempt)` tuples.
    /// Called by `layout_dirty_root` after the walk, alongside the other
    /// sinks.
    pub(super) fn take_layout_failures(
        &self,
    ) -> Vec<(
        flui_foundation::RenderId,
        flui_foundation::RenderId,
        LayoutFailureKind,
        Option<ErasedConstraints>,
    )> {
        std::mem::take(&mut *self.layout_failures.borrow_mut())
    }

    /// Takes the descendant layout successes recorded during this walk
    /// (only nodes with open failure records; see
    /// [`Self::note_layout_success`]).  Called by `layout_dirty_root`
    /// after the walk.
    pub(super) fn take_layout_successes(&self) -> Vec<flui_foundation::RenderId> {
        std::mem::take(&mut *self.layout_successes.borrow_mut())
    }

    /// Lays out the box node identified by `id` under `constraints`, walking its subtree.
    pub(super) fn layout_child(
        &self,
        id: RenderId,
        constraints: BoxConstraints,
    ) -> crate::error::RenderResult<flui_foundation::geometry::Size> {
        layout_subtree_borrowed(self, id, constraints)
    }

    /// Constructs a `SubtreeArena` from a `RenderTree` for the walk rooted at `id`, returning
    /// `Err(NodeNotFound)` if `id` is stale or vacant.
    ///
    /// The one-stop safe constructor used by [`super::PipelineOwner::layout_dirty_root`]. It takes
    /// the tree's lease and leaves every node unentered. `layout_poison` is the owner's poison
    /// table, borrowed for exactly the walk's lifetime so each recursion level can skip poisoned
    /// nodes.
    pub(super) fn from_tree(
        render_tree: &'tree mut RenderTree,
        id: RenderId,
        layout_poison: &'tree LayoutPoison,
        buffers: &'tree LayoutBuffers,
        text: TextLender<'tree>,
        #[cfg(any(test, feature = "testing"))] all_seeds: &'tree FxHashMap<
            RenderId,
            ParentDataSeed,
        >,
    ) -> crate::error::RenderResult<Self> {
        let lease = render_tree.lease();
        if matches!(lease.enter(id), Err(EnterRefusal::Missing)) {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }
        Ok(Self::new(
            lease,
            layout_poison,
            buffers,
            text,
            #[cfg(any(test, feature = "testing"))]
            all_seeds,
        ))
    }
}

// ============================================================================
// LayoutFrame — one level of the layout walk
// ============================================================================

/// One level of the layout walk: the entry of the node it lays out or measures, which holds the
/// node's in-flight bit, and [`SubtreeArena::current`] kept on the innermost frame.
///
/// The frame borrows its node through [`Self::node`] and [`Self::node_mut`] only, so the borrow
/// checker keeps a read of the node apart from the exclusive borrow its `perform_layout` runs
/// under. Dropping the frame, on unwind too, restores `current` and clears the bit, so a
/// panicking widget leaves nothing in flight for the next frame.
struct LayoutFrame<'arena, 'tree> {
    arena: &'arena SubtreeArena<'tree>,
    entry: EnteredNode<'arena, 'tree>,
    /// The frame this one was entered from, restored on drop.
    parent: Option<RenderId>,
}

impl<'arena, 'tree> LayoutFrame<'arena, 'tree> {
    /// Enters `id` as the innermost frame.
    ///
    /// - `Err(RenderError::LayoutCycle(id))` if `id` is already entered up the stack (a
    ///   `perform_layout` asked for an ancestor). Checked first, so a cyclic `children()` edge
    ///   reports a cycle, not a missing node.
    /// - `Err(RenderError::NodeNotFound(id))` if `id` is stale or vacant, or a parent frame
    ///   exists and `id` is not its child (read from the slot headers). The dirty root has no
    ///   parent frame and is validated by the arena's constructor.
    fn enter(arena: &'arena SubtreeArena<'tree>, id: RenderId) -> crate::error::RenderResult<Self> {
        let entry = match arena.lease.enter(id) {
            Ok(entry) => entry,
            Err(EnterRefusal::InFlight) => {
                // Debug-level: the layout-child callback logs the propagated error at
                // `tracing::error` when it collapses it to a stand-in, which is the user-facing
                // line; this one keeps the collision point for tracing.
                tracing::debug!(
                    ?id,
                    "layout_subtree_borrowed: layout cycle detected — id is \
                     already in flight at a parent call level; returning \
                     RenderError::LayoutCycle(id)",
                );
                return Err(crate::error::RenderError::layout_cycle(id));
            }
            Err(EnterRefusal::Missing) => return Err(crate::error::RenderError::NodeNotFound(id)),
        };
        let parent = arena.current.get();
        if let Some(parent) = parent
            && !arena.lease.is_child_of(id, parent)
        {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }
        arena.acquired_count.set(arena.acquired_count.get() + 1);
        arena.current.set(Some(id));
        Ok(Self {
            arena,
            entry,
            parent,
        })
    }

    /// The node this frame entered.
    #[inline]
    fn id(&self) -> RenderId {
        self.entry.id()
    }

    #[inline]
    fn node(&self) -> &RenderNode {
        self.entry.node()
    }

    #[inline]
    fn node_mut(&mut self) -> &mut RenderNode {
        self.entry.node_mut()
    }
}

impl Drop for LayoutFrame<'_, '_> {
    fn drop(&mut self) {
        self.arena.current.set(self.parent);
    }
}

// ============================================================================
// QueryWalk — the dry intrinsic/layout/baseline query walks' view of the tree
// ============================================================================

/// The memoizing query walks' lease on the render tree. Like [`SubtreeArena`] it enters a node
/// when the walk reaches it, so a query costs the nodes it visits and not the subtree it sits on.
pub(super) struct QueryWalk<'tree> {
    lease: TreeLease<'tree>,
}

impl<'tree> QueryWalk<'tree> {
    pub(super) fn new(render_tree: &'tree mut RenderTree) -> Self {
        Self {
            lease: render_tree.lease(),
        }
    }

    /// Enters `id`: sets its in-flight bit and lends its node until the entry drops.
    #[inline]
    pub(super) fn enter(&self, id: RenderId) -> Result<EnteredNode<'_, 'tree>, EnterRefusal> {
        self.lease.enter(id)
    }

    /// A copy of `child`'s committed parent data, or `None` when it has none or is entered up the
    /// stack (its node is then borrowed, and the query treats it as absent).
    pub(super) fn child_parent_data(&self, child: RenderId) -> Option<Box<dyn ParentData>> {
        let entered = self.lease.enter(child).ok()?;
        entered.node().parent_data().map(dyn_clone::clone_box)
    }
}

// ============================================================================
// Stack guard
// ============================================================================

/// Grows the stack ahead of each pipeline-walk recursion level so
/// arbitrarily deep render trees cannot overflow the fixed OS stack
/// (the Windows main thread gets 1 MiB by default; a ~1000-level
/// single-child chain blew it in the `layout/deep/1000` bench, and a
/// production tree of that depth would crash the app identically).
///
/// Same discipline as rustc's `ensure_sufficient_stack`: when fewer
/// than the red-zone bytes remain, the continuation runs on a fresh
/// heap-allocated stack segment.  Cost on the hot path is one
/// stack-pointer probe per recursion level (sub-ns next to the
/// per-node layout/paint work).
///
/// Falls back to a direct call under miri (psm's stack-switching
/// assembly cannot be interpreted) and on wasm32 (no stack switching;
/// the dependency is compiled out in Cargo.toml) — those environments
/// keep plain recursion and its pre-existing depth limits.
#[inline]
pub(super) fn ensure_stack<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(any(miri, target_arch = "wasm32"))]
    {
        f()
    }
    #[cfg(not(any(miri, target_arch = "wasm32")))]
    {
        // 128 KiB red zone: covers the deepest single-level frame
        // chain between two probes (driver frame + typed ctx + the
        // render object's own perform_layout/paint locals).  2 MiB
        // segments amortize one allocation across many levels.
        stacker::maybe_grow(128 * 1024, 2 * 1024 * 1024, f)
    }
}

// ============================================================================
// Shared pieces of the walks
// ============================================================================

/// Builds the per-child parent-data slice for an intrinsic query of the current node's
/// children: a harness seed (test/testing feature only) overlays the copy of each child's
/// committed parent data.
///
/// The returned owned `Box<dyn ParentData>` values coexist with the child-query closure that
/// follows, which enters the children again. A child entered up the stack (a cyclic
/// `children()` edge) gets `None`: the cycle already degrades it to a `LayoutCycle` stand-in,
/// and no parent data matches that geometry.
fn build_intrinsic_child_parent_data(
    arena: &SubtreeArena<'_>,
    child_ids: &[RenderId],
) -> Vec<Option<Box<dyn ParentData>>> {
    child_ids
        .iter()
        .map(|&child_id| {
            let mut seed = None;
            if arena.seed_child_parent_data(child_id, &mut seed) {
                return seed;
            }
            let entered = arena.lease.enter(child_id).ok()?;
            entered.node().parent_data().map(dyn_clone::clone_box)
        })
        .collect()
}

/// What a parent frame's layout callbacks do with a child walk's result: the callbacks must hand
/// `perform_layout` a value, so a child's failure becomes a stand-in here, recorded against the
/// child's retry budget (layout poison) and remembered so the parent stays `NEEDS_LAYOUT`.
#[derive(Clone, Copy)]
struct ChildOutcomes<'a, 'tree> {
    arena: &'a SubtreeArena<'tree>,
    /// The node whose `perform_layout` invoked the callback.
    parent: RenderId,
    /// Set on any descendant failure this frame contained.
    descendant_failed: &'a Cell<bool>,
}

impl ChildOutcomes<'_, '_> {
    /// Passes a success on (and notes it for the poison table), returns a recoverable error
    /// unchanged, and swaps any other failure for `stand_in`, logging `report`. `attempt` is the
    /// constraints of the failed layout, `None` for an intrinsic query.
    fn settle<V>(
        self,
        child: RenderId,
        attempt: Option<ErasedConstraints>,
        outcome: crate::error::RenderResult<V>,
        stand_in: V,
        report: &'static str,
    ) -> crate::error::RenderResult<V> {
        match outcome {
            Ok(value) => {
                self.arena.note_layout_success(child);
                Ok(value)
            }
            Err(err) if err.is_recoverable_layout_error() => Err(err),
            Err(err) => {
                self.arena
                    .note_layout_failure(self.parent, child, &err, attempt);
                self.descendant_failed.set(true);
                tracing::error!(
                    parent = self.parent.as_u64(),
                    child_id = ?child,
                    ?err,
                    "{report}",
                );
                Ok(stand_in)
            }
        }
    }
}

/// Logs a panic `perform_layout` raised and turns it into [`crate::error::RenderError::Poisoned`]
/// for the node. The report runs tracing subscribers, so it is contained too.
fn poisoned_by_panic(
    kind: &'static str,
    payload: Box<dyn std::any::Any + Send>,
    report: &'static str,
) -> crate::error::RenderError {
    let msg = payload_text(&*payload).unwrap_or("(non-string panic payload)");
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a panic report runs tracing subscribers, contained"
    )]
    if let Err(reporting_payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tracing::error!(render_object = kind, panic_msg = msg, "{report}");
    })) {
        // Reporting is secondary; do not retire its opaque failure or report it again.
        flui_foundation::panic::discard_payload(reporting_payload);
    }
    flui_foundation::panic::discard_payload(payload);
    crate::error::RenderError::poisoned(kind, crate::error::PoisonPhase::Layout)
}

// ============================================================================
// Walk 1: Box layout
// ============================================================================

/// Recursive helper for `PipelineOwner::layout_dirty_root`.
///
/// Enters `id` as a [`LayoutFrame`], drives `perform_layout_raw` against a typed
/// `RawBoxLayoutContext`, and recurses through callbacks that capture `&SubtreeArena`. The
/// callbacks run to completion on this thread, synchronously, inside this frame.
fn layout_subtree_borrowed(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    constraints: BoxConstraints,
) -> crate::error::RenderResult<flui_foundation::geometry::Size> {
    ensure_stack(|| layout_subtree_borrowed_impl(arena, id, constraints))
}

/// Why a node with `NEEDS_LAYOUT` clear did not serve its cached geometry.
///
/// Three genuinely different states reach that point and only the third is a
/// defect. They were reported as one — every miss logged "invariant
/// violation" — which is not a cosmetic problem: an animated demo logged it
/// twice per frame for a whole run and it was recorded as a suspected
/// rendering defect while the run's actual defect was frame pacing
/// (ADR-0353). A named classification keeps the distinction where a reader
/// and a test can both see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheMiss {
    /// No constraints have ever been cached: the first pass over a freshly
    /// mounted node. Benign.
    FirstLayout,
    /// Constraints are cached but differ from the incoming ones — a resize,
    /// a viewport change, a parent laying its child out differently. The
    /// cache refusing to serve here IS the invalidation path working.
    /// Benign.
    ConstraintsChanged,
    /// Constraints are cached AND match, the node is clean, and geometry is
    /// still absent. Nothing may clear one without the other, so this is a
    /// real invariant violation — the only one of the three.
    GeometryClearedUnderMatchingConstraints,
}

/// Classifies a cache miss on a clean node. Pure so the distinction is
/// testable without driving a layout walk that can only produce two of the
/// three states through its public API.
const fn classify_cache_miss(has_cached_constraints: bool, constraints_match: bool) -> CacheMiss {
    match (has_cached_constraints, constraints_match) {
        (false, _) => CacheMiss::FirstLayout,
        (true, false) => CacheMiss::ConstraintsChanged,
        (true, true) => CacheMiss::GeometryClearedUnderMatchingConstraints,
    }
}

/// Body of [`layout_subtree_borrowed`]; split out so every recursion
/// level enters through the [`ensure_stack`] probe.
fn layout_subtree_borrowed_impl(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    constraints: BoxConstraints,
) -> crate::error::RenderResult<flui_foundation::geometry::Size> {
    // Entered first, before any read of the node: a cyclic edge is refused here as a
    // `LayoutCycle`. The frame drops on every exit, unwind included.
    let mut frame = LayoutFrame::enter(arena, id)?;

    // Layout-poison skip, keyed on what the poison was earned from:
    // - layout-earned poison under the SAME constraints as the failed
    //   attempt deterministically fails again — serve the stand-in (the
    //   node's last committed geometry, or `Size::ZERO` when it never
    //   succeeded, the same value the error-swallow path used before the
    //   poison mechanism existed);
    // - layout-earned poison under DIFFERENT constraints earns one fresh
    //   attempt: the inputs changed (e.g. an ancestor now hands valid
    //   constraints to a child poisoned by an invalid one), and skipping
    //   here would pin the stand-in even after the cause is gone.  The
    //   retry is registered in `poison_retries` so a success below is
    //   accepted as a recovery; on failure the walk's sink updates the
    //   entry's last-attempt constraints and the node is skipped under
    //   these from then on;
    // - intrinsic-earned poison (no constraints to compare) keeps the
    //   stand-in until an external invalidation lifts the poison — a
    //   layout success cannot vouch for the failed measurement.
    //
    // The skip returns `Ok`, NOT the recorded error: the poison decision
    // was already made and logged when the budget tripped, and the parent
    // still needs a geometry.
    if arena.layout_poison.is_poisoned(id) {
        if arena
            .layout_poison
            .serves_stand_in(id, ErasedConstraints::from(constraints))
        {
            // Serving a stand-in is a degradation: the caller acts on
            // geometry this pass did not compute.
            arena.note_degradation();
            return Ok(frame
                .node()
                .geometry_box()
                .unwrap_or(flui_foundation::geometry::Size::ZERO));
        }
        arena.poison_retries.borrow_mut().insert(id);
    }

    let (
        child_ids,
        needs_layout_flag,
        cached_geometry,
        constraints_match,
        has_cached_constraints,
        geometry_degraded,
        node_protocol,
        is_repaint_boundary,
    ) = {
        let node = frame.node();
        let node_protocol = node.protocol_name();
        let Some(entry) = node.as_box() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "Box",
            });
        };
        let children = entry.links().children();
        let mut child_ids = arena.buffers.child_ids.checkout(children.len());
        child_ids.extend_from_slice(children);
        let needs_layout_flag = entry.needs_layout();
        // Snapshot the cached geometry for the short-circuit check below;
        // no allocation — `Size` is `Copy`.
        // Split deliberately, not folded into one `Option`: a clean node
        // that misses the cache does so for one of three reasons, and only
        // the third is a defect (see the short-circuit below). Collapsing
        // them loses the distinction at the only point that can still tell
        // them apart.
        let constraints_match = entry.state().has_constraints(&constraints);
        let has_cached_constraints = entry.state().constraints().is_some();
        let cached_geometry: Option<flui_foundation::geometry::Size> =
            if needs_layout_flag || !constraints_match {
                None
            } else {
                entry.state().geometry()
            };
        let geometry_degraded = entry.state().geometry_degraded();
        // The short-circuit below serves exactly a cached geometry; a node
        // without one lays out, which the paint that follows reads to record
        // its chunks again (ADR-0304 §2).
        if cached_geometry.is_none() {
            entry.state().flags().mark_laid_out_since_paint();
        }
        // Read here, beside the other reads, so the record below costs a
        // branch rather than another node read.
        let is_repaint_boundary = entry.state().is_repaint_boundary();
        (
            child_ids,
            needs_layout_flag,
            cached_geometry,
            constraints_match,
            has_cached_constraints,
            geometry_degraded,
            node_protocol,
            is_repaint_boundary,
        )
    };
    let is_leaf = child_ids.is_empty();

    // Short-circuit clean children: if NEEDS_LAYOUT is not set AND
    // constraints match the cached value, skip layout entirely (early
    // return before recursing).
    if !needs_layout_flag {
        if let Some(geometry) = cached_geometry {
            // Serving a cache built by a degraded pass is itself a
            // degradation: the broken descendant is never walked on a cache
            // hit, so without this the pass above would look healthy.
            if geometry_degraded {
                arena.note_degradation();
            }
            return Ok(geometry);
        }
        // A clean node that did not hit the cache. Three different states
        // reach here and only one of them is a defect; warning about all
        // three is how a normal relayout gets reported as an invariant
        // violation. (It did: an animated demo logged this warning twice
        // per frame for the whole run, and the frame it was blamed for was
        // a pacing defect elsewhere entirely — ADR-0353.)
        match classify_cache_miss(has_cached_constraints, constraints_match) {
            CacheMiss::FirstLayout => {
                tracing::trace!(node_id = id.as_u64(), "first layout: no cached constraints");
            }
            CacheMiss::ConstraintsChanged => {
                tracing::trace!(node_id = id.as_u64(), "relayout: constraints changed");
            }
            CacheMiss::GeometryClearedUnderMatchingConstraints => {
                tracing::warn!(
                    node_id = id.as_u64(),
                    "layout short-circuit: clean node, matching constraints, but geometry was \
                     cleared without clearing them; proceeding with layout (invariant violation)"
                );
            }
        }
    }

    // Past the short-circuit: this node is being laid out for real. Recorded
    // here rather than at each `clear_needs_layout` because the leaf path
    // returns before those, and one entry point covers leaf and non-leaf
    // alike. An error later in the walk leaves the id recorded, which
    // over-queues that one node — the safe direction, and bounded to nodes
    // that genuinely entered layout.
    arena.note_laid_out();
    if is_repaint_boundary {
        arena.laid_out.borrow_mut().push(id);
    }

    // Seeded before `perform_layout` runs: each child state starts from the child's persisted
    // state. Box parents can host both Box and Sliver children, so the reads go through
    // `RenderNode`'s protocol-generic accessors.
    let mut child_states = LentChildren::new(
        arena,
        id,
        arena.buffers.box_children.checkout(child_ids.len()),
    );
    child_states.extend(
        child_ids
            .iter()
            .map(|&cid| crate::protocol::ErasedChildState::new(cid)),
    );
    child_states.seed(|cs, child| {
        cs.offset = child.offset();
        cs.needs_layout = child.needs_layout();
        cs.geometry_degraded = child.geometry_degraded();
        if let Some(sliver_entry) = child.as_sliver() {
            cs.sliver_constraints = sliver_entry.state().constraints().copied();
            cs.sliver_geometry = sliver_entry.state().geometry();
        }
    });

    let geometry = {
        let Some(entry) = frame.node_mut().as_box_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "Box",
            });
        };

        if is_leaf {
            return entry.layout_leaf_only(
                constraints,
                arena.text.source(id),
                arena.lease.parent(id).is_some(),
            );
        }

        // Set by the callbacks on any descendant failure; the commit below then leaves
        // NEEDS_LAYOUT set so the parent retries next frame.
        let descendant_error_flag = Cell::new(false);
        let outcomes = ChildOutcomes {
            arena,
            parent: id,
            descendant_failed: &descendant_error_flag,
        };

        let cb_owned =
            move |child_id: RenderId,
                  child_constraints: BoxConstraints|
                  -> crate::error::RenderResult<flui_foundation::geometry::Size> {
                outcomes.settle(
                    child_id,
                    Some(child_constraints.into()),
                    layout_subtree_borrowed(arena, child_id, child_constraints),
                    flui_foundation::geometry::Size::ZERO,
                    "layout_dirty_root: descendant layout failed; \
                 returning Size::ZERO to caller's perform_layout. \
                 The failure is recorded against the child's retry \
                 budget (layout poison).",
                )
            };
        let cb_ref: LayoutChildCallback<'_> = &cb_owned;

        // A forwarding box has no baseline of its own and cannot reach its child from
        // `actual_baseline_raw` (that query takes no context), so it flags the forward and the
        // walk continues here. A valid chain cannot visit more nodes than the lease holds, which
        // bounds a malformed cycle without truncating a deep valid tree. A node entered up the
        // stack (a cyclic edge back to this frame or an ancestor) ends the chain with no baseline.
        let baseline_cb_owned = move |child_id: RenderId, baseline: crate::traits::TextBaseline| {
            let mut node_id = child_id;
            let mut offset_y = 0.0;
            for hop in 0..arena.lease.capacity() {
                let Ok(entered) = arena.lease.enter(node_id) else {
                    return Ok(None);
                };
                let node = entered.node();
                let Some(entry) = node.as_box() else {
                    return Ok(None);
                };
                if hop != 0 {
                    offset_y += node.offset().dy;
                }
                if entry.render_object().forwards_baseline_to_only_child() {
                    // Walk on; a proxy with no child has no baseline.
                    let Some(&grandchild) = node.children().first() else {
                        return Ok(None);
                    };
                    node_id = grandchild;
                    continue;
                }
                return entry
                    .render_object()
                    .actual_baseline_raw(baseline)
                    .map(|baseline| {
                        baseline.and_then(|value| {
                            let shifted = value + offset_y;
                            shifted.is_finite().then_some(shifted)
                        })
                    });
            }
            Ok(None)
        };
        let baseline_cb_ref: ActualBaselineChildCallback<'_> = &baseline_cb_owned;

        // Invoked when the Box parent calls `ctx.layout_sliver_child(index, constraints)`.
        let sliver_cb_owned = move |child_id: RenderId,
                                    sliver_constraints: SliverConstraints|
              -> crate::error::RenderResult<SliverGeometry> {
            outcomes.settle(
                child_id,
                Some(sliver_constraints.into()),
                layout_sliver_subtree_borrowed(arena, child_id, sliver_constraints),
                SliverGeometry::ZERO,
                "layout_dirty_root: sliver descendant layout failed; \
                 returning SliverGeometry::ZERO to caller's perform_layout. \
                 The failure is recorded against the child's retry \
                 budget (layout poison).",
            )
        };
        let sliver_cb_ref: SliverLayoutChildCallback<'_> = &sliver_cb_owned;

        // Invoked when the Box parent calls `ctx.child_intrinsic(index, dimension, extent)` from
        // within `perform_layout` (e.g. `RenderIntrinsicWidth` / `RenderIntrinsicHeight`); the
        // twin of the sliver walk's box-intrinsic callback.
        let box_intrinsic_cb_owned = move |child_id: RenderId,
                                           dimension: crate::storage::IntrinsicDimension,
                                           extent: f64|
              -> crate::error::RenderResult<f64> {
            outcomes.settle(
                child_id,
                None,
                box_intrinsic_query_borrowed(arena, child_id, dimension, extent),
                0.0,
                "layout_dirty_root: box child intrinsic query failed; \
                 returning 0.0 to caller's perform_layout. \
                 The failure is recorded against the child's retry \
                 budget (layout poison).",
            )
        };
        let box_intrinsic_cb_ref: crate::protocol::BoxChildIntrinsicCallback<'_> =
            &box_intrinsic_cb_owned;

        // Construct the driver-side PARENT-DATA-ERASED context.  The walk
        // cannot name the parent's ParentData type (it holds dyn nodes);
        // the typed blanket bridge reconstructs RawBoxLayoutContext<T::Arity,
        // T::ParentData> per node and lazily creates each child's
        // parent-data slot with T::ParentData::default(), so Flex/Stack and
        // every other non-BoxParentData parent lay out in production.
        let mut ctx = crate::protocol::DriverBoxLayoutContext::new(
            constraints,
            &mut child_states,
            &child_ids,
            cb_ref,
            baseline_cb_ref,
            Some(sliver_cb_ref),
            Some(box_intrinsic_cb_ref),
            Some(crate::protocol::DegradationProbe::new(
                &arena.degradation_events,
            )),
            arena.text.source(id),
        );
        let erased: &mut dyn ErasedBoxLayoutContext = &mut ctx;

        // `perform_layout_raw` runs inside `catch_unwind` (symmetric with the leaf path's
        // `layout_leaf_only`), so a third-party panic surfaces as `RenderError::Poisoned` instead
        // of unwinding out of `layout_dirty_root`.
        let kind = entry.render_object().kind();
        // Every degradation counted from here to the end of this node's own
        // `perform_layout` happened inside this node's subtree (the walk is
        // depth-first and synchronous), which is what the flag below records.
        let events_before = arena.degradation_count();
        let render_object = entry.render_object_mut();
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: perform_layout runs contained; a panic poisons the node"
        )]
        let unwind_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_object.perform_layout_raw(erased)
        }));
        let geometry = match unwind_result {
            Ok(inner) => inner?,
            Err(payload) => {
                return Err(poisoned_by_panic(
                    kind,
                    payload,
                    "perform_layout panicked in non-leaf path — surfacing as \
                     RenderError::Poisoned (symmetric with leaf-path \
                     layout_leaf_only catch_unwind discipline)",
                ));
            }
        };

        // State update on success path.  On the Err/panic paths above we
        // return early so NEEDS_LAYOUT stays set for next-frame retry.
        //
        // Same protocol-generic geometry guards as layout_leaf_only.  Runtime
        // validation happens before state commit; the debug assertions are
        // debug-only contract checks.
        <BoxProtocol as Protocol>::validate_layout_output(kind, &constraints, &geometry)?;
        <BoxProtocol as Protocol>::debug_assert_layout_output(&constraints, &geometry);

        // Record whether this node's own pass read a stand-in, so a later
        // pass that serves the geometry it is about to commit cannot mistake
        // it for healthy.
        entry
            .state()
            .set_geometry_degraded(arena.degradation_count() > events_before);
        entry.state_mut().set_geometry(geometry);
        entry.state_mut().set_constraints(constraints);

        // Bootstrap the relayout boundary now that constraints are populated.
        let has_parent = arena.lease.parent(id).is_some();
        let sized_by_parent = entry.render_object().sized_by_parent();
        <BoxProtocol as Protocol>::bootstrap_relayout_boundary(
            entry.state(),
            sized_by_parent,
            has_parent,
        );

        // Only clear NEEDS_LAYOUT if the recursive callback observed no
        // descendant failure.  Preserves retry-next-frame semantics.
        if descendant_error_flag.get() {
            tracing::debug!(
                parent = id.as_u64(),
                "layout_dirty_root: a descendant errored during this walk; \
                 keeping parent NEEDS_LAYOUT set for next-frame retry"
            );
        } else {
            entry.state().clear_needs_layout();
        }
        geometry
    };

    // Commit the offsets `perform_layout` wrote via `position_child` into each child's persisted
    // `RenderState.offset`: the child states die with this frame, and paint and hit-test read the
    // persisted offset as the child's position. Only on the parent's success path: the error and
    // panic paths above returned, leaving state unmodified for the NEEDS_LAYOUT retry. A
    // descendant error does NOT skip it, since the parent's own positioning stands.
    child_states.commit_placement(&frame);

    Ok(geometry)
}

// ============================================================================
// Walk 2: Box intrinsic query
// ============================================================================

/// Recursive Box intrinsic query over the layout walk's lease.
///
/// Used when a parent needs a Box child's intrinsics during layout. It shares the layout walk's
/// [`SubtreeArena`] and its [`LayoutFrame`] instead of re-entering `PipelineOwner`.
fn box_intrinsic_query_borrowed(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f64,
) -> crate::error::RenderResult<f64> {
    ensure_stack(|| box_intrinsic_query_borrowed_impl(arena, id, dimension, extent))
}

/// Body of [`box_intrinsic_query_borrowed`].
fn box_intrinsic_query_borrowed_impl(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f64,
) -> crate::error::RenderResult<f64> {
    let mut frame = LayoutFrame::enter(arena, id)?;

    // Layout-poison skip: a node that exhausted its retry budget is not
    // re-measured until freshly invalidated. Its last-good cached value
    // for exactly this `(dimension, extent)` stands in — the intrinsic
    // cache is only written on success, so a hit is a real previously
    // computed answer; a miss falls back to 0.0, the same value the
    // error-swallow path used before the poison mechanism existed.
    if arena.layout_poison.is_poisoned(id) {
        let value = frame
            .node()
            .as_box()
            .and_then(|entry| {
                entry
                    .state()
                    .layout_cache()
                    .peek_intrinsic(dimension, extent)
            })
            .unwrap_or(0.0);
        return Ok(value);
    }

    let node = frame.node_mut();
    let node_protocol = node.protocol_name();
    let Some(entry) = node.as_box_mut() else {
        return Err(crate::error::RenderError::ProtocolMismatch {
            node_protocol,
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

    let children = entry.links().children();
    let mut child_ids = arena.buffers.child_ids.checkout(children.len());
    child_ids.extend_from_slice(children);

    let child_parent_data_owned = build_intrinsic_child_parent_data(arena, &child_ids);
    let child_parent_data_refs: Vec<Option<&dyn ParentData>> = child_parent_data_owned
        .iter()
        .map(|opt| opt.as_deref())
        .collect();

    let value = {
        let mut child_query = |index: usize, dim: crate::storage::IntrinsicDimension, ext: f64| {
            let Some(&child_id) = child_ids.get(index) else {
                let err = crate::error::RenderError::contract_violation(
                    "sliver box child intrinsic query",
                    "child index out of range for this node's children",
                );
                arena.note_layout_failure(id, id, &err, None);
                return Err(err);
            };
            match box_intrinsic_query_borrowed(arena, child_id, dim, ext) {
                Ok(value) => {
                    arena.note_layout_success(child_id);
                    Ok(value)
                }
                Err(err) => {
                    arena.note_layout_failure(id, child_id, &err, None);
                    Err(err)
                }
            }
        };
        let mut query = crate::context::BoxIntrinsicsContext::new(
            child_ids.len(),
            &child_parent_data_refs,
            &mut child_query,
            arena.text.source(id),
        );
        entry
            .render_object()
            .intrinsic_raw(dimension, extent, &mut query)
    }?;

    entry
        .state_mut()
        .layout_cache_mut()
        .insert_intrinsic(dimension, extent, value);
    Ok(value)
}

// ============================================================================
// Walk 3: Sliver layout
// ============================================================================
//
// `layout_sliver_subtree_borrowed` is the Sliver sibling of
// `layout_subtree_borrowed`.  It is called from the sliver-child callback
// inside `layout_subtree_borrowed_impl`'s non-leaf path when a Box parent
// calls `ctx.layout_sliver_child(index, constraints)`, and from its own
// child callback.
//
// The short-circuit clean-child optimisation from the Box path is omitted
// here on purpose: sliver constraints change with scroll position on every
// frame.  Omitting the check is correct and saves a branch.

/// Stack-probe wrapper for [`layout_sliver_subtree_borrowed_impl`].
fn layout_sliver_subtree_borrowed(
    arena: &SubtreeArena<'_>,
    id: flui_foundation::RenderId,
    constraints: SliverConstraints,
) -> crate::error::RenderResult<SliverGeometry> {
    ensure_stack(|| layout_sliver_subtree_borrowed_impl(arena, id, constraints))
}

/// Body of [`layout_sliver_subtree_borrowed`]; split out so every
/// recursion level enters through the [`ensure_stack`] probe.
fn layout_sliver_subtree_borrowed_impl(
    arena: &SubtreeArena<'_>,
    id: flui_foundation::RenderId,
    constraints: SliverConstraints,
) -> crate::error::RenderResult<SliverGeometry> {
    // Entered first, as in the Box walk.
    let mut frame = LayoutFrame::enter(arena, id)?;

    // Layout-poison skip: same contract as the Box walk — a poisoned
    // sliver is not re-laid out until freshly invalidated; its last
    // committed geometry stands in (`SliverGeometry::ZERO` when it never
    // succeeded).
    if arena.layout_poison.is_poisoned(id) {
        if arena
            .layout_poison
            .serves_stand_in(id, ErasedConstraints::from(constraints))
        {
            // Serving a stand-in is a degradation (see the box twin).
            arena.note_degradation();
            return Ok(frame
                .node()
                .geometry_sliver()
                .unwrap_or(SliverGeometry::ZERO));
        }
        arena.poison_retries.borrow_mut().insert(id);
    }

    let (child_ids, node_protocol, is_repaint_boundary) = {
        let node = frame.node();
        let node_protocol = node.protocol_name();
        let Some(entry) = node.as_sliver() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "Sliver",
            });
        };
        let children = entry.links().children();
        let mut child_ids = arena.buffers.child_ids.checkout(children.len());
        child_ids.extend_from_slice(children);
        let is_repaint_boundary = entry.state().is_repaint_boundary();
        // A sliver reached here lays out (see below), which the paint that
        // follows reads to record its chunks again (ADR-0304 §2).
        entry.state().flags().mark_laid_out_since_paint();
        (child_ids, node_protocol, is_repaint_boundary)
    };

    // A sliver reached here is being laid out: this walk has no short-circuit
    // (sliver constraints change with scroll position every frame, see the
    // note above `layout_sliver_subtree_borrowed`), so arriving IS the
    // condition the Box path has to test for.
    //
    // Recording here is not symmetry for its own sake. A custom `RenderSliver`
    // that is a repaint boundary re-lays out under a viewport relayout, and
    // the mark the viewport contributes walks UPWARD — it can never reach a
    // boundary below it. Without this, retained painting would reuse stale
    // sliver content.
    arena.note_laid_out();
    if is_repaint_boundary {
        arena.laid_out.borrow_mut().push(id);
    }

    // No early-return here for the empty case: a lazy sliver (e.g.
    // `RenderSliverList`) starts with zero attached children and must
    // call `request_child_build` on its first frame to schedule
    // initial requests via `pending_child_requests`.  The
    // `DriverSliverLayoutContext` path is always taken for slivers so that the
    // context's `pending_child_requests` / `pending_retain_bands` sinks are
    // wired in — even when the current child list is empty.
    //
    // Seeded before `perform_layout` runs, as in the Box walk. The lent parent data lets lazy
    // sliver consumers (e.g. `RenderSliverList`) read the logical index the element tree stamped
    // on insertion (`stamp_sliver_logical_index` in
    // `flui-view/src/element/behavior_commons.rs`) on a previous frame.
    let mut child_states = LentChildren::new(
        arena,
        id,
        arena.buffers.sliver_children.checkout(child_ids.len()),
    );
    child_states.extend(
        child_ids
            .iter()
            .map(|&cid| crate::protocol::ErasedSliverChildState::new(cid)),
    );
    child_states.seed(|cs, child| cs.offset = child.offset());

    let geometry = {
        let Some(entry) = frame.node_mut().as_sliver_mut() else {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "Sliver",
            });
        };

        let descendant_error_flag = Cell::new(false);
        let outcomes = ChildOutcomes {
            arena,
            parent: id,
            descendant_failed: &descendant_error_flag,
        };

        let cb_owned = move |child_id: RenderId,
                             child_constraints: SliverConstraints|
              -> crate::error::RenderResult<SliverGeometry> {
            outcomes.settle(
                child_id,
                Some(child_constraints.into()),
                layout_sliver_subtree_borrowed(arena, child_id, child_constraints),
                SliverGeometry::ZERO,
                "layout_dirty_root: sliver descendant layout failed; \
                 returning SliverGeometry::ZERO to caller's perform_layout. \
                 The failure is recorded against the child's retry \
                 budget (layout poison).",
            )
        };
        let cb_ref: SliverChildLayoutCallback<'_> = &cb_owned;

        let box_cb_owned =
            move |child_id: RenderId,
                  child_constraints: BoxConstraints|
                  -> crate::error::RenderResult<crate::protocol::BoxChildSize> {
                // Whether the walk below serves the child its stand-in instead
                // of laying it out: the same predicate its poisoned skip asks,
                // read before the call because the walk's failure sink may
                // change the record. A child laid out over a failed descendant
                // is not substituted; its size is the one it paints at.
                let substituted = arena
                    .layout_poison
                    .serves_stand_in(child_id, child_constraints.into());
                match layout_subtree_borrowed(arena, child_id, child_constraints) {
                    Ok(size) if substituted => Ok(crate::protocol::BoxChildSize::StandIn(size)),
                    outcome => outcomes.settle(
                        child_id,
                        Some(child_constraints.into()),
                        outcome.map(crate::protocol::BoxChildSize::Measured),
                        crate::protocol::BoxChildSize::StandIn(
                            flui_foundation::geometry::Size::ZERO,
                        ),
                        "layout_dirty_root: box descendant layout failed from sliver parent; \
                     returning Size::ZERO to caller's perform_layout. \
                     The failure is recorded against the child's retry \
                     budget (layout poison).",
                    ),
                }
            };
        let box_cb_ref: crate::protocol::ErasedBoxChildLayoutCallback<'_> = &box_cb_owned;

        let box_intrinsic_cb_owned = move |child_id: RenderId,
                                           dimension: crate::storage::IntrinsicDimension,
                                           extent: f64|
              -> crate::error::RenderResult<f64> {
            outcomes.settle(
                child_id,
                None,
                box_intrinsic_query_borrowed(arena, child_id, dimension, extent),
                0.0,
                "layout_dirty_root: box intrinsic query failed from sliver parent; \
                 returning 0.0 to caller's perform_layout. \
                 The failure is recorded against the child's retry \
                 budget (layout poison).",
            )
        };
        let box_intrinsic_cb_ref: crate::protocol::BoxChildIntrinsicCallback<'_> =
            &box_intrinsic_cb_owned;

        let mut ctx = crate::protocol::DriverSliverLayoutContext::new(
            constraints,
            &mut child_states,
            &child_ids,
            cb_ref,
            box_cb_ref,
            box_intrinsic_cb_ref,
            id,
            &arena.pending_child_requests,
            &arena.pending_retain_bands,
            Some(crate::protocol::DegradationProbe::new(
                &arena.degradation_events,
            )),
        );
        let erased: &mut dyn ErasedSliverLayoutContext = &mut ctx;

        let kind = entry.render_object().kind();
        // See the box twin: the delta over this call is this subtree's.
        let events_before = arena.degradation_count();
        let render_object = entry.render_object_mut();
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a sliver's perform_layout runs contained; a panic poisons the node"
        )]
        let unwind_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_object.perform_layout_raw(erased)
        }));
        let geometry = match unwind_result {
            Ok(inner) => inner?,
            Err(payload) => {
                return Err(poisoned_by_panic(
                    kind,
                    payload,
                    "perform_layout panicked in non-leaf sliver path — surfacing as \
                     RenderError::Poisoned",
                ));
            }
        };

        <SliverProtocol as Protocol>::validate_layout_output(kind, &constraints, &geometry)?;
        <SliverProtocol as Protocol>::debug_assert_layout_output(&constraints, &geometry);

        // See the box twin.
        entry
            .state()
            .set_geometry_degraded(arena.degradation_count() > events_before);
        entry.state_mut().set_geometry(geometry);
        entry.state_mut().set_constraints(constraints);

        let has_parent = arena.lease.parent(id).is_some();
        let sized_by_parent = entry.render_object().sized_by_parent();
        <SliverProtocol as Protocol>::bootstrap_relayout_boundary(
            entry.state(),
            sized_by_parent,
            has_parent,
        );

        if descendant_error_flag.get() {
            tracing::debug!(
                parent = id.as_u64(),
                "layout_dirty_root: a sliver descendant errored during this walk; \
                 keeping parent NEEDS_LAYOUT set for next-frame retry"
            );
        } else {
            entry.state().clear_needs_layout();
        }
        geometry
    };

    // Commit child paint offsets produced by sliver parents (`RenderSliverPadding` et al.) into
    // the parent-relative offset paint and hit-test consult; on the success path only, as in the
    // Box walk.
    child_states.commit_placement(&frame);

    Ok(geometry)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    mod query_walk {
        use flui_foundation::{Leaf, geometry::Size};

        use super::*;
        use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

        #[derive(Debug, Default)]
        struct LeafStub;
        impl flui_foundation::inspect::Inspect for LeafStub {
            fn kind(&self) -> &'static str {
                "LeafStub"
            }

            fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
        }
        impl RenderBox for LeafStub {
            type Arity = Leaf;
            type ParentData = BoxParentData;
            fn perform_layout(
                &mut self,
                _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
            ) -> crate::RenderResult<Size> {
                Ok(Size::new(10.0, 10.0))
            }
            fn paint(&self, _ctx: &mut crate::context::PaintContext<'_, Leaf>) {}
        }

        #[test]
        fn query_walk_refuses_a_second_entry_and_frees_the_node_on_drop() {
            let mut tree = RenderTree::new();
            let root = tree.insert_box(Box::new(LeafStub));
            let child = tree
                .insert_box_child(root, Box::new(LeafStub))
                .expect("BUG: the root is live");
            let stale = tree.insert_box(Box::new(LeafStub));
            tree.remove_shallow(stale);

            let walk = QueryWalk::new(&mut tree);
            let held = walk.enter(root).expect("BUG: first entry of a live node");
            assert!(held.node().children().contains(&child));
            assert!(matches!(walk.enter(root), Err(EnterRefusal::InFlight)));
            assert!(
                walk.child_parent_data(root).is_none(),
                "an entered node's parent data is not read"
            );
            assert!(matches!(walk.enter(stale), Err(EnterRefusal::Missing)));
            // A different node is enterable beside the held one.
            drop(walk.enter(child).expect("BUG: a distinct node"));

            drop(held);
            assert!(
                walk.enter(root).is_ok(),
                "dropping the entry clears the in-flight bit"
            );
        }
    }

    // The arena never leaves the thread that took the lease: `Scope::spawn` requires a `Send`
    // closure, so moving one is a compile error. Both types are private to this crate, so the
    // externally observable half of the same fact is a `compile_fail` doctest on
    // `LayoutChildCallback`'s doc comment in `protocol/box_protocol.rs` (the nearest public type
    // with the same never-`Send` shape); the entry's own pin is in `storage/node_slab.rs`.
    static_assertions::assert_not_impl_any!(SubtreeArena<'_>: Send, Sync);
    static_assertions::assert_not_impl_any!(LayoutFrame<'_, '_>: Send, Sync);
}
