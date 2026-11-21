//! Slab-based Element tree storage.
//!
//! Elements are stored in a Slab for O(1) access by ElementId; they form the
//! retained tree.

use std::cell::Cell;

use std::rc::Rc;

#[cfg(test)]
use flui_foundation::generation::Generation;
use flui_foundation::generation::{GenSlots, Release};
use flui_foundation::{ElementId, ViewKey};
use slab::Slab;

use crate::element::ElementKind;
use crate::owner::LifecycleHook;
use crate::view::View;

use super::containment::{ChildHookPanic, contain_window};

mod inherited_scope;
pub(crate) use inherited_scope::InheritedScope;
use inherited_scope::compute_inherited_scope;

mod activation;
mod element_node;
mod global_key_retake;
mod mounting;
mod removal;
mod render_children;

pub use element_node::ElementNode;
pub(crate) use mounting::{InsertedChild, ProvisionalOrder};
pub(crate) use removal::SubtreeRemoval;
pub(crate) use render_children::enclosing_sparse_child;

/// Slab-based Element tree storage.
///
/// Provides O(1) access to Elements by ElementId.
/// ElementIds use NonZeroUsize (1-based) while Slab uses 0-based indices.
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
    /// Per-slot generations (`GenSlots`), parallel to `nodes`: an id minted
    /// against a slot carries its live generation, so a straggler id refuses
    /// instead of naming the slot's next occupant; an exhausted generation
    /// closes the slot, and its release refuses before any mutation.
    generations: GenSlots,
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
pub(crate) struct ReconcileGuard {
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
            generations: GenSlots::new(),
            root: None,
            needs_render_reorder: false,
            reconcile_state: Rc::new(ReconcileState::default()),
        }
    }

    /// Create an ElementTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            generations: GenSlots::new(),
            root: None,
            needs_render_reorder: false,
            reconcile_state: Rc::new(ReconcileState::default()),
        }
    }

    pub(crate) fn begin_reconcile(&self, parent_id: ElementId) -> ReconcileGuard {
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

    /// Mint an [`ElementId`] for a freshly-inserted slab slot: a fresh slot
    /// mints its first generation, a reused slot carries the generation the
    /// prior remove's commit left, so the minted id differs from any id the
    /// slot's previous occupant had.
    ///
    /// # Panics
    ///
    /// Panics if `slab_index` exceeds `u32::MAX` — the element tree cannot
    /// hold more than `u32::MAX` live slots because [`ElementId`] packs the
    /// index into 32 bits. This is the index-cap bound.
    fn alloc_id(&mut self, slab_index: usize) -> ElementId {
        let index = slab_index_to_u32(slab_index);
        let generation = if let Some(live) = self.generations.generation(index) {
            // Reused slot: generation already stepped by the prior remove.
            live
        } else {
            // Fresh slot: the slab grew by exactly one. Seed its first.
            debug_assert_eq!(
                slab_index,
                self.generations.slot_count(),
                "slab must grow by exactly one slot per insert"
            );
            self.generations.mint_fresh()
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
        match self.generations.generation(id.index()) {
            Some(live) if live == id.generation() && self.nodes.contains(index) => Some(index),
            _ => None,
        }
    }

    /// Preflight a removal: the peek runs before any hook or unmount, so a
    /// caught panic between it and the commit steps nothing, and the released
    /// identity cannot be minted from under a partially torn-down occupant. A
    /// slot whose generation space is exhausted has no step: the removal
    /// refuses while the current occupant stays live.
    fn next_generation(&self, index: usize) -> Release {
        self.generations
            .peek(slab_index_to_u32(index))
            .unwrap_or_else(|| {
                panic!(
                    "ElementTree: slab slot {index} exhausted its generations \
                     (ABA-safety overflow — removal refused)"
                )
            })
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
    /// [`BuildOwner::build_scope`](crate::owner::BuildOwner) takes the element out
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

    /// Replay the live tree as synthetic `ElementMounted` events, in
    /// pre-order (parent before children, `child_ids` slot order) — the
    /// baseline for a mid-run observer attach (ADR-0040 §3).
    ///
    /// Walks from the root via `child_ids`, so soft-removed keyed elements
    /// parked in the inactive queue are NOT replayed. The caller
    /// (`WidgetsBinding::install_tree_observer`) holds the UI runtime write lock
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
    /// Re-clones `View::key()` into the node so the
    /// stored key tracks whatever the new view carries. `View::can_update`
    /// already ensures the keys match on a successful
    /// update — the re-clone preserves that invariant explicitly rather
    /// than relying on the caller having already filtered by it.
    ///
    /// A panic inside the update (see `ChildHookPanic`'s doc
    /// in `tree/containment.rs`) unwinds through here exactly as it always
    /// has — the crate-internal `try_update` is the fallible core a
    /// bounding caller uses instead, and `update_or_substitute` is the one
    /// that catches and substitutes.
    pub fn update(&mut self, id: ElementId, view: &dyn View, owner: &mut crate::ElementOwner<'_>) {
        if let Err(panic) = self.try_update(id, view, owner) {
            std::panic::resume_unwind(panic.payload);
        }
    }

    /// [`update`](Self::update)'s fallible core: the element's own `update`
    /// (`ViewState::did_update_view`, or a self-building behavior's
    /// `update_render_object`) is the containment window, and a caught panic comes back as `Err`
    /// instead of unwinding through this call. Reading and cloning the new
    /// view's key happens before that literal window, so a key implementation
    /// panic remains an unbounded framework-input failure and user `update`
    /// has not started.
    pub(super) fn try_update(
        &mut self,
        id: ElementId,
        view: &dyn View,
        owner: &mut crate::ElementOwner<'_>,
    ) -> Result<(), ChildHookPanic> {
        let Some(node) = self.get_mut(id) else {
            return Ok(());
        };
        let stored_key = view.key().map(ViewKey::clone_key);
        let update_result = {
            let element = node.element_mut();
            contain_window(|| element.update(view, owner))
        };
        if let Err(payload) = update_result {
            return Err(ChildHookPanic {
                inserted: None,
                hook: LifecycleHook::Update,
                staged: None,
                payload,
            });
        }
        node.set_key(stored_key);
        // A reconfigured `ParentDataView` ancestor reaches this render child via
        // its own re-`update` (the reconciler walks children after their
        // parent), so re-deriving parent data here keeps it current — e.g.
        // `Expanded`'s `flex` changing between frames.
        self.apply_ancestor_parent_data(id);
        Ok(())
    }

    /// Mark an element as needing rebuild.
    pub fn mark_needs_build(&mut self, id: ElementId) {
        if let Some(node) = self.get_mut(id) {
            node.element_mut().mark_needs_build();
        }
    }

    /// Iterate over all live element IDs.
    ///
    /// Each id is minted from the slot's *current* generation, so the yielded
    /// ids round-trip through [`ElementTree::get`] (a `new(index+1)` shortcut
    /// would carry generation 1 and fail the staleness compare on any reused
    /// slot).
    pub fn iter(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.nodes
            .iter()
            .map(move |(index, _)| self.id_of_index(index))
    }

    /// Iterate over all live element nodes.
    pub fn iter_nodes(&self) -> impl Iterator<Item = (ElementId, &ElementNode)> + '_ {
        self.nodes
            .iter()
            .map(|(index, node)| (self.id_of_index(index), node))
    }

    /// The id the live slab slot `index` answers, with its live generation.
    fn id_of_index(&self, index: usize) -> ElementId {
        let slot = slab_index_to_u32(index);
        let generation = self
            .generations
            .generation(slot)
            .expect("BUG: the generation ledger runs parallel to the slab");
        ElementId::new_gen(slot, generation)
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
    use crate::view::{BoxedView, IntoView, ViewExt};
    use flui_rendering::{parent_data::SliverMultiBoxAdaptorParentData, pipeline::PipelineCell};
    use std::any::TypeId;

    #[path = "activation_recovery_tests.rs"]
    mod activation_recovery_tests;

    #[path = "exhaustion_refusal_tests.rs"]
    mod exhaustion_refusal_tests;

    use crate::{BuildContext, GlobalKey, RenderView, StatelessView, View, owner::BuildOwner};

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
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    #[derive(Clone, StatelessView)]
    struct KeyedTransparentView {
        #[view(key)]
        key: GlobalKey<()>,
    }

    impl StatelessView for KeyedTransparentView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            UnitRenderHost.boxed()
        }
    }

    #[derive(Clone, crate::StatefulView)]
    struct ActiveRetakePanicsOnActivate {
        #[view(key)]
        key: GlobalKey<ActiveRetakePanicState>,
        armed: std::rc::Rc<std::cell::Cell<bool>>,
    }

    struct ActiveRetakePanicState {
        armed: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl crate::StatefulView for ActiveRetakePanicsOnActivate {
        type State = ActiveRetakePanicState;

        fn create_state(&self, _ctx: &dyn crate::LifecycleContext) -> Self::State {
            ActiveRetakePanicState {
                armed: std::rc::Rc::clone(&self.armed),
            }
        }
    }

    impl crate::ViewState<ActiveRetakePanicsOnActivate> for ActiveRetakePanicState {
        fn build(
            &self,
            _view: &ActiveRetakePanicsOnActivate,
            _ctx: &dyn BuildContext,
        ) -> impl IntoView {
            UnitRenderHost
        }

        fn activate(&mut self) {
            assert!(!self.armed.get(), "active retake activate panic");
        }
    }

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
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
        }
    }

    /// A view carrying a `GlobalKey`, for subtree-removal fixtures.
    #[derive(Clone, StatelessView)]
    struct KeyedTestView {
        #[view(key)]
        key: crate::key::GlobalKey<()>,
    }

    impl StatelessView for KeyedTestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            TestView {
                name: "keyed-child".into(),
            }
            .boxed()
        }
    }

    /// Removing an UNKEYED subtree must not unmount a `GlobalKey`'d
    /// descendant — it is deactivated so a key can retake it in the same
    /// frame.
    ///
    /// `remove_subtree` used to snapshot the whole subtree and hard-remove
    /// every descendant through `remove_finalized`, which bypasses the
    /// soft-remove path EVEN FOR KEYED ELEMENTS. A keyed element inside an
    /// unkeyed wrapper therefore lost its identity the moment the wrapper was
    /// evicted — the exact thing the key exists to prevent.
    ///
    /// It surfaced sideways: a lazy list's per-item `RepaintBoundary` is an
    /// unkeyed wrapper, so a keyed row under one could not survive eviction,
    /// and the graft test in `flui-widgets` had to turn boundaries off to work
    /// at all. ADR-0052 recorded it as a named gap. It is tree-wide, not
    /// lazy-specific.
    ///
    /// The assertion is that the element survives in the INACTIVE queue with
    /// its slab entry intact, which is what a retake needs. `child_ids` is
    /// built directly here because only a reconcile pass populates it — the
    /// same fixture shape the relocation tests above use.
    fn removing_an_unkeyed_subtree_deactivates_a_keyed_descendant() {
        let mut tree = ElementTree::new();
        let mut owner = crate::owner::BuildOwner::new();

        let root = tree.mount_root(
            &TestView {
                name: "root".into(),
            }
            .boxed(),
            &mut owner.element_owner_mut(),
        );
        let wrapper = tree.insert(
            &TestView {
                name: "unkeyed-wrapper".into(),
            }
            .boxed(),
            root,
            0,
            &mut owner.element_owner_mut(),
        );
        let keyed = tree.insert(
            &KeyedTestView {
                key: crate::key::GlobalKey::<()>::new(),
            }
            .boxed(),
            wrapper,
            0,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(root).expect("root").set_child_ids(&[wrapper]);
        tree.get_mut(wrapper)
            .expect("wrapper")
            .set_child_ids(&[keyed]);

        assert!(
            tree.get(keyed)
                .expect("keyed child mounted")
                .registered_global_key()
                .is_some(),
            "precondition: the fixture's key must actually register",
        );

        tree.remove_subtree(
            wrapper,
            &mut owner.element_owner_mut(),
            SubtreeRemoval::DeactivateKeyed,
        );

        assert!(
            tree.get(wrapper).is_none(),
            "the unkeyed wrapper itself is freed",
        );
        assert!(
            tree.get(keyed).is_some(),
            "the keyed descendant must survive into the inactive queue — \
             freeing it here is what loses the state the key exists for",
        );
        assert!(
            owner.has_inactive_elements(),
            "and it must be ON that queue, so `finalize_tree` unmounts it if \
             no key retakes it this frame",
        );
    }

    fn active_retake_wires_provisional_order_and_sparse_merge_end_to_end() {
        type BoxObject =
            Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let pipeline = PipelineCell::new(flui_rendering::pipeline::PipelineOwner::new(
            flui_rendering::TextContextHandle::standalone(),
        ));
        let root = tree.mount_root_with_pipeline_owner(
            &UnitRenderHost.boxed(),
            Some(pipeline.clone()),
            &mut owner.element_owner_mut(),
        );
        let donor = tree.insert(
            &UnitRenderHost.boxed(),
            root,
            0,
            &mut owner.element_owner_mut(),
        );
        let destination = tree.insert(
            &UnitRenderHost.boxed(),
            root,
            1,
            &mut owner.element_owner_mut(),
        );
        let key = GlobalKey::<()>::new();
        let keyed = KeyedTransparentView { key };
        let candidate = tree.insert(
            &keyed.clone().boxed(),
            donor,
            0,
            &mut owner.element_owner_mut(),
        );
        let first_boundary = tree.insert(
            &UnitRenderHost.boxed(),
            candidate,
            0,
            &mut owner.element_owner_mut(),
        );
        let second_boundary = tree.insert(
            &UnitRenderHost.boxed(),
            candidate,
            1,
            &mut owner.element_owner_mut(),
        );
        let nested = tree.insert(
            &UnitRenderHost.boxed(),
            first_boundary,
            0,
            &mut owner.element_owner_mut(),
        );
        let prefix = tree.insert(
            &UnitRenderHost.boxed(),
            destination,
            0,
            &mut owner.element_owner_mut(),
        );
        let suffix = tree.insert(
            &UnitRenderHost.boxed(),
            destination,
            2,
            &mut owner.element_owner_mut(),
        );
        tree.get_mut(root)
            .expect("root")
            .set_child_ids(&[donor, destination]);
        tree.get_mut(donor)
            .expect("donor")
            .set_child_ids(&[candidate]);
        tree.get_mut(candidate)
            .expect("candidate")
            .set_child_ids(&[first_boundary, second_boundary]);
        tree.get_mut(first_boundary)
            .expect("first boundary")
            .set_child_ids(&[nested]);
        tree.get_mut(destination)
            .expect("destination")
            .set_child_ids(&[prefix, suffix]);

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
            for (id, index, what) in [(sparse_late, 9, "late"), (sparse_early, 3, "early")] {
                let stamped = owner.update_parent_data(id, |slot| {
                    slot.set(Box::new(SliverMultiBoxAdaptorParentData::new(index)));
                    flui_rendering::RenderUpdateImpact::NONE
                });
                assert!(stamped.is_some(), "{what} sparse node");
            }
            (sparse_late, sparse_early)
        });

        let _guard = tree.begin_reconcile(destination);
        let moved = tree.mount_or_substitute(
            &keyed.boxed(),
            destination,
            1,
            &mut owner.element_owner_mut(),
            ProvisionalOrder::during_reconcile(&[prefix], &[Some(suffix)]),
            "retaking child during reconcile test",
        );
        assert_eq!(moved, candidate);
        assert!(
            owner.take_recovered_panics().is_empty(),
            "a successful retake records no recovered panic"
        );
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

    // -----------------------------------------------------------------------
    // Generational staleness (ABA safety)
    // -----------------------------------------------------------------------

    /// The core ABA guard: an id that addressed a since-freed slot must NOT
    /// resolve to the unrelated element that later reuses the same slab slot.
    fn stale_id_after_slot_reuse_resolves_none() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = TestView {
            name: "root".to_string(),
        };
        let root_id = tree.mount_root(&root.boxed(), &mut owner.element_owner_mut());

        // Insert child A, then eagerly remove it (un-keyed → slab slot freed,
        // generation bumped).
        let child_a = TestView {
            name: "a".to_string(),
        };
        let id_a = tree.insert(&child_a.boxed(), root_id, 0, &mut owner.element_owner_mut());
        assert!(tree.remove(id_a, &mut owner.element_owner_mut()).is_some());

        // Insert child B — the slab hands back the slot A just vacated.
        let child_b = TestView {
            name: "b".to_string(),
        };
        let id_b = tree.insert(&child_b.boxed(), root_id, 0, &mut owner.element_owner_mut());

        // Same slot, different generation → distinct ids.
        assert_eq!(
            id_a.index(),
            id_b.index(),
            "test precondition: slab must reuse the freed slot"
        );
        assert_ne!(id_a, id_b, "reused slot must mint a distinct generation");
        assert_eq!(id_b.generation().as_u32(), 2, "reused slot generation = 2");

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

    // ========================================================================
    // Inherited scope: the per-node `inherited` map gives O(1)
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
        child: BoxedView,
    }

    impl crate::view::InheritedView for Theme {
        type Data = ThemeData;

        fn data(&self) -> &Self::Data {
            &self.data
        }

        fn child(&self) -> &BoxedView {
            &self.child
        }

        fn update_should_notify(&self, old: &Self) -> bool {
            self.data != old.data
        }
    }

    impl View for Theme {
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::inherited(self)
        }
    }

    fn theme(color: u32) -> Theme {
        Theme {
            data: ThemeData { color },
            child: TestView {
                name: "unused".to_string(),
            }
            .boxed(),
        }
    }

    fn leaf(name: &str) -> TestView {
        TestView {
            name: name.to_string(),
        }
    }

    fn nested_same_type_provider_shadows_nearest() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        let outer = tree.mount_root(&theme(1).boxed(), &mut owner.element_owner_mut());
        let inner = tree.insert(&theme(2).boxed(), outer, 0, &mut owner.element_owner_mut());
        let leaf_id = tree.insert(&leaf("l").boxed(), inner, 0, &mut owner.element_owner_mut());

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

    /// A provider fixture with a type of its own per `N`.
    #[derive(Clone)]
    struct Tier<const N: usize> {
        data: ThemeData,
        child: BoxedView,
    }

    impl<const N: usize> crate::view::InheritedView for Tier<N> {
        type Data = ThemeData;

        fn data(&self) -> &Self::Data {
            &self.data
        }

        fn child(&self) -> &BoxedView {
            &self.child
        }

        fn update_should_notify(&self, old: &Self) -> bool {
            self.data != old.data
        }
    }

    impl<const N: usize> View for Tier<N> {
        fn create_element(self: std::rc::Rc<Self>) -> crate::element::ElementKind {
            crate::element::ElementKind::inherited(self)
        }
    }

    fn tier<const N: usize>() -> Tier<N> {
        Tier {
            data: ThemeData { color: 0 },
            child: leaf("unused").boxed(),
        }
    }

    /// More provider types than a scope keeps beside its table: the ones
    /// folded into the table still resolve, a type published again below the
    /// fold shadows the folded provider, and a sibling branch that forked
    /// before the fold does not see what was published after it.
    fn providers_folded_into_the_table_resolve_and_shadow() {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();

        let first = tree.mount_root(&tier::<0>().boxed(), &mut owner.element_owner_mut());
        let second = tree.insert(
            &tier::<1>().boxed(),
            first,
            0,
            &mut owner.element_owner_mut(),
        );
        let branch = tree.insert(
            &leaf("branch").boxed(),
            second,
            1,
            &mut owner.element_owner_mut(),
        );
        let third = tree.insert(
            &tier::<2>().boxed(),
            second,
            0,
            &mut owner.element_owner_mut(),
        );
        let fourth = tree.insert(
            &tier::<3>().boxed(),
            third,
            0,
            &mut owner.element_owner_mut(),
        );
        let fifth = tree.insert(
            &tier::<4>().boxed(),
            fourth,
            0,
            &mut owner.element_owner_mut(),
        );
        let sixth = tree.insert(
            &tier::<5>().boxed(),
            fifth,
            0,
            &mut owner.element_owner_mut(),
        );
        let again = tree.insert(
            &tier::<0>().boxed(),
            sixth,
            0,
            &mut owner.element_owner_mut(),
        );
        let leaf_id = tree.insert(&leaf("l").boxed(), again, 0, &mut owner.element_owner_mut());

        let provider =
            |at: ElementId, type_id: TypeId| tree.get(at).unwrap().inherited_provider(type_id);
        let expected = [
            (TypeId::of::<Tier<0>>(), again),
            (TypeId::of::<Tier<1>>(), second),
            (TypeId::of::<Tier<2>>(), third),
            (TypeId::of::<Tier<3>>(), fourth),
            (TypeId::of::<Tier<4>>(), fifth),
            (TypeId::of::<Tier<5>>(), sixth),
        ];
        for (type_id, id) in expected {
            assert_eq!(provider(leaf_id, type_id), Some(id));
        }
        assert_eq!(
            provider(sixth, TypeId::of::<Tier<0>>()),
            Some(first),
            "above the second provider of the type the first is the one in scope"
        );
        assert_eq!(provider(branch, TypeId::of::<Tier<1>>()), Some(second));
        assert_eq!(provider(branch, TypeId::of::<Tier<2>>()), None);
        assert_eq!(provider(leaf_id, TypeId::of::<Theme>()), None);
    }

    #[test]
    fn element_tree_contract_matrix() {
        crate::table_test::run_table(
            "element_tree_contract_matrix",
            &[
                (
                    "exhausted_global_key_counter_never_reissues_an_identity",
                    crate::GlobalKey::<()>::exhausted_global_key_counter_never_reissues_an_identity
                        as fn(),
                ),
                (
                    "removing_an_unkeyed_subtree_deactivates_a_keyed_descendant",
                    removing_an_unkeyed_subtree_deactivates_a_keyed_descendant as fn(),
                ),
                (
                    "active_retake_wires_provisional_order_and_sparse_merge_end_to_end",
                    active_retake_wires_provisional_order_and_sparse_merge_end_to_end as fn(),
                ),
                (
                    "stale_id_after_slot_reuse_resolves_none",
                    stale_id_after_slot_reuse_resolves_none as fn(),
                ),
                (
                    "nested_same_type_provider_shadows_nearest",
                    nested_same_type_provider_shadows_nearest as fn(),
                ),
                (
                    "providers_folded_into_the_table_resolve_and_shadow",
                    providers_folded_into_the_table_resolve_and_shadow as fn(),
                ),
            ],
        );
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
