//! Pins the containment seams around `ViewState::dispose`,
//! `ViewState::deactivate`, `RenderView::did_unmount_render_object`, and
//! `AnimatedView::listenable()`: a panic in any of the first
//! three is caught per element instead of unwinding out of
//! `BuildOwner::build_scope` / `BuildOwner::finalize_tree`; the fourth is
//! closed by caching instead — see below.
//!
//! Flutter's own boundaries for the caught hooks are coarser, not absent:
//! - `dispose` — `_InactiveElements._unmountAll()` carries no per-element
//!   `try`/`catch` of its own, but `BuildOwner.finalizeTree` wraps the
//!   whole drain in one `try`/`catch` that reports through
//!   `_reportException("while finalizing the widget tree")` and lets the
//!   frame continue. Flutter's failure mode is "the rest of `_elements` is
//!   leaked un-unmounted, the frame continues", not an unwound frame; FLUI's
//!   is per element.
//! - `deactivate` — Flutter DOES contain it:
//!   `_InactiveElements._deactivateRecursively` catches, runs
//!   `_deactivateFailedSubtreeRecursively` (the whole subtree is marked
//!   `_ElementLifecycle.failed`), and rethrows into
//!   `ComponentElement.performRebuild`'s second `catch`, which substitutes
//!   an `ErrorWidget` at the *rebuilding ancestor*. FLUI's improvement is a
//!   narrower blast radius: per element, the element stays parked inactive
//!   and is still disposed normally at `finalize_tree` — Flutter never
//!   disposes a `failed` subtree.
//! - `did_unmount_render_object` — `RenderObjectElement.unmount` runs
//!   `super.unmount()` (which detaches the render object) and its
//!   detach-related asserts BEFORE calling
//!   `widget.didUnmountRenderObject(renderObject)`, with no `try`/`catch`
//!   around the hook. A throwing hook there does NOT skip the detach — that
//!   already happened; the lifecycle work it skips is the subsequent
//!   `renderObject.dispose()`.
//!
//! FLUI bounds the panic to the one element whose hook threw and lets the
//! rest of the frame continue: see `StatefulBehavior::{on_unmount,
//! on_deactivate, on_activate}` and `RenderBehavior::on_unmount`
//! (`crates/flui-view/src/element/behavior.rs`) and the containment bullet
//! in `crates/flui-view/AGENTS.md`.
//!
//! `AnimatedView::listenable()` is different: it is the only handle to the
//! listenable an `AnimatedBehavior` subscribed to, so a `catch_unwind`
//! around a *second* call to it at unmount cannot make removal safe — if
//! that call panicked, there would be nothing left to remove the listener
//! from. `AnimatedBehavior` closes this one by caching the `Arc` it
//! subscribed to and removing through the cache, so `listenable()` is never
//! called again on the removal path at all.
//!
//! Also pins the `initialized` gate: `dispose` runs only for a state whose
//! `init_state` actually completed. FLUI's split mount/build (unlike
//! Flutter's synchronous `mount` -> `initState`) means an element can be
//! mounted and removed again before its first `build_scope` drain ever
//! reaches it; `create_state` must not itself acquire lifecycle resources
//! (mirrors Flutter's `createState` contract), so skipping `dispose` there
//! costs nothing a well-behaved state depends on.

use std::{any::TypeId, cell::Cell, rc::Rc, sync::Arc};

use flui_foundation::{ChangeNotifier, Listenable, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    AnimatedView, BoxedView, BuildContext, BuildContextExt, BuildOwner, ElementId, ElementNode,
    ElementTree, GlobalKey, InheritedView, IntoView, LifecycleHook, RebuildReason, RecoveredAt,
    RenderView, StatefulView, StatelessView, View, ViewExt, ViewState,
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
    assert!(
        matches!(
            panic.at,
            RecoveredAt::Element {
                element,
                parent: None,
                ..
            } if element == mid
        ),
        "the recorded element is the panicking child; the unmount-side seam does not see the \
         parent: {:?}",
        panic.at
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
    assert!(
        matches!(
            panic.at,
            RecoveredAt::Element {
                element,
                parent: None,
                ..
            } if element == root_id
        ),
        "the unmount-side seam does not see the parent: {:?}",
        panic.at
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

// ============================================================================
// Deactivate panic containment: a panic inside `ViewState::deactivate` is
// caught inside `StatefulBehavior::on_deactivate` instead of unwinding out
// of the reconcile that dropped the element, and `ElementCore::deactivate`
// (the lifecycle flip to `Inactive`) still runs right after.
// ============================================================================

/// A minimal `InheritedView` provider: descendants read `value` through
/// `BuildContextExt::depend_on` and are recorded as dependents.
#[derive(Clone)]
struct CountProvider {
    value: u32,
    child: Row,
}

impl InheritedView for CountProvider {
    type Data = u32;

    fn data(&self) -> &u32 {
        &self.value
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.value != old.value
    }
}

impl View for CountProvider {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::inherited(self)
    }
}

/// A `GlobalKey`'d Stateful view whose `deactivate` panics exactly once
/// (mirrors `DisposeCounter`'s one-shot `armed` shape), and whose `build`
/// depends on `CountProvider` so the provider's dependent map is exercised
/// end to end by the panic.
#[derive(Clone)]
struct DeactivateCounter {
    key: GlobalKey<DeactivateCounterState>,
    armed: Rc<Cell<bool>>,
    deactivated: Rc<Cell<u32>>,
    disposed: Rc<Cell<u32>>,
}

struct DeactivateCounterState {
    armed: Rc<Cell<bool>>,
    deactivated: Rc<Cell<u32>>,
    disposed: Rc<Cell<u32>>,
}

impl StatefulView for DeactivateCounter {
    type State = DeactivateCounterState;

    fn create_state(&self) -> Self::State {
        DeactivateCounterState {
            armed: self.armed.clone(),
            deactivated: self.deactivated.clone(),
            disposed: self.disposed.clone(),
        }
    }
}

impl ViewState<DeactivateCounter> for DeactivateCounterState {
    fn build(&self, _view: &DeactivateCounter, ctx: &dyn BuildContext) -> impl IntoView {
        // Register as a dependent of `CountProvider` — the panic below must
        // not leave a stale entry in the provider's dependent map once this
        // element is parked.
        let _ = ctx.depend_on::<CountProvider, ()>(|_| ());
        PlainLeaf.boxed()
    }

    fn deactivate(&mut self) {
        self.deactivated.set(self.deactivated.get() + 1);
        if self.armed.get() {
            self.armed.set(false);
            panic!("induced deactivate panic (lifecycle containment test)");
        }
    }

    fn dispose(&mut self) {
        self.disposed.set(self.disposed.get() + 1);
    }
}

impl View for DeactivateCounter {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// A build-counting Stateless leaf — proof that `build_scope` keeps
/// servicing OTHER dirty elements after containing a panic elsewhere in the
/// same pass.
#[derive(Clone)]
struct BuildCountingLeaf {
    build_calls: Rc<Cell<u32>>,
}

impl StatelessView for BuildCountingLeaf {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_calls.set(self.build_calls.get() + 1);
        PlainLeaf.boxed()
    }
}

impl View for BuildCountingLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[test]
fn a_deactivate_panic_is_contained_and_the_element_is_still_parked_inactive() {
    let key = GlobalKey::<DeactivateCounterState>::new();
    let armed = Rc::new(Cell::new(true));
    let deactivated = Rc::new(Cell::new(0u32));
    let disposed = Rc::new(Cell::new(0u32));
    let counter = DeactivateCounter {
        key: key.clone(),
        armed,
        deactivated: deactivated.clone(),
        disposed: disposed.clone(),
    };

    let sibling_build_calls = Rc::new(Cell::new(0u32));
    let sibling = BuildCountingLeaf {
        build_calls: sibling_build_calls.clone(),
    };

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();

    let provider_v1 = CountProvider {
        value: 1,
        child: Row {
            children: vec![counter.clone().boxed(), sibling.clone().boxed()],
        },
    };
    let provider_id = tree.mount_root(&provider_v1, &mut owner.element_owner_mut());
    owner.schedule_build_for(provider_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let deactivate_id = owner
        .element_for_global_key(&key)
        .expect("the GlobalKey'd counter is mounted after the initial build");
    assert!(
        tree.get(deactivate_id).is_some(),
        "the counter is live before removal"
    );

    let row_id = tree
        .get(deactivate_id)
        .and_then(ElementNode::parent)
        .expect("the counter is parented under the Row");
    let row_depth = tree.get(row_id).expect("row is live").depth();
    let sibling_id = direct_children_in_slot_order(&tree, row_id)
        .into_iter()
        .find(|&id| id != deactivate_id)
        .expect("the sibling leaf is the Row's other child");
    let sibling_depth = tree.get(sibling_id).expect("sibling is live").depth();

    // Rebuild the Row WITHOUT the GlobalKey'd counter — a keyed child that
    // disappears from its parent's declared children soft-parks (see
    // `ElementTree::remove`'s "Soft vs eager removal" doc) instead of
    // unmounting eagerly, which is what keeps the slab slot alive for the
    // `tree.get(deactivate_id).is_some()` check below. The armed
    // `deactivate` panic fires during that soft-park, inside
    // `deactivate_subtree`'s reconcile-triggered removal.
    let row_v2 = Row {
        children: vec![sibling.clone().boxed()],
    };
    tree.update(row_id, &row_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(row_id, row_depth, RebuildReason::ParentUpdate);
    // Scheduled independently of the Row's own reconcile, so servicing it
    // in the same `build_scope` pass is direct evidence the pass did not
    // abort when the counter's `deactivate` panicked.
    owner.schedule_build_for(sibling_id, sibling_depth, RebuildReason::StateChange);

    let sibling_build_calls_before = sibling_build_calls.get();

    // Containment means `build_scope` must RETURN, not unwind (asserted
    // implicitly: this test function keeps running past the call below).
    owner.build_scope(&mut tree);

    assert_eq!(
        sibling_build_calls.get(),
        sibling_build_calls_before + 1,
        "a sibling scheduled in the same build_scope still builds despite the contained \
         deactivate panic"
    );

    assert!(
        tree.get(deactivate_id).is_some(),
        "soft removal parks the counter's slab slot alive for a same-frame retake, even though \
         its deactivate panicked"
    );
    assert_eq!(
        tree.get(deactivate_id)
            .expect("the soft-parked element remains addressable")
            .element()
            .lifecycle(),
        flui_view::element::Lifecycle::Inactive,
        "the behavior-level catch must return so ElementCore performs the Active -> Inactive flip"
    );
    assert_eq!(
        deactivated.get(),
        1,
        "deactivate ran exactly once despite panicking"
    );

    // The provider no longer lists the parked element as a dependent:
    // `deactivate_subtree` releases inherited edges before calling
    // `ElementBase::deactivate`, so a provider update after the panic must
    // not reschedule it.
    let provider_v2 = CountProvider {
        value: 2,
        child: Row {
            children: vec![sibling.clone().boxed()],
        },
    };
    tree.update(provider_id, &provider_v2, &mut owner.element_owner_mut());
    assert_eq!(
        owner.dirty_count(),
        0,
        "a provider update after the panic must not reschedule the parked element"
    );
    assert!(
        !owner
            .element_owner_mut()
            .has_pending_dependency_change(deactivate_id),
        "the provider no longer lists the parked element as a dependent"
    );

    // Reintroduce the same key before finalization: this is the same-frame
    // retake the lifecycle flip exists to permit. Identity must survive even
    // though the preceding deactivate hook panicked.
    let row_v3 = Row {
        children: vec![counter.boxed(), sibling.boxed()],
    };
    tree.update(row_id, &row_v3, &mut owner.element_owner_mut());
    owner.schedule_build_for(row_id, row_depth, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    assert_eq!(
        owner.element_for_global_key(&key),
        Some(deactivate_id),
        "a same-frame retake preserves the original element identity"
    );
    assert_eq!(
        tree.get(deactivate_id)
            .expect("the retaken element is live")
            .element()
            .lifecycle(),
        flui_view::element::Lifecycle::Active,
        "the same-frame retake reactivates the parked element"
    );

    // Drop it again so end-of-frame finalization proves dispose still runs.
    tree.update(
        row_id,
        &Row {
            children: vec![PlainLeaf.boxed()],
        },
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(row_id, row_depth, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);

    // End-of-frame finalize drains the inactive queue: `dispose` runs
    // cleanly (it is not armed to panic) because `initialized` is true.
    owner.finalize_tree(&mut tree);

    assert!(
        tree.get(deactivate_id).is_none(),
        "finalize_tree frees the parked slot"
    );
    assert_eq!(disposed.get(), 1, "dispose ran once at finalize");

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "exactly one contained panic must be recorded, got {recovered:?}"
    );
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Deactivate);
    assert!(
        matches!(
            panic.at,
            RecoveredAt::Element {
                element,
                parent: None,
                ..
            } if element == deactivate_id
        ),
        "the recorded element is the panicking counter; the unmount-side seam does not see the \
         parent: {:?}",
        panic.at
    );
    assert_eq!(panic.view_type_id, TypeId::of::<DeactivateCounter>());
    assert!(
        !panic.internal_invariant,
        "an ordinary panic message is not a BUG: internal invariant"
    );
}

// ============================================================================
// `did_unmount_render_object` panic containment: a panic in the render-
// object unmount hook is caught inside `RenderBehavior::on_unmount`'s
// `with_mut` closure instead of unwinding out of `build_scope`, and the
// render node is still detached from the render tree either way.
// ============================================================================

/// A render-backed leaf whose `did_unmount_render_object` panics exactly
/// once (mirrors `DisposeCounter`'s one-shot `armed` shape).
#[derive(Clone)]
struct UnmountRenderObjectPanicLeaf {
    armed: Rc<Cell<bool>>,
    unmount_calls: Rc<Cell<u32>>,
}

impl RenderView for UnmountRenderObjectPanicLeaf {
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

    fn did_unmount_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        self.unmount_calls.set(self.unmount_calls.get() + 1);
        if self.armed.get() {
            self.armed.set(false);
            panic!("induced did_unmount_render_object panic (lifecycle containment test)");
        }
    }
}

impl View for UnmountRenderObjectPanicLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[test]
fn a_did_unmount_render_object_panic_is_contained_and_the_render_node_is_still_removed() {
    // A `PipelineOwner` must be in scope for `RenderBehavior::on_mount` to
    // mint a render object at all — without one, `did_unmount_render_object`
    // never runs and this test would pass for the wrong reason.
    let pipeline = PipelineCell::new(PipelineOwner::new());

    let armed = Rc::new(Cell::new(true));
    let unmount_calls = Rc::new(Cell::new(0u32));
    let panicking_child = UnmountRenderObjectPanicLeaf {
        armed: armed.clone(),
        unmount_calls: unmount_calls.clone(),
    };

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();

    let row_v1 = Row {
        children: vec![
            PlainLeaf.boxed(),
            panicking_child.boxed(),
            PlainLeaf.boxed(),
        ],
    };
    let parent_id = tree.mount_root_with_pipeline_owner(
        &row_v1,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let before = direct_children_in_slot_order(&tree, parent_id);
    assert_eq!(before.len(), 3, "row mounts all three children");
    let mid = before[1];
    let render_nodes_before = pipeline.with(|po| po.render_tree().len());

    // Rebuild WITHOUT the middle child — the armed
    // `did_unmount_render_object` panic fires during the reconcile's inline
    // removal. Containment means `build_scope` must RETURN, not unwind
    // (asserted implicitly: this test function keeps running past the call
    // below).
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

    let render_nodes_after = pipeline.with(|po| po.render_tree().len());
    assert_eq!(
        render_nodes_before - render_nodes_after,
        1,
        "the render tree loses exactly the removed child's one node despite the panic \
         (before={render_nodes_before}, after={render_nodes_after})"
    );

    assert_eq!(
        unmount_calls.get(),
        1,
        "did_unmount_render_object ran exactly once despite panicking"
    );

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "exactly one contained panic must be recorded, got {recovered:?}"
    );
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::UnmountRenderObject);
    assert!(
        matches!(
            panic.at,
            RecoveredAt::Element {
                element,
                parent: None,
                ..
            } if element == mid
        ),
        "the recorded element is the panicking child; the unmount-side seam does not see the \
         parent: {:?}",
        panic.at
    );
    assert_eq!(
        panic.view_type_id,
        TypeId::of::<UnmountRenderObjectPanicLeaf>()
    );
    assert!(
        !panic.internal_invariant,
        "an ordinary panic message is not a BUG: internal invariant"
    );
}

// ============================================================================
// `listenable()` closed by caching: `AnimatedBehavior` caches the
// `Arc<dyn Listenable>` it subscribed to and unsubscribes through that
// cache — it never calls `listenable()` again at unmount, so a
// `listenable()` that panics on every call after mount cannot make removal
// panic too.
// ============================================================================

/// An `AnimatedView` whose `listenable()` panics on every call from the
/// moment the test arms it (right after mount) onward. Unlike
/// `UnmountRenderObjectPanicLeaf`'s one-shot `armed`, this never disarms —
/// ANY later call is fatal, so the only way this test can pass is if the
/// removal path never calls `listenable()` at all.
#[derive(Clone)]
struct ListenablePanicAfterMount {
    notifier: Arc<ChangeNotifier>,
    armed: Rc<Cell<bool>>,
    disposed: Rc<Cell<u32>>,
}

struct ListenablePanicAfterMountState {
    disposed: Rc<Cell<u32>>,
}

impl StatefulView for ListenablePanicAfterMount {
    type State = ListenablePanicAfterMountState;

    fn create_state(&self) -> Self::State {
        ListenablePanicAfterMountState {
            disposed: self.disposed.clone(),
        }
    }
}

impl ViewState<ListenablePanicAfterMount> for ListenablePanicAfterMountState {
    fn build(&self, _view: &ListenablePanicAfterMount, _ctx: &dyn BuildContext) -> impl IntoView {
        PlainLeaf.boxed()
    }

    fn dispose(&mut self) {
        self.disposed.set(self.disposed.get() + 1);
    }
}

impl AnimatedView for ListenablePanicAfterMount {
    fn listenable(&self) -> Arc<dyn Listenable> {
        assert!(
            !self.armed.get(),
            "induced listenable panic (lifecycle containment test)"
        );
        self.notifier.clone()
    }
}

impl View for ListenablePanicAfterMount {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::animated(self)
    }
}

#[test]
fn an_animated_view_whose_listenable_panics_after_mount_is_still_unsubscribed() {
    let notifier = Arc::new(ChangeNotifier::new());
    let armed = Rc::new(Cell::new(false));
    let disposed = Rc::new(Cell::new(0u32));
    let animated_child = ListenablePanicAfterMount {
        notifier: notifier.clone(),
        armed: armed.clone(),
        disposed: disposed.clone(),
    };

    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();

    let row_v1 = Row {
        children: vec![PlainLeaf.boxed(), animated_child.boxed(), PlainLeaf.boxed()],
    };
    let parent_id = tree.mount_root(&row_v1, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    assert_eq!(
        notifier.len(),
        1,
        "mount subscribed exactly one listener to the shared notifier"
    );

    // Arm right after mount: every `listenable()` call from here on panics.
    armed.set(true);

    let before = direct_children_in_slot_order(&tree, parent_id);
    let mid = before[1];

    // Rebuild WITHOUT the animated child. No `catch_unwind` here on
    // purpose: `listenable()` is armed, so if the removal path called it
    // again this call would panic and fail the test outright — that is
    // exactly the claim this test pins (`listenable()` is no longer on the
    // removal path at all).
    let row_v2 = Row {
        children: vec![PlainLeaf.boxed(), PlainLeaf.boxed()],
    };
    tree.update(parent_id, &row_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent_id, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);

    assert!(
        tree.get(mid).is_none(),
        "the panicking-listenable child's slab slot is freed inline during the reconcile that \
         dropped it"
    );
    assert_eq!(
        notifier.len(),
        0,
        "unmount removed the subscription through the cached Arc, without ever calling the \
         now-armed listenable() again"
    );
    assert_eq!(disposed.get(), 1, "dispose still ran for the removed state");

    assert!(
        owner.take_recovered_panics().is_empty(),
        "nothing panicked — listenable() is no longer on the removal path, so there is nothing \
         to recover"
    );
}
