//! [`ElementNode`]: one element and its tree metadata.

use std::any::TypeId;

use flui_foundation::{ElementId, ViewKey};
use smallvec::SmallVec;

use crate::element::ElementKind;
use crate::view::ElementBase;

use super::InheritedScope;

/// A node in the Element tree.
///
/// Contains the Element plus metadata for tree traversal.
pub struct ElementNode {
    /// The actual Element.
    ///
    /// Normally `Some`. A `None` hole exists ONLY transiently inside
    /// [`BuildOwner::build_scope`](crate::owner::BuildOwner), between
    /// [`ElementTree::take_element`](super::ElementTree::take_element) and [`ElementTree::put_element`](super::ElementTree::put_element), while
    /// the element runs its own `build()` against a live read view of the
    /// rest of the tree. By-value extraction is what lets the element be
    /// `&mut`-borrowed AND hand the slab a `&` borrow at the same time
    /// without aliasing. Outside that window every accessor assumes `Some`;
    /// during it, ancestor walks read [`Self::element_opt`] (which returns
    /// `None` for the hole rather than panicking).
    pub(crate) kind: Option<ElementKind>,
    /// Parent Element ID (None for root).
    pub(crate) parent: Option<ElementId>,
    /// Depth in the tree (root = 0). Private so every write goes through
    /// [`Self::set_depth`], which also stamps the element.
    depth: usize,
    /// Slot index within parent's children.
    pub(crate) slot: usize,
    /// Cloned `View::key()` for the view this element currently holds,
    /// or `None` when the view is keyless.
    ///
    /// Populated at every `insert`/`mount_root_*`
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
    /// Kept as a side channel on the node because the `View` value is owned by
    /// `ElementCore` and not available at the dispatch boundary used for
    /// finalization.
    pub(crate) registered_global_key: Option<Box<dyn ViewKey>>,
    /// This node's slab-id-based child list — the single, authoritative
    /// element child graph.
    ///
    /// Written by the production id-based reconciler
    /// [`reconcile_children_by_id`](super::super::id_reconcile::reconcile_children_by_id),
    /// which `build_scope` drives once per dirty element; read by every
    /// child traversal in the framework (build, mount, unmount,
    /// dirty-collection). The old per-element `Box<dyn ElementBase>` child
    /// storage is gone — elements no longer own children.
    ///
    /// Ordering is meaningful: entry `i` is the element occupying child
    /// slot `i` after the most recent id-reconcile, matching the new
    /// view order.
    ///
    /// Two ids sit inline, in the bytes a `Vec` header takes, so a leaf, a
    /// single-child wrapper and a pair allocate no list.
    pub(crate) child_ids: SmallVec<[ElementId; 2]>,
    /// The set of [`InheritedView`](crate::view::InheritedView) providers in
    /// scope at this node — `provider view TypeId → provider ElementId` — so
    /// `BuildContext::depend_on_inherited` / `get_inherited` resolve the
    /// nearest `P` provider in **O(1)** instead of an O(depth) ancestor walk.
    /// (Only those two inherited lookups read this map; the `find_ancestor_*`
    /// family still walks, as it matches arbitrary — not just inherited —
    /// ancestor types.)
    ///
    /// Built top-down at [`insert`](super::ElementTree::insert) /
    /// [`mount_root_with_pipeline_owner`](super::ElementTree::mount_root_with_pipeline_owner):
    /// a non-provider aliases its parent's map by refcount (`Arc::clone`); a
    /// provider stores
    /// `parent_map + (view_type_id → self)` so nested same-type providers
    /// shadow nearest-wins. Recomputed for a re-taken subtree on GlobalKey
    /// reparent. Like `parent`/`depth`/`child_ids` it is a node field, so it
    /// survives the `build_scope` take/put window and a building element can
    /// read its own scope while its `element` slot is a hole.
    ///
    /// The map is keyed on the published `TypeId` and is a plain
    /// `Arc<FxHashMap>` with copy-on-insert-at-providers rather than a persistent
    /// HAMT — provider counts in a UI scope are tiny, so the per-provider
    /// O(k) clone is effectively O(1) and avoids a new dependency. A node
    /// outside every provider holds no map ([`InheritedScope::EMPTY`]).
    pub(crate) inherited: InheritedScope,
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
        let mut node = Self {
            kind: Some(kind),
            parent,
            depth: 0,
            slot,
            key: None,
            registered_global_key: None,
            child_ids: SmallVec::new(),
            // Empty until the tree sets the real scope against the parent's
            // map (insert / mount_root_*), mirroring how `key`/`depth` are
            // finalised by the caller right after construction.
            inherited: InheritedScope::EMPTY,
        };
        // Provisional; the tree stamps the real depth before mount.
        node.set_depth(usize::from(parent.is_some()));
        node
    }

    /// Set this node's depth in the tree and stamp it onto its element, so
    /// [`ElementBase::depth`] and [`Self::depth`] never disagree.
    ///
    /// The only writer of the depth. A node is only `&`-borrowed while its
    /// element is extracted for a build, so a write never meets that hole.
    pub(crate) fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
        debug_assert!(
            self.kind.is_some(),
            "BUG: ElementNode::set_depth during the build_scope take/put window"
        );
        if let Some(kind) = self.kind.as_mut() {
            kind.set_depth(crate::view::ElementDepth::new(depth));
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
        self.inherited.get(type_id)
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
    /// (every `LiveBuildContext` ancestor walk) must go through [`Self::element_opt`]
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
    /// `old_keyed: FxHashMap<u64, ElementId>` index — no view-typed
    /// `downcast::<V>()` needed.
    pub fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_deref()
    }

    /// `View::key().map(ViewKey::key_hash)` for this element.
    ///
    /// Convenience over the two-step `key().map(ViewKey::key_hash)`.
    /// Returns `None` for keyless elements ("no key, fall back to positional"
    /// matching).
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
    /// single element graph.
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
    /// Called by [`reconcile_children_by_id`](super::super::id_reconcile::reconcile_children_by_id)
    /// at the end of a reconcile pass with the freshly computed child
    /// order (one id per new view, in new-view order). Overwrites any
    /// previous list wholesale — the caller is responsible for having
    /// already inserted / removed the corresponding slab nodes so the
    /// stored ids all resolve.
    ///
    /// The list keeps its buffer, so a rebuild that keeps the child count
    /// allocates nothing. A list that outgrows its buffer, or shrank to
    /// under a quarter of it, gets a buffer of exactly its own size: growing
    /// in place would round a one-child list up to four slots.
    pub(crate) fn set_child_ids(&mut self, ids: &[ElementId]) {
        let capacity = self.child_ids.capacity();
        if capacity < ids.len() || capacity / 4 > ids.len() {
            self.child_ids = SmallVec::from_slice(ids);
        } else {
            self.child_ids.clear();
            self.child_ids.extend_from_slice(ids);
        }
    }

    pub(super) fn commit_child_replacement(
        &mut self,
        slot: usize,
        expected_old: ElementId,
        replacement: ElementId,
    ) {
        assert!(
            self.child_ids.get(slot) == Some(&expected_old),
            "BUG: a replacement slot must still contain its preflighted old child after mount",
        );
        self.child_ids[slot] = replacement;
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
