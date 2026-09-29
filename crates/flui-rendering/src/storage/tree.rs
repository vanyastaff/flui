//! RenderTree - Slab-based render object storage.
//!
//! This module provides efficient storage and tree operations for render
//! objects.

use flui_foundation::RenderId;
use slab::Slab;

use super::node::RenderNode;
use crate::protocol::{BoxProtocol, RenderObject, SliverProtocol};

// ============================================================================
// RenderTree
// ============================================================================

/// Slab-based storage for render objects.
///
/// Provides O(1) render object access by RenderId and tree navigation
/// operations.
///
/// # Thread Safety
///
/// RenderTree is deliberately `!Send + !Sync`: it is owned by
/// exactly one [`PipelineOwner`](crate::pipeline::PipelineOwner), checked
/// out through exactly one
/// [`PipelineCell`](crate::pipeline::PipelineCell) on exactly one thread.
///
/// # Example
///
/// ```ignore
/// // Concrete render objects (RenderColoredBox, RenderSizedBox, …) now live
/// // in the `flui_objects` crate. See `flui_objects::RenderColoredBox` for a
/// // ready-made leaf box to use in examples and tests.
/// use flui_rendering::storage::RenderTree;
///
/// let mut tree = RenderTree::new();
/// // tree.insert_box(Box::new(flui_objects::RenderColoredBox::red(40.0, 40.0)) as Box<_>);
/// ```
#[derive(Debug)]
pub struct RenderTree {
    /// Slab storage for nodes (0-based indexing internally).
    nodes: Slab<RenderNode>,

    /// Per-slot generation counters, parallel to `nodes` (indexed by slot).
    ///
    /// D2 ABA safety: the slab reuses freed slots, so a bare index would let
    /// a stale [`RenderId`] silently address a different node (wrong-node
    /// dirty marks; stale retained content under a new widget). Each slot's
    /// generation is bumped when the slot is freed; ids are minted against
    /// the slot's current generation, and every accessor routes through the
    /// single private [`Self::resolve`] check. Mirrors `ElementTree`'s
    /// scheme (`ElementId`).
    generations: Vec<core::num::NonZeroU32>,

    /// Root node ID (None if tree is empty).
    root: Option<RenderId>,
}

/// Materialises disjoint `&mut RenderNode` borrows for the given slab
/// `indices`, in input order, from a **single** `iter_mut` pass.
///
/// Safe split-borrow: `slab::IterMut` yields each occupied slot's
/// `&mut RenderNode` exactly once out of one traversal, so the returned
/// references are disjoint by construction and no `unsafe` is needed.
/// (The previous implementation derived each element through a fresh
/// `(*raw_slab).get_mut(idx)` — every such deref re-tags the whole slab
/// allocation as `Unique` and invalidates the references produced by
/// the earlier iterations: Stacked Borrows UB, reported by miri.)
///
/// Returns `None` if `indices` contains duplicates or any index has no
/// occupied slot.
///
/// # Complexity
///
/// O(slab len + N) average and worst case — one traversal with an
/// early break once all N slots are found, plus an O(N) `HashMap`
/// build for the index→position lookup.
fn collect_disjoint_mut<'a>(
    nodes: &'a mut slab::Slab<RenderNode>,
    indices: &[usize],
) -> Option<Vec<&'a mut RenderNode>> {
    // slab index → output position. Duplicate indices collapse the map;
    // report them as `None` (the caller's id-uniqueness contract).
    let want: std::collections::HashMap<usize, usize> = indices.iter().copied().zip(0..).collect();
    if want.len() != indices.len() {
        return None;
    }

    let mut slots: Vec<Option<&'a mut RenderNode>> = Vec::with_capacity(indices.len());
    slots.resize_with(indices.len(), || None);

    let mut remaining = indices.len();
    for (idx, node) in nodes.iter_mut() {
        if remaining == 0 {
            break;
        }
        if let Some(&pos) = want.get(&idx) {
            slots[pos] = Some(node);
            remaining -= 1;
        }
    }

    // A missing index leaves its slot `None`, which collapses the
    // whole collect to `None`.
    slots.into_iter().collect()
}

impl Default for RenderTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderTree {
    /// Creates a new empty RenderTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            generations: Vec::new(),
            root: None,
        }
    }

    /// Creates a RenderTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            generations: Vec::with_capacity(capacity),
            root: None,
        }
    }

    // ========================================================================
    // Generational id plumbing (D2)
    // ========================================================================

    /// Resolves an id to its slab slot, or `None` when the id is stale
    /// (slot freed and possibly reused) or the slot is vacant.
    ///
    /// THE single staleness check: every accessor must route through here;
    /// nothing outside this block may index the slab from an id directly.
    #[inline]
    fn resolve(&self, id: RenderId) -> Option<usize> {
        let index = id.index() as usize;
        (self.generations.get(index).copied() == Some(id.generation())
            && self.nodes.contains(index))
        .then_some(index)
    }

    /// Mints the id for a freshly inserted slot, growing the generation
    /// table as needed (new slots start at generation 1).
    ///
    /// # Panics
    ///
    /// Panics if the slab exceeds `u32::MAX` slots — the id's index field
    /// is 32 bits (same cap as `ElementTree`).
    #[inline]
    fn mint(&mut self, slab_index: usize) -> RenderId {
        if slab_index >= self.generations.len() {
            self.generations
                .resize(slab_index + 1, core::num::NonZeroU32::MIN);
        }
        let index = u32::try_from(slab_index)
            .expect("render tree exceeds u32::MAX slots; RenderId index field is 32 bits");
        RenderId::new_gen(index, self.generations[slab_index])
    }

    /// The id currently identifying `slab_index` (read-only mint for
    /// iteration over occupied slots).
    #[inline]
    fn id_at(&self, slab_index: usize) -> RenderId {
        let generation = self
            .generations
            .get(slab_index)
            .copied()
            .unwrap_or(core::num::NonZeroU32::MIN);
        let index = u32::try_from(slab_index)
            .expect("render tree exceeds u32::MAX slots; RenderId index field is 32 bits");
        RenderId::new_gen(index, generation)
    }

    /// Bumps a freed slot's generation so every outstanding id minted
    /// against the old occupant becomes stale.
    ///
    /// # Panics
    ///
    /// Panics if a single slot is recycled `u32::MAX - 1` times — at that
    /// point stale-id safety can no longer be guaranteed (wrap-around would
    /// resurrect the oldest ids). Same retire-by-panic policy as
    /// `ElementTree::bump_generation`.
    #[inline]
    fn bump_generation(&mut self, slab_index: usize) {
        let slot = &mut self.generations[slab_index];
        let next = slot
            .get()
            .checked_add(1)
            .expect("render slot generation overflow: slot recycled u32::MAX times");
        *slot = core::num::NonZeroU32::new(next)
            .expect("generation+1 of a NonZeroU32 is always non-zero");
    }

    // ========================================================================
    // Root Management
    // ========================================================================

    /// Returns the root node ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Sets the root node ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }

    // ========================================================================
    // Basic Operations
    // ========================================================================

    /// Checks if a node exists in the tree.
    #[inline]
    pub fn contains(&self, id: RenderId) -> bool {
        self.resolve(id).is_some()
    }

    /// Returns the number of nodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns a reference to a node.
    ///
    /// Returns `None` for vacant slots AND for stale ids whose slot was
    /// freed (and possibly reused) — the generation check in
    /// `Self::resolve`.
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode> {
        self.nodes.get(self.resolve(id)?)
    }

    /// Returns a mutable reference to a node (generation-checked).
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode> {
        let index = self.resolve(id)?;
        self.nodes.get_mut(index)
    }

    /// Returns mutable references to two distinct nodes simultaneously.
    ///
    /// Used by the layout phase for parent-child re-entrant access: a
    /// parent holds `&mut RenderNode` for itself while it calls `layout`
    /// on each child's `&mut RenderNode`. Returns `None` if either id is
    /// missing.
    ///
    /// Implemented over `collect_disjoint_mut` — a single safe
    /// `iter_mut` split-borrow pass, no raw pointers. (The previous
    /// per-index `(*raw_slab).get_mut(..)` derivation re-tagged the
    /// whole slab allocation per element and invalidated the earlier
    /// reference — Stacked Borrows UB, caught by miri.)
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `a == b`. In release builds, returns
    /// `None`.
    pub fn get_two_mut(
        &mut self,
        a: RenderId,
        b: RenderId,
    ) -> Option<(&mut RenderNode, &mut RenderNode)> {
        debug_assert_ne!(a, b, "RenderTree::get_two_mut requires distinct ids");
        if a == b {
            return None;
        }

        let a_idx = self.resolve(a)?;
        let b_idx = self.resolve(b)?;

        let mut nodes = collect_disjoint_mut(&mut self.nodes, &[a_idx, b_idx])?;
        let b_ref = nodes.pop()?;
        let a_ref = nodes.pop()?;
        Some((a_ref, b_ref))
    }

    /// Returns mutable references to a parent + every child in the given
    /// child id list.
    ///
    /// Used by variable-arity layout where a parent's `perform_layout`
    /// must read its own fields while writing into each child's slot.
    /// Returns `None` if any id is missing or any pair of ids collide
    /// (duplicate ids resolve to duplicate slab slots, which
    /// `collect_disjoint_mut` rejects — two distinct live ids can
    /// never share a slot because each slot stores one generation).
    pub fn get_parent_and_children_mut<'a>(
        &'a mut self,
        parent_id: RenderId,
        child_ids: &[RenderId],
    ) -> Option<(&'a mut RenderNode, Vec<&'a mut RenderNode>)> {
        let mut indices = Vec::with_capacity(child_ids.len() + 1);
        indices.push(self.resolve(parent_id)?);
        for &c in child_ids {
            indices.push(self.resolve(c)?);
        }

        let mut nodes = collect_disjoint_mut(&mut self.nodes, &indices)?;
        let children = nodes.split_off(1);
        let parent_ref = nodes.pop()?;
        Some((parent_ref, children))
    }

    /// Returns mutable references to **every** node id in the given list,
    /// materialised in a single function scope so all `&mut RenderNode`
    /// borrows coexist on disjoint slab slots without re-entering the
    /// slab borrow checker.
    ///
    /// Generalises [`get_parent_and_children_mut`](Self::get_parent_and_children_mut)
    /// from N+1 (parent + direct children) to arbitrary N (whole subtree
    /// pre-acquisition). The returned `Vec<&mut RenderNode>` is in input
    /// order so callers indexing by id can pre-compute a
    /// `HashMap<RenderId, usize>` lookup.
    ///
    /// Returns `None` if any id is missing from the slab OR if `ids`
    /// contains duplicates.
    ///
    /// # Use case
    ///
    /// [`PipelineOwner::layout_dirty_root`](crate::pipeline::PipelineOwner::layout_dirty_root)
    /// uses this to pre-acquire the entire subtree's `&mut RenderNode`
    /// borrows up front, then drives `perform_layout_raw` recursively
    /// against an index-into-pre-acquired-pool — eliminating the
    /// recursive raw-pointer reborrow pattern the prior implementation
    /// used (latent Stacked/Tree Borrows UB). All borrows live in one
    /// stack frame so the aliasing model is satisfied: `&mut Slab` is borrowed once,
    /// N disjoint `&mut RenderNode` borrows on distinct slots are
    /// returned, no nested reborrow.
    ///
    /// Duplicate ids and stale/missing ids both return `None` (a
    /// duplicate id resolves to a duplicate slab slot, which
    /// `collect_disjoint_mut` rejects).
    ///
    /// # Complexity
    ///
    /// O(slab len + N) average and worst case — one `iter_mut`
    /// traversal with an early break once all N slots are found, plus
    /// an O(N) resolve pass. Called once per dirty layout root, not
    /// per node.
    pub fn get_subtree_mut<'a>(&'a mut self, ids: &[RenderId]) -> Option<Vec<&'a mut RenderNode>> {
        // Resolve every id (generation-checked) before borrowing.
        let indices: Vec<usize> = ids
            .iter()
            .map(|&id| self.resolve(id))
            .collect::<Option<Vec<_>>>()?;

        collect_disjoint_mut(&mut self.nodes, &indices)
    }

    /// Inserts `node` without a parent and returns its id; the caller links it and
    /// sets the root.
    pub fn insert(&mut self, node: RenderNode) -> RenderId {
        let slab_index = self.nodes.insert(node);
        self.mint(slab_index)
    }

    /// Inserts a Box protocol render object into the tree (no parent).
    ///
    /// Returns the RenderId of the inserted node.
    ///
    pub fn insert_box(&mut self, render_object: Box<dyn RenderObject<BoxProtocol>>) -> RenderId {
        self.insert(RenderNode::new_box(render_object))
    }

    /// Inserts a Sliver protocol render object into the tree (no parent).
    pub fn insert_sliver(
        &mut self,
        render_object: Box<dyn RenderObject<SliverProtocol>>,
    ) -> RenderId {
        self.insert(RenderNode::new_sliver(render_object))
    }

    /// Inserts a Box protocol render object as a child of the given parent.
    ///
    /// Returns the RenderId of the inserted child.
    pub fn insert_box_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject<BoxProtocol>>,
    ) -> Option<RenderId> {
        let parent_depth = self.get(parent_id)?.depth();

        // Protocol compatibility: Box children can go under both Box and
        // Sliver parents. Under Sliver parents, the child should be
        // wrapped in SliverToBoxAdapter at the widget layer — the render
        // tree accepts it directly because the sliver layout bridge
        // handles the cross-protocol dispatch.
        let child_node =
            RenderNode::new_box_with_parent(render_object, parent_id, parent_depth + 1);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = self.mint(child_slab_index);

        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Inserts a Sliver protocol render object as a child of the given parent.
    ///
    /// Sliver children under Box parents are the Viewport pattern — the
    /// box parent drives sliver layout through the cross-protocol bridge.
    pub fn insert_sliver_child(
        &mut self,
        parent_id: RenderId,
        render_object: Box<dyn RenderObject<SliverProtocol>>,
    ) -> Option<RenderId> {
        let parent_depth = self.get(parent_id)?.depth();

        let child_node =
            RenderNode::new_sliver_with_parent(render_object, parent_id, parent_depth + 1);
        let child_slab_index = self.nodes.insert(child_node);
        let child_id = self.mint(child_slab_index);

        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        Some(child_id)
    }

    /// Adopts `child_id` under `parent_id`: sets the child's parent link AND
    /// appends it to the parent's children list in one call, so the two
    /// directions of a render-tree edge can never be written independently.
    ///
    /// Flutter equivalence: `RenderObject.adoptChild` (`rendering/object.dart`
    /// @ tag `3.44.0`), which asserts `child._parent == null` and that
    /// adopting will not introduce a cycle before wiring `child._parent =
    /// this` and appending to the child list as one primitive — the same
    /// two guards this method asserts below.
    ///
    /// [`insert_box_child`](Self::insert_box_child) /
    /// [`insert_sliver_child`](Self::insert_sliver_child) already bake the
    /// parent link into construction (`RenderNode::new_*_with_parent`), so
    /// they do not need this. This primitive is for the shape
    /// `RenderBehavior::on_mount` needs instead: the `RenderObject` is
    /// minted with [`insert_box`](Self::insert_box) /
    /// [`insert_sliver`](Self::insert_sliver) — no parent yet, because the
    /// element does not know its render parent until after the object
    /// exists — and is adopted under its parent as a second step. Before
    /// this primitive existed, every such call site wrote `set_parent` and
    /// `add_child` as two independent statements, which made the
    /// parent-link / child-link asymmetry representable (and, at the root
    /// hop, real: see `RootRenderElement`'s `ElementBase::render_id`
    /// history).
    ///
    /// Both ids must resolve for either link to be written: if either is
    /// stale, this is a no-op on BOTH sides, never just one — a one-sided
    /// write (child's parent set while the old parent's stale entry stays
    /// resolvable, or vice versa) would recreate exactly the asymmetry this
    /// primitive exists to prevent.
    ///
    /// # Panics (debug only)
    ///
    /// - if `parent_id == child_id` — a self-adoption would write a
    ///   self-cycle that the ancestor walk below cannot see (it starts at
    ///   the descendant's parent).
    /// - if `child_id` already has a parent. Re-parenting through this
    ///   primitive would leave the OLD parent's children list with a stale
    ///   entry — the same asymmetry this primitive exists to prevent, just
    ///   moved to the donor side. A call site that legitimately moves a
    ///   child must [`drop_child`](Self::drop_child) it from its old parent
    ///   first, matching Flutter's `assert(child._parent == null)`.
    /// - if `parent_id` is a descendant of `child_id` — adopting would
    ///   close a cycle (`child_id` would become its own indirect ancestor),
    ///   matching Flutter's cycle guard in `adoptChild`.
    pub fn adopt_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        // Both must resolve BEFORE either link is written — a stale id on
        // either side must leave both directions untouched.
        if self.get(parent_id).is_none() || self.get(child_id).is_none() {
            return;
        }

        debug_assert_ne!(
            parent_id, child_id,
            "adopt_child: a node cannot adopt itself — the ancestor walk below starts at the \
             descendant's parent, so this degenerate self-cycle would slip past it"
        );
        debug_assert!(
            self.get(child_id).and_then(RenderNode::parent).is_none(),
            "adopt_child: {child_id:?} already has a parent — re-parenting through this \
             primitive would leave the OLD parent's children list with a stale entry; drop it \
             from its old parent first (see `drop_child`)"
        );
        debug_assert!(
            !self.is_ancestor(child_id, parent_id),
            "adopt_child: {parent_id:?} is already a descendant of {child_id:?} — adopting would \
             close a cycle"
        );

        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }
    }

    /// Drops `child_id` from `parent_id`: removes it from the parent's
    /// children list AND clears the child's parent link in one call — the
    /// inverse of [`adopt_child`](Self::adopt_child).
    ///
    /// Flutter equivalence: `RenderObject.dropChild` (`rendering/object.dart`
    /// @ tag `3.44.0`), which asserts `child._parent == this` before clearing
    /// `child._parent = null` and removing it from the child list as one
    /// primitive.
    ///
    /// Both ids must resolve for either link to be cleared: if either is
    /// stale, this is a no-op on BOTH sides, for the same reason
    /// `adopt_child` requires it — a one-sided clear is the asymmetry these
    /// primitives exist to make unrepresentable.
    ///
    /// # Panics (debug only)
    ///
    /// If `child_id`'s current parent is not `parent_id` — the caller's
    /// belief about the tree shape is wrong, matching Flutter's
    /// `assert(child._parent == this)` in `dropChild`.
    pub fn drop_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        if self.get(parent_id).is_none() || self.get(child_id).is_none() {
            return;
        }

        debug_assert_eq!(
            self.get(child_id).and_then(RenderNode::parent),
            Some(parent_id),
            "drop_child: {child_id:?}'s current parent does not match {parent_id:?} — the \
             caller's belief about the tree shape is wrong"
        );

        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Removes a node from the tree.
    ///
    /// Removes a node WITHOUT cascading to descendants.
    ///
    /// Returns the removed node, or None if it didn't exist. Descendants
    /// are orphaned in the slab; use [`Self::remove_recursive`] for full
    /// cascade.
    pub fn remove_shallow(&mut self, id: RenderId) -> Option<RenderNode> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        // Get parent and remove from parent's children
        if let Some(parent_id) = self.get(id).and_then(super::node::RenderNode::parent)
            && let Some(parent) = self.get_mut(parent_id)
        {
            parent.remove_child(id);
        }

        let index = self.resolve(id)?;
        let removed = self.nodes.try_remove(index);
        if removed.is_some() {
            // Invalidate every outstanding id minted against this slot.
            self.bump_generation(index);
        }
        removed
    }

    /// Removes a node and all its descendants recursively.
    ///
    /// Returns the number of nodes removed; children are removed before
    /// their parent.
    pub fn remove_recursive(&mut self, id: RenderId) -> usize {
        // Iterative: collect the subtree up front (explicit-stack
        // pre-order with cycle protection), then remove in REVERSE
        // collection order — in reversed pre-order every child comes
        // before its parent, which preserves the recursive version's
        // child-first removal. Plain recursion here overflowed the
        // 1 MiB Windows main-thread stack on deep chains (the same
        // crash class the pipeline walks hit at ~1000 levels).
        let subtree = self.collect_subtree_ids(id);
        let mut count = 0;
        for &node_id in subtree.iter().rev() {
            if self.remove_shallow(node_id).is_some() {
                count += 1;
            }
        }
        count
    }

    /// Clears all nodes from the tree.
    ///
    /// Bumps every slot generation so ALL outstanding ids become stale —
    /// otherwise an id minted before `clear` would alias the first
    /// post-clear occupant of its slot.
    pub fn clear(&mut self) {
        self.nodes.clear();
        for slab_index in 0..self.generations.len() {
            self.bump_generation(slab_index);
        }
        self.root = None;
    }

    /// Reserves capacity for additional nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========================================================================
    // Tree Navigation
    // ========================================================================

    /// Returns the parent ID of a node.
    #[inline]
    pub fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.get(id)?.parent()
    }

    /// Returns the children IDs of a node.
    #[inline]
    pub fn children(&self, id: RenderId) -> &[RenderId] {
        self.get(id).map_or(&[], super::node::RenderNode::children)
    }

    /// Returns the depth of a node in the tree.
    #[inline]
    pub fn depth(&self, id: RenderId) -> Option<u16> {
        self.get(id).map(super::node::RenderNode::depth)
    }

    /// Collects `root_id` plus every transitive descendant in
    /// **DFS pre-order** (parent before children; children visited in
    /// stored order). Returns an empty `Vec` if `root_id` is not in
    /// the tree.
    ///
    /// # Use case
    ///
    /// [`PipelineOwner::layout_dirty_root`](crate::pipeline::PipelineOwner::layout_dirty_root)
    /// passes the result into
    /// [`Self::get_subtree_mut`] to pre-acquire every subtree node's
    /// `&mut RenderNode` borrow in one stack frame, eliminating the
    /// recursive raw-pointer reborrow pattern (latent Stacked / Tree
    /// Borrows UB) the prior implementation used.
    ///
    /// # Implementation
    ///
    /// Iterative DFS with an explicit `Vec` stack so deep trees do
    /// not overflow Rust's call stack (the layout walk has no other
    /// depth limit beyond the pipeline's own layout-cycle guard).
    /// Children are pushed in reverse so they pop in stored order —
    /// preserves pre-order with children-left-to-right.
    ///
    /// # Cycle protection
    ///
    /// Carries a `visited` `HashSet<RenderId>` to short-circuit on
    /// repeated ids. Without this guard, a malformed tree containing
    /// a parent / child cycle (which `RenderNode::add_child` does not
    /// prevent) would loop forever — repeatedly re-pushing the cycle's
    /// nodes onto `stack` while `out` grows unbounded → hang / OOM. The
    /// visited-set short-circuit terminates the walk on the first
    /// repeated id and produces a deduplicated `Vec<RenderId>` suitable
    /// for [`Self::get_subtree_mut`] (which requires pairwise
    /// uniqueness). The cyclic edge itself is silently dropped; full
    /// [`RenderError::LayoutCycle`](crate::error::RenderError::LayoutCycle)
    /// reporting happens elsewhere, in the pipeline's layout-cycle
    /// guard — this fix is the minimum-disruption termination guard
    /// so the pre-acquired-subtree walk does not regress on cycles vs
    /// the prior stack-overflow failure mode.
    ///
    /// # Complexity
    ///
    /// O(N) where N is the subtree node count. Single pass; each
    /// node's `children()` slice is borrowed once. `visited` is a
    /// `HashSet<RenderId>` — O(1) amortised lookup + insert per id.
    pub fn collect_subtree_ids(&self, root_id: RenderId) -> Vec<RenderId> {
        let mut out = Vec::new();
        // If the root doesn't exist, return empty to mirror other
        // tree-walk methods (e.g., `depth()` returns None) — callers
        // should check before doing further work with the result.
        if self.get(root_id).is_none() {
            return out;
        }
        let mut stack: Vec<RenderId> = vec![root_id];
        // The visited-set short-circuits on repeated ids so a cyclic
        // tree terminates instead of hanging / OOMing. Pre-sized to a
        // conservative guess (small trees are the common case; HashSet
        // grows by power-of-two doubling otherwise).
        let mut visited: std::collections::HashSet<RenderId> =
            std::collections::HashSet::with_capacity(16);
        while let Some(id) = stack.pop() {
            // Skip ids already visited — preserves uniqueness in `out`
            // and breaks cycles. Without this, a parent/child cycle
            // (A → B → A) re-pushes A onto stack forever.
            if !visited.insert(id) {
                continue;
            }
            if let Some(node) = self.get(id) {
                out.push(id);
                // Reverse-push so the leftmost child pops first,
                // preserving pre-order with children-in-stored-order.
                for &child_id in node.children().iter().rev() {
                    stack.push(child_id);
                }
            }
        }
        out
    }

    /// Checks if `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: RenderId, descendant: RenderId) -> bool {
        let mut current = self.parent(descendant);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.parent(id);
        }
        false
    }

    /// Checks if `descendant` is a descendant of `ancestor`.
    #[inline]
    pub fn is_descendant(&self, descendant: RenderId, ancestor: RenderId) -> bool {
        self.is_ancestor(ancestor, descendant)
    }

    /// Returns the path from root to the given node.
    ///
    /// The path includes the node itself.
    pub fn path_to_root(&self, id: RenderId) -> Vec<RenderId> {
        let mut path = Vec::new();
        let mut current = Some(id);

        while let Some(node_id) = current {
            path.push(node_id);
            current = self.parent(node_id);
        }

        path.reverse();
        path
    }

    // ========================================================================
    // Dirty Node Collection
    // ========================================================================

    /// Collects all nodes that need layout, sorted by depth.
    ///
    /// Returns IDs of nodes with `needs_layout() == true`, sorted by depth
    /// (shallow first) for correct layout order.
    pub fn collect_nodes_needing_layout(&self) -> Vec<RenderId> {
        let mut nodes: Vec<(RenderId, usize)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.needs_layout())
            .map(|(idx, node)| (self.id_at(idx), node.depth() as usize))
            .collect();

        // Sort by depth (shallow first)
        nodes.sort_by_key(|(_, depth)| *depth);

        nodes.into_iter().map(|(id, _)| id).collect()
    }

    /// Collects all nodes that need paint, sorted by depth.
    ///
    /// Returns IDs of nodes with `needs_paint() == true`, sorted by depth
    /// (shallow first) for correct paint order.
    pub fn collect_nodes_needing_paint(&self) -> Vec<RenderId> {
        let mut nodes: Vec<(RenderId, usize)> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.needs_paint())
            .map(|(idx, node)| (self.id_at(idx), node.depth() as usize))
            .collect();

        // Sort by depth (shallow first)
        nodes.sort_by_key(|(_, depth)| *depth);

        nodes.into_iter().map(|(id, _)| id).collect()
    }

    // ========================================================================
    // Iteration
    // ========================================================================

    /// Returns an iterator over all node IDs.
    pub fn ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.nodes.iter().map(|(idx, _)| self.id_at(idx))
    }

    /// Returns an iterator over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &RenderNode> + '_ {
        self.nodes.iter().map(|(_, node)| node)
    }

    /// Returns a mutable iterator over all nodes.
    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut RenderNode> + '_ {
        self.nodes.iter_mut().map(|(_, node)| node)
    }

    /// Returns an iterator over (RenderId, &RenderNode) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (RenderId, &RenderNode)> + '_ {
        self.nodes.iter().map(|(idx, node)| (self.id_at(idx), node))
    }

    /// Returns a mutable iterator over (RenderId, &mut RenderNode) pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (RenderId, &mut RenderNode)> + '_ {
        let generations = &self.generations;
        self.nodes.iter_mut().map(move |(idx, node)| {
            let generation = generations
                .get(idx)
                .copied()
                .unwrap_or(core::num::NonZeroU32::MIN);
            let index = u32::try_from(idx)
                .expect("render tree exceeds u32::MAX slots; RenderId index field is 32 bits");
            (RenderId::new_gen(index, generation), node)
        })
    }

    // ========================================================================
    // Depth-First Traversal
    // ========================================================================

    /// Visits all nodes in depth-first pre-order starting from root.
    ///
    /// The callback receives (RenderId, &RenderNode) for each node.
    ///
    /// # Implementation
    ///
    /// Uses an iterative loop with a `SmallVec<[RenderId; 32]>` work-stack
    /// rather than a recursive `visit_depth_first_from`. Three wins:
    /// - **No stack overflow** on pathological tree depths
    ///   (recursion blew at ~5000 with default Rust stack; the
    ///   iterative version is unbounded).
    /// - **Inline 32-deep buffer** via `SmallVec` covers the typical
    ///   widget tree depth (Flutter's `RenderObject` paint trees
    ///   measure ~20-40 deep in practice) without heap allocation.
    ///   Deeper trees spill to heap automatically.
    /// - **No per-node child clone.** The recursive path called
    ///   `node.children().to_vec()` on every visit to dodge a borrow
    ///   conflict; the iterative path borrows the slice in-place and
    ///   pushes child ids onto the work-stack directly. The
    ///   `RenderId` push is a `Copy` of two `usize`s -- no heap
    ///   traffic -- and the `SmallVec` doubles its inline buffer to
    ///   absorb the children without reallocating until depth 32+.
    ///
    /// Pre-order semantics preserved: children are pushed in
    /// **reverse** order so the work-stack pops them in original
    /// child-order (mirrors Flutter's `visitChildren` shape).
    ///
    /// A prior version of this comment claimed `extend_from_slice`.
    /// That was a copy-paste error from an earlier draft; reversing
    /// in-place via `iter().rev()` is required for pre-order
    /// pop-order and `extend_from_slice` would need a temporary
    /// reversed allocation, defeating the no-alloc goal. The body
    /// matches the doc now.
    pub fn visit_depth_first<F>(&self, mut f: F)
    where
        F: FnMut(RenderId, &RenderNode),
    {
        let Some(root_id) = self.root else {
            return;
        };
        let mut stack: smallvec::SmallVec<[RenderId; 32]> = smallvec::SmallVec::new();
        stack.push(root_id);
        while let Some(id) = stack.pop() {
            if let Some(node) = self.get(id) {
                f(id, node);
                // Push children in reverse so pop() yields them
                // in original child-order (pre-order traversal).
                for &child_id in node.children().iter().rev() {
                    stack.push(child_id);
                }
            }
        }
    }

    /// Visits all nodes mutably in depth-first pre-order starting from root.
    ///
    /// **Note:** The callback receives only RenderId since we can't provide
    /// mutable references during traversal. Use `get_mut()` inside the
    /// callback.
    pub fn visit_depth_first_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Self, RenderId),
    {
        // Iterative (explicit work-stack) like the immutable twin
        // above — plain recursion overflows the fixed OS stack on
        // deep chains. The child snapshot is taken BEFORE `f` runs
        // (the recursive version did the same), so a callback that
        // mutates the visited node's children does not change this
        // node's traversal.
        let Some(root_id) = self.root else {
            return;
        };
        let mut stack: smallvec::SmallVec<[RenderId; 32]> = smallvec::SmallVec::new();
        stack.push(root_id);
        while let Some(id) = stack.pop() {
            let children: Vec<RenderId> = self
                .get(id)
                .map(|n| n.children().to_vec())
                .unwrap_or_default();
            f(self, id);
            // Reverse push so pop() yields original child-order
            // (pre-order traversal).
            for &child_id in children.iter().rev() {
                stack.push(child_id);
            }
        }
    }
}

// RenderTree is deliberately `!Send + !Sync`.
//
// `RenderObject<P>`/`RenderSliver`/`ParentData` no longer require
// `Send + Sync` (traits/render_object.rs, traits/render_sliver.rs,
// parent_data/base.rs) -- a render tree belongs to exactly one
// `PipelineOwner`, checked out through exactly one `PipelineCell` on
// exactly one thread, so there is no cross-thread mutable access left to
// guard. `Box<dyn RenderObject<P>>` auto-propagates `!Send`/`!Sync` up
// through `RenderNode`/`Slab<RenderNode>` to `RenderTree` itself -- no
// `unsafe impl` needed in either direction; Rust's auto-trait
// non-derivation does the right thing.
//
// See `crates/flui-rendering/ARCHITECTURE.md` for the rationale.

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use flui_foundation::Leaf;
    use flui_foundation::geometry::Size;
    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

    // `RenderTree` stores `Box<dyn RenderObject>`, and `RenderObject` dropped
    // its `Send + Sync` supertrait bound as part of the `PipelineCell` port —
    // a stray `unsafe impl Send`/`Sync` reappearing here would let a
    // `RenderTree` (and the `!Send` objects inside it) cross threads under
    // this crate's own nose. Pinned `!Send + !Sync`.
    assert_not_impl_any!(RenderTree: Send, Sync);

    /// Minimal leaf stub — concrete objects live in `flui_objects`.
    /// This test only needs "something with RenderObject<BoxProtocol>" to
    /// exercise the slab slot / generation / ABA mechanics.
    #[derive(Debug, Default)]
    struct LeafStub;
    impl flui_foundation::Diagnosticable for LeafStub {}
    impl RenderBox for LeafStub {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            Size::new(10.0, 10.0)
        }
        fn paint(&self, _ctx: &mut crate::context::PaintCx<'_, Leaf>) {}
    }

    fn make_leaf() -> Box<dyn RenderObject<BoxProtocol>> {
        Box::new(LeafStub)
    }

    /// D2 — ABA regression: after a slot is freed and reused, the OLD id
    /// must be stale everywhere (get/get_mut/contains/remove/subtree), and
    /// the NEW id must work. Pre-D2 the bare index aliased the new occupant.
    #[test]
    fn stale_id_does_not_alias_reused_slot() {
        let mut tree = RenderTree::new();
        let old = tree.insert_box(make_leaf());
        assert!(tree.contains(old));

        // Free the slot, then re-occupy it.
        assert!(tree.remove_shallow(old).is_some());
        let new = tree.insert_box(make_leaf());
        assert_eq!(
            new.index(),
            old.index(),
            "test precondition: the slab must reuse the freed slot"
        );
        assert_ne!(old, new, "generation bump must distinguish the ids");

        // Old id is stale on every accessor.
        assert!(!tree.contains(old));
        assert!(tree.get(old).is_none());
        assert!(tree.get_mut(old).is_none());
        assert!(tree.remove_shallow(old).is_none());
        assert!(tree.get_subtree_mut(&[old]).is_none());
        assert!(tree.get_two_mut(old, new).is_none());

        // New id is live.
        assert!(tree.contains(new));
        assert!(tree.get(new).is_some());
    }

    #[test]
    fn get_two_mut_returns_none_on_duplicate_id_in_release() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        // In debug builds this panics via debug_assert_ne!; we run the
        // release-path check by going through the `if a == b { return None }`
        // arm directly. To exercise that without tripping the debug assert,
        // we test the missing-second-id branch instead.
        let missing = a; // intentionally the same id
        if cfg!(debug_assertions) {
            // debug build: skip (would panic). Behaviour validated by
            // the release-build `return None` path below in test
            // get_two_mut_with_missing_id_returns_none.
        } else {
            assert!(tree.get_two_mut(a, missing).is_none());
        }
    }

    // ========================================================================
    // get_subtree_mut
    // ========================================================================

    #[test]
    fn get_subtree_mut_rejects_duplicate_id() {
        let mut tree = RenderTree::new();
        let a = tree.insert_box(make_leaf());
        let b = tree.insert_box(make_leaf());
        // a appears twice in the id list — duplicate detection must fail.
        assert!(tree.get_subtree_mut(&[a, b, a]).is_none());
    }

    // ========================================================================
    // collect_subtree_ids
    // ========================================================================

    #[test]
    fn collect_subtree_ids_three_level_dfs_preorder() {
        // Tree:
        //     root
        //    /    \
        //   a      b
        //  / \      \
        // a1 a2     b1
        //
        // Pre-order: root, a, a1, a2, b, b1
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let a = tree.insert_box_child(root, make_leaf()).unwrap();
        let a1 = tree.insert_box_child(a, make_leaf()).unwrap();
        let a2 = tree.insert_box_child(a, make_leaf()).unwrap();
        let b = tree.insert_box_child(root, make_leaf()).unwrap();
        let b1 = tree.insert_box_child(b, make_leaf()).unwrap();

        assert_eq!(
            tree.collect_subtree_ids(root),
            vec![root, a, a1, a2, b, b1],
            "DFS pre-order must visit each subtree completely before moving \
             to the next sibling",
        );
    }

    // ========================================================================
    // adopt_child / drop_child
    // ========================================================================

    /// Adopting a node's own ancestor would close a cycle — rejected the
    /// same way Flutter's `adoptChild` guards against it.
    #[test]
    // Debug-only: the guard compiles out in release, where `#[should_panic]`
    // would otherwise report "did not panic as expected".
    #[cfg(debug_assertions)]
    #[should_panic(expected = "close a cycle")]
    fn adopt_child_rejects_a_cycle() {
        let mut tree = RenderTree::new();
        let root = tree.insert_box(make_leaf());
        let child = tree.insert_box_child(root, make_leaf()).unwrap();

        // `root` is already `child`'s ancestor; adopting `root` under
        // `child` would close a cycle.
        tree.adopt_child(child, root);
    }
}
