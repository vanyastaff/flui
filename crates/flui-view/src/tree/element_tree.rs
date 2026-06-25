//! Slab-based Element tree storage.
//!
//! Elements are stored in a Slab for O(1) access by ElementId.
//! This follows Flutter's approach where Elements form the retained tree.

use std::num::NonZeroU32;
use std::sync::Arc;

use flui_foundation::{ElementId, ViewKey};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;
use slab::Slab;

use crate::view::{ElementBase, View};

/// A node in the Element tree.
///
/// Contains the Element plus metadata for tree traversal.
pub struct ElementNode {
    /// The actual Element.
    pub(crate) element: Box<dyn ElementBase>,
    /// Parent Element ID (None for root).
    pub(crate) parent: Option<ElementId>,
    /// Depth in the tree (root = 0).
    pub(crate) depth: usize,
    /// Slot index within parent's children.
    pub(crate) slot: usize,
    /// Cloned `View::key()` for the view this element currently holds,
    /// or `None` when the view is keyless.
    ///
    /// Plan §U7 / FR-022. Populated at every `insert`/`mount_root_*`
    /// call site (cloned via `ViewKey::clone_key`) and re-cloned at
    /// every `update` boundary so the field stays in lock-step with
    /// the view value the element actually holds. Phase 2's keyed
    /// reconciler reads this field directly via `key()` / `key_hash()`
    /// — no `downcast::<V>()` needed.
    ///
    /// Coexists with `registered_global_key_hash` in Phase 1 for
    /// backward compatibility; the side-index field is reduced to a
    /// derived value in Phase 2 §U17 and removed when the GlobalKey
    /// registry consolidation lands.
    pub(crate) key: Option<Box<dyn ViewKey>>,
    /// Hash of the `GlobalKey` registered for this element, if any.
    ///
    /// Set at mount time by `ElementTree::insert` /
    /// `::mount_root_with_pipeline_owner` when the view's
    /// `View::key()` returns a key whose `ViewKey::is_global_key()` is
    /// `true`. Read at end-of-frame `BuildOwner::finalize_tree` to
    /// unregister the entry from `BuildOwner::global_keys`.
    ///
    /// Plan §U14 / R13 / R14. Flutter parity: keys are tracked on the
    /// element itself in `framework.dart:2884`-ish via `Element._widget`
    ///   + `Widget.key`; we mirror the effect with a side-channel hash
    ///     because our `View` value is owned by `ElementCore` and not
    ///     available at the dispatch boundary used for finalization.
    pub(crate) registered_global_key_hash: Option<u64>,
    /// This node's slab-id-based child list — the single, authoritative
    /// element child graph (E3 — atomic box→arena swap).
    ///
    /// Written by the production id-based reconciler
    /// [`reconcile_children_by_id`](super::id_reconcile::reconcile_children_by_id),
    /// which `build_scope` drives once per dirty element; read by every
    /// child traversal in the framework (build, mount, unmount,
    /// dirty-collection). The old per-element `Box<dyn ElementBase>` child
    /// storage is gone — elements no longer own children.
    ///
    /// Ordering is meaningful: entry `i` is the element occupying child
    /// slot `i` after the most recent id-reconcile, matching the new
    /// view order.
    pub(crate) child_ids: Vec<ElementId>,
}

impl ElementNode {
    /// Create a new ElementNode.
    ///
    /// The `key` slot is initialised to `None`; callers that have the
    /// originating `View::key()` in scope set it via the in-crate
    /// `set_key` accessor immediately after construction. The two
    /// production call sites (`ElementTree::mount_root_with_pipeline_owner`
    /// and `ElementTree::insert`) thread the key in immediately
    /// after `ElementNode::new` so the field is populated before
    /// the element is returned.
    pub fn new(element: Box<dyn ElementBase>, parent: Option<ElementId>, slot: usize) -> Self {
        let depth = if parent.is_some() { 1 } else { 0 }; // Will be updated by tree
        Self {
            element,
            parent,
            depth,
            slot,
            key: None,
            registered_global_key_hash: None,
            child_ids: Vec::new(),
        }
    }

    /// Get the Element.
    pub fn element(&self) -> &dyn ElementBase {
        &*self.element
    }

    /// Get the Element mutably.
    pub fn element_mut(&mut self) -> &mut dyn ElementBase {
        &mut *self.element
    }

    /// Get the parent ElementId.
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Get the depth in the tree.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Get the slot index.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Borrow the cloned `View::key()` this element was mounted with,
    /// or `None` for a keyless element.
    ///
    /// Phase 2's keyed reconciler reads this directly to build its
    /// `old_keyed: HashMap<u64, ElementId>` index — no view-typed
    /// `downcast::<V>()` needed. Plan §U7 / FR-022.
    pub fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }

    /// `View::key().map(ViewKey::key_hash)` for this element.
    ///
    /// Convenience over the two-step `key().map(ViewKey::key_hash)`.
    /// Returns `None` for keyless elements (matches Flutter's
    /// "no key, fall back to positional" semantics).
    pub fn key_hash(&self) -> Option<u64> {
        self.key.as_ref().map(|k| k.key_hash())
    }

    /// Replace the stored key.
    ///
    /// Called by `ElementTree` immediately after `ElementNode::new`
    /// (mount path) and at every `update` boundary so the field tracks
    /// the view value the element currently holds. The clone goes
    /// through `ViewKey::clone_key` because `Box<dyn ViewKey>` is not
    /// `Clone` directly.
    pub(crate) fn set_key(&mut self, key: Option<Box<dyn ViewKey>>) {
        self.key = key;
    }

    /// Hash of the `GlobalKey` registered for this element (if any).
    pub fn registered_global_key_hash(&self) -> Option<u64> {
        self.registered_global_key_hash
    }

    /// Borrow this node's parallel, id-based child list.
    ///
    /// The slice is ordered by child slot (entry `i` is slot `i`). It is
    /// the read side of the id-based child model maintained by
    /// [`reconcile_children_by_id`](super::id_reconcile::reconcile_children_by_id);
    /// the single element graph (E3 — atomic box→arena swap).
    pub(crate) fn child_ids(&self) -> &[ElementId] {
        &self.child_ids
    }

    /// Replace this node's parallel, id-based child list.
    ///
    /// Called by [`reconcile_children_by_id`](super::id_reconcile::reconcile_children_by_id)
    /// at the end of a reconcile pass with the freshly computed child
    /// order (one id per new view, in new-view order). Overwrites any
    /// previous list wholesale — the caller is responsible for having
    /// already inserted / removed the corresponding slab nodes so the
    /// stored ids all resolve.
    pub(crate) fn set_child_ids(&mut self, ids: Vec<ElementId>) {
        self.child_ids = ids;
    }
}

impl std::fmt::Debug for ElementNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementNode")
            .field("parent", &self.parent)
            .field("depth", &self.depth)
            .field("slot", &self.slot)
            .field("lifecycle", &self.element.lifecycle())
            .finish()
    }
}

/// Slab-based Element tree storage.
///
/// Provides O(1) access to Elements by ElementId.
/// ElementIds use NonZeroUsize (1-based) while Slab uses 0-based indices.
///
/// # Flutter Equivalent
///
/// This roughly corresponds to how Flutter's Element tree is managed,
/// but uses a Slab for efficient allocation/deallocation.
///
/// # Memory Layout
///
/// ```text
/// ElementTree {
///     nodes: Slab<ElementNode>,  // Contiguous storage
///     root: Option<ElementId>,   // Root element
/// }
/// ```
pub struct ElementTree {
    /// Slab storage for element nodes.
    nodes: Slab<ElementNode>,
    /// Per-slot generation counters, parallel to `nodes` by slab index.
    ///
    /// `generations[i]` is the generation currently *live* in slab slot `i`.
    /// An [`ElementId`] minted against slot `i` carries this value; when the
    /// slot is freed (eager remove / finalize) the counter is bumped, so any
    /// straggler id that still carries the old generation fails the staleness
    /// compare in [`ElementTree::resolve_index`] and resolves to `None`
    /// instead of the unrelated element that later reuses the slot. This is
    /// the use-after-free-by-id guard the old nested `Box` graph never needed
    /// but the slab arena does (ABA safety — Codex E1 P1).
    generations: Vec<NonZeroU32>,
    /// Root element ID.
    root: Option<ElementId>,
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ElementTree {
    /// Create a new empty ElementTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            generations: Vec::new(),
            root: None,
        }
    }

    /// Create an ElementTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            generations: Vec::with_capacity(capacity),
            root: None,
        }
    }

    /// Mint an [`ElementId`] for a freshly-inserted slab slot, threading the
    /// parallel generation counter.
    ///
    /// * Fresh slot (slab grew by one) → append a generation of `1`.
    /// * Reused slot (Slab handed back a previously-freed index) → the slot's
    ///   counter was already bumped at remove time, so reuse the current value.
    ///   The minted id therefore differs from any id that addressed the slot's
    ///   previous occupant.
    ///
    /// # Panics
    ///
    /// Panics if `slab_index` exceeds `u32::MAX` — the element tree cannot hold
    /// more than `u32::MAX` live slots because [`ElementId`] packs the index
    /// into 32 bits. This is the index-cap bound (Codex E1 P2).
    fn alloc_id(&mut self, slab_index: usize) -> ElementId {
        let index = slab_index_to_u32(slab_index);
        let generation = if let Some(&g) = self.generations.get(slab_index) {
            // Reused slot: generation already bumped by the prior remove.
            g
        } else {
            // Fresh slot: the slab grew by exactly one. Seed generation = 1.
            debug_assert_eq!(
                slab_index,
                self.generations.len(),
                "slab must grow by exactly one slot per insert"
            );
            self.generations.push(NonZeroU32::MIN);
            NonZeroU32::MIN
        };
        ElementId::new_gen(index, generation)
    }

    /// Resolve an [`ElementId`] to its live slab index, applying the
    /// generation-staleness compare.
    ///
    /// Returns `None` when the id is stale (its generation no longer matches
    /// the slot's current counter — the slot was freed and possibly reused) or
    /// when the slot is empty. This is the single chokepoint for staleness: all
    /// public accessors route through it, so no call site outside this module
    /// indexes the slab by a raw `id.index()`.
    #[inline]
    fn resolve_index(&self, id: ElementId) -> Option<usize> {
        let index = id.index() as usize;
        if self.generations.get(index).copied() == Some(id.generation())
            && self.nodes.contains(index)
        {
            Some(index)
        } else {
            None
        }
    }

    /// Bump a freed slot's generation so straggler ids that addressed its
    /// previous occupant can never resolve to its next occupant.
    ///
    /// # Panics
    ///
    /// Panics if the slot has been recycled `u32::MAX` times — at which point
    /// the generation can no longer be advanced without wrapping to a value a
    /// stale id might still hold. Retiring on overflow (panic) keeps the ABA
    /// guarantee absolute rather than reintroducing a 1-in-2³² collision
    /// window (Codex E1 P2 generation-overflow policy). `u32::MAX` recycles of
    /// a single slot is unreachable in practice.
    fn bump_generation(&mut self, index: usize) {
        let g = &mut self.generations[index];
        *g = g.checked_add(1).unwrap_or_else(|| {
            panic!(
                "ElementTree: slab slot {index} exhausted u32::MAX generations \
                 (ABA-safety overflow — slot retired)"
            )
        });
    }

    /// Get the root element ID.
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Check if the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the number of elements in the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Mount a View as the root of the tree.
    ///
    /// Returns the ElementId of the root element.
    ///
    /// Note: This method does NOT pass PipelineOwner to the element.
    /// For RenderObjectElements that need PipelineOwner, use
    /// `mount_root_with_pipeline_owner` instead.
    pub fn mount_root(
        &mut self,
        view: &dyn View,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        self.mount_root_with_pipeline_owner(view, None, owner)
    }

    /// Mount a View as the root of the tree with PipelineOwner.
    ///
    /// This method passes the PipelineOwner to the root element before
    /// mounting, which is necessary for RenderObjectElements to create
    /// their RenderObjects.
    ///
    /// # Flutter Equivalent
    ///
    /// In Flutter, this corresponds to `RootWidget.attach(buildOwner,
    /// rootElement)` combined with `_RawViewElement.mount()` which sets up
    /// the PipelineOwner.
    ///
    /// # Arguments
    ///
    /// * `view` - The root View to mount
    /// * `pipeline_owner` - Optional PipelineOwner for render tree management
    /// * `owner` - Split-borrow handle into the `BuildOwner`
    ///   ([`ElementOwner`](crate::ElementOwner)) threaded into the
    ///   element's `mount` call so `GlobalKey` registration / dirty
    ///   scheduling can take effect during initial mount. Plan §U8.
    ///
    /// Returns the ElementId of the root element.
    #[allow(clippy::needless_pass_by_value)] // Arc is cloned into element, taking Option by value is idiomatic
    pub fn mount_root_with_pipeline_owner(
        &mut self,
        view: &dyn View,
        pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        let mut element = view.create_element();

        // Pass PipelineOwner to element BEFORE mounting
        // This is critical for RenderObjectElements to create their RenderObjects
        if let Some(ref pipeline) = pipeline_owner {
            let owner_any: Arc<dyn std::any::Any + Send + Sync> =
                Arc::clone(pipeline) as Arc<dyn std::any::Any + Send + Sync>;
            element.set_pipeline_owner_any(owner_any);
            tracing::debug!(
                "ElementTree::mount_root_with_pipeline_owner: passed PipelineOwner to root element"
            );
        }

        let mut node = ElementNode::new(element, None, 0);
        // Plan §U7 / FR-022: store the cloned `View::key()` on the
        // node so Phase 2's keyed reconciler can index by it without
        // crossing the typed-`V` boundary.
        node.set_key(view.key().map(ViewKey::clone_key));

        let slab_index = self.nodes.insert(node);
        // Mint the generational id from the slot's current generation
        // (fresh slot → gen 1; reused slot → the bumped post-free value).
        let id = self.alloc_id(slab_index);

        // Plan §U15: stamp the element with its own ElementId BEFORE
        // `mount` so the Variable-arity reconciler can read it back
        // when emitting ReconcileEvent's `parent` field.
        self.nodes[slab_index].element.set_self_id(id);

        // Mount the element (now it has PipelineOwner set)
        self.nodes[slab_index].element.mount(None, 0, owner);

        // R13: register GlobalKey on mount. The root element's view is
        // queried here because the dispatch boundary at `Element::mount`
        // can't read the typed `View::key()` (V isn't bounded by View
        // there). Doing the check here keeps the wiring at the level
        // where both `view: &dyn View` and `id` are simultaneously in
        // scope.
        if let Some(hash) = global_key_hash_of(view) {
            register_global_key_with_collision_check(owner, hash, id);
            self.nodes[slab_index].registered_global_key_hash = Some(hash);
        }

        self.root = Some(id);
        id
    }

    /// Insert a new element as a child of the given parent.
    ///
    /// Returns the ElementId of the new element.
    ///
    /// The split-borrow `owner` handle is threaded into the new
    /// element's `mount` call.
    ///
    /// # GlobalKey state migration
    ///
    /// If `view` carries a `GlobalKey` whose hash is already registered
    /// to an element AND that element is currently in the inactive
    /// queue (from a prior soft-remove this frame), the inactive
    /// element is pulled back to the new parent/slot instead of a
    /// fresh element being created. Its `ElementId` and persistent
    /// state survive. Flutter parity:
    /// `framework.dart:4571` `_retakeInactiveElement`.
    ///
    /// Plan §U14 / R14.
    pub fn insert(
        &mut self,
        view: &dyn View,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        // R14 state migration. Before creating a fresh element, check
        // whether `view` has a `GlobalKey` whose hash points at a
        // currently-inactive element. If so, pull it back to the new
        // parent + slot, re-activate, AND apply the new view config
        // (`framework.dart:4581`).
        if let Some(hash) = global_key_hash_of(view)
            && let Some(retaken_id) = try_retake_inactive(self, owner, hash, view, parent, slot)
        {
            return retaken_id;
        }

        let mut element = view.create_element();

        // Read the parent's render-tree propagation context in ONE fresh,
        // immediately-dropped `&ElementTree` borrow, then apply it to the
        // freshly-created child BEFORE mount.
        //
        // E3 (atomic box→arena swap): the old box graph propagated the
        // `PipelineOwner` + parent `RenderId` to children inside
        // `update_or_create_child(ren)`, *before* `mount_children`, because
        // `RenderBehavior::on_mount` creates its `RenderObject` only when a
        // `PipelineOwner` is already in scope. Children are now
        // slab-resident, so that propagate-before-mount ordering moves
        // here: read `pipeline_owner_any()` / `child_render_id()` off the
        // parent node, hand them to the child, then mount.
        let (parent_depth, parent_owner, child_parent_render_id) = match self.get(parent) {
            Some(node) => (
                node.depth,
                node.element().pipeline_owner_any(),
                node.element().child_render_id(),
            ),
            None => (0, None, None),
        };

        if let Some(owner_any) = parent_owner {
            element.set_pipeline_owner_any(owner_any);
        }
        element.set_parent_render_id(child_parent_render_id);

        let mut node = ElementNode::new(element, Some(parent), slot);
        node.depth = parent_depth + 1;
        // Plan §U7 / FR-022.
        node.set_key(view.key().map(ViewKey::clone_key));

        let slab_index = self.nodes.insert(node);
        let id = self.alloc_id(slab_index);

        // Plan §U15: same self-id stamping as mount_root.
        self.nodes[slab_index].element.set_self_id(id);

        // Mount the element (PipelineOwner + parent RenderId already set,
        // so `RenderBehavior::on_mount` can create its RenderObject).
        self.nodes[slab_index]
            .element
            .mount(Some(parent), slot, owner);

        // R13: register the GlobalKey hash → id mapping.
        if let Some(hash) = global_key_hash_of(view) {
            register_global_key_with_collision_check(owner, hash, id);
            self.nodes[slab_index].registered_global_key_hash = Some(hash);
        }

        // A fresh child node appeared at (parent, slot). Emit `Mount` HERE —
        // `insert` is the single site that mints child nodes, and the
        // GlobalKey-retake path above already emitted `Reparent` and returned
        // early, so the two dispositions can never double-fire for one
        // new-side view (the reconciler must NOT also emit `Mount`).
        super::reconcile_event::emit(&super::reconcile_event::ReconcileEvent::mount(
            parent,
            slot,
            view.view_type_id(),
            view.key().map(ViewKey::key_hash),
        ));

        id
    }

    /// Get an element node by ID.
    ///
    /// Returns `None` for a stale id (one that addressed a since-freed slot)
    /// as well as for an absent id — the generational `resolve_index`
    /// staleness check rejects both.
    pub fn get(&self, id: ElementId) -> Option<&ElementNode> {
        let index = self.resolve_index(id)?;
        self.nodes.get(index)
    }

    /// Get an element node mutably by ID.
    ///
    /// Returns `None` for a stale or absent id.
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut ElementNode> {
        let index = self.resolve_index(id)?;
        self.nodes.get_mut(index)
    }

    /// Check if a *live* element with this exact id (index + generation) exists.
    ///
    /// A stale id whose slot was freed (and possibly reused) reports `false`.
    pub fn contains(&self, id: ElementId) -> bool {
        self.resolve_index(id).is_some()
    }

    /// Remove an element from the tree.
    ///
    /// # Soft vs eager removal
    ///
    /// - **Soft (keyed):** If the element carries a `GlobalKey` (i.e.
    ///   `ElementNode::registered_global_key_hash` is `Some`), the
    ///   element is deactivated and pushed onto
    ///   `BuildOwner::inactive_elements` — the slab entry stays alive.
    ///   This enables same-frame state migration: a subsequent
    ///   `insert` with the same GlobalKey pulls the element back via
    ///   `try_retake_inactive` (private). End-of-frame
    ///   [`BuildOwner::finalize_tree`](crate::BuildOwner::finalize_tree) drains any stragglers via
    ///   [`Self::remove_finalized`] (full slab-remove + unregister).
    ///   Flutter parity: `framework.dart:4636` `deactivateChild` +
    ///   `framework.dart:2099` `_InactiveElements`.
    /// - **Eager (un-keyed):** Behaves as before — `Element::unmount`
    ///   then slab-remove. No deferred queue entry.
    ///
    /// This split matches Flutter's behavior where only elements
    /// reachable by `GlobalKey` are deferred; ordinary unmounts are
    /// processed inline.
    ///
    /// Does NOT automatically remove children — caller must handle that.
    ///
    /// Returns the `ElementNode` for an eager removal (so `BuildOwner`
    /// gets back ownership) OR `None` for a soft removal (the node
    /// still lives in the slab). Returns `None` if `id` doesn't exist.
    ///
    /// Plan §U14 / R14. Threads the split-borrow `owner` handle.
    pub fn remove(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Option<ElementNode> {
        // Staleness-checked: a stale id (slot already freed/reused) resolves
        // to `None` here rather than touching the slot's new occupant.
        let index = self.resolve_index(id)?;

        // R14 soft-remove for keyed elements: push to inactive queue
        // without slab-removing. State stays intact for same-frame
        // remount.
        if self.nodes[index].registered_global_key_hash.is_some() {
            let depth = self.nodes[index].depth;
            self.nodes[index].element.deactivate();
            owner.push_inactive(id, depth);
            // Detach from active tree but keep the slot alive.
            self.nodes[index].parent = None;

            if self.root == Some(id) {
                self.root = None;
            }

            tracing::debug!(
                element_id = ?id,
                hash = ?self.nodes[index].registered_global_key_hash,
                "ElementTree::remove soft-removed keyed element into inactive queue"
            );

            // Soft-remove yields no owned node — the caller doesn't
            // get the element back.
            return None;
        }

        // Eager path for un-keyed elements. Drop any stale
        // `did_change_dependencies` flag (plan §U14) — the dependent
        // leaves the active tree before its rebuild ever runs.
        owner.clear_pending_dependency_change(id);
        self.nodes[index].element.unmount(owner);

        let node = self.nodes.remove(index);
        // Slot freed → bump its generation so any straggler id that still
        // names this slot can never resolve to its next occupant (ABA guard).
        self.bump_generation(index);

        if self.root == Some(id) {
            self.root = None;
        }

        Some(node)
    }

    /// Fully remove an element that has already been unmounted (e.g.
    /// from `BuildOwner::finalize_tree`'s end-of-frame drain).
    ///
    /// This bypasses the soft-remove path even for keyed elements:
    /// the slab entry is freed and the `GlobalKey` registration is
    /// cleared via `ElementOwner::unregister_global_key`. Plan §U14 /
    /// R14. Flutter parity: `framework.dart:2118`
    /// `_unmountAll` — the finalization phase that drains
    /// `_inactiveElements` doesn't push back into the queue.
    pub fn remove_finalized(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Option<ElementNode> {
        // Staleness-checked entry (mirror of `remove`).
        let index = self.resolve_index(id)?;

        // Unregister the GlobalKey if this element had one. We do it
        // BEFORE `unmount` so the registry doesn't briefly resolve to
        // a partially-unmounted element.
        if let Some(hash) = self.nodes[index].registered_global_key_hash.take() {
            owner.unregister_global_key(hash);
        }

        // Drop any stale `did_change_dependencies` flag (plan §U14) —
        // the dependent leaves the tree before its rebuild ever runs.
        owner.clear_pending_dependency_change(id);
        self.nodes[index].element.unmount(owner);

        let node = self.nodes.remove(index);
        // Slot freed → bump its generation (ABA guard, see `remove`).
        self.bump_generation(index);

        if self.root == Some(id) {
            self.root = None;
        }

        Some(node)
    }

    /// Update an element with a new view.
    ///
    /// The view must be compatible (same type) with the existing
    /// element. Threads the split-borrow owner handle into the
    /// update call.
    ///
    /// Plan §U7 / FR-022: re-clones `View::key()` into the node so the
    /// stored key tracks whatever the new view carries. `View::can_update`
    /// (FR-028 / U11) already ensures the keys match on a successful
    /// update — the re-clone preserves that invariant explicitly rather
    /// than relying on the caller having already filtered by it.
    pub fn update(&mut self, id: ElementId, view: &dyn View, owner: &mut crate::ElementOwner<'_>) {
        if let Some(node) = self.get_mut(id) {
            node.element.update(view, owner);
            node.set_key(view.key().map(ViewKey::clone_key));
        }
    }

    /// Mark an element as needing rebuild.
    pub fn mark_needs_build(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element.mark_needs_build();
        }
    }

    /// Deactivate an element (temporary removal).
    pub fn deactivate(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element.deactivate();
        }
    }

    /// Activate an element (re-insertion after deactivation).
    pub fn activate(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element.activate();
        }
    }

    /// Iterate over all live element IDs.
    ///
    /// Each id is minted from the slot's *current* generation, so the yielded
    /// ids round-trip through [`ElementTree::get`] (a `new(index+1)` shortcut
    /// would carry generation 1 and fail the staleness compare on any reused
    /// slot).
    pub fn iter(&self) -> impl Iterator<Item = ElementId> + '_ {
        let generations = &self.generations;
        self.nodes
            .iter()
            .map(move |(index, _)| ElementId::new_gen(slab_index_to_u32(index), generations[index]))
    }

    /// Iterate over all live element nodes.
    pub fn iter_nodes(&self) -> impl Iterator<Item = (ElementId, &ElementNode)> + '_ {
        let generations = &self.generations;
        self.nodes.iter().map(move |(index, node)| {
            let id = ElementId::new_gen(slab_index_to_u32(index), generations[index]);
            (id, node)
        })
    }
}

/// Narrow a live slab index to the 32-bit field [`ElementId`] packs it into.
///
/// Every index reaching this helper names an occupied (or about-to-be-occupied)
/// slot, and [`ElementTree::alloc_id`] is the sole minting path — it routes
/// through here, so an index that exceeds `u32::MAX` fails *at insert time*
/// rather than silently truncating later. The `expect` therefore states a real
/// structural cap (the tree cannot hold more than `u32::MAX` live elements),
/// not a "can't happen".
#[inline]
fn slab_index_to_u32(index: usize) -> u32 {
    u32::try_from(index).expect(
        "ElementTree: slab index exceeds u32::MAX live elements \
         (ElementId packs the slot index into 32 bits)",
    )
}

// ============================================================================
// GlobalKey helpers (plan §U14 / R13, R14)
// ============================================================================

/// Extract the `GlobalKey` hash from a view's `View::key()` result, if
/// any. Returns `None` for un-keyed views and for keyed views whose
/// `ViewKey::is_global_key()` is `false` (e.g. `ValueKey`,
/// `UniqueKey`, `ObjectKey`).
///
/// Centralises the "is this a global key, what's its hash?" check so
/// the mount / soft-remove / retake paths all read it the same way.
fn global_key_hash_of(view: &dyn View) -> Option<u64> {
    let key = view.key()?;
    if key.is_global_key() {
        Some(key.key_hash())
    } else {
        None
    }
}

/// Register the `(hash → id)` mapping on the owner. §I4 hash-collision
/// policy: `debug_assert!` on collision in debug builds (matches
/// Flutter's debug-panic-on-collision via the `assert(...)` inside
/// `BuildOwner._registerGlobalKey` at `framework.dart:3160`). Release
/// builds fall through to last-write-wins with a `tracing::error!` so
/// the application doesn't crash on a stray collision.
fn register_global_key_with_collision_check(
    owner: &mut crate::ElementOwner<'_>,
    hash: u64,
    id: ElementId,
) {
    if let Some(existing) = owner.element_for_global_key(hash)
        && existing != id
    {
        tracing::error!(
            ?hash,
            existing = ?existing,
            new = ?id,
            "GlobalKey hash collision: replacing existing registration"
        );
        #[cfg(debug_assertions)]
        {
            panic!(
                "GlobalKey hash collision: hash {hash} already registered to {existing:?} \
                 but new mount wants {id:?}"
            );
        }
    }
    owner.register_global_key(hash, id);
}

/// State-migration entry point. If `hash` resolves to an element
/// currently in the inactive queue, pop it off and re-attach to
/// `(new_parent, new_slot)`. Returns the migrated `ElementId` on
/// success, or `None` when no retakeable element exists (caller falls
/// back to creating a fresh element).
///
/// Flutter parity: `framework.dart:4571` `_retakeInactiveElement`.
fn try_retake_inactive(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    hash: u64,
    view: &dyn View,
    new_parent: ElementId,
    new_slot: usize,
) -> Option<ElementId> {
    let candidate_id = owner.element_for_global_key(hash)?;

    // Only retake if the candidate is actually in the inactive queue.
    // A candidate that's mounted elsewhere in the active tree is a
    // collision, handled by `register_global_key_with_collision_check`.
    if !owner.is_inactive(candidate_id) {
        return None;
    }

    owner.remove_inactive(candidate_id);

    let parent_depth = tree.get(new_parent).map_or(0, ElementNode::depth);

    // Route through the staleness-checked accessor. The candidate came from the
    // live GlobalKey registry and was soft-removed (slot kept, generation NOT
    // bumped), so its generation still matches and this resolves.
    let node = tree.get_mut(candidate_id)?;
    node.parent = Some(new_parent);
    node.slot = new_slot;
    node.depth = parent_depth + 1;

    // Re-activate the element. `Lifecycle::Inactive` → `Active`.
    node.element.activate();

    // Apply the NEW view configuration to the re-taken element. Without
    // this the element keeps the stale view config from before it was
    // deactivated — state persists (the whole point of GlobalKey
    // reparenting) but the view fields, child-list shape, and any
    // update hooks (`didUpdateWidget`-equivalent) would be silently
    // skipped. Flutter's `_retakeInactiveElement` does the same in
    // `framework.dart:4581` (`element.update(newWidget)`) right after
    // activating.
    node.element.update(view, owner);
    // Plan §U7 / FR-022: re-clone the key from the new view value so
    // the stored key tracks the re-taken element's current
    // configuration — the deactivated element's old key may match
    // structurally (`is_global_key` is true on both sides) but the
    // concrete `Box<dyn ViewKey>` is the new view's key now.
    node.set_key(view.key().map(ViewKey::clone_key));

    tracing::debug!(
        candidate = ?candidate_id,
        new_parent = ?new_parent,
        new_slot,
        "ElementTree::insert retook inactive element for GlobalKey state migration"
    );

    // Plan §U17 / SC-003: emit ReconcileEvent::Reparent. The element
    // came from the inactive queue (Lifecycle::Inactive → Active), so
    // `from_parent: None` per ADV-1 branch case 1 — there is no prior
    // *active* parent at the moment of reparent; the donor parent
    // already cleared its slot when it pushed the element into the
    // inactive queue. The cross-parent same-frame Active-to-Active
    // reparent path (ADV-1 branch case 2) requires KTD-9's ID-based
    // Variable storage shape and is deferred — when it lands, that
    // path emits with `from_parent: Some(prior_parent)`.
    super::reconcile_event::emit(&super::reconcile_event::ReconcileEvent {
        kind: super::reconcile_event::ReconcileEventKind::Reparent,
        parent: new_parent,
        child_key: Some(hash),
        slot: new_slot,
        view_type_id: view.view_type_id(),
        from_parent: None,
    });

    Some(candidate_id)
}

impl std::fmt::Debug for ElementTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementTree")
            .field("len", &self.nodes.len())
            .field("root", &self.root)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{IntoView, ViewExt};
    use crate::{BuildContext, BuildOwner, StatelessElement, StatelessView, View};

    #[derive(Clone)]
    struct TestView {
        #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
        name: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }
    }

    #[test]
    fn test_tree_creation() {
        let tree = ElementTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.root().is_none());
    }

    #[test]
    fn test_mount_root() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let view = TestView {
            name: "root".to_string(),
        };

        let id = tree.mount_root(&view, &mut owner.element_owner_mut());

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root(), Some(id));
        assert!(tree.contains(id));
    }

    #[test]
    fn test_insert_child() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root_view = TestView {
            name: "root".to_string(),
        };
        let child_view = TestView {
            name: "child".to_string(),
        };

        let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
        let child_id = tree.insert(&child_view, root_id, 0, &mut owner.element_owner_mut());

        assert_eq!(tree.len(), 2);
        assert!(tree.contains(child_id));

        let child_node = tree.get(child_id).unwrap();
        assert_eq!(child_node.parent(), Some(root_id));
        assert_eq!(child_node.slot(), 0);
        assert_eq!(child_node.depth(), 1);
    }

    #[test]
    fn test_remove() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let view = TestView {
            name: "test".to_string(),
        };

        let id = tree.mount_root(&view, &mut owner.element_owner_mut());
        assert!(tree.contains(id));

        let removed = tree.remove(id, &mut owner.element_owner_mut());
        assert!(removed.is_some());
        assert!(!tree.contains(id));
        assert!(tree.root().is_none());
    }

    // -----------------------------------------------------------------------
    // Generational staleness (ABA safety — Codex E1 P1)
    // -----------------------------------------------------------------------

    /// The core ABA guard: an id that addressed a since-freed slot must NOT
    /// resolve to the unrelated element that later reuses the same slab slot.
    #[test]
    fn stale_id_after_slot_reuse_resolves_none() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = TestView {
            name: "root".to_string(),
        };
        let root_id = tree.mount_root(&root, &mut owner.element_owner_mut());

        // Insert child A, then eagerly remove it (un-keyed → slab slot freed,
        // generation bumped).
        let child_a = TestView {
            name: "a".to_string(),
        };
        let id_a = tree.insert(&child_a, root_id, 0, &mut owner.element_owner_mut());
        assert!(tree.remove(id_a, &mut owner.element_owner_mut()).is_some());

        // Insert child B — the slab hands back the slot A just vacated.
        let child_b = TestView {
            name: "b".to_string(),
        };
        let id_b = tree.insert(&child_b, root_id, 0, &mut owner.element_owner_mut());

        // Same slot, different generation → distinct ids.
        assert_eq!(
            id_a.index(),
            id_b.index(),
            "test precondition: slab must reuse the freed slot"
        );
        assert_ne!(id_a, id_b, "reused slot must mint a distinct generation");
        assert_eq!(id_b.generation().get(), 2, "reused slot generation = 2");

        // The stale id resolves to None; the live id resolves to B.
        assert!(
            tree.get(id_a).is_none(),
            "stale id must NOT resolve to the slot's new occupant"
        );
        assert!(!tree.contains(id_a));
        assert!(tree.get(id_b).is_some(), "live id must resolve");
        assert!(tree.contains(id_b));
        // A stale remove must be a no-op, not a removal of B.
        assert!(tree.remove(id_a, &mut owner.element_owner_mut()).is_none());
        assert!(tree.contains(id_b), "stale remove must not touch B");
    }

    /// Fresh slots seed generation 1; a reused slot advances to the bumped
    /// value. White-box check on the parallel `generations` vec.
    #[test]
    fn reused_slot_increments_generation() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = TestView {
            name: "root".to_string(),
        };
        let root_id = tree.mount_root(&root, &mut owner.element_owner_mut());
        assert_eq!(tree.generations[0].get(), 1, "fresh root slot is gen 1");

        let child = TestView {
            name: "c".to_string(),
        };
        let id1 = tree.insert(&child, root_id, 0, &mut owner.element_owner_mut());
        let slot = id1.index() as usize;
        assert_eq!(tree.generations[slot].get(), 1);

        tree.remove(id1, &mut owner.element_owner_mut());
        assert_eq!(
            tree.generations[slot].get(),
            2,
            "eager remove bumps the freed slot's generation"
        );

        let id2 = tree.insert(&child, root_id, 0, &mut owner.element_owner_mut());
        assert_eq!(id2.index() as usize, slot, "slab reuses the slot");
        assert_eq!(id2.generation().get(), 2, "reused id carries the bump");
    }

    /// Every id produced by `iter`/`iter_nodes` must round-trip through the
    /// staleness-checked accessors — including ids for reused slots, which a
    /// `new(index+1)` shortcut (generation 1) would have failed to resolve.
    #[test]
    fn iter_yields_round_trippable_ids_after_reuse() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = TestView {
            name: "root".to_string(),
        };
        let root_id = tree.mount_root(&root, &mut owner.element_owner_mut());
        let child = TestView {
            name: "c".to_string(),
        };
        let id1 = tree.insert(&child, root_id, 0, &mut owner.element_owner_mut());
        tree.remove(id1, &mut owner.element_owner_mut());
        let _id2 = tree.insert(&child, root_id, 0, &mut owner.element_owner_mut());

        let ids: Vec<_> = tree.iter().collect();
        assert_eq!(ids.len(), tree.len());
        for id in &ids {
            assert!(
                tree.get(*id).is_some(),
                "iter id {id} must resolve — generation must match the live slot"
            );
        }
        assert_eq!(tree.iter_nodes().count(), tree.len());
    }

    /// Generation-overflow policy (Codex E1 P2): a slot recycled `u32::MAX`
    /// times retires by panic rather than wrapping to a value a stale id might
    /// still hold. We drive the boundary directly by pinning the slot's
    /// counter to `u32::MAX` and freeing it once more.
    #[test]
    #[should_panic(expected = "exhausted u32::MAX generations")]
    fn generation_overflow_retires_slot_by_panic() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = TestView {
            name: "root".to_string(),
        };
        let _root_id = tree.mount_root(&root, &mut owner.element_owner_mut());

        // Pin slot 0 to the maximum generation and address it with a matching id.
        tree.generations[0] = NonZeroU32::MAX;
        let saturated = ElementId::new_gen(0, NonZeroU32::MAX);

        // Eager remove resolves (generation matches) then attempts the bump,
        // which overflows and panics.
        let _ = tree.remove(saturated, &mut owner.element_owner_mut());
    }
}
