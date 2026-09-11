//! Pins the containment seam around `ViewState::dispose` (issue #561): a
//! panic inside `dispose` is caught per element instead of unwinding out of
//! `BuildOwner::build_scope` / `BuildOwner::finalize_tree`.
//!
//! Flutter has no equivalent boundary. `StatefulElement.unmount` calls
//! `state.dispose()` with no `try`/`catch` (`framework.dart`), and
//! `BuildOwner.finalizeTree`'s `_inactiveElements._unmountAll()` carries no
//! per-element catch either — a throwing `dispose` there aborts the whole
//! finalize pass. FLUI bounds the panic to the one element whose `dispose`
//! threw and lets the rest of the frame continue: see
//! `StatefulBehavior::on_unmount` (`crates/flui-view/src/element/behavior.rs`)
//! and the containment bullet in `crates/flui-view/AGENTS.md`.
//!
//! Also pins the `initialized` gate: `dispose` runs only for a state whose
//! `init_state` actually completed. FLUI's split mount/build (unlike
//! Flutter's synchronous `mount` -> `initState`) means an element can be
//! mounted and removed again before its first `build_scope` drain ever
//! reaches it; `create_state` must not itself acquire lifecycle resources
//! (mirrors Flutter's `createState` contract), so skipping `dispose` there
//! costs nothing a well-behaved state depends on.

use std::{any::TypeId, cell::Cell, rc::Rc};

use flui_foundation::ViewKey;
use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BoxedView, BuildContext, BuildOwner, ElementId, ElementTree, GlobalKey, IntoView,
    LifecycleHook, RebuildReason, RenderView, StatefulView, View, ViewExt, ViewState,
};

// ============================================================================
// Fixtures
// ============================================================================

/// A true tree leaf: renders directly, with no further `build()` in the
/// chain.
#[derive(Clone)]
struct PlainLeaf;

impl RenderView for PlainLeaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for PlainLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// A `render_variable` parent hosting a variable-length, unkeyed child
/// list. The id-reconciler matches unkeyed children purely positionally
/// (a top/bottom two-pointer scan; see `tree::id_reconcile`'s module doc),
/// so dropping the MIDDLE child on rebuild removes exactly that child and
/// keeps the two around it at their original slab ids.
#[derive(Clone)]
struct Row {
    children: Vec<BoxedView>,
}

impl RenderView for Row {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for child in &self.children {
            visitor(child);
        }
    }
}

impl View for Row {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// A Stateful view whose `dispose` panics exactly once, then behaves. The
/// `armed` flag flips to `false` the first time it fires, so a stray
/// second dispose call — there should never be one; that is exactly what
/// these tests pin — records a count instead of aborting the test process
/// on a double panic.
#[derive(Clone)]
struct DisposeCounter {
    key: Option<GlobalKey<DisposeCounterState>>,
    armed: Rc<Cell<bool>>,
    disposed: Rc<Cell<u32>>,
    init_state_calls: Rc<Cell<u32>>,
}

struct DisposeCounterState {
    armed: Rc<Cell<bool>>,
    disposed: Rc<Cell<u32>>,
    init_state_calls: Rc<Cell<u32>>,
}

impl StatefulView for DisposeCounter {
    type State = DisposeCounterState;

    fn create_state(&self) -> Self::State {
        DisposeCounterState {
            armed: self.armed.clone(),
            disposed: self.disposed.clone(),
            init_state_calls: self.init_state_calls.clone(),
        }
    }
}

impl ViewState<DisposeCounter> for DisposeCounterState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        self.init_state_calls.set(self.init_state_calls.get() + 1);
    }

    fn build(&self, _view: &DisposeCounter, _ctx: &dyn BuildContext) -> impl IntoView {
        PlainLeaf.boxed()
    }

    fn dispose(&mut self) {
        self.disposed.set(self.disposed.get() + 1);
        if self.armed.get() {
            self.armed.set(false);
            panic!("induced dispose panic (lifecycle containment test)");
        }
    }
}

impl View for DisposeCounter {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_ref().map(|key| key as &dyn ViewKey)
    }
}

/// Build a fresh `DisposeCounter` plus the two counters the test reads
/// back — `armed` controls whether its one dispose call panics.
fn new_counter(
    key: Option<GlobalKey<DisposeCounterState>>,
    armed: bool,
) -> (DisposeCounter, Rc<Cell<u32>>, Rc<Cell<u32>>) {
    let disposed = Rc::new(Cell::new(0));
    let init_state_calls = Rc::new(Cell::new(0));
    let view = DisposeCounter {
        key,
        armed: Rc::new(Cell::new(armed)),
        disposed: disposed.clone(),
        init_state_calls: init_state_calls.clone(),
    };
    (view, disposed, init_state_calls)
}

/// `parent`'s direct children, in `child_ids` slot order.
fn direct_children_in_slot_order(tree: &ElementTree, parent: ElementId) -> Vec<ElementId> {
    let mut children: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect();
    children.sort_by_key(|(slot, _)| *slot);
    children.into_iter().map(|(_, id)| id).collect()
}

// ============================================================================
// Inline removal: an unkeyed dispose panic is contained during the
// id-reconcile that drops the child, and the freed slot never re-disposes.
// ============================================================================

#[test]
fn a_dispose_panic_during_finalize_is_contained_and_the_slot_is_freed() {
    let (counter, disposed, _init_state_calls) = new_counter(None, true);

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();

    let row_v1 = Row {
        children: vec![PlainLeaf.boxed(), counter.boxed(), PlainLeaf.boxed()],
    };
    let parent_id = tree.mount_root(&row_v1, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let before = direct_children_in_slot_order(&tree, parent_id);
    assert_eq!(before.len(), 3, "row mounts all three children");
    let mid = before[1];

    // Rebuild WITHOUT the middle child — the armed dispose panic fires
    // during the reconcile's inline removal. Containment means
    // `build_scope` must RETURN, not unwind (asserted implicitly: this
    // test function keeps running past the call below).
    let row_v2 = Row {
        children: vec![PlainLeaf.boxed(), PlainLeaf.boxed()],
    };
    tree.update(parent_id, &row_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent_id, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);

    assert!(
        tree.get(mid).is_none(),
        "the panicking child's slab slot is freed inline during the reconcile that dropped it"
    );
    let after = direct_children_in_slot_order(&tree, parent_id);
    assert_eq!(
        after.len(),
        2,
        "the parent's surviving child list drops the removed middle child, got {after:?}"
    );

    assert_eq!(
        disposed.get(),
        1,
        "dispose ran exactly once despite panicking"
    );

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "exactly one contained panic must be recorded, got {recovered:?}"
    );
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Dispose);
    assert_eq!(
        panic.element, mid,
        "the recorded element is the panicking child"
    );
    assert_eq!(
        panic.parent, None,
        "the unmount-side seam does not see the parent"
    );
    assert_eq!(panic.view_type_id, TypeId::of::<DisposeCounter>());
    assert!(
        !panic.internal_invariant,
        "an ordinary panic message is not a BUG: internal invariant"
    );

    // A freed slab slot is never disposed a second time — dropping the
    // tree/owner must not touch it again.
    drop(tree);
    drop(owner);
    assert_eq!(
        disposed.get(),
        1,
        "dropping the tree/owner must not re-dispose a freed slot"
    );
}

// ============================================================================
// The finalize path: a GlobalKey'd element's dispose panic is contained
// inside `BuildOwner::finalize_tree`, and the key registry is still
// cleared.
// ============================================================================

#[test]
fn a_dispose_panic_on_a_global_keyed_element_still_clears_the_registry() {
    let key = GlobalKey::<DisposeCounterState>::new();
    let (counter, disposed, init_state_calls) = new_counter(Some(key.clone()), true);

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&counter, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    assert_eq!(
        init_state_calls.get(),
        1,
        "init_state ran once at the first build"
    );
    assert_eq!(
        owner.element_for_global_key(&key),
        Some(root_id),
        "the key resolves to the mounted element before removal"
    );

    // Soft-remove: a GlobalKey'd top node parks into the inactive queue
    // instead of finalizing immediately (`ElementTree::remove`), keeping
    // a same-frame retake window open.
    tree.remove(root_id, &mut owner.element_owner_mut());
    assert!(
        tree.get(root_id).is_some(),
        "soft removal keeps the slab slot alive for a same-frame retake"
    );

    // End-of-frame finalize drains the inactive queue: dispose runs,
    // panics, and is contained — `finalize_tree` must RETURN (asserted
    // implicitly: this test function keeps running past the call below).
    owner.finalize_tree(&mut tree);

    assert!(
        tree.get(root_id).is_none(),
        "finalize_tree frees the slab slot"
    );
    assert_eq!(
        owner.element_for_global_key(&key),
        None,
        "finalize_tree unregisters the GlobalKey even though dispose panicked"
    );
    assert_eq!(disposed.get(), 1);

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "got {recovered:?}");
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Dispose);
    assert_eq!(panic.element, root_id);
    assert_eq!(
        panic.parent, None,
        "the unmount-side seam does not see the parent"
    );
    assert_eq!(panic.view_type_id, TypeId::of::<DisposeCounter>());
    assert!(!panic.internal_invariant);

    drop(tree);
    drop(owner);
    assert_eq!(
        disposed.get(),
        1,
        "no re-dispose of a freed slot after the tree/owner drop"
    );
}

// ============================================================================
// The `initialized` gate: an element removed before its first `build_scope`
// drain never had `init_state` run, so `dispose` must not run for it
// either.
// ============================================================================

#[test]
fn a_state_whose_init_state_never_ran_is_not_disposed() {
    let (counter, disposed, init_state_calls) = new_counter(None, false);

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let parent_id = tree.mount_root(
        &Row {
            children: Vec::new(),
        },
        &mut owner.element_owner_mut(),
    );
    let child_id = tree.insert(&counter, parent_id, 0, &mut owner.element_owner_mut());

    // Removed before the first `build_scope` ever reaches it: no build
    // was ever scheduled/drained for `child_id`, so `init_state` never
    // ran.
    tree.remove(child_id, &mut owner.element_owner_mut());

    assert_eq!(init_state_calls.get(), 0, "init_state never ran");
    assert_eq!(
        disposed.get(),
        0,
        "dispose must not run for a state whose init_state never ran"
    );
    assert!(
        owner.take_recovered_panics().is_empty(),
        "nothing panicked, so nothing should be recorded"
    );
}
