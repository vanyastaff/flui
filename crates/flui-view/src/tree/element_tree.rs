//! Slab-based Element tree storage.
//!
//! Elements are stored in a Slab for O(1) access by ElementId.
//! This follows Flutter's approach where Elements form the retained tree.

use std::any::TypeId;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::{ElementId, RenderId, ViewKey};
use flui_rendering::{parent_data::SliverMultiBoxAdaptorParentData, pipeline::PipelineCell};
use slab::Slab;

use crate::element::ElementKind;
use crate::view::{ElementBase, View};

fn append_sparse_sliver_children(
    render_tree: &flui_rendering::storage::RenderTree,
    parent_render: RenderId,
    desired_children: &mut Vec<RenderId>,
) {
    // Lazy sliver children intentionally stay out of ElementNode::child_ids;
    // their render order is recovered from SliverMultiBoxAdaptorParentData.
    let Some(parent_node) = render_tree.get(parent_render) else {
        return;
    };

    let desired_membership: HashSet<_> = desired_children.iter().copied().collect();
    let mut sparse_children = parent_node
        .children()
        .iter()
        .copied()
        .filter(|child| !desired_membership.contains(child))
        .filter_map(|child| {
            let child_node = render_tree.get(child)?;
            if child_node.parent() != Some(parent_render) {
                return None;
            }
            let index = child_node
                .parent_data()?
                .downcast_ref::<SliverMultiBoxAdaptorParentData>()?
                .index;
            Some((index, child))
        })
        .collect::<Vec<_>>();

    sparse_children.sort_by_key(|&(index, child)| (index, child));
    desired_children.extend(sparse_children.into_iter().map(|(_, child)| child));
}

/// A node in the Element tree.
///
/// Contains the Element plus metadata for tree traversal.
pub struct ElementNode {
    /// The actual Element.
    ///
    /// Normally `Some`. A `None` hole exists ONLY transiently inside
    /// [`BuildOwner::build_scope`](crate::BuildOwner), between
    /// [`ElementTree::take_element`] and [`ElementTree::put_element`], while
    /// the element runs its own `build()` against a live read view of the
    /// rest of the tree. By-value extraction is what lets the element be
    /// `&mut`-borrowed AND hand the slab a `&` borrow at the same time
    /// without aliasing. Outside that window every accessor assumes `Some`;
    /// during it, ancestor walks read [`Self::element_opt`] (which returns
    /// `None` for the hole rather than panicking).
    pub(crate) kind: Option<ElementKind>,
    /// Parent Element ID (None for root).
    pub(crate) parent: Option<ElementId>,
    /// Depth in the tree (root = 0).
    pub(crate) depth: usize,
    /// Slot index within parent's children.
    pub(crate) slot: usize,
    /// Cloned `View::key()` for the view this element currently holds,
    /// or `None` when the view is keyless.
    ///
    /// FR-022. Populated at every `insert`/`mount_root_*`
    /// call site (cloned via `ViewKey::clone_key`) and re-cloned at
    /// every `update` boundary so the field stays in lock-step with
    /// the view value the element actually holds. The keyed
    /// reconciler reads this field directly via `key()` / `key_hash()`
    /// — no `downcast::<V>()` needed.
    ///
    /// Coexists with `registered_global_key`, which narrows this to just
    /// the keys that are `GlobalKey`s and were actually registered.
    pub(crate) key: Option<Box<dyn ViewKey>>,
    /// The `GlobalKey` registered for this element, if any.
    ///
    /// Set at mount time by `ElementTree::insert` /
    /// `::mount_root_with_pipeline_owner` when the view's
    /// `View::key()` returns a key whose `ViewKey::is_global_key()` is
    /// `true`. Read at end-of-frame `BuildOwner::finalize_tree` to
    /// unregister the entry from `BuildOwner::global_keys`.
    ///
    /// Stored by value rather than as a hash: the registry it unregisters
    /// from resolves by key *identity* (`ViewKey::key_eq`), using the hash
    /// only to pick a bucket, so a hash alone could unregister the wrong
    /// entry whenever two distinct keys collide.
    ///
    /// Flutter parity: keys are tracked on the
    /// element itself in `framework.dart:2884`-ish via `Element._widget`
    ///   + `Widget.key`; we mirror the effect with a side channel
    ///     because our `View` value is owned by `ElementCore` and not
    ///     available at the dispatch boundary used for finalization.
    pub(crate) registered_global_key: Option<Box<dyn ViewKey>>,
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
    /// The set of [`InheritedView`](crate::view::InheritedView) providers in
    /// scope at this node — `provider view TypeId → provider ElementId` — so
    /// `BuildContext::depend_on_inherited` / `get_inherited` resolve the
    /// nearest `P` provider in **O(1)** instead of an O(depth) ancestor walk.
    /// (Only those two inherited lookups read this map; the `find_ancestor_*`
    /// family still walks, as it matches arbitrary — not just inherited —
    /// ancestor types.)
    ///
    /// Built top-down at [`insert`](ElementTree::insert) /
    /// [`mount_root_with_pipeline_owner`](ElementTree::mount_root_with_pipeline_owner):
    /// a non-provider aliases its parent's map by refcount (`Arc::clone`, the
    /// `framework.dart:5129` pointer-copy); a provider stores
    /// `parent_map + (view_type_id → self)` so nested same-type providers
    /// shadow nearest-wins. Recomputed for a re-taken subtree on GlobalKey
    /// reparent. Like `parent`/`depth`/`child_ids` it is a node field, so it
    /// survives the `build_scope` take/put window and a building element can
    /// read its own scope while its `element` slot is a hole.
    ///
    /// Flutter parity: `Element._inheritedElements` (`framework.dart:5053`,
    /// `_updateInheritance` at `:5127`/`:6270`). flui keys on the provider
    /// view `TypeId` (== Flutter's `widget.runtimeType`) and uses a plain
    /// `Arc<HashMap>` with copy-on-insert-at-providers rather than a persistent
    /// HAMT — provider counts in a UI scope are tiny, so the per-provider
    /// O(k) clone is effectively O(1) and avoids a new dependency.
    pub(crate) inherited: Arc<HashMap<TypeId, ElementId>>,
}

/// Compute a child node's inherited scope from its parent's.
///
/// A non-provider returns the parent map unchanged (`Arc::clone` — refcount
/// bump, no allocation); a provider returns `parent_map + (view_type_id →
/// id)`, so the nearest same-type provider shadows. Average/worst case O(k)
/// where k = providers in scope (only at provider nodes; tiny in practice).
///
/// A provider's resolved scope therefore includes ITSELF — so
/// `depend_on::<P>()` from inside a `P` provider's own build resolves that
/// provider, where the old strict-ancestor walk skipped self and found the
/// next `P` up (or `None`). This is an intentional, Flutter-faithful shift
/// (`_updateInheritance` puts `this` into `_inheritedElements`,
/// `framework.dart:6274`); it is currently unreachable because
/// `InheritedBehavior::build_into_views` only returns the child and never
/// self-depends.
fn compute_inherited_scope(
    parent_map: &Arc<HashMap<TypeId, ElementId>>,
    element: &dyn ElementBase,
    id: ElementId,
) -> Arc<HashMap<TypeId, ElementId>> {
    if element.as_inherited().is_some() {
        let mut map = (**parent_map).clone();
        map.insert(element.view_type_id(), id);
        Arc::new(map)
    } else {
        Arc::clone(parent_map)
    }
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
    pub fn new(kind: ElementKind, parent: Option<ElementId>, slot: usize) -> Self {
        let depth = usize::from(parent.is_some()); // Will be updated by tree
        Self {
            kind: Some(kind),
            parent,
            depth,
            slot,
            key: None,
            registered_global_key: None,
            child_ids: Vec::new(),
            // Empty until the tree sets the real scope against the parent's
            // map (insert / mount_root_*), mirroring how `key`/`depth` are
            // finalised by the caller right after construction.
            inherited: Arc::new(HashMap::new()),
        }
    }

    /// The nearest in-scope [`InheritedView`](crate::view::InheritedView)
    /// provider whose view type is `type_id`, in O(1).
    ///
    /// This is the resolved scope at THIS node: for a non-provider it is the
    /// parent's set; for a provider it also includes itself. Build-time
    /// `depend_on` / `find_ancestor` read it via the node (which outlives the
    /// `build_scope` element hole).
    pub(crate) fn inherited_provider(&self, type_id: TypeId) -> Option<ElementId> {
        self.inherited.get(&type_id).copied()
    }

    /// Message for the `expect` in the element accessors — the element is
    /// absent only inside the `build_scope` take/put window.
    const ELEMENT_PRESENT: &'static str =
        "ElementNode::element accessed while extracted by build_scope (take/put window)";

    /// Get the Element.
    ///
    /// # Panics
    ///
    /// Panics if the element is currently extracted — the transient hole
    /// `build_scope` opens between `take_element` and `put_element` while it
    /// builds the node by value. Any lookup that can run *during* a build
    /// (every `BuildCtx` ancestor walk) must go through [`Self::element_opt`]
    /// instead, so reaching the in-flight node is a clean miss, not a panic.
    pub fn element(&self) -> &dyn ElementBase {
        self.kind.as_ref().expect(Self::ELEMENT_PRESENT).element()
    }

    /// Get the Element mutably.
    ///
    /// # Panics
    ///
    /// Panics on the same extracted-element hole as [`Self::element`] — see
    /// its `# Panics` note.
    pub fn element_mut(&mut self) -> &mut dyn ElementBase {
        self.kind
            .as_mut()
            .expect(Self::ELEMENT_PRESENT)
            .element_mut()
    }

    /// Get the Element, or `None` if it is currently extracted (the
    /// transient `build_scope` hole — see [`Self::element`]).
    ///
    /// Build-time ancestor walks use this instead of [`Self::element`] so a
    /// lookup that reaches the in-flight node returns a clean miss in every
    /// build profile rather than panicking.
    pub fn element_opt(&self) -> Option<&dyn ElementBase> {
        self.kind.as_ref().map(ElementKind::element)
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
    /// The keyed reconciler reads this directly to build its
    /// `old_keyed: HashMap<u64, ElementId>` index — no view-typed
    /// `downcast::<V>()` needed. FR-022.
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

    /// The `GlobalKey` registered for this element (if any).
    pub fn registered_global_key(&self) -> Option<&dyn ViewKey> {
        self.registered_global_key.as_deref()
    }

    /// Hash of the `GlobalKey` registered for this element (if any).
    ///
    /// A derived convenience over [`Self::registered_global_key`] for
    /// tracing and tests. Never an identity: the registry that owns the
    /// registration resolves by `ViewKey::key_eq`.
    pub fn registered_global_key_hash(&self) -> Option<u64> {
        self.registered_global_key.as_ref().map(|k| k.key_hash())
    }

    /// Borrow this node's parallel, id-based child list.
    ///
    /// The slice is ordered by child slot (entry `i` is slot `i`). It is
    /// the read side of the id-based child model maintained by the id-based
    /// reconciler (`super::id_reconcile::reconcile_children_by_id`); the
    /// single element graph (E3 — atomic box→arena swap).
    ///
    /// Public because it is the only way to observe a parent's own view of
    /// its children — [`Self::parent`] answers the child's side, and the two
    /// disagreeing is exactly the dangling edge the duplicate-`GlobalKey`
    /// repair exists to clear. Read-only: the write side stays crate-internal
    /// to the reconciler.
    pub fn child_ids(&self) -> &[ElementId] {
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
            .field(
                "lifecycle",
                &self.kind.as_ref().map(|k| k.element().lifecycle()),
            )
            .finish_non_exhaustive()
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
    /// but the slab arena does (ABA safety).
    generations: Vec<NonZeroU32>,
    /// Root element ID.
    root: Option<ElementId>,
    /// Set whenever a render-bearing element is inserted, so the post-build
    /// pass ([`reorder_render_children_after_build`]) knows a render child may
    /// have attached out of element-slot order (a component ancestor — a
    /// `StatelessView`/`ParentDataView` — builds its render descendant in a
    /// *later* `build_scope` iteration than a render sibling that already
    /// appended itself). Cleared by that pass. No insert ⇒ no reorder work.
    ///
    /// [`reorder_render_children_after_build`]: ElementTree::reorder_render_children_after_build
    needs_render_reorder: bool,
    reconcile_state: Rc<ReconcileState>,
}

#[derive(Debug, Default)]
struct ReconcileState {
    active_parent: Cell<Option<ElementId>>,
}

/// Unwind-safe scope for one non-reentrant child reconciliation.
///
/// Current reconciliation is deliberately non-reentrant. A future nested
/// reconciler must replace the single active parent with a stack and preserve
/// the forgotten-child protocol at every level before nesting is enabled.
pub(super) struct ReconcileGuard {
    state: Rc<ReconcileState>,
}

impl Drop for ReconcileGuard {
    fn drop(&mut self) {
        self.state.active_parent.set(None);
    }
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
            needs_render_reorder: false,
            reconcile_state: Rc::new(ReconcileState::default()),
        }
    }

    /// Create an ElementTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            generations: Vec::with_capacity(capacity),
            root: None,
            needs_render_reorder: false,
            reconcile_state: Rc::new(ReconcileState::default()),
        }
    }

    pub(super) fn begin_reconcile(&self, parent_id: ElementId) -> ReconcileGuard {
        assert!(
            self.reconcile_state.active_parent.get().is_none(),
            "BUG: child reconciliation is non-reentrant; nested reconciliation must first implement a stacked forgotten-child protocol",
        );
        self.reconcile_state.active_parent.set(Some(parent_id));
        ReconcileGuard {
            state: Rc::clone(&self.reconcile_state),
        }
    }

    fn is_reconciling_parent(&self, parent_id: ElementId) -> bool {
        self.reconcile_state.active_parent.get() == Some(parent_id)
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
    /// into 32 bits. This is the index-cap bound.
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
    /// window under the generation-overflow policy. `u32::MAX` recycles of
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
    ///   scheduling can take effect during initial mount.
    ///
    /// Returns the ElementId of the root element.
    pub fn mount_root_with_pipeline_owner(
        &mut self,
        view: &dyn View,
        pipeline_owner: Option<PipelineCell>,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        let mut element = view.create_element();

        // Pass the pipeline cell to the element BEFORE mounting.
        // This is critical for RenderObjectElements to create their RenderObjects
        if let Some(ref pipeline) = pipeline_owner {
            element.element_mut().set_pipeline_owner(pipeline.clone());
            tracing::debug!(
                "ElementTree::mount_root_with_pipeline_owner: passed PipelineCell to root element"
            );
        }

        let mut node = ElementNode::new(element, None, 0);
        // FR-022: store the cloned `View::key()` on the
        // node so the keyed reconciler can index by it without
        // crossing the typed-`V` boundary.
        node.set_key(view.key().map(ViewKey::clone_key));

        let slab_index = self.nodes.insert(node);
        // Mint the generational id from the slot's current generation
        // (fresh slot → gen 1; reused slot → the bumped post-free value).
        let id = self.alloc_id(slab_index);

        // Stamp the element with its own ElementId BEFORE
        // `mount` so the Variable-arity reconciler can read it back
        // when emitting ReconcileEvent's `parent` field.
        self.nodes[slab_index].element_mut().set_self_id(id);

        // Root inherited scope: empty parent map, plus self if the root is
        // itself a provider. Set before `mount` (see `insert`).
        self.nodes[slab_index].inherited = {
            let empty = Arc::new(HashMap::new());
            let node = &self.nodes[slab_index];
            compute_inherited_scope(&empty, node.element(), id)
        };

        // Mount the element (now it has PipelineOwner set)
        self.nodes[slab_index].element_mut().mount(None, 0, owner);

        // Register GlobalKey on mount. The root element's view is
        // queried here because the dispatch boundary at `Element::mount`
        // can't read the typed `View::key()` (V isn't bounded by View
        // there). Doing the check here keeps the wiring at the level
        // where both `view: &dyn View` and `id` are simultaneously in
        // scope.
        //
        // No reservation is recorded here: a reservation names the PARENT
        // that declared the key, and a root has none. Flutter reaches the
        // same place from the other direction — reservations are recorded
        // in `Element.updateChild`, which a root mount never runs.
        if let Some(key) = global_key_of(view) {
            register_global_key_with_collision_check(owner, key, id);
            self.nodes[slab_index].registered_global_key = Some(key.clone_key());
        }

        // ADR-0040: a fresh root was minted and mounted.
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_mounted(&flui_foundation::observe::ElementMounted::new(
                id,
                None,
                0,
                view.view_type_id(),
            ));
        });

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
    /// If `view` carries a `GlobalKey` that is already registered
    /// to an element AND that element is currently in the inactive
    /// queue (from a prior soft-remove this frame), the inactive
    /// element is pulled back to the new parent/slot instead of a
    /// fresh element being created. Its `ElementId` and persistent
    /// state survive. Flutter parity:
    /// `framework.dart:4571` `_retakeInactiveElement`.
    pub fn insert(
        &mut self,
        view: &dyn View,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        self.insert_with_provisional_order(view, parent, slot, owner, &[], &[])
    }

    pub(super) fn insert_during_reconcile(
        &mut self,
        view: &dyn View,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
        reconciled_prefix: &[ElementId],
        unclaimed_old_slots: &[Option<ElementId>],
    ) -> ElementId {
        self.insert_with_provisional_order(
            view,
            parent,
            slot,
            owner,
            reconciled_prefix,
            unclaimed_old_slots,
        )
    }

    fn insert_with_provisional_order(
        &mut self,
        view: &dyn View,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
        reconciled_prefix: &[ElementId],
        unclaimed_old_slots: &[Option<ElementId>],
    ) -> ElementId {
        // ADV-1 state migration. Before creating a fresh element,
        // check whether `view` has a `GlobalKey` that points at an
        // existing element. If it is inactive, pull it back; if it is still
        // active under a different parent, forget it from that parent and move
        // it here. In both cases the `ElementId` and state survive.
        if let Some(key) = global_key_of(view) {
            let destination = RetakeDestination {
                parent,
                slot,
                reconciled_prefix,
                unclaimed_old_slots,
            };
            match try_retake_global_key(self, owner, key, view, destination) {
                GlobalKeyRetake::Absent => {}
                GlobalKeyRetake::Retaken(retaken_id) => {
                    // A parent declaring a keyed child has made a claim on
                    // that key for this frame, whether the element was
                    // grafted or freshly mounted. The frame boundary reads
                    // the claims back to tell a legal reparent (one
                    // claimant) from a duplicate (two).
                    owner.reserve_global_key(parent, retaken_id, key);
                    return retaken_id;
                }
                GlobalKeyRetake::Rejected => panic!(
                    "BUG: existing GlobalKey candidate failed relocation preflight; refusing to mount a duplicate"
                ),
            }
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
        // here: read `pipeline_owner()` / `child_render_id()` off the
        // parent node, hand them to the child, then mount.
        let (parent_depth, parent_owner, child_parent_render_id, parent_inherited) =
            match self.get(parent) {
                Some(node) => (
                    node.depth,
                    node.element().pipeline_owner(),
                    node.element().child_render_id(),
                    Arc::clone(&node.inherited),
                ),
                None => (0, None, None, Arc::new(HashMap::new())),
            };

        if let Some(pipeline_owner) = parent_owner {
            element.element_mut().set_pipeline_owner(pipeline_owner);
        }
        element
            .element_mut()
            .set_parent_render_id(child_parent_render_id);

        let mut node = ElementNode::new(element, Some(parent), slot);
        node.depth = parent_depth + 1;
        // FR-022.
        node.set_key(view.key().map(ViewKey::clone_key));

        let slab_index = self.nodes.insert(node);
        let id = self.alloc_id(slab_index);

        // Same self-id stamping as mount_root.
        self.nodes[slab_index].element_mut().set_self_id(id);

        // Resolve this child's inherited scope from the parent's now that the
        // element knows whether it is itself a provider (`as_inherited`) and
        // its `view_type_id`. Computed before `mount` so an
        // `InheritedBehavior::on_mount` (or any mount-time lookup) already sees
        // its own scope.
        self.nodes[slab_index].inherited = {
            let node = &self.nodes[slab_index];
            compute_inherited_scope(&parent_inherited, node.element(), id)
        };

        // Mount the element (PipelineOwner + parent RenderId already set,
        // so `RenderBehavior::on_mount` can create its RenderObject).
        self.nodes[slab_index]
            .element_mut()
            .mount(Some(parent), slot, owner);

        // Register the GlobalKey → id mapping, and record the declaration
        // for the frame boundary's duplicate check.
        if let Some(key) = global_key_of(view) {
            register_global_key_with_collision_check(owner, key, id);
            self.nodes[slab_index].registered_global_key = Some(key.clone_key());
            owner.reserve_global_key(parent, id, key);
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

        // ADR-0040: same single-mint funnel as the tracing Mount above — the
        // GlobalKey-retake early-return can never reach this point, so a
        // retake never double-fires as a mount.
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_mounted(&flui_foundation::observe::ElementMounted::new(
                id,
                Some(parent),
                slot,
                view.view_type_id(),
            ));
        });

        // A freshly-attached render child reads the parent-data its nearest
        // ancestor `ParentDataView` (`Expanded`, `Positioned`) contributes — the
        // E3 analogue of Flutter's `RenderObjectElement.attachRenderObject`
        // calling `_findAncestorParentDataElement`.
        self.apply_ancestor_parent_data(id);

        // A render-bearing child appended itself to its render parent in
        // *attach* order, which only matches element-slot order when no
        // component ancestor deferred its build. Flag a post-build reorder so
        // the render children settle into slot order regardless.
        if self
            .get(id)
            .is_some_and(|node| node.element().render_id().is_some())
        {
            self.needs_render_reorder = true;
        }

        id
    }

    /// Install or update the nearest ancestor `ParentDataView`'s typed
    /// configuration on `child_id`'s render node.
    ///
    /// No-op unless `child_id` owns a render node. Walks strictly upward from
    /// the child, taking the *nearest* `parent_data_config()` and stopping the
    /// search at the first ancestor render object (the render parent that reads
    /// the data during layout) — Flutter's `_findAncestorParentDataElement`
    /// fused with `_findAncestorRenderObjectElement`. The nearest config wins.
    /// Existing data is mutated through the typed hook so layout-owned fields
    /// survive; its returned impact controls the render-parent dirty work.
    ///
    /// Average case O(1) — for a plain render child the very first ancestor is
    /// the render parent, so the walk stops in one hop and never touches the
    /// pipeline owner. Worst case O(proxy-nesting depth) between the render
    /// child and its render parent.
    ///
    /// The element tree and render tree are disjoint stores. The typed element
    /// hook reads widget configuration while the pipeline checkout mutates only
    /// the render node.
    fn apply_ancestor_parent_data(&mut self, child_id: ElementId) {
        let Some(child) = self.get(child_id) else {
            return;
        };
        let Some(child_render_id) = child.element().render_id() else {
            return;
        };
        let pipeline_owner = child.element().pipeline_owner();
        let mut cursor = child.parent();

        let mut nearest_parent_data_element: Option<ElementId> = None;
        let mut parent_render_id: Option<flui_foundation::RenderId> = None;
        while let Some(ancestor_id) = cursor {
            let Some(node) = self.get(ancestor_id) else {
                break;
            };
            if let Some(render_id) = node.element().render_id() {
                parent_render_id = Some(render_id);
                break;
            }
            if nearest_parent_data_element.is_none()
                && node.element().parent_data_config().is_some()
            {
                nearest_parent_data_element = Some(ancestor_id);
            }
            cursor = node.parent();
        }

        // Nothing to apply unless a ParentDataView sits between this render
        // child and its render parent.
        let Some(parent_data_element_id) = nearest_parent_data_element else {
            return;
        };
        let Some(pipeline_owner) = pipeline_owner else {
            return;
        };

        let parent_data_element = self
            .get(parent_data_element_id)
            .expect("BUG: located ParentDataView element must remain live");
        // The element-tree borrow and render-tree checkout are disjoint.
        pipeline_owner.with_mut(|owner| {
            let impact = {
                let Some(node) = owner.render_tree_mut().get_mut(child_render_id) else {
                    return;
                };
                if let Some(parent_data) = node.parent_data_mut() {
                    parent_data_element
                        .element()
                        .apply_parent_data_config(parent_data)
                } else {
                    let config = parent_data_element
                        .element()
                        .parent_data_config()
                        .expect("BUG: located ParentDataView must create parent data");
                    node.set_parent_data(config);
                    flui_rendering::RenderUpdateImpact::NONE
                }
            };
            if let Some(parent_render_id) = parent_render_id {
                owner.apply_render_update_impact(parent_render_id, impact);
            }
        });
    }

    fn reset_ancestor_parent_data(&mut self, child_id: ElementId) {
        let Some(child) = self.get(child_id) else {
            return;
        };
        let Some(render_id) = child.element().render_id() else {
            return;
        };
        if let Some(pipeline) = child.element().pipeline_owner() {
            pipeline.with_mut(|owner| {
                if let Some(node) = owner.render_tree_mut().get_mut(render_id) {
                    let _ = node.clear_parent_data();
                }
            });
        }
        self.apply_ancestor_parent_data(child_id);
    }

    /// Reorder every render object's children to match element-slot order.
    ///
    /// A render child appends itself to its render parent during mount, so when
    /// a component ancestor (a `StatelessView` / `ParentDataView`) builds its
    /// render descendant in a *later* `build_scope` iteration than a render
    /// sibling that already attached, the parent's children list ends up in
    /// attach order, not slot order. This single post-build pass walks the
    /// element tree depth-first in slot order, derives each render parent's
    /// correct child sequence, and rewrites only those that drifted — the
    /// arena analogue of Flutter slotting each child via `insertRenderObjectChild`.
    ///
    /// No-op unless insertion or committed child reconciliation set
    /// `needs_render_reorder`.
    /// Average/worst case O(element-tree size) for the DFS, plus O(children) per
    /// drifted render parent. The element walk completes before the pipeline
    /// owner is locked; the lock guards only the render tree.
    ///
    /// # Exempt from [`adopt_child`](flui_rendering::storage::RenderTree::adopt_child)
    ///
    /// `adopt_child` is the single-edge primitive for attaching ONE freshly
    /// mounted child under its parent. This pass is a different shape: a
    /// bulk diff of a parent's WHOLE children list against the element-slot
    /// order, so it writes the parent-pointer sync loop and the
    /// children-list rewrite loop separately rather than one `adopt_child`
    /// call per child — an `adopt_child`-per-child here would re-derive the
    /// same target list one element at a time for no benefit, and would
    /// still need the separate stale-child eviction the children-list loop
    /// already does. The two loops are still required to leave both edge
    /// directions consistent; a debug-only invariant check at the end of
    /// this function proves that rather than trusting it silently (this is
    /// exactly the invariant the root-hop parent-link defect violated: a
    /// child recorded in its parent's children list while its own `parent`
    /// link disagreed).
    pub(crate) fn reorder_render_children_after_build(&mut self) -> usize {
        if !self.needs_render_reorder {
            return 0;
        }
        self.needs_render_reorder = false;
        self.synchronize_render_children()
    }

    pub(super) fn mark_render_reorder_needed(&mut self) {
        self.needs_render_reorder = true;
    }

    /// Synchronize only the destination's nearest render parent's direct
    /// children for the provisional GlobalKey order.
    ///
    /// This must run before the moved element's update and observer callbacks,
    /// but a global element-tree walk would make each retake O(tree size). The
    /// walk therefore starts at the destination render element, follows only
    /// its logical descendants, and stops at each first render boundary.
    pub(super) fn synchronize_destination_render_children_for_relocation(
        &mut self,
        parent_id: ElementId,
        provisional_children: &[ElementId],
    ) -> usize {
        let mut render_element_id = Some(parent_id);
        let (render_element_id, render_parent_id, pipeline) = loop {
            let Some(element_id) = render_element_id else {
                return 0;
            };
            let Some(node) = self.get(element_id) else {
                return 0;
            };
            if let Some(render_id) = node.element().render_id() {
                let Some(pipeline) = node.element().pipeline_owner() else {
                    return 0;
                };
                break (element_id, render_id, pipeline);
            }
            render_element_id = node.parent();
        };

        let root_children = if render_element_id == parent_id {
            provisional_children
        } else {
            self.get(render_element_id)
                .map_or(&[][..], ElementNode::child_ids)
        };
        let mut desired_children = Vec::new();
        let mut visited = 1;
        let mut stack: Vec<ElementId> = root_children.iter().rev().copied().collect();
        while let Some(element_id) = stack.pop() {
            visited += 1;
            let Some(node) = self.get(element_id) else {
                continue;
            };
            if let Some(render_id) = node.element().render_id() {
                desired_children.push(render_id);
                continue;
            }
            let children = if element_id == parent_id {
                provisional_children
            } else {
                node.child_ids()
            };
            stack.extend(children.iter().rev().copied());
        }

        pipeline.with_mut(|owner| {
            append_sparse_sliver_children(
                owner.render_tree(),
                render_parent_id,
                &mut desired_children,
            );
            let children_match = owner
                .render_tree()
                .get(render_parent_id)
                .is_some_and(|node| node.children() == desired_children.as_slice());
            if children_match {
                return;
            }
            let current = owner
                .render_tree()
                .get(render_parent_id)
                .map_or_else(Vec::new, |node| node.children().to_vec());
            {
                let render_tree = owner.render_tree_mut();
                for &child in &desired_children {
                    if let Some(node) = render_tree.get_mut(child) {
                        node.set_parent(Some(render_parent_id));
                    }
                }
                let Some(parent) = render_tree.get_mut(render_parent_id) else {
                    return;
                };
                for child in current {
                    parent.remove_child(child);
                }
                for (index, child) in desired_children.iter().copied().enumerate() {
                    parent.insert_child(index, child);
                }
            }
            owner.apply_render_update_impact(
                render_parent_id,
                flui_rendering::RenderUpdateImpact::LAYOUT
                    | flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                    | flui_rendering::RenderUpdateImpact::SEMANTICS,
            );
        });
        visited
    }

    fn synchronize_render_children(&mut self) -> usize {
        // Depth-first in slot order, tracking each node's nearest render
        // ancestor. A render node is appended to that ancestor's target order,
        // then becomes the render ancestor for its own subtree.
        let mut target: HashMap<flui_foundation::RenderId, Vec<flui_foundation::RenderId>> =
            HashMap::new();
        let mut desired_parent: HashMap<
            flui_foundation::RenderId,
            Option<flui_foundation::RenderId>,
        > = HashMap::new();
        let mut pipeline_owner: Option<PipelineCell> = None;
        let mut visited = 0;

        let roots: Vec<ElementId> = self
            .iter_nodes()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(id, _)| id)
            .collect();

        // Children are pushed reversed so siblings pop in ascending slot order.
        let mut stack: Vec<(ElementId, Option<flui_foundation::RenderId>)> =
            roots.into_iter().rev().map(|id| (id, None)).collect();
        while let Some((element_id, render_ancestor)) = stack.pop() {
            visited += 1;
            let Some(node) = self.get(element_id) else {
                continue;
            };
            let child_ancestor = if let Some(render_id) = node.element().render_id() {
                desired_parent.insert(render_id, render_ancestor);
                if let Some(parent_render) = render_ancestor {
                    target.entry(parent_render).or_default().push(render_id);
                }
                if pipeline_owner.is_none() {
                    pipeline_owner = node.element().pipeline_owner();
                }
                Some(render_id)
            } else {
                render_ancestor
            };
            let children = node.child_ids();
            for &child in children.iter().rev() {
                stack.push((child, child_ancestor));
            }
        }

        if desired_parent.is_empty() {
            return visited;
        }
        let Some(pipeline_owner) = pipeline_owner else {
            return visited;
        };

        // Tree borrows are dropped; the checkout guards only the render tree.
        pipeline_owner.with_mut(|owner| {
            let mut dirty_render_parents: HashSet<flui_foundation::RenderId> = HashSet::new();
            let mut membership_changed_parents: HashSet<flui_foundation::RenderId> =
                HashSet::new();
            {
                let render_tree = owner.render_tree_mut();
                let render_ids: Vec<_> = render_tree.iter().map(|(id, _)| id).collect();

                // Sync render parent pointers first. Sibling sorting alone is not
                // enough when a GlobalKey move transfers an already-attached
                // render subtree from one render parent to another.
                for render_id in &render_ids {
                    let Some(&desired) = desired_parent.get(render_id) else {
                        continue;
                    };
                    let Some(node) = render_tree.get_mut(*render_id) else {
                        continue;
                    };
                    let current = node.parent();
                    if current != desired {
                        if let Some(parent) = current {
                            dirty_render_parents.insert(parent);
                            membership_changed_parents.insert(parent);
                        }
                        if let Some(parent) = desired {
                            dirty_render_parents.insert(parent);
                            membership_changed_parents.insert(parent);
                        }
                        node.set_parent(desired);
                    }
                }

                // Sync every element-managed render node's child list exactly to
                // element slot order. Parents absent from `target` are render
                // leaves in the element graph, so their desired child list is
                // empty; clearing them removes donor-side stale children after a
                // cross-parent move.
                for parent_render in &render_ids {
                    if !desired_parent.contains_key(parent_render) {
                        continue;
                    }
                    let mut desired_children = target.remove(parent_render).unwrap_or_default();
                    append_sparse_sliver_children(render_tree, *parent_render, &mut desired_children);
                    let Some(parent_node) = render_tree.get_mut(*parent_render) else {
                        continue;
                    };
                    if parent_node.children() == desired_children.as_slice() {
                        continue;
                    }
                    let current = parent_node.children().to_vec();
                    for child in current {
                        parent_node.remove_child(child);
                    }
                    for (target_index, child) in desired_children.into_iter().enumerate() {
                        parent_node.insert_child(target_index, child);
                    }
                    dirty_render_parents.insert(*parent_render);
                }

                // Invariant check (debug-only, O(desired_parent size)): the two
                // loops above write the parent-pointer and the children-list
                // separately, so prove they agree rather than trust it silently.
                // This is exactly the asymmetry the root-hop parent-link defect
                // produced — a child present in its parent's children list with
                // its own `parent` link disagreeing (or missing).
                #[cfg(debug_assertions)]
                for (&render_id, &desired) in &desired_parent {
                    let Some(node) = render_tree.get(render_id) else {
                        continue;
                    };
                    debug_assert_eq!(
                        node.parent(),
                        desired,
                        "reorder pass: render node {render_id:?}'s parent link disagrees with the \
                         desired parent the DFS computed for it — the two directions of a render-tree \
                         edge must always be written consistently"
                    );
                    if let Some(parent_id) = desired {
                        let parent_has_child = render_tree.get(parent_id).is_some_and(
                            |parent_node| parent_node.children().contains(&render_id),
                        );
                        debug_assert!(
                            parent_has_child,
                            "reorder pass: render node {render_id:?} claims parent {parent_id:?}, \
                             but that parent's children list does not contain it"
                        );
                    }
                }
            }

            for parent in dirty_render_parents {
                if owner.render_tree().contains(parent) {
                    let impact = if membership_changed_parents.contains(&parent) {
                        flui_rendering::RenderUpdateImpact::LAYOUT
                            | flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                            | flui_rendering::RenderUpdateImpact::SEMANTICS
                    } else {
                        flui_rendering::RenderUpdateImpact::LAYOUT
                    };
                    owner.apply_render_update_impact(parent, impact);
                }
            }
        });
        visited
    }

    /// Recompute ancestry-derived metadata for the subtree rooted at
    /// `root_id`, top-down against each node's current parent.
    ///
    /// Needed after a GlobalKey reparent ([`try_retake_global_key`]): the moved
    /// subtree carries both inherited scopes and depths derived from its OLD
    /// ancestor chain. Leaving either stale can resolve the wrong provider or
    /// let the dirty heap rebuild a descendant before its moved parent.
    ///
    /// A node is processed only after its parent, so each child observes the
    /// parent's already-updated scope and depth. This is the Rust-native
    /// equivalent of Flutter's recursive `_updateDepth` plus the
    /// `_updateInheritance` work performed while reactivating a subtree.
    /// Average/worst case O(subtree size), paid only for a reparent.
    fn recompute_subtree_ancestry(&mut self, root_id: ElementId) {
        // A `visited` set bounds the walk to each node once. The element tree
        // is acyclic by construction (`child_ids` come from the reconciler),
        // so this never trips in practice — but it converts a malformed
        // `child_ids` cycle from an unbounded hang into clean termination.
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = self.get(id) else {
                continue;
            };
            let (parent_map, depth, parent_render_id) = match node.parent {
                Some(parent_id) => self.get(parent_id).map_or_else(
                    || (Arc::new(HashMap::new()), 0, None),
                    |parent| {
                        (
                            Arc::clone(&parent.inherited),
                            parent.depth + 1,
                            parent.element().child_render_id(),
                        )
                    },
                ),
                None => (Arc::new(HashMap::new()), 0, None),
            };
            let scope = {
                let node = self.get(id).expect("id resolved at loop top");
                compute_inherited_scope(&parent_map, node.element(), id)
            };
            let node = self.get_mut(id).expect("id resolved at loop top");
            node.inherited = scope;
            node.depth = depth;
            node.element_mut().set_parent_render_id(parent_render_id);
            stack.extend_from_slice(&node.child_ids);
        }
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

    /// Extract the element out of node `id`'s slot, leaving a transient
    /// `None` hole (see [`ElementNode::element`]).
    ///
    /// Companion to [`Self::put_element`]:
    /// [`BuildOwner::build_scope`](crate::BuildOwner) takes the element out
    /// so it can run that element's own `build()` against a shared `&` view
    /// of the rest of the slab without aliasing, then puts it back in the
    /// same iteration. Returns `None` for a stale/absent id or an
    /// already-empty slot (a re-entrant take is a framework bug).
    pub(crate) fn take_element(&mut self, id: ElementId) -> Option<ElementKind> {
        let index = self.resolve_index(id)?;
        self.nodes.get_mut(index)?.kind.take()
    }

    /// Restore an element previously removed by [`Self::take_element`] into
    /// node `id`'s slot. No-op for a stale/absent id (cannot happen on the
    /// build path: the node is re-addressed by the same id taken moments
    /// earlier).
    pub(crate) fn put_element(&mut self, id: ElementId, kind: ElementKind) {
        if let Some(index) = self.resolve_index(id)
            && let Some(node) = self.nodes.get_mut(index)
        {
            node.kind = Some(kind);
        }
    }

    /// Remove `dependent` from the forward maps of the exact providers owned
    /// by the reverse index.
    fn release_inherited_providers(
        &mut self,
        dependent: ElementId,
        providers: impl IntoIterator<Item = ElementId>,
    ) {
        for provider in providers {
            if let Some(node) = self.get_mut(provider)
                && let Some(accessor) = node.element_mut().as_inherited_mut()
            {
                accessor.remove_dependent(dependent);
            }
        }
    }

    /// Release active inherited edges at the start of a temporary detach.
    fn deactivate_inherited_dependencies(
        &mut self,
        dependent: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let providers = owner.deactivate_inherited_dependent(dependent);
        self.release_inherited_providers(dependent, providers);
    }

    /// Release every inherited edge and lifecycle marker on permanent teardown.
    fn unmount_inherited_dependencies(
        &mut self,
        dependent: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let providers = owner.unmount_inherited_dependent(dependent);
        self.release_inherited_providers(dependent, providers);
        owner.clear_pending_dependency_change(dependent);
    }

    /// Reactivate a subtree and schedule dependency lifecycle work for every
    /// node that was detached from at least one provider.
    fn activate_subtree(&mut self, root: ElementId, owner: &mut crate::ElementOwner<'_>) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.get(id) else {
                continue;
            };
            let depth = node.depth();
            let children = node.child_ids().to_vec();

            if let Some(node) = self.get_mut(id) {
                node.element_mut().activate();
            }
            if owner.activate_inherited_dependent(id) {
                owner.note_dependency_change(id);
                owner.schedule_build_for(id, depth, crate::RebuildReason::DependencyChange);
            }

            stack.extend(children.into_iter().rev());
        }
    }

    /// Deactivate a subtree and synchronously release every inherited edge.
    ///
    /// Provider maps are clean before this returns, so a notification later in
    /// the same frame cannot enqueue an inactive historical dependent.
    fn deactivate_subtree(&mut self, root: ElementId, owner: &mut crate::ElementOwner<'_>) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let children = self
                .get(id)
                .map(|node| node.child_ids().to_vec())
                .unwrap_or_default();
            self.deactivate_inherited_dependencies(id, owner);
            if let Some(node) = self.get_mut(id) {
                node.element_mut().deactivate();
            }
            stack.extend(children.into_iter().rev());
        }
    }

    /// Remove an element from the tree.
    ///
    /// # Soft vs eager removal
    ///
    /// - **Soft (keyed):** If the element carries a `GlobalKey` (i.e.
    ///   `ElementNode::registered_global_key` is `Some`), the
    ///   element is deactivated and pushed onto
    ///   `BuildOwner::inactive_elements` — the slab entry stays alive.
    ///   This enables same-frame state migration: a subsequent
    ///   `insert` with the same GlobalKey pulls the element back via
    ///   `try_retake_global_key` (private). End-of-frame
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
    /// Threads the split-borrow `owner` handle.
    pub fn remove(
        &mut self,
        id: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Option<ElementNode> {
        // Staleness-checked: a stale id (slot already freed/reused) resolves
        // to `None` here rather than touching the slot's new occupant.
        let index = self.resolve_index(id)?;

        // Soft-remove for keyed elements: push to inactive queue
        // without slab-removing. State stays intact for same-frame
        // remount.
        if self.nodes[index].registered_global_key.is_some() {
            let depth = self.nodes[index].depth;
            let former_parent = self.nodes[index].parent;
            let mut relocation = RenderRelocation {
                pipeline: self.nodes[index].element().pipeline_owner(),
                frontier: collect_render_frontier(self, id).unwrap_or_default(),
                detached_render_subtrees: None,
            };
            if let Some(parent_id) = former_parent
                && let Some(parent) = self.get_mut(parent_id)
            {
                parent.child_ids.retain(|child_id| *child_id != id);
            }
            detach_render_relocation(&mut relocation);
            self.deactivate_subtree(id, owner);
            if let Some(token) = relocation.detached_render_subtrees {
                owner.push_inactive_with_detached_render_subtrees(id, depth, token);
            } else {
                owner.push_inactive(id, depth);
            }
            // Detach from active tree but keep the slot alive.
            self.nodes[index].parent = None;

            if self.root == Some(id) {
                self.root = None;
            }

            tracing::debug!(
                element_id = ?id,
                key = ?self.nodes[index].registered_global_key,
                "ElementTree::remove soft-removed keyed element into inactive queue"
            );

            // Soft-remove yields no owned node — the caller doesn't
            // get the element back.
            return None;
        }

        // Eager path for un-keyed elements. Drop any stale
        // `did_change_dependencies` flag — the dependent
        // leaves the active tree before its rebuild ever runs.
        self.unmount_inherited_dependencies(id, owner);
        self.nodes[index].element_mut().unmount(owner);

        // ADR-0040: `Element::unmount` ran and the slot is about to free —
        // one of the two primitives every unmount path bottoms out in.
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_unmounted(&flui_foundation::observe::ElementUnmounted::new(id));
        });

        let node = self.nodes.remove(index);
        // Slot freed → bump its generation so any straggler id that still
        // names this slot can never resolve to its next occupant (ABA guard).
        self.bump_generation(index);

        if self.root == Some(id) {
            self.root = None;
        }

        Some(node)
    }

    /// Remove `id` and its entire descendant subtree.
    ///
    /// Used by [`crate::element::sparse_children::SparseChildren::evict`] to
    /// evict a lazy sliver child together with all of its own descendant
    /// elements and render nodes (e.g. a `Container(Padding(Text))` child
    /// produces three elements; a single-node `remove` would leak the inner
    /// two). Also the removal path a rebuild takes for any un-keyed child
    /// whose element type changed (`ElementTree::update`'s can't-reuse
    /// branch) — e.g. `LaidOut::pump_widget`'s root-swap in the headless
    /// harness.
    ///
    /// The algorithm mirrors `id_reconcile::remove_child` / `collect_subtree_preorder`:
    ///
    /// 1. Peek whether the root is keyed (`registered_global_key`),
    ///    side-effect-free, before touching anything. A keyed root
    ///    soft-removes via `remove` and returns immediately — descendants
    ///    stay untouched, parked for `finalize_tree`'s deferred drain in
    ///    case the root is reclaimed same-frame via `GlobalKey` — so the
    ///    subtree snapshot below is never needed for this branch and would
    ///    be pure wasted work if taken first.
    /// 2. For an un-keyed root, snapshot the subtree in pre-order (parent
    ///    before children) while all `child_ids` lists are intact.
    /// 3. Free descendants deepest-first via `remove_finalized` BEFORE the
    ///    root's own removal (step 4). Each element's own `unmount` — e.g.
    ///    `RenderView::did_unmount_render_object` for
    ///    `MouseRegion`/`ClipPath`/`Listener`, which unregisters from the
    ///    owner-local interaction lane — needs its OWN render object still
    ///    reachable in the render tree to fire at all
    ///    (`RenderBehavior::on_unmount` looks it up by id and silently skips
    ///    the view-level hook when the lookup misses). Removing the root
    ///    first would cascade-delete every descendant render node in one
    ///    shot (`PipelineOwner::remove_render_object` recurses), so a
    ///    descendant unmounted afterward would find its render object
    ///    already gone and its own unmount hook would silently never run —
    ///    orphaning its interaction-lane registration for the life of the
    ///    owner. Processing descendants first makes each one's own render
    ///    removal a no-op by the time the root's cascade reaches it.
    /// 4. Remove the root via `remove`.
    ///
    /// Complexity: O(n) time + O(n) peak heap for the work-stack (n = subtree
    /// size), O(h) call-stack for the constant-stack iterative walk — paid
    /// only for an un-keyed root; a keyed root is O(1).
    pub(crate) fn remove_subtree(&mut self, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        // A keyed root soft-removes into the inactive queue instead of
        // freeing outright (`remove`'s own branch, keyed on
        // `registered_global_key`); descendants stay untouched. Check
        // this FIRST: it needs no subtree walk, and a keyed root never uses
        // the snapshot below, so paying for that walk before checking would
        // be wasted work for every keyed-root removal.
        let root_is_keyed = self
            .get(id)
            .is_some_and(|node| node.registered_global_key().is_some());

        if root_is_keyed {
            self.remove(id, owner);
            return;
        }

        // Un-keyed root: snapshot the subtree pre-order (parent before
        // children) before touching any node, while every `child_ids` list
        // is still intact.
        let mut subtree: Vec<ElementId> = Vec::new();
        {
            let mut work_stack: Vec<ElementId> = vec![id];
            while let Some(node_id) = work_stack.pop() {
                subtree.push(node_id);
                if let Some(node) = self.get(node_id) {
                    // Push children in reverse slot order so the leftmost child
                    // is popped next — preserves pre-order on a LIFO stack.
                    work_stack.extend(node.child_ids().iter().rev().copied());
                }
            }
        }

        // Free descendants deepest-first -- `subtree[0]` is the root itself
        // (removed below, last); iterating the rest in reverse visits each
        // descendant after all of ITS OWN descendants, so a nested unmount
        // hook (e.g. a `MouseRegion` two levels down) always still finds its
        // own render object present.
        for &descendant in subtree[1..].iter().rev() {
            self.remove_finalized(descendant, owner);
        }
        self.remove(id, owner);
    }

    /// Fully remove an element that has already been unmounted (e.g.
    /// from `BuildOwner::finalize_tree`'s end-of-frame drain).
    ///
    /// This bypasses the soft-remove path even for keyed elements:
    /// the slab entry is freed and the `GlobalKey` registration is
    /// cleared via `ElementOwner::unregister_global_key`. Flutter parity: `framework.dart:2118`
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
        if let Some(key) = self.nodes[index].registered_global_key.take() {
            owner.unregister_global_key(key.as_ref());
        }

        // Drop any stale `did_change_dependencies` flag —
        // the dependent leaves the tree before its rebuild ever runs.
        self.unmount_inherited_dependencies(id, owner);
        self.nodes[index].element_mut().unmount(owner);

        // ADR-0040: the second of the two unmount primitives (finalize
        // drains, subtree evictions, root detach all bottom out here).
        crate::owner::emit_observation(owner.tree_observer, |o| {
            o.element_unmounted(&flui_foundation::observe::ElementUnmounted::new(id));
        });

        let node = self.nodes.remove(index);
        // Slot freed → bump its generation (ABA guard, see `remove`).
        self.bump_generation(index);

        if self.root == Some(id) {
            self.root = None;
        }

        Some(node)
    }

    /// Replay the live tree as synthetic `ElementMounted` events, in
    /// pre-order (parent before children, `child_ids` slot order) — the
    /// baseline for a mid-run observer attach (ADR-0040 §3).
    ///
    /// Walks from the root via `child_ids`, so soft-removed keyed elements
    /// parked in the inactive queue are NOT replayed. The caller
    /// (`WidgetsBinding::install_tree_observer`) holds the realm write lock
    /// across replay + install, so no mutation can interleave.
    pub fn replay_mounts(&self, observer: &dyn flui_foundation::observe::TreeObserver) {
        let Some(root) = self.root else {
            return;
        };
        let mut stack: Vec<ElementId> = vec![root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.get(id) else {
                continue;
            };
            observer.element_mounted(&flui_foundation::observe::ElementMounted::new(
                id,
                node.parent(),
                node.slot(),
                node.element().view_type_id(),
            ));
            // Reverse push keeps pre-order on a LIFO stack.
            stack.extend(node.child_ids().iter().rev().copied());
        }
    }

    /// Update an element with a new view.
    ///
    /// The view must be compatible (same type) with the existing
    /// element. Threads the split-borrow owner handle into the
    /// update call.
    ///
    /// FR-022: re-clones `View::key()` into the node so the
    /// stored key tracks whatever the new view carries. `View::can_update`
    /// (FR-028) already ensures the keys match on a successful
    /// update — the re-clone preserves that invariant explicitly rather
    /// than relying on the caller having already filtered by it.
    pub fn update(&mut self, id: ElementId, view: &dyn View, owner: &mut crate::ElementOwner<'_>) {
        if let Some(node) = self.get_mut(id) {
            node.element_mut().update(view, owner);
            node.set_key(view.key().map(ViewKey::clone_key));
        }
        // A reconfigured `ParentDataView` ancestor reaches this render child via
        // its own re-`update` (the reconciler walks children after their
        // parent), so re-deriving parent data here keeps it current — e.g.
        // `Expanded`'s `flex` changing between frames.
        self.apply_ancestor_parent_data(id);
    }

    /// Mark an element as needing rebuild.
    pub fn mark_needs_build(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element_mut().mark_needs_build();
        }
    }

    /// Deactivate an element (temporary removal).
    pub fn deactivate(&mut self, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        self.deactivate_subtree(id, owner);
    }

    /// Activate an element (re-insertion after deactivation).
    pub fn activate(&mut self, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        self.activate_subtree(id, owner);
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
// GlobalKey helpers
// ============================================================================

/// Borrow a view's `GlobalKey`, if it has one. Returns `None` for
/// un-keyed views and for keyed views whose `ViewKey::is_global_key()` is
/// `false` (e.g. `ValueKey`, `UniqueKey`, `ObjectKey`).
///
/// Centralises the "is this a global key?" check so the mount /
/// soft-remove / retake paths all read it the same way. The key is
/// returned by reference rather than as a hash because every consumer
/// resolves by identity — a hash would only be an index into a bucket.
fn global_key_of(view: &dyn View) -> Option<&dyn ViewKey> {
    let key = view.key()?;
    key.is_global_key().then_some(key)
}

/// Register the `(key → id)` mapping on the owner.
///
/// Reaching this with the SAME key already registered to a DIFFERENT
/// element means one key is live on two elements at once inside one tree —
/// a real duplicate, not the hash accident the old hash-keyed registry
/// could also produce here (two distinct keys that collide now hold two
/// independent entries and never meet). Debug builds panic, matching
/// Flutter's `assert` inside `BuildOwner._registerGlobalKey`
/// (`framework.dart:3160`); release builds trace and fall through to
/// last-write-wins so an application does not crash on it, and the frame
/// boundary's reservation check reports the duplicate as data.
fn register_global_key_with_collision_check(
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    id: ElementId,
) {
    if let Some(existing) = owner.element_for_global_key(key)
        && existing != id
    {
        tracing::error!(
            key = ?key,
            existing = ?existing,
            new = ?id,
            "duplicate GlobalKey: replacing an existing registration in the same tree"
        );
        #[cfg(debug_assertions)]
        {
            panic!(
                "duplicate GlobalKey: {key:?} is already registered to {existing:?} \
                 but new mount wants {id:?}"
            );
        }
    }
    owner.register_global_key(key, id);
}

/// State-migration entry point. If `key` resolves to an existing element,
/// reuse that element instead of mounting a fresh one:
///
/// - inactive candidate: pop it out of the inactive queue and re-attach;
/// - active candidate under a different parent: forget it from that parent,
///   deactivate/activate it, then attach it at the new `(parent, slot)`.
///
/// Returns the migrated `ElementId` on success, or `None` when no retakeable
/// element exists (caller falls back to creating a fresh element).
///
/// Flutter parity: `framework.dart:4571` `_retakeInactiveElement`.
enum GlobalKeyRetake {
    Absent,
    Retaken(ElementId),
    Rejected,
}

#[derive(Clone, Copy)]
struct RetakeDestination<'a> {
    parent: ElementId,
    slot: usize,
    reconciled_prefix: &'a [ElementId],
    unclaimed_old_slots: &'a [Option<ElementId>],
}

fn try_retake_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    view: &dyn View,
    destination: RetakeDestination<'_>,
) -> GlobalKeyRetake {
    let Some(candidate_id) = owner.element_for_global_key(key) else {
        return GlobalKeyRetake::Absent;
    };
    if !can_retake_global_key_candidate(tree, candidate_id, view) {
        return GlobalKeyRetake::Rejected;
    }

    let retaken = if owner.is_inactive(candidate_id) {
        retake_inactive_global_key(tree, owner, key, view, candidate_id, destination)
    } else {
        retake_active_global_key(tree, owner, key, view, candidate_id, destination)
    };
    retaken.map_or(GlobalKeyRetake::Rejected, GlobalKeyRetake::Retaken)
}

fn can_retake_global_key_candidate(
    tree: &ElementTree,
    candidate_id: ElementId,
    view: &dyn View,
) -> bool {
    let Some(node) = tree.get(candidate_id) else {
        return false;
    };
    let element = node.element();
    if element.view_type_id() != view.view_type_id() {
        return false;
    }
    let Some(new_key) = view.key() else {
        return false;
    };
    element
        .current_key()
        .is_some_and(|old_key| new_key.key_eq(old_key))
}

#[derive(Debug)]
struct RenderRelocation {
    pipeline: Option<PipelineCell>,
    frontier: Vec<(ElementId, RenderId)>,
    detached_render_subtrees: Option<flui_rendering::pipeline::DetachedRenderSubtrees>,
}

fn collect_render_frontier(
    tree: &ElementTree,
    candidate_id: ElementId,
) -> Option<Vec<(ElementId, RenderId)>> {
    let mut frontier = Vec::new();
    let mut stack = vec![candidate_id];
    while let Some(element_id) = stack.pop() {
        let node = tree.get(element_id)?;
        if let Some(render_id) = node.element().render_id() {
            frontier.push((element_id, render_id));
            continue;
        }
        stack.extend(node.child_ids().iter().rev().copied());
    }
    Some(frontier)
}

fn preflight_render_relocation(
    tree: &ElementTree,
    candidate_id: ElementId,
    new_parent: ElementId,
) -> Option<RenderRelocation> {
    // The destination cannot be inside the candidate's own element subtree.
    let mut cursor = Some(new_parent);
    while let Some(element_id) = cursor {
        if element_id == candidate_id {
            return None;
        }
        cursor = tree.get(element_id).and_then(ElementNode::parent);
    }

    let candidate = tree.get(candidate_id)?;
    let destination = tree.get(new_parent)?;
    let candidate_pipeline = candidate.element().pipeline_owner();
    let destination_pipeline = destination.element().pipeline_owner();
    match (&candidate_pipeline, &destination_pipeline) {
        (Some(candidate), Some(destination)) if !candidate.ptr_eq(destination) => return None,
        (Some(_), None) | (None, Some(_)) => return None,
        _ => {}
    }

    let frontier = collect_render_frontier(tree, candidate_id)?;

    if let Some(pipeline) = &candidate_pipeline {
        let destination_render_parent = destination.element().child_render_id();
        let is_valid = pipeline.with(|owner| {
            let render_tree = owner.render_tree();
            let mut moved_render_nodes = HashSet::new();
            let mut stack = Vec::new();
            for &(_, render_root) in &frontier {
                if !render_tree.contains(render_root) {
                    return false;
                }
                stack.push(render_root);
                while let Some(render_id) = stack.pop() {
                    if !moved_render_nodes.insert(render_id)
                        || destination_render_parent == Some(render_id)
                    {
                        return false;
                    }
                    let Some(node) = render_tree.get(render_id) else {
                        return false;
                    };
                    stack.extend(node.children().iter().rev().copied());
                }
            }
            true
        });
        if !is_valid {
            return None;
        }
    } else if !frontier.is_empty() {
        return None;
    }

    Some(RenderRelocation {
        pipeline: candidate_pipeline,
        frontier,
        detached_render_subtrees: None,
    })
}

fn detach_render_relocation(relocation: &mut RenderRelocation) {
    if relocation.frontier.is_empty() {
        return;
    }
    let Some(pipeline) = &relocation.pipeline else {
        return;
    };
    let token = pipeline.with_mut(|owner| {
        let roots: Vec<_> = relocation.frontier.iter().map(|&(_, root)| root).collect();
        owner
            .detach_render_subtrees(&roots)
            .expect("BUG: render relocation preflight must guarantee a detachable disjoint batch")
    });
    relocation.detached_render_subtrees = Some(token);
}

fn attach_render_relocation(relocation: &mut RenderRelocation) {
    if relocation.frontier.is_empty() {
        return;
    }
    let Some(pipeline) = &relocation.pipeline else {
        return;
    };
    let token = relocation
        .detached_render_subtrees
        .take()
        .expect("BUG: a render relocation with a pipeline must own its detached-subtree token");
    pipeline.with_mut(|owner| {
        owner.attach_render_subtrees(token).expect(
            "BUG: destination edge synchronization must preserve the detached render batch",
        );
    });
}

fn provisional_child_order(
    reconciled_prefix: &[ElementId],
    candidate_id: ElementId,
    unclaimed_old_slots: &[Option<ElementId>],
) -> Vec<ElementId> {
    let mut order = Vec::with_capacity(reconciled_prefix.len() + unclaimed_old_slots.len() + 1);
    let mut seen = HashSet::with_capacity(order.capacity());
    for element_id in reconciled_prefix
        .iter()
        .copied()
        .chain(std::iter::once(candidate_id))
        .chain(unclaimed_old_slots.iter().flatten().copied())
    {
        if seen.insert(element_id) {
            order.push(element_id);
        }
    }
    order
}

fn retake_inactive_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    view: &dyn View,
    candidate_id: ElementId,
    destination: RetakeDestination<'_>,
) -> Option<ElementId> {
    let new_parent = destination.parent;
    let new_slot = destination.slot;
    let mut relocation = preflight_render_relocation(tree, candidate_id, new_parent)?;
    // A candidate with render nodes can only be reattached through the token
    // its soft removal minted. Reject *before* dequeueing: `remove_inactive`
    // is the point of no return, and bailing after it would strand the
    // element — no finalization record, and its render subtree detached with
    // nothing left holding the right to reattach or release it.
    if !relocation.frontier.is_empty()
        && relocation.pipeline.is_some()
        && !owner.inactive_holds_detached_render_subtrees(candidate_id)
    {
        return None;
    }
    relocation.detached_render_subtrees = owner.remove_inactive(candidate_id);

    let parent_depth = tree.get(new_parent).map_or(0, ElementNode::depth);
    let child_parent_render_id = tree
        .get(new_parent)
        .and_then(|node| node.element().child_render_id());

    // Route through the staleness-checked accessor. The candidate came from the
    // live GlobalKey registry and was soft-removed (slot kept, generation NOT
    // bumped), so its generation still matches and this resolves.
    {
        let node = tree.get_mut(candidate_id)?;
        node.parent = Some(new_parent);
        node.slot = new_slot;
        node.depth = parent_depth + 1;
        node.element_mut()
            .set_parent_render_id(child_parent_render_id);
    }

    // Reactivate the whole subtree. Deactivation released every inherited
    // edge; the owner schedules `did_change_dependencies` for nodes that had
    // dependencies so their next build registers against the new ancestry.
    tree.activate_subtree(candidate_id, owner);

    // The subtree moved under a new parent, so its inherited scopes (built
    // against the OLD ancestor chain) and descendant depths are stale.
    // Recompute both top-down against `new_parent`; this combines Flutter's
    // recursive `_updateDepth` and `_updateInheritance` reactivation work.
    tree.recompute_subtree_ancestry(candidate_id);
    let provisional = provisional_child_order(
        destination.reconciled_prefix,
        candidate_id,
        destination.unclaimed_old_slots,
    );
    tree.synchronize_destination_render_children_for_relocation(new_parent, &provisional);
    for &(element_id, _) in &relocation.frontier {
        tree.reset_ancestor_parent_data(element_id);
    }
    attach_render_relocation(&mut relocation);

    {
        let node = tree.get_mut(candidate_id)?;
        node.element_mut().update(view, owner);
        node.set_key(view.key().map(ViewKey::clone_key));
    }

    tracing::debug!(
        candidate = ?candidate_id,
        new_parent = ?new_parent,
        new_slot,
        "ElementTree::insert retook inactive element for GlobalKey state migration"
    );

    // Emit ReconcileEvent::Reparent. The element
    // came from the inactive queue (Lifecycle::Inactive → Active), so
    // `from_parent: None` per ADV-1 branch case 1 — there is no prior
    // *active* parent at the moment of reparent; the donor parent
    // already cleared its slot when it pushed the element into the
    // inactive queue.
    super::reconcile_event::emit(&super::reconcile_event::ReconcileEvent {
        kind: super::reconcile_event::ReconcileEventKind::Reparent,
        parent: new_parent,
        child_key: Some(key.key_hash()),
        slot: new_slot,
        view_type_id: view.view_type_id(),
        from_parent: None,
    });

    // ADR-0040: identity and state survived — a move, never a second mount.
    crate::owner::emit_observation(owner.tree_observer, |o| {
        o.element_moved(&flui_foundation::observe::ElementMoved::new(
            candidate_id,
            new_parent,
            new_slot,
        ));
    });

    Some(candidate_id)
}

fn retake_active_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    view: &dyn View,
    candidate_id: ElementId,
    destination: RetakeDestination<'_>,
) -> Option<ElementId> {
    let new_parent = destination.parent;
    let new_slot = destination.slot;
    if !tree.is_reconciling_parent(new_parent) {
        return None;
    }
    let from_parent = tree.get(candidate_id)?.parent()?;
    if from_parent == new_parent {
        tracing::error!(
            key = ?key,
            ?candidate_id,
            ?new_parent,
            "GlobalKey appears twice under the same active parent"
        );
        #[cfg(debug_assertions)]
        {
            panic!(
                "GlobalKey {key:?} is already active under {new_parent:?}; \
                 duplicate GlobalKey children are not allowed"
            );
        }
        #[cfg(not(debug_assertions))]
        {
            return None;
        }
    }

    // Fail closed before mutating anything: if the old parent no longer
    // lists this child, the relocation preflight has not been satisfied and
    // the caller must not graft (it reports `Rejected`).
    if !tree
        .get(from_parent)
        .is_some_and(|parent| parent.child_ids.contains(&candidate_id))
    {
        return None;
    }

    // The graft is about to pull this child out from under a parent that is
    // still live. That is a legitimate reparent only if `from_parent` goes on
    // to rebuild without the child; if it never rebuilds, it ends the frame
    // describing a child it no longer has. Record it so the frame boundary
    // can tell the two apart — `note_parent_rebuild` drops this the moment
    // `from_parent` rebuilds.
    owner.displace_global_key(from_parent, candidate_id, key, new_parent);

    if let Some(old_parent) = tree.get_mut(from_parent)
        && let Some(pos) = old_parent
            .child_ids
            .iter()
            .position(|&child| child == candidate_id)
    {
        old_parent.child_ids.remove(pos);
    }
    let mut relocation = preflight_render_relocation(tree, candidate_id, new_parent)?;

    if let Some(old_parent) = tree.get_mut(from_parent) {
        old_parent.child_ids.retain(|child| *child != candidate_id);
    }
    detach_render_relocation(&mut relocation);

    let (parent_depth, child_parent_render_id) = tree.get(new_parent).map_or((0, None), |node| {
        (node.depth(), node.element().child_render_id())
    });

    tree.deactivate_subtree(candidate_id, owner);
    {
        let node = tree.get_mut(candidate_id)?;
        node.parent = Some(new_parent);
        node.slot = new_slot;
        node.depth = parent_depth + 1;
        node.element_mut()
            .set_parent_render_id(child_parent_render_id);
    }
    tree.activate_subtree(candidate_id, owner);
    // Deactivation above synchronously released dependency edges for the
    // entire subtree. Repair both inherited scopes and recursive depths before
    // the next dirty-heap drain.
    tree.recompute_subtree_ancestry(candidate_id);
    let provisional = provisional_child_order(
        destination.reconciled_prefix,
        candidate_id,
        destination.unclaimed_old_slots,
    );
    tree.synchronize_destination_render_children_for_relocation(new_parent, &provisional);
    for &(element_id, _) in &relocation.frontier {
        tree.reset_ancestor_parent_data(element_id);
    }
    attach_render_relocation(&mut relocation);
    {
        let node = tree.get_mut(candidate_id)?;
        node.element_mut().update(view, owner);
        node.set_key(view.key().map(ViewKey::clone_key));
    }

    tracing::debug!(
        candidate = ?candidate_id,
        from_parent = ?from_parent,
        new_parent = ?new_parent,
        new_slot,
        "ElementTree::insert moved active GlobalKey element to a new parent"
    );

    super::reconcile_event::emit(&super::reconcile_event::ReconcileEvent::reparent(
        from_parent,
        new_parent,
        new_slot,
        view.view_type_id(),
        key.key_hash(),
    ));

    // ADR-0040: cross-parent move of a live element.
    crate::owner::emit_observation(owner.tree_observer, |o| {
        o.element_moved(&flui_foundation::observe::ElementMoved::new(
            candidate_id,
            new_parent,
            new_slot,
        ));
    });

    Some(candidate_id)
}

impl std::fmt::Debug for ElementTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementTree")
            .field("len", &self.nodes.len())
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{IntoView, ViewExt};

    use crate::{
        BuildContext, BuildContextExt, BuildOwner, ElementBuildContext, GlobalKey,
        InheritedElement, ParentDataView, RenderView, StatelessView, View,
    };

    #[derive(Clone)]
    struct UnitRenderHost;

    impl RenderView for UnitRenderHost {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for UnitRenderHost {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    #[derive(Clone)]
    struct KeyedUnitRenderHost {
        key: GlobalKey<()>,
    }

    impl RenderView for KeyedUnitRenderHost {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for KeyedUnitRenderHost {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone)]
    struct LocallyKeyedUnitRenderHost {
        key: flui_foundation::ValueKey<u32>,
    }

    impl RenderView for LocallyKeyedUnitRenderHost {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = flui_objects::RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            flui_objects::RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for LocallyKeyedUnitRenderHost {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone)]
    struct KeyedTransparentView {
        key: GlobalKey<()>,
    }

    impl StatelessView for KeyedTransparentView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            UnitRenderHost.boxed()
        }
    }

    impl View for KeyedTransparentView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct RelocationParentDataA {
        value: u32,
    }

    impl flui_rendering::ParentData for RelocationParentDataA {}

    #[derive(Clone)]
    struct ParentDataAView {
        value: u32,
        child: KeyedUnitRenderHost,
    }

    impl ParentDataView for ParentDataAView {
        type ParentData = RelocationParentDataA;

        fn child(&self) -> &dyn View {
            &self.child
        }

        fn create_parent_data(&self) -> Self::ParentData {
            RelocationParentDataA { value: self.value }
        }

        fn apply_parent_data(
            &self,
            parent_data: &mut Self::ParentData,
        ) -> flui_rendering::RenderUpdateImpact {
            parent_data.value = self.value;
            flui_rendering::RenderUpdateImpact::LAYOUT
        }
    }

    crate::impl_parent_data_view!(ParentDataAView);

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct RelocationParentDataB {
        label: u32,
    }

    impl flui_rendering::ParentData for RelocationParentDataB {}

    #[derive(Clone)]
    struct ParentDataBView {
        label: u32,
        child: KeyedUnitRenderHost,
    }

    impl ParentDataView for ParentDataBView {
        type ParentData = RelocationParentDataB;

        fn child(&self) -> &dyn View {
            &self.child
        }

        fn create_parent_data(&self) -> Self::ParentData {
            RelocationParentDataB { label: self.label }
        }

        fn apply_parent_data(
            &self,
            parent_data: &mut Self::ParentData,
        ) -> flui_rendering::RenderUpdateImpact {
            parent_data.label = self.label;
            flui_rendering::RenderUpdateImpact::LAYOUT
        }
    }

    crate::impl_parent_data_view!(ParentDataBView);

    type RelocationEvents = std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>;

    #[derive(Debug)]
    struct AttachOrderBox {
        events: RelocationEvents,
    }

    impl flui_foundation::Diagnosticable for AttachOrderBox {}

    impl flui_rendering::traits::RenderBox for AttachOrderBox {
        type Arity = flui_tree::Leaf;
        type ParentData = flui_rendering::parent_data::BoxParentData;

        fn perform_layout(
            &mut self,
            _ctx: &mut flui_rendering::context::BoxLayoutContext<
                '_,
                flui_tree::Leaf,
                Self::ParentData,
            >,
        ) -> flui_types::geometry::Size {
            flui_types::geometry::Size::new(
                flui_types::geometry::px(1.0),
                flui_types::geometry::px(1.0),
            )
        }

        fn attach(&mut self, _handle: flui_rendering::pipeline::RenderInvalidationHandle) {
            self.events
                .lock()
                .expect("events lock")
                .push("render-attach");
        }

        fn detach(&mut self) {
            self.events
                .lock()
                .expect("events lock")
                .push("render-detach");
        }
    }

    #[derive(Clone)]
    struct AttachOrderView {
        key: GlobalKey<()>,
        events: RelocationEvents,
    }

    impl RenderView for AttachOrderView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = AttachOrderBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            AttachOrderBox {
                events: self.events.clone(),
            }
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }

        fn did_unmount_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) {
            self.events
                .lock()
                .expect("events lock")
                .push("view-unmount");
        }
    }

    impl View for AttachOrderView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[derive(Clone, Debug, Default)]
    struct AttachOrderParentData {
        events: RelocationEvents,
        value: u32,
    }

    impl flui_rendering::ParentData for AttachOrderParentData {
        fn detach(&mut self) {
            self.events
                .lock()
                .expect("events lock")
                .push("parent-data-detach");
        }
    }

    #[derive(Clone)]
    struct AttachOrderParentDataView {
        value: u32,
        child: AttachOrderView,
        events: RelocationEvents,
    }

    impl ParentDataView for AttachOrderParentDataView {
        type ParentData = AttachOrderParentData;

        fn child(&self) -> &dyn View {
            &self.child
        }

        fn create_parent_data(&self) -> Self::ParentData {
            self.events
                .lock()
                .expect("events lock")
                .push("parent-data-create");
            AttachOrderParentData {
                events: self.events.clone(),
                value: self.value,
            }
        }

        fn apply_parent_data(
            &self,
            parent_data: &mut Self::ParentData,
        ) -> flui_rendering::RenderUpdateImpact {
            parent_data.value = self.value;
            flui_rendering::RenderUpdateImpact::LAYOUT
        }
    }

    crate::impl_parent_data_view!(AttachOrderParentDataView);

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
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
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
    fn relocation_preflight_rejects_cross_pipeline_and_element_cycles_without_mutation() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline_a = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let pipeline_b = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let candidate = tree.mount_root_with_pipeline_owner(
            &TestView {
                name: "candidate".into(),
            },
            Some(pipeline_a),
            &mut owner.element_owner_mut(),
        );
        let foreign_destination = tree.mount_root_with_pipeline_owner(
            &TestView {
                name: "foreign".into(),
            },
            Some(pipeline_b),
            &mut owner.element_owner_mut(),
        );
        let before_len = tree.len();
        let before_candidate_parent = tree.get(candidate).expect("candidate").parent();
        assert!(preflight_render_relocation(&tree, candidate, foreign_destination).is_none());
        assert_eq!(tree.len(), before_len);
        assert_eq!(
            tree.get(candidate).expect("candidate").parent(),
            before_candidate_parent
        );

        let descendant = tree.insert(
            &TestView {
                name: "descendant".into(),
            },
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![descendant]);
        let before_descendant_parent = tree.get(descendant).expect("descendant").parent();
        assert!(preflight_render_relocation(&tree, candidate, descendant).is_none());
        assert_eq!(
            tree.get(descendant).expect("descendant").parent(),
            before_descendant_parent
        );
        assert_eq!(
            tree.get(candidate).expect("candidate").child_ids(),
            &[descendant]
        );
    }

    #[test]
    fn relocation_preflight_rejects_render_cycles_without_mutation() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let host = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let candidate = tree.insert(
            &TestView {
                name: "candidate".into(),
            },
            host,
            0,
            &mut owner.element_owner_mut(),
        );
        let moved_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        let destination = tree.insert(&UnitRenderHost, host, 1, &mut owner.element_owner_mut());
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![moved_boundary]);
        tree.get_mut(host)
            .expect("host")
            .set_child_ids(vec![candidate, destination]);

        let moved_render = tree
            .get(moved_boundary)
            .expect("moved boundary")
            .element()
            .render_id()
            .expect("moved render id");
        let destination_render = tree
            .get(destination)
            .expect("destination")
            .element()
            .render_id()
            .expect("destination render id");
        pipeline.with_mut(|owner| owner.adopt_render_child(moved_render, destination_render));
        let before_moved_children = pipeline.with(|owner| {
            owner
                .render_tree()
                .get(moved_render)
                .expect("moved render")
                .children()
                .to_vec()
        });
        let before_destination_parent =
            pipeline.with(|owner| owner.render_tree().parent(destination_render));

        assert!(preflight_render_relocation(&tree, candidate, destination).is_none());
        assert_eq!(
            pipeline.with(|owner| owner.render_tree().parent(destination_render)),
            before_destination_parent,
        );
        assert_eq!(
            pipeline.with(|owner| {
                owner
                    .render_tree()
                    .get(moved_render)
                    .expect("moved render")
                    .children()
                    .to_vec()
            }),
            before_moved_children,
        );
        assert_eq!(
            tree.get(candidate).expect("candidate").child_ids(),
            &[moved_boundary],
        );
    }

    #[test]
    fn production_retake_rejects_cross_pipeline_without_mutating_identity_or_epoch() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline_a = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let pipeline_b = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let donor = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline_a.clone()),
            &mut owner.element_owner_mut(),
        );
        let destination = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline_b.clone()),
            &mut owner.element_owner_mut(),
        );
        let key = GlobalKey::<()>::new();
        let keyed = KeyedUnitRenderHost { key: key.clone() };
        let candidate = tree.insert(&keyed, donor, 0, &mut owner.element_owner_mut());
        tree.get_mut(donor)
            .expect("donor")
            .set_child_ids(vec![candidate]);
        let render_id = tree
            .get(candidate)
            .expect("candidate")
            .element()
            .render_id()
            .expect("candidate render id");
        pipeline_a.with_mut(|pipeline| {
            pipeline.clear_all_dirty_nodes();
            pipeline
                .render_tree()
                .get(render_id)
                .expect("candidate render")
                .clear_needs_paint();
        });
        let old_handle = pipeline_a
            .with(|pipeline| pipeline.render_invalidation_handle(render_id))
            .expect("candidate is attached");
        old_handle.mark_needs_paint().expect("queue old interval");
        let before_donor_children = tree.get(donor).unwrap().child_ids().to_vec();
        let before_parent = pipeline_a.with(|pipeline| pipeline.render_tree().parent(render_id));
        let before_registry = owner.element_for_global_key(&key);

        let rejected = catch_unwind(AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(destination);
            tree.insert(&keyed, destination, 0, &mut owner.element_owner_mut());
        }));
        assert!(rejected.is_err());
        assert_eq!(tree.get(donor).unwrap().child_ids(), before_donor_children);
        assert!(tree.get(destination).unwrap().child_ids().is_empty());
        assert_eq!(tree.get(candidate).unwrap().parent(), Some(donor));
        assert_eq!(
            owner.element_for_global_key(&key),
            before_registry
        );
        assert_eq!(
            pipeline_a.with(|pipeline| pipeline.render_tree().parent(render_id)),
            before_parent,
        );
        assert_eq!(
            pipeline_a.with_mut(flui_rendering::PipelineOwner::drain_pending_dirty),
            1,
        );
        assert!(
            pipeline_a.with(|pipeline| {
                pipeline
                    .render_tree()
                    .get(render_id)
                    .expect("candidate render")
                    .needs_paint()
            }),
            "the request queued before rejection must apply to the unchanged interval",
        );
        pipeline_a.with_mut(|pipeline| {
            pipeline.clear_all_dirty_nodes();
            pipeline
                .render_tree()
                .get(render_id)
                .expect("candidate render")
                .clear_needs_paint();
        });
        old_handle
            .mark_needs_paint()
            .expect("the unchanged attachment interval remains usable");
        assert_eq!(
            pipeline_a.with_mut(flui_rendering::PipelineOwner::drain_pending_dirty),
            1,
        );
        assert!(
            pipeline_a.with(|pipeline| {
                pipeline
                    .render_tree()
                    .get(render_id)
                    .expect("candidate render")
                    .needs_paint()
            }),
            "the old handle must still apply after rejection because no interval changed",
        );
    }

    #[test]
    fn production_retake_rejects_render_cycle_without_mutating_edges() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let host = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let key = GlobalKey::<()>::new();
        let keyed = KeyedTransparentView { key: key.clone() };
        let candidate = tree.insert(&keyed, host, 0, &mut owner.element_owner_mut());
        let moved_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        let destination = tree.insert(&UnitRenderHost, host, 1, &mut owner.element_owner_mut());
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![moved_boundary]);
        tree.get_mut(host)
            .expect("host")
            .set_child_ids(vec![candidate, destination]);
        let moved_render = tree
            .get(moved_boundary)
            .unwrap()
            .element()
            .render_id()
            .unwrap();
        let destination_render = tree
            .get(destination)
            .unwrap()
            .element()
            .render_id()
            .unwrap();
        pipeline.with_mut(|owner| owner.adopt_render_child(moved_render, destination_render));
        let before_candidate_children = tree.get(candidate).unwrap().child_ids().to_vec();
        let before_moved_children = pipeline.with(|owner| {
            owner
                .render_tree()
                .get(moved_render)
                .unwrap()
                .children()
                .to_vec()
        });
        let before_registry = owner.element_for_global_key(&key);

        let rejected = catch_unwind(AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(destination);
            tree.insert(&keyed, destination, 0, &mut owner.element_owner_mut());
        }));
        assert!(rejected.is_err());
        assert_eq!(
            tree.get(candidate).unwrap().child_ids(),
            before_candidate_children
        );
        assert_eq!(tree.get(candidate).unwrap().parent(), Some(host));
        assert_eq!(
            owner.element_for_global_key(&key),
            before_registry
        );
        assert_eq!(
            pipeline.with(|owner| owner.render_tree().parent(destination_render)),
            Some(moved_render),
        );
        assert_eq!(
            pipeline.with(|owner| {
                owner
                    .render_tree()
                    .get(moved_render)
                    .unwrap()
                    .children()
                    .to_vec()
            }),
            before_moved_children,
        );
    }

    #[test]
    fn production_retake_rejects_overlapping_render_frontiers_without_mutation() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::PipelineOwner::new());
        let host = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let key = GlobalKey::<()>::new();
        let keyed = KeyedTransparentView { key: key.clone() };
        let candidate = tree.insert(&keyed, host, 0, &mut owner.element_owner_mut());
        let first_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        let second_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            1,
            &mut owner.element_owner_mut(),
        );
        let destination = tree.insert(&UnitRenderHost, host, 1, &mut owner.element_owner_mut());
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![first_boundary, second_boundary]);
        tree.get_mut(host)
            .expect("host")
            .set_child_ids(vec![candidate, destination]);
        let first_render = tree
            .get(first_boundary)
            .unwrap()
            .element()
            .render_id()
            .unwrap();
        let second_render = tree
            .get(second_boundary)
            .unwrap()
            .element()
            .render_id()
            .unwrap();
        pipeline.with_mut(|owner| owner.adopt_render_child(first_render, second_render));
        let before_first_children = pipeline.with(|owner| {
            owner
                .render_tree()
                .get(first_render)
                .unwrap()
                .children()
                .to_vec()
        });
        let before_candidate_children = tree.get(candidate).unwrap().child_ids().to_vec();
        let before_registry = owner.element_for_global_key(&key);
        let old_handle = pipeline
            .with(|owner| owner.render_invalidation_handle(first_render))
            .expect("first frontier remains attached");

        let rejected = catch_unwind(AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(destination);
            tree.insert(&keyed, destination, 0, &mut owner.element_owner_mut());
        }));
        assert!(rejected.is_err());
        assert_eq!(tree.get(candidate).unwrap().parent(), Some(host));
        assert_eq!(
            tree.get(candidate).unwrap().child_ids(),
            before_candidate_children
        );
        assert_eq!(
            owner.element_for_global_key(&key),
            before_registry
        );
        assert_eq!(
            pipeline.with(|owner| owner.render_tree().parent(second_render)),
            Some(first_render),
        );
        assert_eq!(
            pipeline.with(|owner| {
                owner
                    .render_tree()
                    .get(first_render)
                    .unwrap()
                    .children()
                    .to_vec()
            }),
            before_first_children,
        );
        old_handle
            .mark_needs_layout()
            .expect("rejection must not close the original attachment epoch");
    }

    #[test]
    fn production_retake_rejects_element_cycle_without_mutating_membership() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let key = GlobalKey::<()>::new();
        let keyed = KeyedTransparentView { key: key.clone() };
        let candidate = tree.mount_root(&keyed, &mut owner.element_owner_mut());
        let descendant = tree.insert(
            &TestView {
                name: "descendant".into(),
            },
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![descendant]);
        let before_children = tree.get(candidate).unwrap().child_ids().to_vec();
        let before_registry = owner.element_for_global_key(&key);

        let rejected = catch_unwind(AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(descendant);
            tree.insert(&keyed, descendant, 0, &mut owner.element_owner_mut());
        }));
        assert!(rejected.is_err());
        assert_eq!(tree.get(candidate).unwrap().parent(), None);
        assert_eq!(tree.get(candidate).unwrap().child_ids(), before_children);
        assert_eq!(tree.get(descendant).unwrap().parent(), Some(candidate));
        assert!(tree.get(descendant).unwrap().child_ids().is_empty());
        assert_eq!(
            owner.element_for_global_key(&key),
            before_registry
        );
    }

    #[test]
    fn relocation_preflight_accepts_an_empty_transparent_frontier() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let candidate = tree.mount_root(
            &TestView {
                name: "candidate".into(),
            },
            &mut owner.element_owner_mut(),
        );
        let destination = tree.mount_root(
            &TestView {
                name: "destination".into(),
            },
            &mut owner.element_owner_mut(),
        );

        let relocation = preflight_render_relocation(&tree, candidate, destination)
            .expect("an empty transparent subtree has no render-cycle risk");
        assert!(relocation.pipeline.is_none());
        assert!(relocation.frontier.is_empty());
        assert_eq!(tree.get(candidate).expect("candidate").parent(), None);
    }

    #[test]
    fn provisional_child_order_preserves_large_fanout_order_with_linear_deduplication() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let ids = (0..1_024)
            .map(|index| {
                tree.mount_root(
                    &TestView {
                        name: format!("child-{index}"),
                    },
                    &mut owner.element_owner_mut(),
                )
            })
            .collect::<Vec<_>>();
        let prefix = &ids[..400];
        let candidate = ids[400];
        let old_slots = prefix
            .iter()
            .copied()
            .map(Some)
            .chain([Some(candidate), None])
            .chain(ids[401..].iter().copied().map(Some))
            .collect::<Vec<_>>();

        let order = provisional_child_order(prefix, candidate, &old_slots);
        assert_eq!(order, ids);
    }

    fn relocate_keyed_render_child_through(
        destination_wrapper: &dyn View,
    ) -> (PipelineCell, RenderId) {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let key = GlobalKey::<()>::new();
        let keyed_child = KeyedUnitRenderHost { key: key.clone() };
        let donor_wrapper = tree.insert(
            &ParentDataAView {
                value: 7,
                child: keyed_child.clone(),
            },
            root,
            0,
            &mut owner.element_owner_mut(),
        );
        let moved = tree.insert(
            &keyed_child,
            donor_wrapper,
            0,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(donor_wrapper)
            .expect("donor wrapper")
            .set_child_ids(vec![moved]);

        let destination_render_parent =
            tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
        let destination_wrapper = tree.insert(
            destination_wrapper,
            destination_render_parent,
            0,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(destination_render_parent)
            .expect("destination render parent")
            .set_child_ids(vec![destination_wrapper]);
        tree.get_mut(root)
            .expect("root")
            .set_child_ids(vec![donor_wrapper, destination_render_parent]);

        let _reconcile_guard = tree.begin_reconcile(destination_wrapper);
        let retaken = tree.insert(
            &keyed_child,
            destination_wrapper,
            0,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(
            retaken, moved,
            "GlobalKey relocation must preserve identity"
        );
        assert_eq!(
            tree.get(moved).expect("moved element").parent(),
            Some(destination_wrapper),
        );
        let render_id = tree
            .get(moved)
            .expect("moved element")
            .element()
            .render_id()
            .expect("moved render id");
        assert_eq!(
            pipeline.with(|owner| owner.render_tree().parent(render_id)),
            tree.get(destination_render_parent)
                .expect("destination render parent")
                .element()
                .render_id(),
        );
        (pipeline, render_id)
    }

    #[test]
    fn relocation_recreates_compatible_parent_data_with_destination_configuration() {
        let key = GlobalKey::<()>::new();
        let destination = ParentDataAView {
            value: 41,
            child: KeyedUnitRenderHost { key },
        };
        let (pipeline, moved_render) = relocate_keyed_render_child_through(&destination);
        pipeline.with(|owner| {
            assert_eq!(
                owner
                    .render_tree()
                    .get(moved_render)
                    .expect("moved render")
                    .parent_data()
                    .and_then(|data| data.downcast_ref::<RelocationParentDataA>()),
                Some(&RelocationParentDataA { value: 41 }),
            );
        });
    }

    #[test]
    fn relocation_replaces_incompatible_parent_data_with_destination_type() {
        let key = GlobalKey::<()>::new();
        let destination = ParentDataBView {
            label: 73,
            child: KeyedUnitRenderHost { key },
        };
        let (pipeline, moved_render) = relocate_keyed_render_child_through(&destination);
        pipeline.with(|owner| {
            let moved = owner.render_tree().get(moved_render).expect("moved render");
            assert!(
                moved
                    .parent_data()
                    .and_then(|data| data.downcast_ref::<RelocationParentDataA>())
                    .is_none(),
            );
            assert_eq!(
                moved
                    .parent_data()
                    .and_then(|data| data.downcast_ref::<RelocationParentDataB>()),
                Some(&RelocationParentDataB { label: 73 }),
            );
        });
    }

    #[test]
    fn relocation_without_destination_configuration_leaves_parent_data_unset() {
        let destination = TestView {
            name: "transparent destination".into(),
        };
        let (pipeline, moved_render) = relocate_keyed_render_child_through(&destination);
        pipeline.with(|owner| {
            assert!(
                owner
                    .render_tree()
                    .get(moved_render)
                    .expect("moved render")
                    .parent_data()
                    .is_none(),
                "the destination render protocol may lazily supply its default during layout",
            );
        });
    }

    #[test]
    fn relocation_detaches_old_parent_data_before_render_and_configures_before_attach() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let events = RelocationEvents::default();
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let key = GlobalKey::<()>::new();
        let keyed = AttachOrderView {
            key,
            events: events.clone(),
        };
        let donor_wrapper = tree.insert(
            &AttachOrderParentDataView {
                value: 11,
                child: keyed.clone(),
                events: events.clone(),
            },
            root,
            0,
            &mut owner.element_owner_mut(),
        );
        let moved = tree.insert(&keyed, donor_wrapper, 0, &mut owner.element_owner_mut());
        tree.get_mut(donor_wrapper)
            .unwrap()
            .set_child_ids(vec![moved]);
        let destination_render_parent =
            tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
        let destination_wrapper = tree.insert(
            &AttachOrderParentDataView {
                value: 88,
                child: keyed.clone(),
                events: events.clone(),
            },
            destination_render_parent,
            0,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(destination_render_parent)
            .unwrap()
            .set_child_ids(vec![destination_wrapper]);
        tree.get_mut(root)
            .unwrap()
            .set_child_ids(vec![donor_wrapper, destination_render_parent]);
        events.lock().expect("events lock").clear();

        let _guard = tree.begin_reconcile(destination_wrapper);
        assert_eq!(
            tree.insert(
                &keyed,
                destination_wrapper,
                0,
                &mut owner.element_owner_mut(),
            ),
            moved,
        );
        assert_eq!(
            *events.lock().expect("events lock"),
            [
                "parent-data-detach",
                "render-detach",
                "parent-data-create",
                "parent-data-create",
                "render-attach",
            ],
        );
        let moved_render = tree.get(moved).unwrap().element().render_id().unwrap();
        pipeline.with(|owner| {
            let data = owner
                .render_tree()
                .get(moved_render)
                .unwrap()
                .parent_data()
                .and_then(|data| data.downcast_ref::<AttachOrderParentData>())
                .expect("fresh destination parent data");
            assert_eq!(data.value, 88);
        });
    }

    #[test]
    fn a_queued_element_without_a_relocation_token_is_rejected_before_it_leaves_the_queue() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let events = RelocationEvents::default();
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let keyed_view = AttachOrderView {
            key: GlobalKey::new(),
            events,
        };
        let keyed_element = tree.insert(&keyed_view, root, 0, &mut owner.element_owner_mut());
        let destination = tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
        tree.get_mut(root)
            .expect("root element")
            .set_child_ids(vec![keyed_element, destination]);
        // Take the key off the view, the way the production path does —
        // borrowing it out of the tree would clash with the `&mut tree`
        // `try_retake_global_key` needs below.
        let registered_key =
            global_key_of(&keyed_view).expect("a keyed element registers its global key");

        // Queue the element the way an unkeyed deactivation does — no
        // relocation token — even though it still owns a render frontier.
        owner.element_owner_mut().push_inactive(keyed_element, 1);

        let retake = try_retake_global_key(
            &mut tree,
            &mut owner.element_owner_mut(),
            registered_key,
            &keyed_view,
            RetakeDestination {
                parent: destination,
                slot: 0,
                reconciled_prefix: &[],
                unclaimed_old_slots: &[],
            },
        );

        assert!(matches!(retake, GlobalKeyRetake::Rejected));
        assert!(
            owner.element_owner_mut().is_inactive(keyed_element),
            "a rejected retake must leave the element's only finalization record queued",
        );
    }

    #[test]
    fn finalization_releases_token_before_unmount_without_hiding_the_render_object() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let events = RelocationEvents::default();
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let keyed_view = AttachOrderView {
            key: GlobalKey::new(),
            events: events.clone(),
        };
        let keyed_element = tree.insert(&keyed_view, root, 0, &mut owner.element_owner_mut());
        tree.get_mut(root)
            .expect("root element")
            .set_child_ids(vec![keyed_element]);
        let render_id = tree
            .get(keyed_element)
            .expect("keyed element")
            .element()
            .render_id()
            .expect("keyed render id");
        events.lock().expect("events lock").clear();

        tree.remove(keyed_element, &mut owner.element_owner_mut());
        assert!(pipeline.with(|pipeline_owner| pipeline_owner.render_tree().contains(render_id)));
        owner.finalize_tree(&mut tree);

        assert!(!pipeline.with(|pipeline_owner| pipeline_owner.render_tree().contains(render_id)));
        assert_eq!(
            events.lock().expect("events lock").as_slice(),
            &["render-detach", "view-unmount"],
            "release must preserve the render node for the view hook and must not detach twice",
        );
    }

    #[test]
    fn transparent_multi_frontier_synchronizes_exact_global_dfs_order() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let destination = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let prefix = tree.insert(
            &UnitRenderHost,
            destination,
            0,
            &mut owner.element_owner_mut(),
        );
        let candidate = tree.insert(
            &TestView {
                name: "transparent".into(),
            },
            destination,
            1,
            &mut owner.element_owner_mut(),
        );
        let first_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        let second_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            1,
            &mut owner.element_owner_mut(),
        );
        let nested = tree.insert(
            &UnitRenderHost,
            first_boundary,
            0,
            &mut owner.element_owner_mut(),
        );
        let suffix = tree.insert(
            &UnitRenderHost,
            destination,
            2,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![first_boundary, second_boundary]);
        tree.get_mut(first_boundary)
            .expect("first boundary")
            .set_child_ids(vec![nested]);
        tree.get_mut(destination)
            .expect("destination")
            .set_child_ids(vec![prefix, candidate, suffix]);
        for index in 0..128 {
            tree.mount_root(
                &TestView {
                    name: format!("unrelated-{index}"),
                },
                &mut owner.element_owner_mut(),
            );
        }

        let frontier = collect_render_frontier(&tree, candidate).expect("frontier is live");
        assert_eq!(
            frontier
                .iter()
                .map(|(_, render_id)| *render_id)
                .collect::<Vec<_>>(),
            vec![
                tree.get(first_boundary)
                    .unwrap()
                    .element()
                    .render_id()
                    .unwrap(),
                tree.get(second_boundary)
                    .unwrap()
                    .element()
                    .render_id()
                    .unwrap(),
            ],
            "the nested render subtree stays behind its first boundary",
        );

        let visited = tree.synchronize_destination_render_children_for_relocation(
            destination,
            &[prefix, candidate, suffix],
        );
        assert_eq!(
            visited, 6,
            "the provisional synchronizer visits only the destination render subtree and stops at first boundaries",
        );
        assert!(
            tree.len() > visited * 20,
            "fixture must expose a global-scan regression"
        );
        let destination_render = tree
            .get(destination)
            .unwrap()
            .element()
            .render_id()
            .unwrap();
        let expected = [prefix, first_boundary, second_boundary, suffix]
            .map(|element_id| tree.get(element_id).unwrap().element().render_id().unwrap());
        pipeline.with(|pipeline| {
            assert_eq!(
                pipeline
                    .render_tree()
                    .get(destination_render)
                    .unwrap()
                    .children(),
                expected.as_slice(),
            );
        });
    }

    #[test]
    fn active_retake_wires_provisional_order_and_sparse_merge_end_to_end() {
        type BoxObject =
            Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let donor = tree.insert(&UnitRenderHost, root, 0, &mut owner.element_owner_mut());
        let destination = tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
        let key = GlobalKey::<()>::new();
        let keyed = KeyedTransparentView { key: key.clone() };
        let candidate = tree.insert(&keyed, donor, 0, &mut owner.element_owner_mut());
        let first_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        let second_boundary = tree.insert(
            &UnitRenderHost,
            candidate,
            1,
            &mut owner.element_owner_mut(),
        );
        let nested = tree.insert(
            &UnitRenderHost,
            first_boundary,
            0,
            &mut owner.element_owner_mut(),
        );
        let prefix = tree.insert(
            &UnitRenderHost,
            destination,
            0,
            &mut owner.element_owner_mut(),
        );
        let suffix = tree.insert(
            &UnitRenderHost,
            destination,
            2,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(root)
            .expect("root")
            .set_child_ids(vec![donor, destination]);
        tree.get_mut(donor)
            .expect("donor")
            .set_child_ids(vec![candidate]);
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(vec![first_boundary, second_boundary]);
        tree.get_mut(first_boundary)
            .expect("first boundary")
            .set_child_ids(vec![nested]);
        tree.get_mut(destination)
            .expect("destination")
            .set_child_ids(vec![prefix, suffix]);

        let destination_render = tree
            .get(destination)
            .expect("destination")
            .element()
            .render_id()
            .expect("destination render id");
        let (sparse_late, sparse_early) = pipeline.with_mut(|owner| {
            let sparse_late = owner
                .insert_child_render_object(
                    destination_render,
                    Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject,
                )
                .expect("late sparse child");
            let sparse_early = owner
                .insert_child_render_object(
                    destination_render,
                    Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject,
                )
                .expect("early sparse child");
            owner
                .render_tree_mut()
                .get_mut(sparse_late)
                .expect("late sparse node")
                .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(9)));
            owner
                .render_tree_mut()
                .get_mut(sparse_early)
                .expect("early sparse node")
                .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(3)));
            (sparse_late, sparse_early)
        });

        let _guard = tree.begin_reconcile(destination);
        let moved = tree.insert_during_reconcile(
            &keyed,
            destination,
            1,
            &mut owner.element_owner_mut(),
            &[prefix],
            &[Some(suffix)],
        );
        assert_eq!(moved, candidate);
        assert!(
            !tree
                .get(donor)
                .expect("donor")
                .child_ids()
                .contains(&candidate),
        );

        let dense = [prefix, first_boundary, second_boundary, suffix]
            .map(|element_id| tree.get(element_id).unwrap().element().render_id().unwrap());
        let expected = dense
            .into_iter()
            .chain([sparse_early, sparse_late])
            .collect::<Vec<_>>();
        pipeline.with(|owner| {
            assert_eq!(
                owner
                    .render_tree()
                    .get(destination_render)
                    .expect("destination render")
                    .children(),
                expected,
            );
        });
    }

    #[test]
    fn reconcile_commit_repairs_suffix_reorder_after_provisional_global_key_move() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new());
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost,
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let donor = tree.insert(&UnitRenderHost, root, 0, &mut owner.element_owner_mut());
        let destination = tree.insert(&UnitRenderHost, root, 1, &mut owner.element_owner_mut());
        let global_key = GlobalKey::<()>::new();
        let global = KeyedUnitRenderHost {
            key: global_key.clone(),
        };
        let moved = tree.insert(&global, donor, 0, &mut owner.element_owner_mut());
        let view_a = LocallyKeyedUnitRenderHost {
            key: flui_foundation::ValueKey::new(1),
        };
        let view_b = LocallyKeyedUnitRenderHost {
            key: flui_foundation::ValueKey::new(2),
        };
        let old_a = tree.insert(&view_a, destination, 0, &mut owner.element_owner_mut());
        let old_b = tree.insert(&view_b, destination, 1, &mut owner.element_owner_mut());
        tree.get_mut(root)
            .expect("root")
            .set_child_ids(vec![donor, destination]);
        tree.get_mut(donor)
            .expect("donor")
            .set_child_ids(vec![moved]);
        tree.get_mut(destination)
            .expect("destination")
            .set_child_ids(vec![old_a, old_b]);

        let new_views: Vec<Box<dyn View>> = vec![
            Box::new(global),
            Box::new(view_b.clone()),
            Box::new(view_a.clone()),
        ];
        crate::tree::id_reconcile::reconcile_children_by_id(
            &mut tree,
            destination,
            &new_views,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(
            tree.get(destination).expect("destination").child_ids(),
            &[moved, old_b, old_a],
        );
        let expected = [moved, old_b, old_a]
            .map(|element_id| tree.get(element_id).unwrap().element().render_id().unwrap());
        let destination_render = tree
            .get(destination)
            .unwrap()
            .element()
            .render_id()
            .unwrap();
        let provisional = [moved, old_a, old_b]
            .map(|element_id| tree.get(element_id).unwrap().element().render_id().unwrap());
        pipeline.with(|owner| {
            assert_eq!(
                owner
                    .render_tree()
                    .get(destination_render)
                    .unwrap()
                    .children(),
                provisional,
                "retake callbacks observe the provisional prefix/candidate/live suffix order",
            );
        });
        assert!(tree.reorder_render_children_after_build() > 0);
        assert_eq!(
            tree.reorder_render_children_after_build(),
            0,
            "the authoritative full-tree synchronization is coalesced once per build drain",
        );
        pipeline.with(|owner| {
            assert_eq!(
                owner
                    .render_tree()
                    .get(destination_render)
                    .unwrap()
                    .children(),
                expected,
            );
        });
    }

    #[test]
    fn sparse_sliver_children_remain_after_dense_dfs_children_in_logical_order() {
        type BoxObject =
            Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;
        let mut pipeline = flui_rendering::pipeline::PipelineOwner::new();
        let parent = pipeline.insert(Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject);
        let dense = pipeline
            .insert_child_render_object(
                parent,
                Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject,
            )
            .expect("dense child");
        let sparse_late = pipeline
            .insert_child_render_object(
                parent,
                Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject,
            )
            .expect("late sparse child");
        let sparse_early = pipeline
            .insert_child_render_object(
                parent,
                Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject,
            )
            .expect("early sparse child");
        pipeline
            .render_tree_mut()
            .get_mut(sparse_late)
            .unwrap()
            .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(8)));
        pipeline
            .render_tree_mut()
            .get_mut(sparse_early)
            .unwrap()
            .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(2)));

        let mut desired = vec![dense];
        append_sparse_sliver_children(pipeline.render_tree(), parent, &mut desired);
        assert_eq!(desired, vec![dense, sparse_early, sparse_late]);
    }

    #[test]
    fn sparse_merge_preserves_dense_order_and_sorts_large_sparse_fanout() {
        type BoxObject =
            Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;
        let mut pipeline = flui_rendering::pipeline::PipelineOwner::new();
        let parent = pipeline.insert(Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject);
        let children = (0..1_024)
            .map(|_| {
                pipeline
                    .insert_child_render_object(
                        parent,
                        Box::new(flui_objects::RenderSizedBox::shrink()) as BoxObject,
                    )
                    .expect("child")
            })
            .collect::<Vec<_>>();
        let dense = children[..512].to_vec();
        for (logical_index, &child) in children[512..].iter().rev().enumerate() {
            pipeline
                .render_tree_mut()
                .get_mut(child)
                .expect("sparse child")
                .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(
                    logical_index,
                )));
        }

        let mut desired = dense.clone();
        append_sparse_sliver_children(pipeline.render_tree(), parent, &mut desired);
        let expected = dense
            .into_iter()
            .chain(children[512..].iter().rev().copied())
            .collect::<Vec<_>>();
        assert_eq!(desired, expected);
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
    // Generational staleness (ABA safety)
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

    /// Generation-overflow policy: a slot recycled `u32::MAX`
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

    // ========================================================================
    // Inherited scope (PR-2): the per-node `inherited` map gives O(1)
    // `depend_on` resolution. These exercise the map directly (it is
    // `pub(crate)`), independent of the build pipeline.
    // ========================================================================

    #[derive(Clone, Debug, PartialEq)]
    struct ThemeData {
        color: u32,
    }

    /// An `InheritedView` provider fixture. `child` is required by the trait
    /// but never built here — these tests insert the tree shape directly.
    #[derive(Clone)]
    struct Theme {
        data: ThemeData,
        child: TestView,
    }

    impl crate::view::InheritedView for Theme {
        type Data = ThemeData;

        fn data(&self) -> &Self::Data {
            &self.data
        }

        fn child(&self) -> &dyn View {
            &self.child
        }

        fn update_should_notify(&self, old: &Self) -> bool {
            self.data != old.data
        }
    }

    impl View for Theme {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::inherited(self)
        }
    }

    fn theme(color: u32) -> Theme {
        Theme {
            data: ThemeData { color },
            child: TestView {
                name: "unused".to_string(),
            },
        }
    }

    fn leaf(name: &str) -> TestView {
        TestView {
            name: name.to_string(),
        }
    }

    #[test]
    fn inherited_scope_resolves_provider_in_o1() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        let provider = tree.mount_root(&theme(1), &mut owner.element_owner_mut());
        let child = tree.insert(&leaf("c"), provider, 0, &mut owner.element_owner_mut());

        let theme_ty = TypeId::of::<Theme>();
        // A provider's own scope includes itself (Flutter `_inheritedElements`
        // for an InheritedElement contains `this`).
        assert_eq!(
            tree.get(provider).unwrap().inherited_provider(theme_ty),
            Some(provider),
        );
        // A descendant resolves the ancestor provider via the aliased map.
        assert_eq!(
            tree.get(child).unwrap().inherited_provider(theme_ty),
            Some(provider),
        );
        // A non-provider view type is absent from the scope.
        assert_eq!(
            tree.get(child)
                .unwrap()
                .inherited_provider(TypeId::of::<TestView>()),
            None,
        );
        // Non-providers alias the parent's map by refcount — no per-node clone.
        assert!(
            Arc::ptr_eq(
                &tree.get(provider).unwrap().inherited,
                &tree.get(child).unwrap().inherited,
            ),
            "a non-provider child must share its parent's inherited map Arc",
        );
    }

    #[test]
    fn nested_same_type_provider_shadows_nearest() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        let outer = tree.mount_root(&theme(1), &mut owner.element_owner_mut());
        let inner = tree.insert(&theme(2), outer, 0, &mut owner.element_owner_mut());
        let leaf_id = tree.insert(&leaf("l"), inner, 0, &mut owner.element_owner_mut());

        let theme_ty = TypeId::of::<Theme>();
        // Nearest-wins: the leaf resolves the inner provider, not the outer.
        assert_eq!(
            tree.get(leaf_id).unwrap().inherited_provider(theme_ty),
            Some(inner),
            "the nearest same-type provider must shadow the outer one",
        );
        assert_eq!(
            tree.get(inner).unwrap().inherited_provider(theme_ty),
            Some(inner),
        );
        assert_eq!(
            tree.get(outer).unwrap().inherited_provider(theme_ty),
            Some(outer),
        );
    }

    #[test]
    fn recompute_inherited_subtree_after_reparent() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        // root(non-provider) -> [ provider_a(1), provider_b(2) ];
        // k under provider_a, child c under k.
        let root = tree.mount_root(&leaf("root"), &mut owner.element_owner_mut());
        let provider_a = tree.insert(&theme(1), root, 0, &mut owner.element_owner_mut());
        let provider_b = tree.insert(&theme(2), root, 1, &mut owner.element_owner_mut());
        let k = tree.insert(&leaf("k"), provider_a, 0, &mut owner.element_owner_mut());
        let c = tree.insert(&leaf("c"), k, 0, &mut owner.element_owner_mut());
        // Direct `insert` does not maintain `child_ids` (the reconciler does);
        // model the post-build subtree the reparent path actually walks.
        tree.get_mut(k).unwrap().set_child_ids(vec![c]);

        let theme_ty = TypeId::of::<Theme>();
        assert_eq!(
            tree.get(k).unwrap().inherited_provider(theme_ty),
            Some(provider_a),
        );
        assert_eq!(
            tree.get(c).unwrap().inherited_provider(theme_ty),
            Some(provider_a),
        );

        // Reparent k under provider_b and recompute the moved subtree.
        tree.get_mut(k).unwrap().parent = Some(provider_b);
        tree.recompute_subtree_ancestry(k);

        assert_eq!(
            tree.get(k).unwrap().inherited_provider(theme_ty),
            Some(provider_b),
            "the moved node resolves the new provider after recompute",
        );
        assert_eq!(
            tree.get(c).unwrap().inherited_provider(theme_ty),
            Some(provider_b),
            "a descendant of the moved node is recomputed too (top-down walk)",
        );
    }

    #[test]
    fn recompute_reshadows_nested_provider_in_moved_subtree() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        // root -> [ provider_a(1), provider_b(2) ]; k under provider_a;
        // a NESTED provider(3) under k; leaf d under the nested provider.
        let root = tree.mount_root(&leaf("root"), &mut owner.element_owner_mut());
        let provider_a = tree.insert(&theme(1), root, 0, &mut owner.element_owner_mut());
        let provider_b = tree.insert(&theme(2), root, 1, &mut owner.element_owner_mut());
        let k = tree.insert(&leaf("k"), provider_a, 0, &mut owner.element_owner_mut());
        let nested = tree.insert(&theme(3), k, 0, &mut owner.element_owner_mut());
        let d = tree.insert(&leaf("d"), nested, 0, &mut owner.element_owner_mut());
        tree.get_mut(k).unwrap().set_child_ids(vec![nested]);
        tree.get_mut(nested).unwrap().set_child_ids(vec![d]);

        let theme_ty = TypeId::of::<Theme>();
        assert_eq!(
            tree.get(d).unwrap().inherited_provider(theme_ty),
            Some(nested)
        );

        // Move k under provider_b and recompute the whole moved subtree.
        tree.get_mut(k).unwrap().parent = Some(provider_b);
        tree.recompute_subtree_ancestry(k);

        assert_eq!(
            tree.get(k).unwrap().inherited_provider(theme_ty),
            Some(provider_b),
            "the moved root resolves the new outer provider",
        );
        assert_eq!(
            tree.get(nested).unwrap().inherited_provider(theme_ty),
            Some(nested),
            "a provider inside the moved subtree re-shadows itself after recompute",
        );
        assert_eq!(
            tree.get(d).unwrap().inherited_provider(theme_ty),
            Some(nested),
            "below the nested provider the nearest (nested) one still wins",
        );
    }

    /// A keyed stateless view used to drive the REAL GlobalKey reparent path
    /// (`try_retake_global_key` → `recompute_subtree_ancestry`). `GlobalKey<T>`
    /// is phantom in `T`, so a stateless `GlobalKey<()>` is enough to register
    /// in the migration registry.
    #[derive(Clone)]
    struct Keyed {
        key: crate::GlobalKey<()>,
    }

    impl StatelessView for Keyed {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            leaf("keyed-child").boxed()
        }
    }

    impl View for Keyed {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            Some(&self.key)
        }
    }

    #[test]
    #[serial_test::serial(global_key_registry)]
    fn globalkey_retake_recomputes_inherited_scope() {
        use parking_lot::RwLock;

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));
        crate::test_only_set_global_key_registry(&tree, &owner);

        let root = tree
            .write()
            .mount_root(&leaf("root"), &mut owner.write().element_owner_mut());
        let provider_a =
            tree.write()
                .insert(&theme(1), root, 0, &mut owner.write().element_owner_mut());
        let provider_b =
            tree.write()
                .insert(&theme(2), root, 1, &mut owner.write().element_owner_mut());

        let keyed = Keyed {
            key: crate::GlobalKey::new(),
        };
        let k = tree.write().insert(
            &keyed,
            provider_a,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let c = tree
            .write()
            .insert(&leaf("c"), k, 0, &mut owner.write().element_owner_mut());
        // Soft-remove only detaches the top, preserving `child_ids`; model the
        // built subtree so the post-retake recompute reaches `c`.
        tree.write().get_mut(k).unwrap().set_child_ids(vec![c]);

        let theme_ty = TypeId::of::<Theme>();
        assert_eq!(
            tree.read().get(k).unwrap().inherited_provider(theme_ty),
            Some(provider_a),
        );

        let context =
            ElementBuildContext::for_element(k, Arc::clone(&tree), Arc::clone(&owner)).unwrap();
        assert_eq!(
            context.depend_on::<Theme, u32>(|provider| provider.data.color),
            Some(1),
        );
        assert!(
            tree.read()
                .get(provider_a)
                .unwrap()
                .element()
                .downcast_ref::<InheritedElement<Theme>>()
                .unwrap()
                .dependents()
                .contains_key(&k),
            "the keyed element starts registered with its old provider",
        );

        // Soft-remove K (→ inactive queue), then re-insert under provider_b
        // with the SAME GlobalKey: the real `try_retake_global_key` reactivates
        // it and calls `recompute_subtree_ancestry`.
        {
            let mut owner = owner.write();
            tree.write().remove(k, &mut owner.element_owner_mut());
        }
        assert!(
            !tree
                .read()
                .get(provider_a)
                .unwrap()
                .element()
                .downcast_ref::<InheritedElement<Theme>>()
                .unwrap()
                .dependents()
                .contains_key(&k),
            "deactivate removes the keyed subtree from its former provider",
        );
        let migrated = {
            let mut owner = owner.write();
            tree.write()
                .insert(&keyed, provider_b, 0, &mut owner.element_owner_mut())
        };
        assert_eq!(migrated, k, "GlobalKey retake reuses the same ElementId");

        assert_eq!(
            tree.read().get(k).unwrap().inherited_provider(theme_ty),
            Some(provider_b),
            "the retaken node resolves the new provider after the real reparent path",
        );
        assert_eq!(
            tree.read().get(c).unwrap().inherited_provider(theme_ty),
            Some(provider_b),
            "the retaken node's child is recomputed too (try_retake_global_key wiring)",
        );

        let context =
            ElementBuildContext::for_element(k, Arc::clone(&tree), Arc::clone(&owner)).unwrap();
        assert_eq!(
            context.depend_on::<Theme, u32>(|provider| provider.data.color),
            Some(2),
        );
        assert!(
            tree.read()
                .get(provider_b)
                .unwrap()
                .element()
                .downcast_ref::<InheritedElement<Theme>>()
                .unwrap()
                .dependents()
                .contains_key(&k),
            "the retaken element registers only with its new provider",
        );

        crate::test_only_clear_global_key_registry();
    }

    #[test]
    #[serial_test::serial(global_key_registry)]
    fn globalkey_retake_recomputes_depth_for_the_entire_subtree() {
        use parking_lot::RwLock;

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));
        crate::test_only_set_global_key_registry(&tree, &owner);

        let root = tree
            .write()
            .mount_root(&leaf("root"), &mut owner.write().element_owner_mut());
        let shallow_parent = tree.write().insert(
            &leaf("shallow"),
            root,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let deep_1 = tree.write().insert(
            &leaf("deep-1"),
            root,
            1,
            &mut owner.write().element_owner_mut(),
        );
        let deep_2 = tree.write().insert(
            &leaf("deep-2"),
            deep_1,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let deep_3 = tree.write().insert(
            &leaf("deep-3"),
            deep_2,
            0,
            &mut owner.write().element_owner_mut(),
        );

        let keyed = Keyed {
            key: crate::GlobalKey::new(),
        };
        let moved = tree.write().insert(
            &keyed,
            shallow_parent,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let child = tree.write().insert(
            &leaf("child"),
            moved,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let grandchild = tree.write().insert(
            &leaf("grandchild"),
            child,
            0,
            &mut owner.write().element_owner_mut(),
        );
        tree.write()
            .get_mut(moved)
            .expect("moved node exists")
            .set_child_ids(vec![child]);
        tree.write()
            .get_mut(child)
            .expect("child exists")
            .set_child_ids(vec![grandchild]);

        assert_eq!(tree.read().get(moved).expect("moved exists").depth(), 2);
        assert_eq!(tree.read().get(child).expect("child exists").depth(), 3);
        assert_eq!(
            tree.read()
                .get(grandchild)
                .expect("grandchild exists")
                .depth(),
            4
        );

        tree.write()
            .remove(moved, &mut owner.write().element_owner_mut());
        let migrated =
            tree.write()
                .insert(&keyed, deep_3, 0, &mut owner.write().element_owner_mut());
        assert_eq!(migrated, moved, "GlobalKey retake preserves identity");

        let tree = tree.read();
        assert_eq!(tree.get(moved).expect("moved exists").depth(), 4);
        assert_eq!(
            tree.get(child).expect("child exists").depth(),
            5,
            "the first descendant must follow the moved parent's new depth"
        );
        assert_eq!(
            tree.get(grandchild).expect("grandchild exists").depth(),
            6,
            "depth repair must cover the whole moved subtree"
        );
        drop(tree);

        crate::test_only_clear_global_key_registry();
    }

    /// After a GlobalKey reparent to a deeper parent, every descendant's
    /// `depth` must follow `parent.depth + 1` — Flutter's `_updateDepth`
    /// recurses over the whole moved subtree. Stale descendant depths would
    /// mis-order the dirty heap (a child could build before its parent) and
    /// `finalize_tree`'s deepest-first unmount sweep.
    #[test]
    #[serial_test::serial(global_key_registry)]
    fn globalkey_reparent_updates_descendant_depths() {
        use parking_lot::RwLock;

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));
        crate::test_only_set_global_key_registry(&tree, &owner);

        let root = tree
            .write()
            .mount_root(&leaf("root"), &mut owner.write().element_owner_mut());
        let shallow = tree.write().insert(
            &leaf("shallow"),
            root,
            0,
            &mut owner.write().element_owner_mut(),
        );
        // A deeper branch to reparent into: root -> d1 -> ... -> d7.
        let mut deep = root;
        for _ in 0..7 {
            deep = tree.write().insert(
                &leaf("deep"),
                deep,
                0,
                &mut owner.write().element_owner_mut(),
            );
        }
        assert_eq!(tree.read().get(deep).unwrap().depth(), 7);

        // Keyed subtree under `shallow`: k(2) -> c(3) -> g(4).
        let keyed = Keyed {
            key: crate::GlobalKey::new(),
        };
        let k = tree
            .write()
            .insert(&keyed, shallow, 0, &mut owner.write().element_owner_mut());
        let c = tree
            .write()
            .insert(&leaf("c"), k, 0, &mut owner.write().element_owner_mut());
        let g = tree
            .write()
            .insert(&leaf("g"), c, 0, &mut owner.write().element_owner_mut());
        // Direct `insert` does not maintain `child_ids` (the reconciler
        // does); model the built subtree the reparent walk traverses.
        tree.write().get_mut(k).unwrap().set_child_ids(vec![c]);
        tree.write().get_mut(c).unwrap().set_child_ids(vec![g]);

        assert_eq!(tree.read().get(k).unwrap().depth(), 2);
        assert_eq!(tree.read().get(c).unwrap().depth(), 3);
        assert_eq!(tree.read().get(g).unwrap().depth(), 4);

        // Soft-remove K (→ inactive queue), then re-insert under the deep
        // branch with the SAME GlobalKey: the real retake path moves the
        // whole subtree.
        tree.write()
            .remove(k, &mut owner.write().element_owner_mut());
        let migrated = tree
            .write()
            .insert(&keyed, deep, 0, &mut owner.write().element_owner_mut());
        assert_eq!(migrated, k, "GlobalKey retake reuses the same ElementId");

        assert_eq!(
            tree.read().get(k).unwrap().depth(),
            8,
            "the moved root takes new_parent.depth + 1",
        );
        assert_eq!(
            tree.read().get(c).unwrap().depth(),
            9,
            "a descendant of the moved root must follow it (parent.depth + 1)",
        );
        assert_eq!(
            tree.read().get(g).unwrap().depth(),
            10,
            "the depth update recurses to the bottom of the moved subtree",
        );

        crate::test_only_clear_global_key_registry();
    }

    /// A keyed dependent that is soft-removed (deactivated into the inactive
    /// queue) must be deregistered from its provider's dependent map right
    /// away — Flutter's `Element.deactivate` removes the element from every
    /// provider in `_dependencies` — and `finalize_tree` must leave the map
    /// empty once the element is truly gone.
    #[test]
    #[serial_test::serial(global_key_registry)]
    fn soft_removed_keyed_dependent_is_deregistered() {
        use parking_lot::RwLock;

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));
        crate::test_only_set_global_key_registry(&tree, &owner);

        let provider = tree
            .write()
            .mount_root(&theme(1), &mut owner.write().element_owner_mut());
        let keyed = Keyed {
            key: crate::GlobalKey::new(),
        };
        let dependent =
            tree.write()
                .insert(&keyed, provider, 0, &mut owner.write().element_owner_mut());

        // Register both halves of a completed `depend_on`: the provider's
        // notification map and the BuildOwner's sparse reverse index.
        tree.write()
            .get_mut(provider)
            .expect("provider exists")
            .element_mut()
            .as_inherited_mut()
            .expect("root is inherited")
            .record_dependent(dependent, 1);
        owner
            .write()
            .register_inherited_dependency(dependent, provider);
        let dependent_count = |tree: &ElementTree| {
            tree.get(provider)
                .expect("provider exists")
                .element()
                .downcast_ref::<crate::InheritedElement<Theme>>()
                .expect("root is InheritedElement<Theme>")
                .dependents()
                .len()
        };
        assert_eq!(dependent_count(&tree.read()), 1);

        // Soft-remove (keyed → inactive queue): the deactivate purges the
        // registration even though the element's slot (and state) survives.
        tree.write()
            .remove(dependent, &mut owner.write().element_owner_mut());
        assert_eq!(
            dependent_count(&tree.read()),
            0,
            "deactivation deregisters the dependent from the provider's map",
        );

        // End-of-frame: the element is truly unmounted; the map stays empty.
        owner.write().finalize_tree(&mut tree.write());
        assert_eq!(dependent_count(&tree.read()), 0);

        crate::test_only_clear_global_key_registry();
    }
}
#[cfg(test)]
#[test]
fn reconcile_guard_rejects_nesting_and_recovers_after_unwind() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let tree = ElementTree::new();
    let parent = ElementId::new(1);
    let outer = tree.begin_reconcile(parent);
    assert!(catch_unwind(AssertUnwindSafe(|| tree.begin_reconcile(parent))).is_err());
    drop(outer);

    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _guard = tree.begin_reconcile(parent);
            panic!("probe unwind");
        }))
        .is_err()
    );

    let recovered = tree.begin_reconcile(parent);
    assert!(tree.is_reconciling_parent(parent));
    drop(recovered);
    assert!(!tree.is_reconciling_parent(parent));
}
