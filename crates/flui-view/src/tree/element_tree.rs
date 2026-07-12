//! Slab-based Element tree storage.
//!
//! Elements are stored in a Slab for O(1) access by ElementId.
//! This follows Flutter's approach where Elements form the retained tree.

use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::Arc;

use flui_foundation::{ElementId, RenderId, ViewKey};
use flui_rendering::{parent_data::SliverMultiBoxAdaptorParentData, pipeline::PipelineOwner};
use parking_lot::RwLock;
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

    let mut sparse_children = parent_node
        .children()
        .iter()
        .copied()
        .filter(|child| !desired_children.contains(child))
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
    /// Coexists with `registered_global_key_hash` for
    /// backward compatibility; the side-index field is reduced to a
    /// derived value and removed when the GlobalKey
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
    /// Flutter parity: keys are tracked on the
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
            registered_global_key_hash: None,
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
        }
    }

    /// Create an ElementTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            generations: Vec::with_capacity(capacity),
            root: None,
            needs_render_reorder: false,
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
            element.element_mut().set_pipeline_owner_any(owner_any);
            tracing::debug!(
                "ElementTree::mount_root_with_pipeline_owner: passed PipelineOwner to root element"
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
    pub fn insert(
        &mut self,
        view: &dyn View,
        parent: ElementId,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) -> ElementId {
        // ADV-1 state migration. Before creating a fresh element,
        // check whether `view` has a `GlobalKey` whose hash points at an
        // existing element. If it is inactive, pull it back; if it is still
        // active under a different parent, forget it from that parent and move
        // it here. In both cases the `ElementId` and state survive.
        if let Some(hash) = global_key_hash_of(view)
            && let Some(retaken_id) = try_retake_global_key(self, owner, hash, view, parent, slot)
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
        let (parent_depth, parent_owner, child_parent_render_id, parent_inherited) =
            match self.get(parent) {
                Some(node) => (
                    node.depth,
                    node.element().pipeline_owner_any(),
                    node.element().child_render_id(),
                    Arc::clone(&node.inherited),
                ),
                None => (0, None, None, Arc::new(HashMap::new())),
            };

        if let Some(owner_any) = parent_owner {
            element.element_mut().set_pipeline_owner_any(owner_any);
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

        // Register the GlobalKey hash → id mapping.
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

    /// Write the nearest ancestor `ParentDataView`'s configuration onto
    /// `child_id`'s render node, and mark the owning render parent dirty.
    ///
    /// No-op unless `child_id` owns a render node. Walks strictly upward from
    /// the child, taking the *nearest* `parent_data_config()` and stopping the
    /// search at the first ancestor render object (the render parent that reads
    /// the data during layout) — Flutter's `_findAncestorParentDataElement`
    /// fused with `_findAncestorRenderObjectElement`. The nearest config wins
    /// (`set_parent_data` replaces), matching Flutter taking the closest
    /// `ParentDataWidget`.
    ///
    /// Average case O(1) — for a plain render child the very first ancestor is
    /// the render parent, so the walk stops in one hop and never touches the
    /// pipeline owner. Worst case O(proxy-nesting depth) between the render
    /// child and its render parent.
    ///
    /// The tree borrow is fully dropped before the pipeline owner is locked:
    /// the collected config and owner handle are owned values, and the write
    /// targets the render tree, never `self`.
    fn apply_ancestor_parent_data(&mut self, child_id: ElementId) {
        let Some(child) = self.get(child_id) else {
            return;
        };
        let Some(child_render_id) = child.element().render_id() else {
            return;
        };
        let pipeline_any = child.element().pipeline_owner_any();
        let mut cursor = child.parent();

        let mut nearest_config: Option<Box<dyn flui_rendering::parent_data::ParentData>> = None;
        let mut parent_render_id: Option<flui_foundation::RenderId> = None;
        while let Some(ancestor_id) = cursor {
            let Some(node) = self.get(ancestor_id) else {
                break;
            };
            if let Some(render_id) = node.element().render_id() {
                parent_render_id = Some(render_id);
                break;
            }
            if nearest_config.is_none() {
                nearest_config = node.element().parent_data_config();
            }
            cursor = node.parent();
        }

        // Nothing to apply unless a ParentDataView sits between this render
        // child and its render parent.
        let Some(config) = nearest_config else {
            return;
        };
        let Some(pipeline_any) = pipeline_any else {
            return;
        };
        let Ok(pipeline_owner) = pipeline_any.downcast::<RwLock<PipelineOwner>>() else {
            return;
        };

        // Tree borrows are dropped; the lock guards only the render tree.
        let mut owner = pipeline_owner.write();
        if let Some(node) = owner.render_tree_mut().get_mut(child_render_id) {
            node.set_parent_data(config);
        }
        if let Some(parent_render_id) = parent_render_id {
            owner.mark_needs_layout(parent_render_id);
        }
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
    /// No-op unless an [`insert`](Self::insert) set `needs_render_reorder`.
    /// Average/worst case O(element-tree size) for the DFS, plus O(children) per
    /// drifted render parent. The element walk completes before the pipeline
    /// owner is locked; the lock guards only the render tree.
    pub(crate) fn reorder_render_children_after_build(&mut self) {
        if !self.needs_render_reorder {
            return;
        }
        self.needs_render_reorder = false;

        // Depth-first in slot order, tracking each node's nearest render
        // ancestor. A render node is appended to that ancestor's target order,
        // then becomes the render ancestor for its own subtree.
        let mut target: HashMap<flui_foundation::RenderId, Vec<flui_foundation::RenderId>> =
            HashMap::new();
        let mut desired_parent: HashMap<
            flui_foundation::RenderId,
            Option<flui_foundation::RenderId>,
        > = HashMap::new();
        let mut pipeline_any: Option<Arc<dyn std::any::Any + Send + Sync>> = None;

        let roots: Vec<ElementId> = self
            .iter_nodes()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(id, _)| id)
            .collect();

        // Children are pushed reversed so siblings pop in ascending slot order.
        let mut stack: Vec<(ElementId, Option<flui_foundation::RenderId>)> =
            roots.into_iter().rev().map(|id| (id, None)).collect();
        while let Some((element_id, render_ancestor)) = stack.pop() {
            let Some(node) = self.get(element_id) else {
                continue;
            };
            let child_ancestor = if let Some(render_id) = node.element().render_id() {
                desired_parent.insert(render_id, render_ancestor);
                if let Some(parent_render) = render_ancestor {
                    target.entry(parent_render).or_default().push(render_id);
                }
                if pipeline_any.is_none() {
                    pipeline_any = node.element().pipeline_owner_any();
                }
                Some(render_id)
            } else {
                render_ancestor
            };
            for &child in node.child_ids().iter().rev() {
                stack.push((child, child_ancestor));
            }
        }

        if desired_parent.is_empty() {
            return;
        }
        let Some(pipeline_any) = pipeline_any else {
            return;
        };
        let Ok(pipeline_owner) = pipeline_any.downcast::<RwLock<PipelineOwner>>() else {
            return;
        };

        // Tree borrows are dropped; the lock guards only the render tree.
        let mut owner = pipeline_owner.write();
        let mut dirty_render_parents: HashSet<flui_foundation::RenderId> = HashSet::new();
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
                    }
                    if let Some(parent) = desired {
                        dirty_render_parents.insert(parent);
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
                let mut desired_children = target.get(parent_render).cloned().unwrap_or_default();
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
        }

        for parent in dirty_render_parents {
            if owner.render_tree().contains(parent) {
                owner.mark_needs_layout(parent);
            }
        }
    }

    /// Recompute the inherited scope ([`ElementNode::inherited`]) for the
    /// subtree rooted at `root_id`, top-down against each node's current
    /// parent.
    ///
    /// Needed after a GlobalKey reparent ([`try_retake_global_key`]): the moved
    /// subtree's nodes carry maps built against their OLD ancestor chain, so
    /// `depend_on` would resolve providers from the old location. A node is
    /// only processed after its parent (the stack guarantees parent-before-
    /// child), so each child recomputes against its parent's already-updated
    /// scope — mirroring Flutter re-running `_updateInheritance` down a
    /// reactivated subtree. Average/worst case O(subtree size).
    fn recompute_inherited_subtree(&mut self, root_id: ElementId) {
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
            let parent_map = match node.parent {
                Some(parent_id) => self
                    .get(parent_id)
                    .map_or_else(|| Arc::new(HashMap::new()), |p| Arc::clone(&p.inherited)),
                None => Arc::new(HashMap::new()),
            };
            let scope = {
                let node = self.get(id).expect("id resolved at loop top");
                compute_inherited_scope(&parent_map, node.element(), id)
            };
            let node = self.get_mut(id).expect("id resolved at loop top");
            node.inherited = scope;
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
    #[allow(
        dead_code,
        reason = "consumed by the build-context wiring (PR-K wire-real)"
    )]
    pub(crate) fn take_element(&mut self, id: ElementId) -> Option<ElementKind> {
        let index = self.resolve_index(id)?;
        self.nodes.get_mut(index)?.kind.take()
    }

    /// Restore an element previously removed by [`Self::take_element`] into
    /// node `id`'s slot. No-op for a stale/absent id (cannot happen on the
    /// build path: the node is re-addressed by the same id taken moments
    /// earlier).
    #[allow(
        dead_code,
        reason = "consumed by the build-context wiring (PR-K wire-real)"
    )]
    pub(crate) fn put_element(&mut self, id: ElementId, kind: ElementKind) {
        if let Some(index) = self.resolve_index(id)
            && let Some(node) = self.nodes.get_mut(index)
        {
            node.kind = Some(kind);
        }
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
        if self.nodes[index].registered_global_key_hash.is_some() {
            let depth = self.nodes[index].depth;
            self.nodes[index].element_mut().deactivate();
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
        // `did_change_dependencies` flag — the dependent
        // leaves the active tree before its rebuild ever runs.
        owner.clear_pending_dependency_change(id);
        self.nodes[index].element_mut().unmount(owner);

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
    /// two).
    ///
    /// The algorithm mirrors `id_reconcile::remove_child` / `collect_subtree_preorder`:
    ///
    /// 1. Snapshot the subtree in pre-order (parent before children) while all
    ///    `child_ids` lists are intact.
    /// 2. Remove the root via `remove` (soft-removes keyed elements).
    /// 3. If the root was eagerly removed (un-keyed), free its descendants
    ///    deepest-first via `remove_finalized`.
    ///
    /// Complexity: O(n) time + O(n) peak heap for the work-stack (n = subtree
    /// size), O(h) call-stack for the constant-stack iterative walk.
    pub(crate) fn remove_subtree(&mut self, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        // Snapshot subtree pre-order (parent before children) before touching
        // any node, while every `child_ids` list is still intact.
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

        // Remove the root; `Some` ⇒ eagerly freed (un-keyed), `None` ⇒
        // soft-removed (keyed) and parked for `finalize_tree`.
        let root_removed_eagerly = self.remove(id, owner).is_some();

        if root_removed_eagerly {
            // Free orphaned descendants deepest-first.  `subtree[0]` is the
            // root (already freed above); iterating in reverse visits each
            // child after all of its own descendants.
            for &descendant in subtree[1..].iter().rev() {
                self.remove_finalized(descendant, owner);
            }
        }
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
        if let Some(hash) = self.nodes[index].registered_global_key_hash.take() {
            owner.unregister_global_key(hash);
        }

        // Drop any stale `did_change_dependencies` flag —
        // the dependent leaves the tree before its rebuild ever runs.
        owner.clear_pending_dependency_change(id);
        self.nodes[index].element_mut().unmount(owner);

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
    pub fn deactivate(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element_mut().deactivate();
        }
    }

    /// Activate an element (re-insertion after deactivation).
    pub fn activate(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element_mut().activate();
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
// GlobalKey helpers
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

/// State-migration entry point. If `hash` resolves to an existing element,
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
fn try_retake_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    hash: u64,
    view: &dyn View,
    new_parent: ElementId,
    new_slot: usize,
) -> Option<ElementId> {
    let candidate_id = owner.element_for_global_key(hash)?;
    if !can_retake_global_key_candidate(tree, candidate_id, view) {
        return None;
    }

    if owner.is_inactive(candidate_id) {
        return retake_inactive_global_key(
            tree,
            owner,
            hash,
            view,
            candidate_id,
            new_parent,
            new_slot,
        );
    }

    retake_active_global_key(tree, owner, hash, view, candidate_id, new_parent, new_slot)
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

fn retake_inactive_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    hash: u64,
    view: &dyn View,
    candidate_id: ElementId,
    new_parent: ElementId,
    new_slot: usize,
) -> Option<ElementId> {
    owner.remove_inactive(candidate_id);

    let parent_depth = tree.get(new_parent).map_or(0, ElementNode::depth);
    let child_parent_render_id = tree
        .get(new_parent)
        .and_then(|node| node.element().child_render_id());

    // Route through the staleness-checked accessor. The candidate came from the
    // live GlobalKey registry and was soft-removed (slot kept, generation NOT
    // bumped), so its generation still matches and this resolves.
    let node = tree.get_mut(candidate_id)?;
    node.parent = Some(new_parent);
    node.slot = new_slot;
    node.depth = parent_depth + 1;
    node.element_mut()
        .set_parent_render_id(child_parent_render_id);

    // Re-activate the element. `Lifecycle::Inactive` → `Active`.
    node.element_mut().activate();

    // Apply the NEW view configuration to the re-taken element. Without
    // this the element keeps the stale view config from before it was
    // deactivated — state persists (the whole point of GlobalKey
    // reparenting) but the view fields, child-list shape, and any
    // update hooks (`didUpdateWidget`-equivalent) would be silently
    // skipped. Flutter's `_retakeInactiveElement` does the same in
    // `framework.dart:4581` (`element.update(newWidget)`) right after
    // activating.
    node.element_mut().update(view, owner);
    // FR-022: re-clone the key from the new view value so
    // the stored key tracks the re-taken element's current
    // configuration — the deactivated element's old key may match
    // structurally (`is_global_key` is true on both sides) but the
    // concrete `Box<dyn ViewKey>` is the new view's key now.
    node.set_key(view.key().map(ViewKey::clone_key));

    // The subtree moved under a new parent, so its inherited scopes (built
    // against the OLD ancestor chain) are stale — recompute top-down against
    // `new_parent`. Flutter re-runs `_updateInheritance` on reactivation
    // (`framework.dart:4775`). `node`'s `&mut` borrow ends above, freeing
    // `tree` for this walk.
    tree.recompute_inherited_subtree(candidate_id);
    tree.apply_ancestor_parent_data(candidate_id);
    tree.needs_render_reorder = true;

    tracing::debug!(
        candidate = ?candidate_id,
        new_parent = ?new_parent,
        new_slot,
        "ElementTree::insert retook inactive element for GlobalKey state migration"
    );

    // SC-003: emit ReconcileEvent::Reparent. The element
    // came from the inactive queue (Lifecycle::Inactive → Active), so
    // `from_parent: None` per ADV-1 branch case 1 — there is no prior
    // *active* parent at the moment of reparent; the donor parent
    // already cleared its slot when it pushed the element into the
    // inactive queue.
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

fn retake_active_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    hash: u64,
    view: &dyn View,
    candidate_id: ElementId,
    new_parent: ElementId,
    new_slot: usize,
) -> Option<ElementId> {
    let from_parent = tree.get(candidate_id)?.parent()?;
    if from_parent == new_parent {
        tracing::error!(
            ?hash,
            ?candidate_id,
            ?new_parent,
            "GlobalKey appears twice under the same active parent"
        );
        #[cfg(debug_assertions)]
        {
            panic!(
                "GlobalKey hash {hash} is already active under {new_parent:?}; \
                 duplicate GlobalKey children are not allowed"
            );
        }
        #[cfg(not(debug_assertions))]
        {
            return None;
        }
    }

    if let Some(old_parent) = tree.get_mut(from_parent) {
        if let Some(pos) = old_parent
            .child_ids
            .iter()
            .position(|&child| child == candidate_id)
        {
            old_parent.child_ids.remove(pos);
        } else {
            tracing::warn!(
                ?candidate_id,
                ?from_parent,
                "active GlobalKey candidate was registered under a parent that no longer lists it"
            );
        }
    }

    let (parent_depth, child_parent_render_id) = tree.get(new_parent).map_or((0, None), |node| {
        (node.depth(), node.element().child_render_id())
    });

    let node = tree.get_mut(candidate_id)?;
    node.element_mut().deactivate();
    node.parent = Some(new_parent);
    node.slot = new_slot;
    node.depth = parent_depth + 1;
    node.element_mut()
        .set_parent_render_id(child_parent_render_id);
    node.element_mut().activate();
    node.element_mut().update(view, owner);
    node.set_key(view.key().map(ViewKey::clone_key));

    tree.recompute_inherited_subtree(candidate_id);
    tree.apply_ancestor_parent_data(candidate_id);
    tree.needs_render_reorder = true;

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
        hash,
    ));

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
    use crate::{BuildContext, BuildOwner, StatelessView, View};

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
        tree.recompute_inherited_subtree(k);

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
        tree.recompute_inherited_subtree(k);

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
    /// (`try_retake_global_key` → `recompute_inherited_subtree`). `GlobalKey<T>`
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

        // Soft-remove K (→ inactive queue), then re-insert under provider_b
        // with the SAME GlobalKey: the real `try_retake_global_key` reactivates
        // it and calls `recompute_inherited_subtree`.
        tree.write()
            .remove(k, &mut owner.write().element_owner_mut());
        let migrated = tree.write().insert(
            &keyed,
            provider_b,
            0,
            &mut owner.write().element_owner_mut(),
        );
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

        crate::test_only_clear_global_key_registry();
    }
}
