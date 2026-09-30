use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::Leaf;
use flui_foundation::ViewKey;
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, RenderBox, Size};

use super::SparseChildren;
use crate::GlobalKey;
use crate::owner::RecoveredAt;
use crate::view::{RenderView, View};
use crate::{BuildOwner, ElementTree};

/// A minimal render-bearing leaf view used as both host and child in these
/// tests — mirrors the `SizedBoxView` in `view/render.rs` tests.
#[derive(Clone)]
struct LeafBox {
    side: f64,
}

impl RenderView for LeafBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(self.side), Some(self.side))
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_size(Some(self.side), Some(self.side))
    }
}

impl View for LeafBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// Like [`LeafBox`] but carries a [`GlobalKey`] so `tree.remove` soft-removes
/// it into the inactive queue instead of freeing the slab entry immediately.
/// Used to test the globally-keyed eviction → `finalize_tree` → slab-free path.
#[derive(Clone)]
struct GlobalKeyedLeafBox {
    side: f64,
    key: GlobalKey<Self>,
    detach_count: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct DetachCountingBox {
    side: f64,
    detach_count: Arc<AtomicUsize>,
}

impl flui_foundation::Diagnosticable for DetachCountingBox {}

impl RenderBox for DetachCountingBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(self.side, self.side)
    }

    fn detach(&mut self) {
        self.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl RenderView for GlobalKeyedLeafBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = DetachCountingBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        DetachCountingBox {
            side: self.side,
            detach_count: Arc::clone(&self.detach_count),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.side = self.side;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl View for GlobalKeyedLeafBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// Mount a render-bearing host root wired to a fresh `PipelineOwner`, and
/// return everything the tests drive `SparseChildren` against.
fn host_tree() -> (
    ElementTree,
    BuildOwner,
    PipelineCell,
    flui_foundation::ElementId,
) {
    let pipeline = PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    ));
    let mut build_owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let host = tree.mount_root_with_pipeline_owner(
        &LeafBox { side: 10.0 },
        Some(pipeline.clone()),
        &mut build_owner.element_owner_mut(),
    );
    (tree, build_owner, pipeline, host)
}

/// A GlobalKey moving between two lazy hosts preserves the element's
/// identity and updates its parent. Lazy hosts keep resident children in
/// `SparseChildren`, so relocation must not require membership in the
/// donor's dense `child_ids` list. `ensure` supplies the reconciliation guard
/// needed to move the child's render subtree.
fn a_global_key_moving_between_lazy_hosts_relocates_instead_of_panicking() {
    let (mut tree, mut build_owner, pipeline, host_a) = host_tree();
    let host_b = tree.insert(
        &LeafBox { side: 10.0 },
        host_a,
        1,
        &mut build_owner.element_owner_mut(),
    );

    let keyed_item = GlobalKeyedLeafBox {
        side: 4.0,
        key: GlobalKey::<GlobalKeyedLeafBox>::new(),
        detach_count: Arc::new(AtomicUsize::new(0)),
    };

    let mut list_a = SparseChildren::new();
    let first = list_a.ensure(
        0,
        &keyed_item,
        host_a,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    // The same key surfacing under a different lazy host — a keyed item
    // scrolled from one list into another.
    let mut list_b = SparseChildren::new();
    let moved = list_b.ensure(
        0,
        &keyed_item,
        host_b,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_eq!(
        moved, first,
        "the keyed child must relocate, preserving element identity, not mount a duplicate"
    );
    assert_eq!(
        tree.get(moved).and_then(crate::ElementNode::parent),
        Some(host_b),
        "the relocated child must be reparented onto the new host"
    );
}

/// A `GlobalKey`'d stateful item whose `ViewState::activate` panics once
/// armed — the retake half of a mount that runs *before* the retake's own
/// `update`, so this exercises the containment window's earlier half
/// (the `Retaken` report is written before `activate_subtree` runs, so
/// the undo always has a relocated element to act on).
#[derive(Clone)]
struct GlobalKeyedPanicsOnActivate {
    key: GlobalKey<Self>,
    armed: Arc<std::sync::atomic::AtomicBool>,
}

struct GlobalKeyedPanicsOnActivateState {
    armed: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::StatefulView for GlobalKeyedPanicsOnActivate {
    type State = GlobalKeyedPanicsOnActivateState;

    fn create_state(&self) -> Self::State {
        GlobalKeyedPanicsOnActivateState {
            armed: Arc::clone(&self.armed),
        }
    }
}

impl crate::ViewState<GlobalKeyedPanicsOnActivate> for GlobalKeyedPanicsOnActivateState {
    fn build(
        &self,
        _view: &GlobalKeyedPanicsOnActivate,
        _ctx: &dyn crate::BuildContext,
    ) -> impl crate::IntoView {
        LeafBox { side: 4.0 }
    }

    fn activate(&mut self) {
        assert!(
            !self.armed.load(std::sync::atomic::Ordering::SeqCst),
            "retake activate boom"
        );
    }
}

impl View for GlobalKeyedPanicsOnActivate {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// A panic in `activate_subtree` (not the retake's `update`) is undone
/// too. Before the `Retaken` write moved ahead of `activate_subtree`, it
/// ran AFTER it instead, so an `activate` panic left the relocated
/// element live and reparented but reported nowhere: the undo found
/// nothing armed and removed nothing, stranding it.
fn a_panicking_activate_removes_the_reactivated_element_instead_of_stranding_it() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let pre_mount_count = tree.len();
    let item = GlobalKeyedPanicsOnActivate {
        key: GlobalKey::<GlobalKeyedPanicsOnActivate>::new(),
        armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    let mut children = SparseChildren::new();
    let first = children.ensure(
        0,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    // Drive the first build so `init_state` actually runs before the
    // retake — `StatefulBehavior::on_activate` is
    // gated on a completed `init_state` (the guaranteed
    // `init_state` -> `activate` ordering), so an item that was only
    // mounted, never built, never runs its `activate` callback (and
    // this fixture's induced panic would never fire).
    build_owner.build_scope(&mut tree);

    // Evict: the `GlobalKey` makes this a soft removal, so the element
    // waits in the inactive queue for a retake instead of being freed.
    children.evict(0, &mut tree, &mut build_owner.element_owner_mut());
    assert!(
        tree.get(first).is_some(),
        "a globally-keyed eviction is a soft removal — the slab entry survives"
    );

    item.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    // Re-ensured at a DIFFERENT index within the same frame — the retake
    // still resolves by GlobalKey, not by index.
    let recovered = children.ensure(
        1,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_ne!(
        recovered, first,
        "the reactivated element was removed, not handed back broken"
    );
    assert!(
        tree.get(first).is_none(),
        "the half-activated element must not stay parented under the host"
    );
    assert!(
        build_owner.element_for_global_key(&item.key).is_none(),
        "the GlobalKey registration must not still resolve to the removed element"
    );
    assert_eq!(
        tree.get(recovered)
            .expect("a live element")
            .element()
            .view_type_id(),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the index carries the error view"
    );
    assert_eq!(
        tree.len(),
        pre_mount_count + 1,
        "only the substitute error view remains — the stranded original is gone"
    );

    let recovered_panics = build_owner.take_recovered_panics();
    assert_eq!(
        recovered_panics.len(),
        1,
        "exactly one panic must be recorded, not double-counted across the retake \
             and its substitute mount"
    );
    assert_eq!(recovered_panics[0].hook, crate::LifecycleHook::Activate);
    // `StatefulBehavior::on_activate` catches its OWN panic and records
    // it under its own accurate identity — `first`,
    // the retaken candidate itself, since `activate` panicked on the
    // candidate's own state, not a descendant's — then re-raises so
    // the retake window still observes the unwind. Without that
    // behavior-level catch the retake window one level up would be the
    // only thing to record it, and could only name the retake as a
    // `Substituted { element: Some(first), .. }` (a coarser identity
    // than the element whose hook actually panicked; see
    // `a_panicking_activate_records_the_actual_panicking_descendant_not_the_retake_root`
    // for the case where those two differ).
    assert!(matches!(
        recovered_panics[0].at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if element == first
    ));
}

#[test]
fn sparse_children_global_key_matrix() {
    crate::table_test::run_table(
        "sparse_children_global_key_matrix",
        &[
            (
                "a_global_key_moving_between_lazy_hosts_relocates_instead_of_panicking",
                a_global_key_moving_between_lazy_hosts_relocates_instead_of_panicking as fn(),
            ),
            (
                "a_panicking_activate_removes_the_reactivated_element_instead_of_stranding_it",
                a_panicking_activate_removes_the_reactivated_element_instead_of_stranding_it
                    as fn(),
            ),
        ],
    );
}
