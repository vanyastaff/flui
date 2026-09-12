//! Dense reconciliation update containment across the three production update phases.

use std::{any::TypeId, cell::Cell, collections::HashSet, rc::Rc};

use flui_foundation::{ElementId, RebuildReason, RebuildReasons, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::{RenderUpdateImpact, protocol::BoxProtocol};
use flui_types::{Size, geometry::px};
use flui_view::{
    BoxedView, BuildContext, BuildOwner, ElementTree, ErrorView, GlobalKey, IntoView,
    LifecycleHook, RecoveredAt, RenderView, StatefulView, View, ViewExt, ViewState,
};

use crate::dense_reconcile_containment::{
    DENSE_CHILD_COUNT, DenseGlobalKeyUpdatePanicSubtree, DenseHealthyLeaf, DenseObservation,
    DenseObserver, DenseRow, DenseSnapshot, assert_dense_snapshot_with_count,
    first_render_descendant, mount_dense_root, run_real_pipeline_frame,
};

pub(super) const PHASE_ONE_FAILED_SLOT: usize = 3;

#[derive(Clone)]
pub(super) struct DenseRenderUpdateLeaf {
    key: ValueKey<u32>,
    marker: usize,
    armed: Rc<Cell<bool>>,
    should_panic: bool,
}

impl DenseRenderUpdateLeaf {
    pub(super) fn healthy(key: u32, marker: usize, armed: Rc<Cell<bool>>) -> Self {
        Self {
            key: ValueKey::new(key),
            marker,
            armed,
            should_panic: false,
        }
    }

    pub(super) fn panicking(key: u32, marker: usize, armed: Rc<Cell<bool>>) -> Self {
        Self {
            key: ValueKey::new(key),
            marker,
            armed,
            should_panic: true,
        }
    }

    fn size(&self) -> Size {
        Size::new(px(10.0 + self.marker as f32), px(14.0))
    }
}

impl RenderView for DenseRenderUpdateLeaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let size = self.size();
        RenderSizedBox::new(Some(size.width), Some(size.height))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        assert!(
            !(self.should_panic && self.armed.get()),
            "dense update_render_object panic"
        );
        let size = self.size();
        render_object.set_size(Some(size.width), Some(size.height))
    }
}

impl View for DenseRenderUpdateLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
pub(super) struct DenseDidUpdateView {
    key: ValueKey<u32>,
    armed: Rc<Cell<bool>>,
}

pub(super) struct DenseDidUpdateState;

impl DenseDidUpdateView {
    pub(super) fn new(key: u32, armed: Rc<Cell<bool>>) -> Self {
        Self {
            key: ValueKey::new(key),
            armed,
        }
    }
}

impl StatefulView for DenseDidUpdateView {
    type State = DenseDidUpdateState;

    fn create_state(&self) -> Self::State {
        DenseDidUpdateState
    }
}

impl ViewState<DenseDidUpdateView> for DenseDidUpdateState {
    fn build(&self, _view: &DenseDidUpdateView, _ctx: &dyn BuildContext) -> impl IntoView {
        DenseHealthyLeaf { marker: 0 }
    }

    fn did_update_view(&mut self, _old_view: &DenseDidUpdateView, new_view: &DenseDidUpdateView) {
        assert!(!new_view.armed.get(), "dense did_update_view panic");
    }
}

impl View for DenseDidUpdateView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

pub(super) fn keyed_render_children(
    keys: &[u32],
    panicking_key: Option<u32>,
    armed: Rc<Cell<bool>>,
) -> Vec<BoxedView> {
    keys.iter()
        .copied()
        .enumerate()
        .map(|(marker, key)| {
            if panicking_key == Some(key) {
                DenseRenderUpdateLeaf::panicking(key, marker, armed.clone()).boxed()
            } else {
                DenseRenderUpdateLeaf::healthy(key, marker, armed.clone()).boxed()
            }
        })
        .collect()
}

pub(super) fn phase_one_children(failed_slot: usize, child: BoxedView) -> Vec<BoxedView> {
    assert!(failed_slot < DENSE_CHILD_COUNT, "failed slot must be dense");
    let mut failed_child = Some(child);
    (0..DENSE_CHILD_COUNT)
        .map(|marker| {
            if marker == failed_slot {
                failed_child
                    .take()
                    .expect("the unique failed slot consumes the child exactly once")
            } else {
                DenseHealthyLeaf { marker }.boxed()
            }
        })
        .collect()
}

pub(super) fn update_dense_parent(
    tree: &mut ElementTree,
    owner: &mut BuildOwner,
    parent: ElementId,
    children: Vec<BoxedView>,
) {
    let depth = tree.get(parent).expect("dense parent").depth();
    tree.update(
        parent,
        &DenseRow { children },
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent, depth, RebuildReason::ParentUpdate);
    owner.build_scope(tree);
}

fn assert_finite_nonzero_layout(
    tree: &ElementTree,
    pipeline: &flui_rendering::pipeline::PipelineCell,
    snapshot: &DenseSnapshot,
) {
    for child in &snapshot.child_ids {
        let render_id = first_render_descendant(tree, *child)
            .expect("every committed dense child must expose a render frontier");
        let geometry = pipeline.with(|owner| {
            owner
                .render_tree()
                .get(render_id)
                .expect("the committed render id must resolve")
                .geometry_box()
                .expect("the committed render child must have box geometry")
        });
        assert!(geometry.is_finite(), "layout geometry must be finite");
        assert!(!geometry.is_zero_area(), "layout geometry must be nonzero");
    }
}

struct UpdateRecoveryExpectation {
    resident: ElementId,
    substitute: ElementId,
    parent: ElementId,
    slot: usize,
    failed_view_type_id: TypeId,
}

fn assert_update_recovery(
    owner: &mut BuildOwner,
    observer: &DenseObserver,
    observation_start: usize,
    expected: UpdateRecoveryExpectation,
) {
    let UpdateRecoveryExpectation {
        resident,
        substitute,
        parent,
        slot,
        failed_view_type_id,
    } = expected;
    let mut recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "one update hook panic records once");
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Update);
    assert_eq!(panic.view_type_id, failed_view_type_id);
    assert!(matches!(
        panic.at,
        RecoveredAt::Substituted {
            element: Some(recorded_resident),
            substitute: recorded_substitute,
            parent: recorded_parent,
            slot: recorded_slot,
            ..
        } if recorded_resident == resident
            && recorded_substitute == substitute
            && recorded_parent == parent
            && recorded_slot == slot
    ));
    assert!(
        owner.take_recovered_panics().is_empty(),
        "the recovery drain must consume the update record"
    );

    let new_events = &observer.events()[observation_start..];
    let resident_events: Vec<_> = new_events
        .iter()
        .filter(|event| match event {
            DenseObservation::Unmount { element }
            | DenseObservation::Move { element, .. }
            | DenseObservation::Rebuilt { element, .. } => *element == resident,
            DenseObservation::Mount { .. } => false,
        })
        .copied()
        .collect();
    assert_eq!(
        resident_events,
        vec![DenseObservation::Unmount { element: resident }],
        "the failed resident must unmount once without a stale move or rebuild"
    );

    let substitute_events: Vec<_> = new_events
        .iter()
        .filter(|event| match event {
            DenseObservation::Mount { element, .. }
            | DenseObservation::Move { element, .. }
            | DenseObservation::Rebuilt { element, .. }
            | DenseObservation::Unmount { element } => *element == substitute,
        })
        .copied()
        .collect();
    assert_eq!(
        substitute_events,
        vec![
            DenseObservation::Mount {
                element: substitute,
                parent: Some(parent),
                slot,
                view_type_id: TypeId::of::<ErrorView>(),
            },
            DenseObservation::Rebuilt {
                element: substitute,
                view_type_id: TypeId::of::<ErrorView>(),
                reasons: RebuildReasons::from_reason(RebuildReason::InitialMount),
            },
        ],
        "the substitute must announce one final-slot Mount and one InitialMount rebuild"
    );
}

fn assert_sibling_ids(snapshot: &DenseSnapshot, expected_by_slot: &[(usize, ElementId)]) {
    for &(slot, expected) in expected_by_slot {
        assert_eq!(
            snapshot.child_ids[slot], expected,
            "healthy sibling at slot {slot} must preserve its generational identity"
        );
    }
}

#[test]
fn phase_one_did_update_view_panic_substitutes_at_same_slot() {
    let armed = Rc::new(Cell::new(false));
    let (mut tree, mut owner, pipeline, observer, parent) = mount_dense_root(DenseRow {
        children: phase_one_children(
            PHASE_ONE_FAILED_SLOT,
            DenseDidUpdateView::new(1, armed.clone()).boxed(),
        ),
    });
    owner.build_scope(&mut tree);
    let before =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let resident = before.child_ids[PHASE_ONE_FAILED_SLOT];
    let observation_start = observer.events().len();
    armed.set(true);

    update_dense_parent(
        &mut tree,
        &mut owner,
        parent,
        phase_one_children(
            PHASE_ONE_FAILED_SLOT,
            DenseDidUpdateView::new(1, armed).boxed(),
        ),
    );

    let after =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let substitute = after.child_ids[PHASE_ONE_FAILED_SLOT];
    assert!(
        tree.get(resident).is_none(),
        "the failed resident must be freed"
    );
    assert_eq!(
        tree.get(substitute)
            .expect("same-slot substitute")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>()
    );
    assert_sibling_ids(
        &after,
        &(0..DENSE_CHILD_COUNT)
            .filter(|slot| *slot != PHASE_ONE_FAILED_SLOT)
            .map(|slot| (slot, before.child_ids[slot]))
            .collect::<Vec<_>>(),
    );
    run_real_pipeline_frame(&tree, &pipeline, parent);
    assert_finite_nonzero_layout(&tree, &pipeline, &after);
    assert_update_recovery(
        &mut owner,
        &observer,
        observation_start,
        UpdateRecoveryExpectation {
            resident,
            substitute,
            parent,
            slot: PHASE_ONE_FAILED_SLOT,
            failed_view_type_id: TypeId::of::<DenseDidUpdateView>(),
        },
    );
}

#[test]
fn phase_one_update_render_object_panic_repeats_without_ghosts() {
    let armed = Rc::new(Cell::new(false));
    let initial = phase_one_children(
        PHASE_ONE_FAILED_SLOT,
        DenseRenderUpdateLeaf::healthy(1, 0, armed.clone()).boxed(),
    );
    let (mut tree, mut owner, pipeline, observer, parent) =
        mount_dense_root(DenseRow { children: initial });
    owner.build_scope(&mut tree);
    let baseline =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let stable_live_count = baseline.live_ids.len();
    let stable_siblings: Vec<_> = (0..DENSE_CHILD_COUNT)
        .filter(|slot| *slot != PHASE_ONE_FAILED_SLOT)
        .map(|slot| (slot, baseline.child_ids[slot]))
        .collect();
    let mut resident = baseline.child_ids[PHASE_ONE_FAILED_SLOT];

    for attempt in 0..2 {
        let observation_start = observer.events().len();
        armed.set(true);
        update_dense_parent(
            &mut tree,
            &mut owner,
            parent,
            phase_one_children(
                PHASE_ONE_FAILED_SLOT,
                DenseRenderUpdateLeaf::panicking(1, attempt + 1, armed.clone()).boxed(),
            ),
        );
        let failed = assert_dense_snapshot_with_count(
            &tree,
            &pipeline,
            parent,
            &observer,
            DENSE_CHILD_COUNT,
        );
        let substitute = failed.child_ids[PHASE_ONE_FAILED_SLOT];
        assert_eq!(failed.live_ids.len(), stable_live_count);
        assert_sibling_ids(&failed, &stable_siblings);
        run_real_pipeline_frame(&tree, &pipeline, parent);
        assert_finite_nonzero_layout(&tree, &pipeline, &failed);
        assert_update_recovery(
            &mut owner,
            &observer,
            observation_start,
            UpdateRecoveryExpectation {
                resident,
                substitute,
                parent,
                slot: PHASE_ONE_FAILED_SLOT,
                failed_view_type_id: TypeId::of::<DenseRenderUpdateLeaf>(),
            },
        );

        armed.set(false);
        update_dense_parent(
            &mut tree,
            &mut owner,
            parent,
            phase_one_children(
                PHASE_ONE_FAILED_SLOT,
                DenseRenderUpdateLeaf::healthy(1, attempt + 1, armed.clone()).boxed(),
            ),
        );
        let remounted = assert_dense_snapshot_with_count(
            &tree,
            &pipeline,
            parent,
            &observer,
            DENSE_CHILD_COUNT,
        );
        assert_eq!(remounted.live_ids.len(), stable_live_count);
        assert_sibling_ids(&remounted, &stable_siblings);
        assert!(tree.get(substitute).is_none());
        resident = remounted.child_ids[PHASE_ONE_FAILED_SLOT];
        assert_eq!(
            tree.get(resident)
                .expect("healthy remount")
                .element()
                .view_type_id(),
            TypeId::of::<DenseRenderUpdateLeaf>()
        );
    }
}

#[test]
fn phase_one_global_key_update_panic_releases_resident_and_reservation() {
    let key = GlobalKey::<DenseGlobalKeyUpdatePanicSubtree>::new();
    let armed = Rc::new(Cell::new(false));
    let keyed = DenseGlobalKeyUpdatePanicSubtree::new(key.clone(), armed.clone());
    let (mut tree, mut owner, pipeline, observer, parent) = mount_dense_root(DenseRow {
        children: phase_one_children(PHASE_ONE_FAILED_SLOT, keyed.clone().boxed()),
    });
    owner.build_scope(&mut tree);
    let before =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let resident = before.child_ids[PHASE_ONE_FAILED_SLOT];
    let resident_render = tree
        .get(resident)
        .expect("the keyed resident must resolve before its update")
        .element()
        .render_id()
        .expect("the keyed render resident must own a render object");
    assert_eq!(owner.element_for_global_key(&key), Some(resident));
    let stable_siblings: Vec<_> = (0..DENSE_CHILD_COUNT)
        .filter(|slot| *slot != PHASE_ONE_FAILED_SLOT)
        .map(|slot| (slot, before.child_ids[slot]))
        .collect();
    let observation_start = observer.events().len();
    armed.set(true);

    update_dense_parent(
        &mut tree,
        &mut owner,
        parent,
        phase_one_children(PHASE_ONE_FAILED_SLOT, keyed.clone().boxed()),
    );

    let failed =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let substitute = failed.child_ids[PHASE_ONE_FAILED_SLOT];
    assert!(
        tree.get(resident).is_none(),
        "the old keyed id must be freed"
    );
    pipeline.with(|pipeline_owner| {
        assert!(
            pipeline_owner.render_tree().get(resident_render).is_none(),
            "the old keyed render id must be freed"
        );
    });
    assert_eq!(owner.element_for_global_key(&key), None);
    assert_sibling_ids(&failed, &stable_siblings);
    run_real_pipeline_frame(&tree, &pipeline, parent);
    assert_finite_nonzero_layout(&tree, &pipeline, &failed);
    assert_update_recovery(
        &mut owner,
        &observer,
        observation_start,
        UpdateRecoveryExpectation {
            resident,
            substitute,
            parent,
            slot: PHASE_ONE_FAILED_SLOT,
            failed_view_type_id: TypeId::of::<DenseGlobalKeyUpdatePanicSubtree>(),
        },
    );
    owner.finalize_tree(&mut tree);
    assert!(
        owner.take_global_key_diagnostics().is_empty(),
        "the discarded resident must leave neither a reservation nor a duplicate diagnostic"
    );

    armed.set(false);
    update_dense_parent(
        &mut tree,
        &mut owner,
        parent,
        phase_one_children(PHASE_ONE_FAILED_SLOT, keyed.boxed()),
    );
    let recovered =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let remounted = recovered.child_ids[PHASE_ONE_FAILED_SLOT];
    assert_ne!(remounted, resident, "the freed generation cannot be reused");
    assert!(
        tree.get(substitute).is_none(),
        "the substitute must unmount"
    );
    assert_eq!(owner.element_for_global_key(&key), Some(remounted));
    assert_sibling_ids(&recovered, &stable_siblings);
    run_real_pipeline_frame(&tree, &pipeline, parent);
    assert_finite_nonzero_layout(&tree, &pipeline, &recovered);
    owner.finalize_tree(&mut tree);
    assert!(owner.take_global_key_diagnostics().is_empty());
}

#[test]
fn phase_four_keyed_middle_move_update_panic_uses_final_slot() {
    let armed = Rc::new(Cell::new(false));
    let old_keys: Vec<_> = (0..DENSE_CHILD_COUNT as u32).collect();
    let (mut tree, mut owner, pipeline, observer, parent) = mount_dense_root(DenseRow {
        children: keyed_render_children(&old_keys, None, armed.clone()),
    });
    owner.build_scope(&mut tree);
    let before =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let resident = before.child_ids[3];
    let observation_start = observer.events().len();
    armed.set(true);
    let new_keys = [100, 3, 0, 1, 2, 4, 5, 6, 101, 9];

    update_dense_parent(
        &mut tree,
        &mut owner,
        parent,
        keyed_render_children(&new_keys, Some(3), armed),
    );

    let after =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let substitute = after.child_ids[1];
    assert_sibling_ids(
        &after,
        &[
            (2, before.child_ids[0]),
            (3, before.child_ids[1]),
            (4, before.child_ids[2]),
            (5, before.child_ids[4]),
            (6, before.child_ids[5]),
            (7, before.child_ids[6]),
            (9, before.child_ids[9]),
        ],
    );
    assert!(tree.get(resident).is_none());
    run_real_pipeline_frame(&tree, &pipeline, parent);
    assert_finite_nonzero_layout(&tree, &pipeline, &after);
    assert_update_recovery(
        &mut owner,
        &observer,
        observation_start,
        UpdateRecoveryExpectation {
            resident,
            substitute,
            parent,
            slot: 1,
            failed_view_type_id: TypeId::of::<DenseRenderUpdateLeaf>(),
        },
    );
}

#[test]
fn phase_five_a_shifted_suffix_update_panic_uses_final_slot() {
    let armed = Rc::new(Cell::new(false));
    let old_keys: Vec<_> = (0..DENSE_CHILD_COUNT as u32).collect();
    let (mut tree, mut owner, pipeline, observer, parent) = mount_dense_root(DenseRow {
        children: keyed_render_children(&old_keys, None, armed.clone()),
    });
    owner.build_scope(&mut tree);
    let before =
        assert_dense_snapshot_with_count(&tree, &pipeline, parent, &observer, DENSE_CHILD_COUNT);
    let resident = before.child_ids[9];
    let observation_start = observer.events().len();
    armed.set(true);
    let new_keys = [0, 1, 2, 3, 4, 5, 6, 7, 8, 100, 9];

    update_dense_parent(
        &mut tree,
        &mut owner,
        parent,
        keyed_render_children(&new_keys, Some(9), armed),
    );

    let after = assert_dense_snapshot_with_count(
        &tree,
        &pipeline,
        parent,
        &observer,
        DENSE_CHILD_COUNT + 1,
    );
    let substitute = after.child_ids[10];
    assert_sibling_ids(
        &after,
        &(0..9)
            .map(|slot| (slot, before.child_ids[slot]))
            .collect::<Vec<_>>(),
    );
    assert!(tree.get(resident).is_none());
    run_real_pipeline_frame(&tree, &pipeline, parent);
    assert_finite_nonzero_layout(&tree, &pipeline, &after);
    assert_update_recovery(
        &mut owner,
        &observer,
        observation_start,
        UpdateRecoveryExpectation {
            resident,
            substitute,
            parent,
            slot: 10,
            failed_view_type_id: TypeId::of::<DenseRenderUpdateLeaf>(),
        },
    );

    let live_ids: HashSet<_> = tree.iter_nodes().map(|(id, _)| id).collect();
    assert_eq!(
        after.live_ids, live_ids,
        "no update ghost may escape the slab"
    );
}
