//! Pre-acquired subtree borrow arena and recursive layout walks.
//!
//! This module owns **all** of the `unsafe` code for the re-entrant layout
//! walk.  Every export to `super` is a **safe** function or method; callers
//! in `owner/mod.rs` carry zero `unsafe` after this extraction.
//!
//! ## Design overview
//!
//! Before the layout walk the pipeline pre-acquires N disjoint
//! `&mut RenderNode` borrows for every node in the dirty subtree via
//! [`crate::storage::RenderTree::get_subtree_mut`].  [`SubtreeArena`] wraps
//! those borrows as raw [`NodePtr`] aliases and allows the recursive walk to
//! reborrow exactly one slot per call level — a different slot at each
//! recursion depth — without ever holding a `&mut RenderTree` inside a
//! callback that could itself call back into the tree.
//!
//! The aliasing invariant (distinct slots, one live `&mut` per slot at a
//! time) is enforced by:
//! 1. [`NodePtr`]'s `unsafe impl Send/Sync` satisfying the
//!    [`crate::protocol::box_protocol::LayoutChildCallback`] `Send+Sync`
//!    bound without making the type actually thread-safe for raw deref —
//!    thread-safety for deref is instead enforced by
//!    [`SubtreeArena::check_thread`] at every [`SubtreeArena::get`] call.
//! 2. [`LayoutCycleGuard`] detecting and rejecting re-entry into a slot
//!    whose `&mut` is already live up the call stack.
//! 3. [`SubtreeArena`]'s `PhantomData<&'tree mut ()>` tying the arena's
//!    lifetime to the source `&mut RenderTree` borrow, preventing any
//!    [`NodePtr`] from outliving the borrow window.
//!
//! ## Safety-gate note
//!
//! All three `unsafe fn` bodies in this file must be reviewed by
//! `unsafe-auditor` before the SAFETY-GATE passes.  The SAFETY comments
//! attached to each `unsafe` block document the invariant that makes the
//! operation sound; they are the primary artefact for the audit.

// The sanctioned `unsafe` island for the layout walk (see module docs above).
// The opt-out is scoped to this file; every block carries a `// SAFETY:`
// comment, and the invariants are machine-checked by the miri CI job /
// `just miri`, which runs exactly this module's tests.
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::RenderId;
use parking_lot::Mutex;
#[cfg(any(test, feature = "testing"))]
use rustc_hash::FxHashMap;

#[cfg(any(test, feature = "testing"))]
use crate::testing::parent_data::ParentDataSeed;

use crate::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    parent_data::ParentData,
    protocol::{
        BoxProtocol, Protocol, SliverProtocol,
        box_protocol::{
            ActualBaselineChildCallback, BoxLayoutCtxErased, LayoutChildCallback,
            SliverLayoutChildCallback,
        },
        sliver_protocol::{SliverChildLayoutCallback, SliverLayoutCtxErased},
    },
    storage::{RenderEntry, RenderNode, RenderTree},
};

use super::poison::{LayoutFailureKind, LayoutPoison};

// ============================================================================
// NodePtr — Send+Sync raw-pointer alias of a single &mut RenderNode borrow
// ============================================================================

/// `Send + Sync` raw-pointer alias of a single `&mut RenderNode` borrow
/// held in [`SubtreeArena`].
///
/// Each `NodePtr` in [`SubtreeArena::by_id`] is derived from one of
/// the N disjoint `&mut RenderNode` references returned by
/// [`RenderTree::get_subtree_mut`].  The pointer is stable for the
/// lifetime of the `SubtreeArena` instance because the underlying
/// `&mut RenderTree` is held by the caller (`PipelineOwner` while
/// `layout_dirty_root` runs) and the slab's slot allocation is
/// position-stable (no moves during the borrow window).
///
/// The wrapper is `Copy` so the layout-child closure can capture the
/// pointer by value without `Arc` ceremony.  `Send + Sync` is declared
/// because [`LayoutChildCallback`] inherits those bounds from
/// `BoxLayoutCtxErased: Send + Sync`.  Single-thread
/// access is enforced at the [`SubtreeArena::check_thread`] entry.
#[derive(Clone, Copy)]
struct NodePtr(*mut RenderNode);

// SAFETY: the raw pointer is just an address; the load-bearing borrow
// is the `&mut RenderNode` returned by `get_subtree_mut` that this
// pointer aliases.  Cross-thread reborrow is rejected by
// [`SubtreeArena::check_thread`] before any deref.
unsafe impl Send for NodePtr {}
// SAFETY: same as Send — the cross-thread deref guard lives in
// `SubtreeArena::check_thread`, not in the type itself.
unsafe impl Sync for NodePtr {}

// ============================================================================
// SubtreeArena — pre-acquired borrow pool (was SubtreeBorrows)
// ============================================================================

/// Pre-acquired set of N disjoint `&mut RenderNode` borrows on a
/// subtree, indexed by [`RenderId`] for O(1) lookup.
///
/// Replaces the prior `TreePtr` + recursive-tree-reborrow scheme that
/// surfaced as latent Stacked / Tree Borrows UB.  The new scheme acquires
/// ALL subtree `&mut RenderNode` borrows in ONE call to
/// [`RenderTree::get_subtree_mut`] (single `&mut Slab` reborrow scope),
/// stores raw aliases in this map, and lets the recursive walk reborrow
/// one slot at a time per call level.  No `&mut RenderTree` ever appears
/// inside the layout-child callback chain — eliminates the UB.
///
/// # Lifetime
///
/// `'tree` ties `SubtreeArena` to the source `&mut RenderTree`
/// borrow's lifetime via `PhantomData<&'tree mut ()>`.  Constructed via
/// [`Self::new`] from a `Vec<&'tree mut RenderNode>` (the output of
/// `get_subtree_mut`); the references are immediately converted to
/// raw pointers and aggregated by id.  The `&mut RenderTree` source
/// borrow keeps the slab's slots position-stable for the lifetime of
/// every aliased `NodePtr`.
///
/// # Thread affinity
///
/// `SubtreeArena` records the constructing thread's `ThreadId` and
/// checks it on every [`Self::get`] call.  The check survives even
/// though [`NodePtr`] declares `Send + Sync` — the auto-trait bound
/// is mechanically required to satisfy
/// `LayoutChildCallback: Send + Sync` (inherited from
/// `BoxLayoutCtxErased`), but at the call site we panic loudly on
/// cross-thread access instead of corrupting the slab silently.
/// Cheap: one `ThreadId::eq` per lookup.
pub(super) struct SubtreeArena<'tree> {
    /// Per-node raw-pointer index with a side in-flight flag.
    ///
    /// Each entry is `(NodePtr, AtomicBool)` where the `AtomicBool` is
    /// the cycle-detection in-flight marker for that node.  The map is
    /// structurally immutable after [`Self::new`] — only the atomic
    /// *values* change, via interior mutability through `&self`.
    ///
    /// Having the in-flight bit here (a *side* structure that is never
    /// reachable through a `NodePtr`) is the soundness invariant: reading
    /// the flag never touches a node whose `&mut` may be live on the
    /// call stack.  This is the same invariant the former separate
    /// `Mutex<FxHashSet>` provided, now without the lock.
    ///
    /// `AtomicBool` is `Sync`, so `SubtreeArena` stays `Send + Sync`
    /// with **no new `unsafe impl`**.  `Relaxed` ordering suffices
    /// because [`Self::check_thread`] already enforces single-thread
    /// access; no cross-thread synchronisation is needed.
    by_id: HashMap<RenderId, (NodePtr, AtomicBool)>,
    #[cfg(any(test, feature = "testing"))]
    parent_data_seeds: FxHashMap<RenderId, ParentDataSeed>,
    /// On-demand child builds requested during this walk that the frozen
    /// mid-pass borrows could not insert synchronously — the re-entrant
    /// build contract's v1 next-frame backend (ADR-0003 Decision 2).
    /// `layout_dirty_root` drains this into the deferred-mutation queue
    /// after the walk releases its borrows.  `Mutex` because the layout-
    /// child closure requires `&SubtreeArena: Send + Sync`.  Empty unless
    /// a lazy sliver requests a not-yet-built child.
    pending_builds: Mutex<Vec<crate::protocol::sliver_protocol::PendingBuild>>,
    /// Symmetric remove sink (U3c D2): `(parent, child)` pairs of children
    /// the consumer wants evicted from the tree.  The `parent` is the
    /// sliver's own `node_id` — **not** the walk root `id` passed to
    /// `layout_dirty_root`.  For a real `viewport → sliver → lazy →
    /// child` chain the walk root is the viewport, but `defer_remove` /
    /// `mark_needs_layout` must target the lazy sliver so it reflows after
    /// its child list changes.
    ///
    /// Drained before `pending_builds` in `layout_dirty_root`
    /// (Remove → Insert ordering, D3), post-drop of the subtree borrows,
    /// so no aliased `NodePtr` is live when the `defer_remove` calls touch
    /// `&mut self`.  `Mutex` for the same reason as `pending_builds`
    /// (the layout-child closure requires `&SubtreeArena: Send + Sync`).
    pending_removes: Mutex<Vec<(flui_foundation::RenderId, flui_foundation::RenderId)>>,
    /// Child-build requests from `RenderSliverList`: `(sliver_id,
    /// logical_index)` pairs recorded when an absent in-band child is
    /// encountered.  Unlike `pending_builds`, no render object is pre-built
    /// here — the element tree decides what to build.  Drained after
    /// `pending_builds` in `layout_dirty_root` and moved into
    /// `PipelineOwner::pending_child_requests` for the binding layer.  Same
    /// `Mutex` discipline.
    pending_child_requests: Mutex<Vec<(flui_foundation::RenderId, usize)>>,
    /// Retain-band signals from element-owned slivers.
    ///
    /// Each entry is `(sliver_id, cache_first, cache_last)` — the `[first,
    /// last)` band the sliver retained this pass.  Drained after the walk
    /// (after `pending_child_requests`, after `drop(arena)`) and moved into
    /// `PipelineOwner::pending_retain_bands` for the binding layer.  Same
    /// `Mutex` discipline as the other sinks.  Empty unless an element-owned
    /// sliver (`RenderSliverList`) completed layout this frame.
    pending_retain_bands: Mutex<Vec<(flui_foundation::RenderId, usize, usize)>>,
    /// Read-only view of the owner's layout-poison table for the whole
    /// walk.  Consulted at the top of each recursion level: a poisoned
    /// node is skipped (its last committed geometry stands in) instead of
    /// re-running a `perform_layout` that keeps failing.  Also gates the
    /// success sink below so a skip's stand-in `Ok` is never mistaken for
    /// a recovery.
    layout_poison: &'tree LayoutPoison,
    /// Descendant layout AND intrinsic-measurement failures swallowed by
    /// the layout-child / intrinsic-child callbacks during this walk:
    /// `(direct layout parent, failed node, kind)` triples.  The callbacks
    /// must return a geometry / measured value, so the typed error cannot
    /// propagate through `perform_layout`; this sink carries the failure
    /// identity to `layout_dirty_root`, which feeds the poison counters
    /// after the walk.  Same `Mutex` discipline as the other
    /// sinks.  Empty unless a descendant errored this walk.
    layout_failures: Mutex<
        Vec<(
            flui_foundation::RenderId,
            flui_foundation::RenderId,
            LayoutFailureKind,
        )>,
    >,
    /// Descendants whose own layout succeeded this walk AND which had open
    /// failure records — the only successes the poison table needs (a
    /// success clears the record).  Filtered at record time so the sink
    /// stays empty in the common all-healthy case.
    layout_successes: Mutex<Vec<flui_foundation::RenderId>>,
    owner_thread: std::thread::ThreadId,
    _lifetime: PhantomData<&'tree mut ()>,
}

impl<'tree> SubtreeArena<'tree> {
    /// Constructs a `SubtreeArena` from the output of
    /// [`RenderTree::collect_subtree_ids`] paired with the matching
    /// output of [`RenderTree::get_subtree_mut`].
    ///
    /// Precondition: `ids.len() == refs.len()` and each
    /// `ids[i]` corresponds to `refs[i]` (in order).  Caller must
    /// satisfy this — currently the only caller is
    /// [`super::PipelineOwner::layout_dirty_root`] which feeds the two
    /// methods' outputs directly to this constructor.
    pub(super) fn new(
        ids: &[RenderId],
        refs: Vec<&'tree mut RenderNode>,
        layout_poison: &'tree LayoutPoison,
        #[cfg(any(test, feature = "testing"))] all_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    ) -> Self {
        debug_assert_eq!(
            ids.len(),
            refs.len(),
            "SubtreeArena::new precondition violated: ids and refs \
             must have the same length",
        );
        let owner_thread = std::thread::current().id();
        let mut by_id = HashMap::with_capacity(ids.len());
        for (&id, r) in ids.iter().zip(refs) {
            // `AtomicBool::new(false)` — not in-flight at construction.
            by_id.insert(
                id,
                (
                    NodePtr(std::ptr::from_mut::<RenderNode>(r)),
                    AtomicBool::new(false),
                ),
            );
        }
        #[cfg(any(test, feature = "testing"))]
        let parent_data_seeds = ids
            .iter()
            .filter_map(|id| all_seeds.get(id).map(|seed| (*id, seed.clone())))
            .collect();
        Self {
            by_id,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds,
            pending_builds: Mutex::new(Vec::new()),
            pending_removes: Mutex::new(Vec::new()),
            pending_child_requests: Mutex::new(Vec::new()),
            pending_retain_bands: Mutex::new(Vec::new()),
            layout_poison,
            layout_failures: Mutex::new(Vec::new()),
            layout_successes: Mutex::new(Vec::new()),
            owner_thread,
            _lifetime: PhantomData,
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub(super) fn seed_child_parent_data(
        &self,
        child_id: RenderId,
        slot: &mut Option<Box<dyn crate::parent_data::ParentData>>,
    ) {
        if let Some(seed) = self.parent_data_seeds.get(&child_id) {
            *slot = Some(seed.to_box());
        }
    }

    /// Panics if the calling thread is not the constructing thread.
    /// Called by [`Self::get`] before returning any [`NodePtr`].
    #[inline]
    fn check_thread(&self) {
        let current = std::thread::current().id();
        assert!(
            current == self.owner_thread,
            "SubtreeArena accessed from non-owner thread: \
             owner = {:?}, current = {:?}. The layout walk \
             requires the layout_child callback to fire on the \
             same thread as PipelineOwner::layout_dirty_root \
             (the pipeline phase holds &mut self synchronously). \
             User RenderBox::perform_layout body must not spawn \
             ctx.layout_child(...) calls to other threads — the \
             underlying RenderTree slab is not Sync.",
            self.owner_thread,
            current,
        );
    }

    /// Returns the [`NodePtr`] for `id` if present, panicking
    /// (via [`Self::check_thread`]) on cross-thread access.
    #[inline]
    fn get(&self, id: RenderId) -> Option<NodePtr> {
        self.check_thread();
        self.by_id.get(&id).map(|(ptr, _)| *ptr)
    }

    /// Returns `true` if `id`'s in-flight flag is set, meaning its
    /// `&mut RenderNode` is live somewhere on the call stack above the
    /// current frame.
    ///
    /// Used by the child-offset commit (Phase 4) to skip writing to a slot
    /// whose Unique borrow tag is still live: a write to such a slot would
    /// invalidate the ancestor's `&mut` provenance under Stacked / Tree
    /// Borrows, producing UB even if the write is atomic.
    ///
    /// The check is only meaningful on the layout thread (same atomic flag
    /// set/cleared by [`LayoutCycleGuard`]).  Lock-free: reads the
    /// `AtomicBool` in the arena's `by_id` map — a *side* structure that
    /// never aliases any `NodePtr` slot.  `Relaxed` suffices because
    /// [`Self::check_thread`] enforces single-thread access.
    #[inline]
    fn is_in_flight(&self, id: RenderId) -> bool {
        self.by_id
            .get(&id)
            .is_some_and(|(_, flag)| flag.load(Ordering::Relaxed))
    }

    /// Takes the on-demand child builds recorded during this walk, leaving
    /// the sink empty.  Called by [`super::PipelineOwner::layout_dirty_root`]
    /// once the walk has returned, so the requests can be enqueued on the
    /// deferred-mutation queue (which needs `&mut PipelineOwner`).  Touches
    /// only the sink — never a [`NodePtr`] — so it does not interact with
    /// the raw-pointer aliasing.
    pub(super) fn take_pending_builds(
        &self,
    ) -> Vec<crate::protocol::sliver_protocol::PendingBuild> {
        std::mem::take(&mut *self.pending_builds.lock())
    }

    /// Takes the deferred child removals recorded during this walk.
    /// Returns `(parent, child)` pairs — the parent is the sliver's
    /// `node_id`, not the walk root — so `defer_remove` targets the correct
    /// ancestor.  Symmetric to [`Self::take_pending_builds`]; called in
    /// `layout_dirty_root` BEFORE `take_pending_builds` (Remove → Insert
    /// ordering, D3) and AFTER `drop(arena)` so no `NodePtr` alias is live
    /// when the removes are applied.
    pub(super) fn take_pending_removes(
        &self,
    ) -> Vec<(flui_foundation::RenderId, flui_foundation::RenderId)> {
        std::mem::take(&mut *self.pending_removes.lock())
    }

    /// Takes the child-build requests recorded by request-strategy slivers
    /// during this walk.  Returns `(sliver_id, logical_index)` pairs
    /// for the binding layer to service after the frame.  Called in
    /// `layout_dirty_root` AFTER `take_pending_builds` (Remove → Insert →
    /// Request ordering) and AFTER `drop(arena)` so no `NodePtr` alias is
    /// live.
    pub(super) fn take_pending_child_requests(&self) -> Vec<(flui_foundation::RenderId, usize)> {
        std::mem::take(&mut *self.pending_child_requests.lock())
    }

    /// Takes the retain-band signals recorded by element-owned slivers
    /// (`RenderSliverList`) during this walk.  Returns `(sliver_id,
    /// cache_first, cache_last)` triples — the `[first, last)` band each
    /// element-owned sliver retained this frame.  Called in `layout_dirty_root`
    /// AFTER `take_pending_child_requests` and AFTER `drop(arena)` so no
    /// `NodePtr` alias is live when the results are consumed.
    ///
    /// Symmetric to [`Self::take_pending_child_requests`].  The binding layer
    /// drives `SparseChildren::retain_band` from these entries.
    pub(super) fn take_pending_retain_bands(
        &self,
    ) -> Vec<(flui_foundation::RenderId, usize, usize)> {
        std::mem::take(&mut *self.pending_retain_bands.lock())
    }

    /// Records a descendant layout failure swallowed at a layout-child
    /// callback.  `parent` is the node whose `perform_layout` invoked the
    /// callback; `failed` is the direct child whose own layout returned
    /// `err`.  Read by `layout_dirty_root` after the walk to feed the
    /// poison counters.
    fn note_layout_failure(
        &self,
        parent: RenderId,
        failed: RenderId,
        err: &crate::error::RenderError,
    ) {
        self.layout_failures
            .lock()
            .push((parent, failed, LayoutFailureKind::of(err)));
    }

    /// Records a descendant layout success, but only for a node with open
    /// (not yet poisoned) failure records.  A poisoned node never reaches
    /// this point as a real layout — the poison skip returns its stand-in
    /// geometry through the same callback `Ok` arm — so filtering here is
    /// what keeps a skip from being misread as a recovery.
    fn note_layout_success(&self, id: RenderId) {
        if self.layout_poison.has_open_failures(id) {
            self.layout_successes.lock().push(id);
        }
    }

    /// Takes the descendant layout failures recorded during this walk:
    /// `(direct layout parent, failed node, kind)` triples.  Called by
    /// `layout_dirty_root` after the walk, alongside the other sinks.
    pub(super) fn take_layout_failures(
        &self,
    ) -> Vec<(
        flui_foundation::RenderId,
        flui_foundation::RenderId,
        LayoutFailureKind,
    )> {
        std::mem::take(&mut *self.layout_failures.lock())
    }

    /// Takes the descendant layout successes recorded during this walk
    /// (only nodes with open failure records; see
    /// [`Self::note_layout_success`]).  Called by `layout_dirty_root`
    /// after the walk.
    pub(super) fn take_layout_successes(&self) -> Vec<flui_foundation::RenderId> {
        std::mem::take(&mut *self.layout_successes.lock())
    }

    // =========================================================================
    // Safe public API — callers in owner/mod.rs carry zero `unsafe`
    // =========================================================================

    /// Lays out the box node identified by `id` under `constraints`,
    /// recursively walking its subtree via the pre-acquired borrow pool.
    ///
    /// Wraps the private [`layout_subtree_borrowed`] unsafe walk.
    /// The `unsafe` keyword is confined to this module; callers see only
    /// a safe `Result`-returning method.
    pub(super) fn layout_child(
        &self,
        id: RenderId,
        constraints: BoxConstraints,
    ) -> crate::error::RenderResult<flui_types::Size> {
        // SAFETY: `self` is alive for the entire duration of this call
        // and all recursive calls it triggers.  Each recursive level
        // reborrows a DISTINCT slab slot (parent ≠ child enforced by tree
        // acyclicity + LayoutCycleGuard).  No concurrent reborrows of the
        // same NodePtr exist.
        unsafe { layout_subtree_borrowed(self, id, constraints) }
    }

    /// Lays out the sliver node identified by `id` under `constraints`.
    ///
    /// Wraps the private [`layout_sliver_subtree_borrowed`] unsafe walk.
    /// Called from the sliver-child callback inside `layout_subtree_borrowed_impl`
    /// (Box parent → Sliver child cross-protocol path) — not called directly
    /// from `owner/mod.rs`.
    #[allow(dead_code)]
    pub(super) fn layout_sliver_child(
        &self,
        id: RenderId,
        constraints: SliverConstraints,
    ) -> crate::error::RenderResult<SliverGeometry> {
        // SAFETY: identical contract as `layout_child` — distinct slot
        // per recursion level, LayoutCycleGuard rejects re-entry, arena
        // is alive for the full call chain.
        unsafe { layout_sliver_subtree_borrowed(self, id, constraints) }
    }

    /// Queries the box intrinsic dimension of `id`, using the pre-acquired
    /// borrow pool so no fresh `&mut RenderTree` is needed.
    ///
    /// Wraps the private [`box_intrinsic_query_borrowed`] unsafe walk.
    /// Called from the box-intrinsic callback inside `layout_sliver_subtree_borrowed_impl`
    /// (Sliver parent querying Box child intrinsics) — not called directly from
    /// `owner/mod.rs`, so the dead_code lint fires through `dyn Fn` indirection.
    #[allow(dead_code)]
    pub(super) fn box_intrinsic(
        &self,
        id: RenderId,
        dimension: crate::storage::IntrinsicDimension,
        extent: f32,
    ) -> crate::error::RenderResult<f32> {
        // SAFETY: identical contract as `layout_child`.
        unsafe { box_intrinsic_query_borrowed(self, id, dimension, extent) }
    }

    /// Constructs a `SubtreeArena` from a `RenderTree` for the subtree
    /// rooted at `id`, returning `Err(NodeNotFound)` if the subtree is
    /// empty or a slot disappeared between collect and acquire.
    ///
    /// This is the one-stop safe constructor used by
    /// [`super::PipelineOwner::layout_dirty_root`].  `layout_poison` is the
    /// owner's poison table, borrowed for exactly the walk's lifetime so
    /// each recursion level can skip poisoned nodes.
    pub(super) fn from_tree(
        render_tree: &'tree mut RenderTree,
        id: RenderId,
        layout_poison: &'tree LayoutPoison,
        #[cfg(any(test, feature = "testing"))] all_seeds: &FxHashMap<RenderId, ParentDataSeed>,
    ) -> crate::error::RenderResult<Self> {
        let subtree_ids = render_tree.collect_subtree_ids(id);
        if subtree_ids.is_empty() {
            return Err(crate::error::RenderError::NodeNotFound(id));
        }
        let node_refs = render_tree
            .get_subtree_mut(&subtree_ids)
            .ok_or(crate::error::RenderError::NodeNotFound(id))?;
        Ok(Self::new(
            &subtree_ids,
            node_refs,
            layout_poison,
            #[cfg(any(test, feature = "testing"))]
            all_seeds,
        ))
    }
}

// ============================================================================
// RAII layout-cycle guard
// ============================================================================

/// RAII guard that sets `id`'s in-flight flag in [`SubtreeArena::by_id`]
/// on construction and clears it on drop.
///
/// Detects re-entry into a node's `layout_subtree_borrowed` call (the
/// situation where a user `perform_layout` body calls `ctx.layout_child`
/// for an ancestor id
/// whose layout is already in flight up the stack).  On collision the
/// constructor returns [`crate::error::RenderError::LayoutCycle`]
/// instead of attempting a second [`NodePtr`] reborrow (which would be
/// UB under aliasing rules — the same slot's Unique tag is live up the
/// recursion stack).
///
/// The guard's `Drop` impl unconditionally clears the in-flight flag,
/// even on unwind (Rust's drop semantics guarantee this for any
/// `Drop`-implementing value going out of scope).  Combined with the
/// `catch_unwind` wrapper around `perform_layout_raw` in the non-leaf
/// path, this means the in-flight state stays consistent across frames:
/// a panicking widget's id is cleared, the next frame's walk does not
/// see it as in-flight.
struct LayoutCycleGuard<'arena, 'tree> {
    arena: &'arena SubtreeArena<'tree>,
    id: RenderId,
}

impl<'arena, 'tree> LayoutCycleGuard<'arena, 'tree> {
    /// Registers `id` as currently-laying-out by atomically setting its
    /// in-flight flag.  Returns `Err(RenderError::LayoutCycle(id))` if
    /// the flag was already set (id already in flight at a parent call
    /// level) — caller must propagate immediately.
    ///
    /// Uses `swap(true, Relaxed)`: if the old value was `true` the node
    /// was already in-flight (cycle); if `false` we atomically claim it.
    /// Semantics are identical to the former `set.insert(id)` check —
    /// re-entry is rejected, first entry proceeds.  No lock is taken.
    fn enter(arena: &'arena SubtreeArena<'tree>, id: RenderId) -> crate::error::RenderResult<Self> {
        // check_thread here so the diagnostic surfaces at the cycle-
        // guard layer too (covers callers that bypass `get`).
        arena.check_thread();
        // Look up the side flag for `id`.  An id that is not in `by_id`
        // cannot be in-flight (it was never seeded into the arena), so
        // we treat it as not-in-flight and let the subsequent `get` call
        // produce `NodeNotFound` — matching the former HashSet behaviour
        // where an absent id was not in the set.
        let Some((_, flag)) = arena.by_id.get(&id) else {
            return Ok(Self { arena, id });
        };
        // swap returns the *previous* value.  If it was already `true`
        // the node is in-flight → cycle.  If it was `false` we just
        // claimed it.
        if flag.swap(true, Ordering::Relaxed) {
            // Debug-level: the layout-child callback in
            // `layout_subtree_borrowed` already logs the propagated
            // Err at tracing::error when it collapses descendant Err
            // to Size::ZERO.  Logging here at error too would produce
            // 2 log lines per cycle event.  The API-boundary error log is the
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
        Ok(Self { arena, id })
    }
}

impl Drop for LayoutCycleGuard<'_, '_> {
    fn drop(&mut self) {
        // Unconditional clear — runs on every exit path including unwind.
        // The in-flight flag stays consistent for the next frame.
        // If the id is not in `by_id` (enter's early-return path for
        // unknown ids), the store is a no-op.
        if let Some((_, flag)) = self.arena.by_id.get(&self.id) {
            flag.store(false, Ordering::Relaxed);
        }
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
// Testing helpers
// ============================================================================

/// Builds the per-child parent-data slice for the current node's children,
/// reading from the [`SubtreeArena`] (production) with harness seed overlay
/// (test/testing feature only).
///
/// The returned vec of owned `Box<dyn ParentData>` can coexist with the
/// mutable child-query closure that follows because the slice is fully
/// constructed from read-only accesses before any recursion descends.
///
/// # Safety
///
/// Each shared `&RenderNode` this derives must not alias a live
/// `&mut RenderNode` for the same slot. `links().children()` lists are NOT
/// statically acyclic — a cyclic edge whose child is also an in-flight ancestor
/// is a reachable input (`LayoutCycleGuard`, `tests/layout_cycle_guard.rs`) — so
/// this function gates every deref on [`SubtreeArena::is_in_flight`] and yields
/// `None` for any in-flight slot, exactly like the canonical position/offset
/// sites in this file (the `set_offset` loop). The remaining preconditions:
/// - No other arena walk may run concurrently; `arena.get()` enforces the
///   single-thread check (`check_thread`) at every call.
/// - The arena pointer is allocation-stable for the arena's lifetime
///   (pre-acquired slab; slots are not moved during the borrow window).
/// - This must be called BEFORE the child-recursion closure is created, so no
///   *non-cyclic* child slot has been entered yet either.
unsafe fn build_intrinsic_child_parent_data(
    arena: &SubtreeArena<'_>,
    child_ids: &[RenderId],
    #[cfg(any(test, feature = "testing"))] seeds: &FxHashMap<RenderId, ParentDataSeed>,
    #[cfg(not(any(test, feature = "testing")))] _seeds: &(),
) -> Vec<Option<Box<dyn ParentData>>> {
    child_ids
        .iter()
        .map(|child_id| {
            // Harness seeds overlay production parent data so headless tests
            // can provide widget-level configuration without an element tree.
            // (Seed reads never deref the arena, so they need no in-flight gate.)
            #[cfg(any(test, feature = "testing"))]
            if let Some(seed) = seeds.get(child_id) {
                return Some(seed.to_box());
            }
            // A cyclic children() edge can name a slot whose `&mut` is live on
            // an ancestor frame; a shared read of it would be aliasing UB
            // (Stacked/Tree Borrows). Skip it — its parent data is unobservable
            // this pass, which is correct: the cycle already degrades that child
            // to a LayoutCycle / Size::ZERO result, so a `None` (no flex/parent
            // data) entry matches the degraded geometry.
            if arena.is_in_flight(*child_id) {
                return None;
            }
            // Production: derive a shared borrow to read `parent_data` from
            // the child node's raw pointer.
            arena.get(*child_id).and_then(|NodePtr(ptr)| {
                // SAFETY: `child_id` is NOT in-flight (guard above), so no
                // ancestor frame holds a live `&mut` to this slot. The arena
                // pointer is allocation-stable (pre-acquired slab; slots are not
                // moved during the borrow window). The derived `&RenderNode` is
                // the only live borrow of the slot and feeds a read-only
                // `parent_data()` access.
                let child_node: &RenderNode = unsafe { &*ptr };
                child_node.parent_data().map(dyn_clone::clone_box)
            })
        })
        .collect()
}

// ============================================================================
// Unsafe walk 1: Box layout
// ============================================================================

/// Recursive helper for `PipelineOwner::layout_dirty_root`.
///
/// Reborrows one [`NodePtr`] from the pre-acquired [`SubtreeArena`]
/// at each call level, drives `perform_layout_raw` against a typed
/// `BoxLayoutCtx`, and recurses via a closure that captures
/// `&SubtreeArena` (Sync via [`NodePtr`]'s `unsafe impl`).  Distinct
/// call levels reborrow distinct slab slots (parent ≠ child) — no
/// aliasing.
///
/// # Safety
///
/// Caller must guarantee:
///
/// 1. `arena` is alive for the entire duration of this call AND
///    every recursive call this helper triggers via the callback.  The
///    [`super::PipelineOwner::layout_dirty_root`] flow constructs
///    `SubtreeArena` on the caller's stack and only invokes this
///    helper while the binding is live.
/// 2. At any moment, no two concurrent reborrows of the SAME
///    [`NodePtr`] exist.  Sequential call levels (parent → child →
///    grandchild) reborrow DIFFERENT slots — preserved by the
///    `LayoutCycleGuard` (returns
///    [`crate::error::RenderError::LayoutCycle`] on re-entry into
///    a slot already in flight up the stack).
unsafe fn layout_subtree_borrowed(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    constraints: BoxConstraints,
) -> crate::error::RenderResult<flui_types::Size> {
    ensure_stack(|| {
        // SAFETY: identical contract, forwarded verbatim from this
        // wrapper's own `# Safety` section; the stack-growth wrapper
        // only relocates which memory the frames live in, never their
        // borrow structure, lifetimes, or drop order.
        unsafe { layout_subtree_borrowed_impl(arena, id, constraints) }
    })
}

/// Body of [`layout_subtree_borrowed`]; split out so every recursion
/// level enters through the [`ensure_stack`] probe.
///
/// # Safety
///
/// Same contract as [`layout_subtree_borrowed`].
unsafe fn layout_subtree_borrowed_impl(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    constraints: BoxConstraints,
) -> crate::error::RenderResult<flui_types::Size> {
    // Cycle guard: set `id`'s in-flight flag FIRST — before any
    // NodePtr reborrow (shared or exclusive).  On a cyclic edge the
    // guard's `enter` returns Err(LayoutCycle) here, so the aliasing
    // shared-read that would otherwise fire on a cyclic child never
    // happens.  Drop on every exit path (RAII) — flag stays consistent
    // across panics via the catch_unwind in the non-leaf path below +
    // Rust's drop-on-unwind discipline.
    let _cycle_guard = LayoutCycleGuard::enter(arena, id)?;

    // Resolve id → NodePtr.  Cross-thread access panics inside `get`.
    let Some(NodePtr(node_ptr)) = arena.get(id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };

    // Layout-poison skip: a node that exhausted its retry budget (or
    // failed structurally) is not re-laid out until freshly invalidated —
    // its last committed geometry stands in (`Size::ZERO` when it never
    // succeeded, the same value the error-swallow path used before the
    // poison mechanism existed).  The walk returns `Ok` here, NOT the
    // recorded error: the poison decision was already made and logged
    // when the budget tripped, and the parent still needs a geometry.
    //
    // SAFETY: the cycle guard is held, so no `&mut` of this slot is live
    // on an ancestor frame (re-entry would have been rejected at
    // `LayoutCycleGuard::enter`).  The shared reborrow is the only live
    // borrow of the slot — the same invariant Phase 1's `parent_shared`
    // relies on below.
    if arena.layout_poison.is_poisoned(id) {
        let node: &RenderNode = unsafe { &*node_ptr };
        let geometry = node.geometry_box().unwrap_or(flui_types::Size::ZERO);
        return Ok(geometry);
    }

    // -----------------------------------------------------------------------
    // Phase 1 — shared reads of the parent slot (no &mut live).
    //
    // SAFETY: We hold the `LayoutCycleGuard` for `id`, which means any
    // recursive call that would re-enter this slot via `layout_child` is
    // rejected by `LayoutCycleGuard::enter` BEFORE it can take any borrow
    // of this slot.  The shared reborrow here is therefore the ONLY live
    // borrow of `id`'s slot at this point.  The scope is intentionally
    // narrow: `parent_shared` and everything derived from it must not be
    // used after the `&mut *node_ptr` reborrow opens Phase 2.
    // -----------------------------------------------------------------------
    let (child_ids, needs_layout_flag, cached_geometry, node_protocol, is_leaf) = {
        let parent_shared: &RenderNode = unsafe { &*node_ptr };
        let node_protocol = parent_shared.protocol_name();
        let entry: &RenderEntry<BoxProtocol> = match parent_shared.as_box() {
            Some(e) => e,
            None => {
                return Err(crate::error::RenderError::ProtocolMismatch {
                    node_protocol,
                    constraints_protocol: "Box",
                });
            }
        };
        let child_ids: Vec<RenderId> = entry.links().children().to_vec();
        let needs_layout_flag = entry.needs_layout();
        // Snapshot the cached geometry for the short-circuit check below;
        // no allocation — `Size` is `Copy`.
        let cached_geometry: Option<flui_types::Size> = if needs_layout_flag {
            None
        } else {
            entry
                .state()
                .has_constraints(&constraints)
                .then(|| entry.state().geometry())
                .flatten()
        };
        let is_leaf = child_ids.is_empty();
        (
            child_ids,
            needs_layout_flag,
            cached_geometry,
            node_protocol,
            is_leaf,
        )
        // `parent_shared` drops here — the shared borrow of `id`'s slot ends.
    };

    // Short-circuit clean children: if NEEDS_LAYOUT is not set AND
    // constraints match the cached value, skip layout entirely.
    // (Flutter rendering/object.dart:2852: early return before recurse)
    if !needs_layout_flag {
        if let Some(geometry) = cached_geometry {
            return Ok(geometry);
        }
        // Constraints matched but geometry was absent — invariant violation.
        // Fall through to a full layout pass with a warning.
        tracing::warn!(
            node_id = ?id,
            "layout short-circuit: clean constraints cache but missing geometry; \
             proceeding with layout (invariant violation)"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 1b — seed child states while NO &mut to the parent slot is live.
    //
    // Child-seeding reads MUST complete before the parent's &mut (Phase 2)
    // opens.  On a cyclic edge where a child id resolves to the same slot as
    // the parent, taking `&*child_ptr.0` while `&mut *node_ptr` is live would
    // alias the parent's Unique tag → UB under both Stacked Borrows and Tree
    // Borrows.  The `LayoutCycleGuard` guarantees re-entry into THIS frame's
    // `id` will fail at `LayoutCycleGuard::enter` (above), but a child whose
    // id happens to be `id` bypasses the guard's call-stack check if we read
    // the child AFTER opening the parent's &mut.  Seeding here, while only
    // the `LayoutCycleGuard` (not any &mut) is live, removes the race.
    // -----------------------------------------------------------------------
    let mut child_states: Vec<crate::protocol::ErasedChildState> = child_ids
        .iter()
        .map(|&cid| crate::protocol::ErasedChildState::new(cid))
        .collect();

    if !is_leaf {
        // Seed each ChildState from the child's persisted RenderState.
        // A parent that does not re-position a child during this walk must
        // preserve the child's prior offset (Flutter parity:
        // BoxParentData.offset persists until positionChild overwrites it).
        // Box parents can host both Box and Sliver children, so seed through
        // RenderNode's protocol-generic accessors.
        //
        // SOUNDNESS CONSTRAINT — in-flight skip:
        // If we are called as a recursive child (e.g. P1 → P2 and P2 is now
        // running), an ancestor's Phase 2+3 block may hold a live `&mut` over
        // a child id in our list (e.g. P2's child list contains P1, whose
        // `&mut` is live on P1's frame).  Reading that child's memory while
        // P1's Unique tag is live is a foreign read that can break P1's
        // provenance under Stacked / Tree Borrows.  We detect this via
        // `is_in_flight` (reads the `AtomicBool` in `by_id`) and skip the seed for
        // such children; their `ChildState` retains default values (zero
        // offset, None parent_data), which is safe since any positioning
        // decision for an in-flight ancestor that the callback yielded
        // `Size::ZERO` for would be discarded anyway.
        //
        // SAFETY: for children NOT in-flight, `id`'s slot carries no borrow
        // (Phase 1's `parent_shared` has dropped), and each `child_ptr`
        // addresses a DISTINCT slot from `id`'s slot.  No live &mut overlaps
        // the shared reads below.
        for cs in &mut child_states {
            // Skip children whose &mut is live on an ancestor frame.
            if arena.is_in_flight(cs.id) {
                continue;
            }
            if let Some(child_ptr) = arena.get(cs.id) {
                // SAFETY: child `cs.id` is NOT in-flight (guard above).
                let child_node: &RenderNode = unsafe { &*child_ptr.0 };
                cs.offset = child_node.offset();
                cs.needs_layout = child_node.needs_layout();
                // Seed parent data from the child's persistent RenderState.
                cs.parent_data = child_node.parent_data().map(dyn_clone::clone_box);
                if let Some(sliver_entry) = child_node.as_sliver() {
                    cs.sliver_constraints = sliver_entry.state().constraints().copied();
                    cs.sliver_geometry = sliver_entry.state().geometry();
                }
            }
            #[cfg(any(test, feature = "testing"))]
            arena.seed_child_parent_data(cs.id, &mut cs.parent_data);
        }
        // All shared child reads complete; no borrow of any slot is live.
    }

    // -----------------------------------------------------------------------
    // Phase 2 + 3 — exclusive reborrow of the parent slot for mutable work,
    // scoped so it ends before Phase 4.
    //
    // SAFETY: Phase 1 (`parent_shared`) and Phase 1b (child shared reads) have
    // both ended.  No other live borrow of `id`'s slot or of any child slot
    // exists.  The `LayoutCycleGuard` prevents a concurrent recursive entry
    // into `id`'s slot via `layout_child`, so the `&mut` below is the ONLY
    // live borrow of `id`'s slot.  Distinct slot reborrows (parent vs children,
    // opened by the recursive callbacks) have independent Unique tags under
    // Stacked / Tree Borrows — no aliasing.
    //
    // The closing brace of this block ends both `node_ref` and `entry`,
    // releasing the parent slot's Unique tag before Phase 4 opens shared
    // reads on child slots (which on a cyclic tree may alias `node_ptr`).
    // -----------------------------------------------------------------------
    let geometry = {
        let node_ref: &mut RenderNode = unsafe { &mut *node_ptr };

        let entry: &mut RenderEntry<BoxProtocol> = match node_ref.as_box_mut() {
            Some(e) => e,
            None => {
                // Protocol name already snapshotted in Phase 1.
                return Err(crate::error::RenderError::ProtocolMismatch {
                    node_protocol,
                    constraints_protocol: "Box",
                });
            }
        };

        // Leaf path: delegate to layout_leaf_only.
        if is_leaf {
            return entry.layout_leaf_only(constraints);
        }

        // Descendant-error tracking flag.  Closure flips to `true` on any
        // descendant `RenderError`; stage 6 below skips `clear_needs_layout`
        // when set so the parent stays dirty for next-frame retry.  Shared
        // via `Arc<AtomicBool>` because the closure is `Send + Sync`
        // (inherited from `LayoutChildCallback`'s bound).
        let descendant_error_flag: std::sync::Arc<std::sync::atomic::AtomicBool> =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descendant_error_for_cb = std::sync::Arc::clone(&descendant_error_flag);

        // Capture `&SubtreeArena` for the recursive callback.  `&T` is
        // `Send` iff `T: Sync`; `SubtreeArena: Sync` because its
        // `HashMap<RenderId, NodePtr>` is Sync (NodePtr declares Sync via
        // unsafe impl + RenderId is Sync) and the `PhantomData<&'tree mut ()>`
        // is Sync too.  So `&SubtreeArena: Send + Sync`, satisfying
        // `LayoutChildCallback: Send + Sync`.
        let arena_for_cb: &SubtreeArena<'_> = arena;
        let descendant_error_for_sliver_cb = std::sync::Arc::clone(&descendant_error_flag);
        let cb_owned = move |child_id: RenderId,
                             child_constraints: BoxConstraints|
              -> flui_types::Size {
            // SAFETY: `arena_for_cb` is alive (held by the outer
            // layout_dirty_root stack frame for the entire walk).  The
            // recursive reborrow happens on `child_id`'s slot — distinct
            // from the current `id`'s slot (LayoutCycleGuard rejects
            // re-entry into `id`).  No two concurrent reborrows of the
            // same NodePtr.
            match unsafe { layout_subtree_borrowed(arena_for_cb, child_id, child_constraints) } {
                Ok(size) => {
                    arena_for_cb.note_layout_success(child_id);
                    size
                }
                Err(err) => {
                    arena_for_cb.note_layout_failure(id, child_id, &err);
                    descendant_error_for_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                    tracing::error!(
                        parent = ?id,
                        ?child_id,
                        ?err,
                        "layout_dirty_root: descendant layout failed; \
                         returning Size::ZERO to caller's perform_layout. \
                         The failure is recorded against the child's retry \
                         budget (layout poison).",
                    );
                    flui_types::Size::ZERO
                }
            }
        };
        let cb_ref: LayoutChildCallback<'_> = &cb_owned;

        let baseline_cb_owned = move |child_id: RenderId, baseline: crate::traits::TextBaseline| {
            arena_for_cb.get(child_id).and_then(|child_ptr| {
                // SAFETY: shared reborrow of a distinct child slot after its
                // layout completed in this walk; no concurrent &mut to the slot.
                let child_node: &RenderNode = unsafe { &*child_ptr.0 };
                child_node
                    .as_box()
                    .and_then(|entry| entry.render_object().actual_baseline_raw(baseline))
            })
        };
        let baseline_cb_ref: ActualBaselineChildCallback<'_> = &baseline_cb_owned;

        // Sliver child callback: invoked when the Box parent calls
        // `ctx.layout_sliver_child(index, sliver_constraints)`.  Uses the
        // same `arena_for_cb` pool and `descendant_error_flag` as the box
        // callback — no extra pre-acquisition needed since sliver children
        // are already in the pre-acquired `SubtreeArena` set.
        let sliver_cb_owned =
            move |child_id: RenderId, sliver_constraints: SliverConstraints| -> SliverGeometry {
                // SAFETY: `arena_for_cb` is alive for the entire walk (held
                // by the `layout_dirty_root` stack frame).  The reborrow targets
                // `child_id`'s slot — distinct from the Box parent's slot
                // (LayoutCycleGuard blocks re-entry into `id`).  No two
                // concurrent reborrows of the same NodePtr.
                match unsafe {
                    layout_sliver_subtree_borrowed(arena_for_cb, child_id, sliver_constraints)
                } {
                    Ok(geometry) => {
                        arena_for_cb.note_layout_success(child_id);
                        geometry
                    }
                    Err(err) => {
                        arena_for_cb.note_layout_failure(id, child_id, &err);
                        descendant_error_for_sliver_cb
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        tracing::error!(
                            parent = ?id,
                            ?child_id,
                            ?err,
                            "layout_dirty_root: sliver descendant layout failed; \
                             returning SliverGeometry::ZERO to caller's perform_layout. \
                             The failure is recorded against the child's retry \
                             budget (layout poison).",
                        );
                        SliverGeometry::ZERO
                    }
                }
            };
        let sliver_cb_ref: SliverLayoutChildCallback<'_> = &sliver_cb_owned;

        // Box child intrinsic callback: invoked when the Box parent calls
        // `ctx.child_intrinsic(index, dimension, extent)` from within
        // `perform_layout` (e.g. `RenderIntrinsicWidth` / `RenderIntrinsicHeight`).
        // Uses the same `arena_for_cb` pool and `descendant_error_flag` as the
        // layout callback.  Mirrors the sliver→box intrinsic callback wired in
        // `layout_sliver_subtree_borrowed_impl` — no extra pre-acquisition needed
        // because all child slots are already in the pre-acquired `SubtreeArena`.
        let descendant_error_for_intrinsics_cb = std::sync::Arc::clone(&descendant_error_flag);
        let box_intrinsic_cb_owned = move |child_id: RenderId,
                                           dimension: crate::storage::IntrinsicDimension,
                                           extent: f32|
              -> f32 {
            // SAFETY: `arena_for_cb` is alive (held by the outer
            // layout_dirty_root stack frame for the entire walk).  The
            // query targets `child_id`'s slot — distinct from the current
            // Box parent's slot (`LayoutCycleGuard` rejects re-entry into
            // `id`).  No two concurrent reborrows of the same NodePtr.
            match unsafe { box_intrinsic_query_borrowed(arena_for_cb, child_id, dimension, extent) }
            {
                Ok(value) => {
                    arena_for_cb.note_layout_success(child_id);
                    value
                }
                Err(err) => {
                    arena_for_cb.note_layout_failure(id, child_id, &err);
                    descendant_error_for_intrinsics_cb
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    tracing::error!(
                        parent = ?id,
                        ?child_id,
                        ?err,
                        "layout_dirty_root: box child intrinsic query failed; \
                         returning 0.0 to caller's perform_layout. \
                         The failure is recorded against the child's retry \
                         budget (layout poison).",
                    );
                    0.0
                }
            }
        };
        let box_intrinsic_cb_ref: crate::protocol::box_protocol::BoxChildIntrinsicCallback<'_> =
            &box_intrinsic_cb_owned;

        // Construct the driver-side PARENT-DATA-ERASED context.  The walk
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
            baseline_cb_ref,
            Some(sliver_cb_ref),
            Some(box_intrinsic_cb_ref),
        );
        let erased: &mut dyn BoxLayoutCtxErased = &mut ctx;

        // Invoke perform_layout_raw wrapped in catch_unwind (symmetric
        // with the leaf path's layout_leaf_only — third-party panics
        // surface as RenderError::Poisoned instead of unwinding out of
        // layout_dirty_root).  Capture debug_name BEFORE the &mut reborrow.
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

        // State update on success path.  On the Err/panic paths above we
        // return early so NEEDS_LAYOUT stays set for next-frame retry.
        //
        // Same protocol-generic geometry guards as layout_leaf_only.  Runtime
        // validation happens before state commit; debug assertions mirror
        // Flutter's debug-only contract checks.
        <BoxProtocol as Protocol>::validate_layout_output(debug_name, &constraints, &geometry)?;
        <BoxProtocol as Protocol>::debug_assert_layout_output(&constraints, &geometry);

        entry.state_mut().set_geometry(geometry);
        entry.state_mut().set_constraints(constraints);

        // Bootstrap the relayout boundary now that constraints are populated.
        let has_parent = entry.links().parent().is_some();
        let sized_by_parent = entry.render_object().sized_by_parent();
        <BoxProtocol as Protocol>::bootstrap_relayout_boundary(
            entry.state(),
            sized_by_parent,
            has_parent,
        );

        // Only clear NEEDS_LAYOUT if the recursive callback observed no
        // descendant failure.  Preserves retry-next-frame semantics.
        let had_descendant_error = descendant_error_flag.load(std::sync::atomic::Ordering::Relaxed);
        if had_descendant_error {
            tracing::debug!(
                parent = ?id,
                "layout_dirty_root: a descendant errored during this walk; \
                 keeping parent NEEDS_LAYOUT set for next-frame retry"
            );
        } else {
            entry.clear_needs_layout();
        }

        // `entry`, `node_ref`, and all callbacks drop here.
        // The parent slot's Unique tag (`&mut *node_ptr`) is released.
        geometry
    };

    // -----------------------------------------------------------------------
    // Phase 4 — child-offset commit (no &mut to any slot is live AT THIS
    // CALL LEVEL).
    //
    // Commit the offsets perform_layout wrote via `position_child` into each
    // child's persisted `RenderState.offset`.  The `ChildState` vec is a
    // per-walk transient — without this commit every positioned offset dies
    // with the stack frame and paint / hit-test (which read
    // `RenderState.offset` as the authoritative child position) would place
    // all children at the parent origin.
    //
    // Runs only on the parent-success path: on the Err / panic paths above,
    // we return early so state stays unmodified and NEEDS_LAYOUT retry
    // semantics hold.  A descendant error does NOT skip the commit — the
    // parent's perform_layout returned Ok, so its positioning decisions are
    // valid regardless of a failed grandchild.
    //
    // SOUNDNESS CONSTRAINT — in-flight skip:
    // A child whose in-flight flag is set has its `&mut RenderNode`
    // live on the call stack of an ancestor `layout_subtree_borrowed_impl`
    // frame (held by that frame's Phase 2+3 block).  Writing to that slot's
    // memory (even via an atomic `set_offset`) while the ancestor's `&mut` is
    // live is a foreign write that invalidates the Unique provenance tag under
    // both Stacked Borrows and Tree Borrows — UB regardless of atomicity.
    // We therefore skip any child whose id is in-flight.  Semantically this
    // is correct: a node that is currently being laid out cannot be validly
    // positioned by a descendant that observed only a LayoutCycle error for it
    // (the descendant's positioning decision is based on Size::ZERO, not real
    // geometry); the next frame P1 re-lays-out and P2 re-positions it.
    //
    // SAFETY: For children NOT in flight, the Phase 2+3 block has ended and
    // their slots carry no live borrow from this call level.  `set_offset` is
    // an atomic store through `&self`.  The `&*child_ptr.0` shared reborrow is
    // the only live borrow of that child slot at this point.
    // -----------------------------------------------------------------------
    for cs in &child_states {
        // Skip slots whose &mut is live on an ancestor frame (see above).
        if arena.is_in_flight(cs.id) {
            continue;
        }
        if let Some(child_ptr) = arena.get(cs.id) {
            // SAFETY: child `cs.id` is NOT in-flight (guard above), so no
            // ancestor frame holds a live &mut to this slot.
            let child_node: &RenderNode = unsafe { &*child_ptr.0 };
            child_node.set_offset(cs.offset);
        }
    }

    Ok(geometry)
}

// ============================================================================
// Unsafe walk 2: Box intrinsic query
// ============================================================================

/// Recursive Box intrinsic query over the pre-acquired layout subtree.
///
/// Used when a Sliver parent needs Flutter-style Box child intrinsics during
/// layout.  It shares the layout walk's [`SubtreeArena`] pool instead of
/// re-entering `PipelineOwner`, preserving the same disjoint-slot discipline
/// as `layout_subtree_borrowed`.
unsafe fn box_intrinsic_query_borrowed(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
) -> crate::error::RenderResult<f32> {
    ensure_stack(|| {
        // SAFETY: forwarded from this wrapper; ensure_stack only changes stack
        // placement, not borrow lifetimes or aliasing.
        unsafe { box_intrinsic_query_borrowed_impl(arena, id, dimension, extent) }
    })
}

/// Body of [`box_intrinsic_query_borrowed`].
///
/// # Safety
///
/// Same contract as [`layout_subtree_borrowed`]: `arena` must outlive this
/// call and recursive child callbacks, and re-entry into an in-flight node is
/// rejected by [`LayoutCycleGuard`] before any second mutable reborrow occurs.
unsafe fn box_intrinsic_query_borrowed_impl(
    arena: &SubtreeArena<'_>,
    id: RenderId,
    dimension: crate::storage::IntrinsicDimension,
    extent: f32,
) -> crate::error::RenderResult<f32> {
    let _cycle_guard = LayoutCycleGuard::enter(arena, id)?;

    let Some(NodePtr(node_ptr)) = arena.get(id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };

    // Layout-poison skip: a node that exhausted its retry budget is not
    // re-measured until freshly invalidated. Its last-good cached value
    // for exactly this `(dimension, extent)` stands in — the intrinsic
    // cache is only written on success, so a hit is a real previously
    // computed answer; a miss falls back to 0.0, the same value the
    // error-swallow path used before the poison mechanism existed.
    //
    // SAFETY: the cycle guard is held, so no `&mut` of this slot is live
    // on an ancestor frame (re-entry would have been rejected at
    // `LayoutCycleGuard::enter`).  The shared reborrow is the only live
    // borrow of the slot.
    if arena.layout_poison.is_poisoned(id) {
        let node: &RenderNode = unsafe { &*node_ptr };
        let value = node
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

    // SAFETY: this is the only live reborrow of `id` in this intrinsic
    // query frame.  Recursive callbacks target child slots; cycle guard
    // rejects attempts to revisit an in-flight ancestor.
    let node_ref: &mut RenderNode = unsafe { &mut *node_ptr };
    let node_protocol = node_ref.protocol_name();
    let entry: &mut RenderEntry<BoxProtocol> = match node_ref.as_box_mut() {
        Some(e) => e,
        None => {
            return Err(crate::error::RenderError::ProtocolMismatch {
                node_protocol,
                constraints_protocol: "box",
            });
        }
    };

    if let Some(hit) = entry
        .state()
        .layout_cache()
        .peek_intrinsic(dimension, extent)
    {
        return Ok(hit);
    }

    let child_ids: Vec<RenderId> = entry.links().children().to_vec();

    // Build the per-child parent-data slice before creating the child-query
    // closure. The owned boxes coexist with the `&mut` the closure captures
    // because the slice is read-only and refers to different nodes.
    //
    // SAFETY: `child_ids` are the current node's children. The tree is not
    // statically acyclic — a cyclic edge can name an in-flight ancestor — so
    // `build_intrinsic_child_parent_data` gates each deref on `is_in_flight`
    // and skips any in-flight slot (see its `# Safety`). This call precedes the
    // child-query closure, so no non-cyclic child slot has been entered yet, and
    // `arena.get` enforces single-thread access. All its preconditions hold.
    let child_parent_data_owned: Vec<Option<Box<dyn ParentData>>> = unsafe {
        build_intrinsic_child_parent_data(
            arena,
            &child_ids,
            #[cfg(any(test, feature = "testing"))]
            &arena.parent_data_seeds,
            #[cfg(not(any(test, feature = "testing")))]
            &(),
        )
    };
    let child_parent_data_refs: Vec<Option<&dyn ParentData>> = child_parent_data_owned
        .iter()
        .map(|opt| opt.as_deref())
        .collect();

    let mut child_err: Option<crate::error::RenderError> = None;
    let value = {
        let child_err = &mut child_err;
        let mut child_query =
            |index: usize, dim: crate::storage::IntrinsicDimension, ext: f32| -> f32 {
                let Some(&child_id) = child_ids.get(index) else {
                    let err = crate::error::RenderError::contract_violation(
                        "sliver box child intrinsic query",
                        "child index out of range for this node's children",
                    );
                    arena.note_layout_failure(id, id, &err);
                    child_err.get_or_insert(err);
                    return 0.0;
                };
                // SAFETY: the child query targets a child slot distinct from the
                // current box node; the pre-acquired subtree arena is still live;
                // `LayoutCycleGuard` will reject re-entry into any ancestor slot.
                match unsafe { box_intrinsic_query_borrowed(arena, child_id, dim, ext) } {
                    Ok(value) => {
                        arena.note_layout_success(child_id);
                        value
                    }
                    Err(err) => {
                        arena.note_layout_failure(id, child_id, &err);
                        child_err.get_or_insert(err);
                        0.0
                    }
                }
            };
        entry.render_object().intrinsic_raw(
            dimension,
            extent,
            child_ids.len(),
            &child_parent_data_refs,
            &mut child_query,
        )
    };

    if let Some(err) = child_err {
        return Err(err);
    }

    entry
        .state_mut()
        .layout_cache_mut()
        .insert_intrinsic(dimension, extent, value);
    Ok(value)
}

// ============================================================================
// Unsafe walk 3: Sliver layout
// ============================================================================
//
// `layout_sliver_subtree_borrowed` is the Sliver sibling of
// `layout_subtree_borrowed`.  It is called from the `sliver_cb_owned`
// closure captured inside `layout_subtree_borrowed_impl`'s non-leaf path
// when a Box parent calls `ctx.layout_sliver_child(index, constraints)`.
//
// Scope: sliver subtrees.  Leaf nodes still delegate to
// `RenderEntry::layout_leaf_only`; non-leaf slivers get an erased driver
// context and may call `ctx.layout_child(...)` to lay out sliver children.
//
// The short-circuit clean-child optimisation from the Box path is omitted
// here on purpose: sliver constraints change with scroll position on every
// frame.  Omitting the check is correct and saves a branch.

/// Stack-probe wrapper for [`layout_sliver_subtree_borrowed_impl`].
///
/// # Safety
///
/// Same contract as [`layout_subtree_borrowed`]:
/// 1. `arena` must outlive every recursive invocation this helper
///    triggers (it is held by the outer `layout_dirty_root` stack frame
///    for the entire walk).
/// 2. At any moment, no two concurrent reborrows of the SAME [`NodePtr`]
///    exist.  The `LayoutCycleGuard` enforces this by returning
///    [`crate::error::RenderError::LayoutCycle`] on re-entry into a slot
///    already in flight, preventing the otherwise-UB second Unique tag.
unsafe fn layout_sliver_subtree_borrowed(
    arena: &SubtreeArena<'_>,
    id: flui_foundation::RenderId,
    constraints: SliverConstraints,
) -> crate::error::RenderResult<SliverGeometry> {
    ensure_stack(|| {
        // SAFETY: identical contract, forwarded verbatim from this
        // wrapper's own `# Safety` section; the stack-growth wrapper
        // only relocates which memory the frames live in, never their
        // borrow structure, lifetimes, or drop order.
        unsafe { layout_sliver_subtree_borrowed_impl(arena, id, constraints) }
    })
}

/// Body of [`layout_sliver_subtree_borrowed`]; split out so every
/// recursion level enters through the [`ensure_stack`] probe.
///
/// # Safety
///
/// Same contract as [`layout_sliver_subtree_borrowed`].
unsafe fn layout_sliver_subtree_borrowed_impl(
    arena: &SubtreeArena<'_>,
    id: flui_foundation::RenderId,
    constraints: SliverConstraints,
) -> crate::error::RenderResult<SliverGeometry> {
    // Cycle guard: set `id`'s in-flight flag FIRST — before any
    // NodePtr reborrow (shared or exclusive).  On a cyclic edge the
    // guard's `enter` returns Err(LayoutCycle) here so the aliasing
    // shared read that would otherwise fire never happens.
    // Drop runs on every exit including unwind so the flag stays consistent.
    let _cycle_guard = LayoutCycleGuard::enter(arena, id)?;

    // Resolve id → NodePtr.  Cross-thread access panics inside `get`.
    let Some(NodePtr(node_ptr)) = arena.get(id) else {
        return Err(crate::error::RenderError::NodeNotFound(id));
    };

    // Layout-poison skip: same contract as the Box walk — a poisoned
    // sliver is not re-laid out until freshly invalidated; its last
    // committed geometry stands in (`SliverGeometry::ZERO` when it never
    // succeeded).
    //
    // SAFETY: the cycle guard is held, so no `&mut` of this slot is live
    // on an ancestor frame; the shared reborrow is the only live borrow
    // of the slot.
    if arena.layout_poison.is_poisoned(id) {
        let node: &crate::storage::RenderNode = unsafe { &*node_ptr };
        let geometry = node.geometry_sliver().unwrap_or(SliverGeometry::ZERO);
        return Ok(geometry);
    }

    // -----------------------------------------------------------------------
    // Phase 1 — shared reads of the parent slot (no &mut live).
    //
    // SAFETY: `LayoutCycleGuard` for `id` is held; recursive re-entry into
    // this slot via `layout_sliver_child` / `layout_child` is rejected before
    // any borrow of this slot opens.  The narrow scope below is the ONLY live
    // borrow of `id`'s slot at this point.  Nothing derived from
    // `parent_shared` may be used after the `&mut *node_ptr` reborrow (Phase 2).
    // -----------------------------------------------------------------------
    let (child_ids, node_protocol) = {
        let parent_shared: &crate::storage::RenderNode = unsafe { &*node_ptr };
        let node_protocol = parent_shared.protocol_name();
        let entry: &RenderEntry<SliverProtocol> = match parent_shared.as_sliver() {
            Some(e) => e,
            None => {
                return Err(crate::error::RenderError::ProtocolMismatch {
                    node_protocol,
                    constraints_protocol: "Sliver",
                });
            }
        };
        let child_ids: Vec<RenderId> = entry.links().children().to_vec();
        (child_ids, node_protocol)
        // `parent_shared` drops here — the shared borrow of `id`'s slot ends.
    };

    // No early-return here for the empty case: a lazy sliver (e.g.
    // `RenderSliverListLazy`) starts with zero attached children and must
    // call `build_and_layout_box_child` on its first frame to schedule
    // initial builds via `pending_builds`.  The `ErasedSliverLayoutCtx`
    // path is always taken for slivers so that the context's
    // `pending_builds` / `pending_removes` sinks are wired in — even when
    // the current child list is empty.
    let mut child_states: Vec<crate::protocol::ErasedSliverChildState> = child_ids
        .iter()
        .map(|&cid| crate::protocol::ErasedSliverChildState::new(cid))
        .collect();

    // -----------------------------------------------------------------------
    // Phase 1b — seed child states while NO &mut to the parent slot is live.
    //
    // Symmetric with the Box path: child-seeding shared reads MUST complete
    // before the parent's &mut (Phase 2) opens.  On a cyclic edge where
    // child_id == id, `&*child_ptr.0` would alias the parent's Unique tag if
    // taken while `&mut *node_ptr` were live → UB under SB and TB.
    //
    // SOUNDNESS CONSTRAINT — in-flight skip (same contract as Box Phase 1b):
    // When this function is called recursively (e.g. P2 as a child of P1),
    // an ancestor frame's Phase 2+3 block may hold a live `&mut` over a child
    // id in our list.  Reading that child's memory while the ancestor's Unique
    // tag is live is UB.  We detect such ids via `is_in_flight` and skip them.
    //
    // Parent-data: `ErasedSliverChildState.parent_data` starts as `None`;
    // seed from persisted state so lazy sliver consumers (e.g.
    // `RenderSliverListLazy`) can read the logical index installed by
    // `apply_deferred_mutation` on the previous frame.
    // `ParentData: DynClone` makes this a cheap heap clone (one `Box` per
    // attached child; K is bounded by viewport/cache band, not by item count).
    //
    // SAFETY: for children NOT in-flight, Phase 1's `parent_shared` has been
    // dropped, each `child_ptr` addresses a DISTINCT slab slot from `id`'s
    // slot, and no live `&mut` covers those slots.
    // -----------------------------------------------------------------------
    for cs in &mut child_states {
        // Skip children whose &mut is live on an ancestor frame.
        if arena.is_in_flight(cs.id) {
            continue;
        }
        if let Some(child_ptr) = arena.get(cs.id) {
            // SAFETY: child `cs.id` is NOT in-flight (guard above).
            let child_node: &crate::storage::RenderNode = unsafe { &*child_ptr.0 };
            cs.offset = child_node.offset();
            if let Some(pd) = child_node.parent_data() {
                cs.parent_data = Some(dyn_clone::clone_box(pd));
            }
        }
        #[cfg(any(test, feature = "testing"))]
        arena.seed_child_parent_data(cs.id, &mut cs.parent_data);
    }
    // All shared child reads complete; no borrow of any slot is live.

    // -----------------------------------------------------------------------
    // Phase 2 + 3 — exclusive reborrow of the parent slot for mutable work,
    // scoped so it ends before Phase 4.
    //
    // SAFETY: Phase 1 and Phase 1b have both ended.  No live borrow of
    // `id`'s slot or of any child slot exists.  The `LayoutCycleGuard`
    // prevents concurrent re-entry into `id`'s slot via a layout callback.
    // The `&mut` below is the ONLY live borrow of `id`'s slot.  Distinct
    // child slots opened by recursive callbacks carry independent Unique tags.
    //
    // The closing brace of this block ends both `node_ref` and `entry`,
    // releasing the parent slot's Unique tag before Phase 4 opens shared
    // reads on child slots (which on a cyclic tree may alias `node_ptr`).
    // -----------------------------------------------------------------------
    let geometry = {
        let node_ref: &mut crate::storage::RenderNode = unsafe { &mut *node_ptr };

        let entry: &mut RenderEntry<SliverProtocol> = match node_ref.as_sliver_mut() {
            Some(e) => e,
            None => {
                return Err(crate::error::RenderError::ProtocolMismatch {
                    node_protocol,
                    constraints_protocol: "Sliver",
                });
            }
        };

        let descendant_error_flag: std::sync::Arc<std::sync::atomic::AtomicBool> =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let descendant_error_for_cb = std::sync::Arc::clone(&descendant_error_flag);
        let descendant_error_for_box_cb = std::sync::Arc::clone(&descendant_error_flag);
        let descendant_error_for_intrinsic_cb = std::sync::Arc::clone(&descendant_error_flag);
        let arena_for_cb: &SubtreeArena<'_> = arena;

        let cb_owned =
            move |child_id: RenderId, child_constraints: SliverConstraints| -> SliverGeometry {
                // SAFETY: `arena_for_cb` is alive for the whole dirty-root
                // walk, and this callback reborrows a child slot distinct from
                // the current sliver node.  `LayoutCycleGuard` rejects re-entry.
                match unsafe {
                    layout_sliver_subtree_borrowed(arena_for_cb, child_id, child_constraints)
                } {
                    Ok(geometry) => {
                        arena_for_cb.note_layout_success(child_id);
                        geometry
                    }
                    Err(err) => {
                        arena_for_cb.note_layout_failure(id, child_id, &err);
                        descendant_error_for_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                        tracing::error!(
                            parent = ?id,
                            ?child_id,
                            ?err,
                            "layout_dirty_root: sliver descendant layout failed; \
                             returning SliverGeometry::ZERO to caller's perform_layout. \
                             The failure is recorded against the child's retry \
                             budget (layout poison).",
                        );
                        SliverGeometry::ZERO
                    }
                }
            };
        let cb_ref: SliverChildLayoutCallback<'_> = &cb_owned;

        let box_cb_owned = move |child_id: RenderId,
                                 child_constraints: BoxConstraints|
              -> flui_types::Size {
            // SAFETY: same subtree-borrow contract as the sliver child
            // callback, but routed through the Box layout walk.
            match unsafe { layout_subtree_borrowed(arena_for_cb, child_id, child_constraints) } {
                Ok(size) => {
                    arena_for_cb.note_layout_success(child_id);
                    size
                }
                Err(err) => {
                    arena_for_cb.note_layout_failure(id, child_id, &err);
                    descendant_error_for_box_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                    tracing::error!(
                        parent = ?id,
                        ?child_id,
                        ?err,
                        "layout_dirty_root: box descendant layout failed from sliver parent; \
                         returning Size::ZERO to caller's perform_layout. \
                         The failure is recorded against the child's retry \
                         budget (layout poison).",
                    );
                    flui_types::Size::ZERO
                }
            }
        };
        let box_cb_ref: crate::protocol::sliver_protocol::BoxChildLayoutCallback<'_> =
            &box_cb_owned;

        let box_intrinsic_cb_owned = move |child_id: RenderId,
                                           dimension: crate::storage::IntrinsicDimension,
                                           extent: f32|
              -> f32 {
            // SAFETY: same subtree-borrow contract as the Sliver -> Box
            // layout callback, routed through the Box intrinsic bridge.
            match unsafe { box_intrinsic_query_borrowed(arena_for_cb, child_id, dimension, extent) }
            {
                Ok(value) => {
                    arena_for_cb.note_layout_success(child_id);
                    value
                }
                Err(err) => {
                    arena_for_cb.note_layout_failure(id, child_id, &err);
                    descendant_error_for_intrinsic_cb
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    tracing::error!(
                        parent = ?id,
                        ?child_id,
                        ?err,
                        "layout_dirty_root: box intrinsic query failed from sliver parent; \
                         returning 0.0 to caller's perform_layout. \
                         The failure is recorded against the child's retry \
                         budget (layout poison).",
                    );
                    0.0
                }
            }
        };
        let box_intrinsic_cb_ref: crate::protocol::sliver_protocol::BoxChildIntrinsicCallback<'_> =
            &box_intrinsic_cb_owned;

        let mut ctx = crate::protocol::ErasedSliverLayoutCtx::new(
            constraints,
            &mut child_states,
            &child_ids,
            cb_ref,
            box_cb_ref,
            box_intrinsic_cb_ref,
            id,
            &arena.pending_builds,
            &arena.pending_removes,
            &arena.pending_child_requests,
            &arena.pending_retain_bands,
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

        <SliverProtocol as Protocol>::validate_layout_output(debug_name, &constraints, &geometry)?;
        <SliverProtocol as Protocol>::debug_assert_layout_output(&constraints, &geometry);

        entry.state_mut().set_geometry(geometry);
        entry.state_mut().set_constraints(constraints);

        let has_parent = entry.links().parent().is_some();
        let sized_by_parent = entry.render_object().sized_by_parent();
        <SliverProtocol as Protocol>::bootstrap_relayout_boundary(
            entry.state(),
            sized_by_parent,
            has_parent,
        );

        if descendant_error_flag.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::debug!(
                parent = ?id,
                "layout_dirty_root: a sliver descendant errored during this walk; \
                 keeping parent NEEDS_LAYOUT set for next-frame retry"
            );
        } else {
            entry.clear_needs_layout();
        }

        // `entry`, `node_ref`, and all callbacks drop here.
        // The parent slot's Unique tag (`&mut *node_ptr`) is released.
        geometry
    };

    // -----------------------------------------------------------------------
    // Phase 4 — child-offset commit (no &mut to any slot is live AT THIS
    // CALL LEVEL).
    //
    // Commit child paint offsets produced by sliver parents
    // (`RenderSliverPadding` et al.) into the same parent-relative
    // offset slot that paint / hit-test already consult.  The
    // `ErasedSliverChildState` vec is per-walk transient; without
    // this commit, child placement dies with the layout stack frame.
    //
    // SOUNDNESS CONSTRAINT — in-flight skip: identical contract to the
    // Box path Phase 4 above.  A child whose in-flight flag is set has
    // its `&mut RenderNode` live on an ancestor frame's Phase 2+3 block.
    // Writing to that slot (even atomically) while the Unique tag is live is
    // UB under SB and TB.  We skip such children; their next-frame layout
    // will re-establish correct offsets.
    //
    // SAFETY: For children NOT in flight, the Phase 2+3 block has ended and
    // their slots carry no live borrow from this call level.  `set_offset` is
    // an atomic store through `&self`.
    // -----------------------------------------------------------------------
    for cs in &child_states {
        // Skip slots whose &mut is live on an ancestor frame.
        if arena.is_in_flight(cs.id) {
            continue;
        }
        if let Some(child_ptr) = arena.get(cs.id) {
            // SAFETY: child `cs.id` is NOT in-flight (guard above).
            let child_node: &crate::storage::RenderNode = unsafe { &*child_ptr.0 };
            child_node.set_offset(cs.offset);
        }
    }

    Ok(geometry)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `SubtreeArena::new` builds a correctly-indexed arena when
    /// given zero ids/refs (the empty-tree edge case) and that the pending
    /// sinks start empty.
    ///
    /// Concrete adversarial tests (re-entrant layout_child, LayoutCycleGuard
    /// rejection, cross-thread panic, pending-sink ordering) live in the
    /// integration test files under `tests/` where a full `PipelineOwner` is
    /// available: `tests/layout_dirty_root.rs` and `tests/layout_cycle_guard.rs`.
    #[test]
    fn new_with_zero_ids_produces_empty_arena() {
        // Exercise `SubtreeArena::new` with the matched-length empty case
        // (zero ids, zero refs).  Verifies the debug_assert does not fire
        // and the resulting arena is empty with drained pending sinks.
        let poison = LayoutPoison::default();
        let arena: SubtreeArena<'_> = SubtreeArena::new(
            &[],
            vec![],
            &poison,
            #[cfg(any(test, feature = "testing"))]
            &FxHashMap::default(),
        );
        assert!(arena.by_id.is_empty());
        assert!(arena.take_pending_builds().is_empty());
        assert!(arena.take_pending_removes().is_empty());
        assert!(arena.take_pending_child_requests().is_empty());
        assert!(arena.take_pending_retain_bands().is_empty());
        assert!(arena.take_layout_failures().is_empty());
        assert!(arena.take_layout_successes().is_empty());
    }

    /// Verify that `SubtreeArena::new` panics (debug_assert fires) when
    /// `ids.len() != refs.len()`.  This is the length-invariant violation
    /// the precondition documents.
    ///
    /// Only meaningful in debug builds (debug_assert is a no-op in release),
    /// so the test is gated with `#[cfg(debug_assertions)]`.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "ids and refs must have the same length")]
    fn new_panics_on_mismatched_ids_refs_length() {
        use flui_foundation::RenderId;
        // Two ids but zero refs — precondition violated.
        let id_a = RenderId::new(1);
        let id_b = RenderId::new(2);
        let poison = LayoutPoison::default();
        let _ = SubtreeArena::new(
            &[id_a, id_b],
            vec![], // wrong length
            &poison,
            #[cfg(any(test, feature = "testing"))]
            &FxHashMap::default(),
        );
    }

    /// Verify that `check_thread` panics when called from a different thread.
    ///
    /// This is gate (c) from the adversarial test spec in the plan.
    #[test]
    fn check_thread_panics_on_wrong_thread() {
        // Leaked so the arena may be moved into the spawned thread: the
        // thread-handle closure requires `'static`, and a test-scope leak
        // of an empty table is the simplest way to satisfy it.
        let poison: &'static LayoutPoison = Box::leak(Box::new(LayoutPoison::default()));
        let arena: SubtreeArena<'_> = SubtreeArena {
            by_id: HashMap::new(),
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_builds: Mutex::new(Vec::new()),
            pending_removes: Mutex::new(Vec::new()),
            pending_child_requests: Mutex::new(Vec::new()),
            pending_retain_bands: Mutex::new(Vec::new()),
            layout_poison: poison,
            layout_failures: Mutex::new(Vec::new()),
            layout_successes: Mutex::new(Vec::new()),
            owner_thread: std::thread::current().id(),
            _lifetime: PhantomData,
        };

        // Move arena into a different thread; check_thread must panic.
        // We use catch_unwind inside the thread to capture the panic and
        // relay it to the test thread via a channel.
        let (sender, receiver) = std::sync::mpsc::channel::<bool>();
        std::thread::spawn(move || {
            // arena.check_thread() is private — trigger it via arena.get(),
            // which calls check_thread() before any HashMap lookup.
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // RenderId::new(1) produces a valid id that won't be in the
                // empty map, but check_thread fires before the HashMap lookup.
                arena.get(RenderId::new(1));
            }))
            .is_err();
            sender.send(panicked).ok();
        })
        .join()
        .ok();

        let panicked = receiver.recv().expect("thread must send result");
        assert!(panicked, "check_thread must panic on wrong-thread access");
    }

    /// Verify that `LayoutCycleGuard` rejects re-entry and that `Drop`
    /// clears the id so a second entry attempt on a fresh guard succeeds.
    ///
    /// This is gate (b) from the adversarial test spec in the plan.
    #[test]
    fn layout_cycle_guard_rejects_reentry_and_clears_on_drop() {
        // The cycle guard operates on `by_id` entries; seed the map with
        // the test id so `enter` / `drop` have a flag to read and write.
        let id = RenderId::new(42);
        let mut by_id: HashMap<RenderId, (NodePtr, AtomicBool)> = HashMap::new();
        // NodePtr is never dereferenced in this test — we only exercise the
        // AtomicBool side flag.  Use a dangling non-null pointer as the
        // address; the `unsafe impl Send/Sync` on NodePtr and `check_thread`
        // mean no deref occurs during LayoutCycleGuard operations.
        by_id.insert(
            id,
            (
                NodePtr(std::ptr::NonNull::dangling().as_ptr()),
                AtomicBool::new(false),
            ),
        );
        let poison = LayoutPoison::default();
        let arena: SubtreeArena<'_> = SubtreeArena {
            by_id,
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_builds: Mutex::new(Vec::new()),
            pending_removes: Mutex::new(Vec::new()),
            pending_child_requests: Mutex::new(Vec::new()),
            pending_retain_bands: Mutex::new(Vec::new()),
            layout_poison: &poison,
            layout_failures: Mutex::new(Vec::new()),
            layout_successes: Mutex::new(Vec::new()),
            owner_thread: std::thread::current().id(),
            _lifetime: PhantomData,
        };

        let id = RenderId::new(42);

        // First entry must succeed.
        let guard = LayoutCycleGuard::enter(&arena, id).expect("first entry must succeed");

        // Second entry while `guard` is live must fail with LayoutCycle.
        let second = LayoutCycleGuard::enter(&arena, id);
        assert!(
            matches!(second, Err(crate::error::RenderError::LayoutCycle(_))),
            "second entry must return LayoutCycle error",
        );

        // Drop the guard — id must be cleared.
        drop(guard);

        // Third entry after drop must succeed again.
        let third = LayoutCycleGuard::enter(&arena, id);
        assert!(third.is_ok(), "entry after drop must succeed");
    }

    /// Verify that all three pending sinks (builds, removes, child
    /// requests) drain and leave themselves empty.
    #[test]
    fn pending_sink_drains_are_idempotent() {
        let poison = LayoutPoison::default();
        let arena: SubtreeArena<'_> = SubtreeArena {
            by_id: HashMap::new(),
            #[cfg(any(test, feature = "testing"))]
            parent_data_seeds: FxHashMap::default(),
            pending_builds: Mutex::new(Vec::new()),
            pending_removes: Mutex::new(Vec::new()),
            pending_child_requests: Mutex::new(Vec::new()),
            pending_retain_bands: Mutex::new(Vec::new()),
            layout_poison: &poison,
            layout_failures: Mutex::new(Vec::new()),
            layout_successes: Mutex::new(Vec::new()),
            owner_thread: std::thread::current().id(),
            _lifetime: PhantomData,
        };

        // Push a remove entry directly into the sink (simulating what
        // ErasedSliverLayoutCtx does during a walk).
        let parent_id = RenderId::new(1);
        let child_id = RenderId::new(2);
        arena.pending_removes.lock().push((parent_id, child_id));

        // First drain must return the entry.
        let removes = arena.take_pending_removes();
        assert_eq!(removes.len(), 1);
        assert_eq!(removes[0], (parent_id, child_id));

        // Second drain must be empty (idempotent / mem::take).
        let removes2 = arena.take_pending_removes();
        assert!(removes2.is_empty(), "second drain must be empty");

        // Same for builds and request sink (empty — idempotency).
        assert!(arena.take_pending_builds().is_empty());
        let sliver_id = RenderId::new(3);
        arena.pending_child_requests.lock().push((sliver_id, 7));
        let requests = arena.take_pending_child_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0], (sliver_id, 7));
        let requests2 = arena.take_pending_child_requests();
        assert!(requests2.is_empty(), "second request drain must be empty");

        // Same for retain-band sink.
        let band_sliver = RenderId::new(4);
        arena.pending_retain_bands.lock().push((band_sliver, 3, 8));
        let bands = arena.take_pending_retain_bands();
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0], (band_sliver, 3, 8));
        let bands2 = arena.take_pending_retain_bands();
        assert!(bands2.is_empty(), "second retain-band drain must be empty");
    }
}
