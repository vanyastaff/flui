//! Full-frame containment for panics raised while reconciling a dense row.
//!
//! Both cases start from a healthy, painted tree and fail only after a root
//! swap, so the assertions exercise `HeadlessBinding::pump_frame` rather than
//! the bootstrap path.

use std::{any::TypeId, cell::Cell, rc::Rc, time::Duration};

use flui_foundation::{ElementId, RenderId};
use flui_objects::{RenderErrorBox, RenderFlex, RenderSizedBox};
use flui_rendering::{
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
    testing::inspect,
};
use flui_testing::{
    HeadlessBinding,
    bootstrap::{MountOptions, MountOwners},
};
use flui_types::{Offset, Size, geometry::px};
use flui_view::{
    BoxedView, BuildContext, ErrorView, IntoView, LifecycleHook, RecoveredAt, RenderView,
    StatefulView, View, ViewExt, ViewState,
};

const CHILD_COUNT: usize = 10;
const FAILING_SLOT: usize = 7;

#[derive(Clone)]
struct DenseRow {
    children: Vec<BoxedView>,
}

impl RenderView for DenseRow {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderFlex::row()
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

impl View for DenseRow {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
struct HealthyLeaf {
    slot: usize,
}

impl HealthyLeaf {
    fn size(&self) -> Size {
        if self.slot == FAILING_SLOT {
            // `RenderErrorBox` receives an unbounded horizontal constraint and
            // the row's 80px vertical bound, so this keeps every later sibling's
            // offset stable across the replacement as well as its own size.
            Size::new(px(48.0), px(80.0))
        } else {
            Size::new(px(10.0 + self.slot as f32), px(20.0 + self.slot as f32))
        }
    }
}

impl RenderView for HealthyLeaf {
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
    ) -> flui_rendering::RenderUpdateImpact {
        let size = self.size();
        render_object.set_size(Some(size.width), Some(size.height))
    }
}

impl View for HealthyLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
struct PanicsInInitState {
    failed_id: Rc<Cell<Option<ElementId>>>,
    init_calls: Rc<Cell<u32>>,
    dispose_calls: Rc<Cell<u32>>,
}

struct PanickingState {
    failed_id: Rc<Cell<Option<ElementId>>>,
    init_calls: Rc<Cell<u32>>,
    dispose_calls: Rc<Cell<u32>>,
}

impl StatefulView for PanicsInInitState {
    type State = PanickingState;

    fn create_state(&self) -> Self::State {
        PanickingState {
            failed_id: Rc::clone(&self.failed_id),
            init_calls: Rc::clone(&self.init_calls),
            dispose_calls: Rc::clone(&self.dispose_calls),
        }
    }
}

impl ViewState<PanicsInInitState> for PanickingState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.failed_id.set(Some(ctx.element_id()));
        self.init_calls.set(self.init_calls.get() + 1);
        panic!("induced headless init_state panic");
    }

    fn build(&self, _view: &PanicsInInitState, _ctx: &dyn BuildContext) -> impl IntoView {
        HealthyLeaf { slot: FAILING_SLOT }
    }

    fn dispose(&mut self) {
        self.dispose_calls.set(self.dispose_calls.get() + 1);
    }
}

impl View for PanicsInInitState {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

#[derive(Clone)]
struct PanicsOnCreateRenderObject;

impl RenderView for PanicsOnCreateRenderObject {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        panic!("induced headless create_render_object panic");
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for PanicsOnCreateRenderObject {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Debug)]
struct DenseSnapshot {
    child_ids: Vec<ElementId>,
    render_ids: Vec<RenderId>,
    geometries: Vec<(Size, Offset)>,
}

fn healthy_children() -> Vec<BoxedView> {
    (0..CHILD_COUNT)
        .map(|slot| HealthyLeaf { slot }.boxed())
        .collect()
}

fn children_with_failure(failure: BoxedView) -> Vec<BoxedView> {
    (0..CHILD_COUNT)
        .map(|slot| {
            if slot == FAILING_SLOT {
                failure.clone()
            } else {
                HealthyLeaf { slot }.boxed()
            }
        })
        .collect()
}

fn snapshot(
    binding: &mut HeadlessBinding,
    pipeline: &PipelineCell,
    root: ElementId,
) -> DenseSnapshot {
    let (child_ids, render_ids, parent_render) = {
        let tree = binding.tree_mut();
        let root_node = tree.get(root).expect("dense root remains live");
        let child_ids = root_node.child_ids().to_vec();
        assert_eq!(child_ids.len(), CHILD_COUNT, "logical child count");
        let render_ids = child_ids
            .iter()
            .enumerate()
            .map(|(slot, child)| {
                let node = tree.get(*child).expect("stored child id resolves");
                assert_eq!(node.parent(), Some(root), "child reverse parent");
                assert_eq!(node.slot(), slot, "child reverse slot");
                node.element()
                    .render_id()
                    .expect("every dense child owns a render object")
            })
            .collect::<Vec<_>>();
        let parent_render = root_node
            .element()
            .render_id()
            .expect("DenseRow owns RenderFlex");
        (child_ids, render_ids, parent_render)
    };

    let geometries = pipeline.with(|owner| {
        let render_tree = owner.render_tree();
        assert_eq!(
            render_tree.children(parent_render),
            render_ids,
            "logical and render child order must agree"
        );
        render_ids
            .iter()
            .map(|render_id| {
                (
                    inspect::box_geometry(owner, *render_id).expect("child was laid out"),
                    inspect::render_offset(owner, *render_id).expect("child has an offset"),
                )
            })
            .collect()
    });
    DenseSnapshot {
        child_ids,
        render_ids,
        geometries,
    }
}

fn mount_healthy_row() -> (HeadlessBinding, PipelineCell, ElementId, DenseSnapshot) {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut binding = HeadlessBinding::new();
    let mounted = binding.mount_root(
        &DenseRow {
            children: healthy_children(),
        },
        MountOwners::with_pipeline_owner(pipeline.clone()),
        MountOptions::tight(300.0, 80.0),
    );
    assert!(mounted.painted, "healthy bootstrap paints");
    assert!(
        binding.build_owner_mut().take_recovered_panics().is_empty(),
        "healthy bootstrap has no recovery"
    );
    let root = mounted.content_element;
    let initial = snapshot(&mut binding, &pipeline, root);
    (binding, pipeline, root, initial)
}

fn assert_recovered_frame(
    binding: &mut HeadlessBinding,
    pipeline: &PipelineCell,
    root: ElementId,
    initial: &DenseSnapshot,
) -> DenseSnapshot {
    let initial_paint_count = binding.painted_frame_count();
    let initial_layer_root = binding
        .layer_tree()
        .expect("bootstrap retained a layer tree")
        .root();
    binding.pump_frame(Duration::from_millis(16));
    assert!(
        binding.did_paint_last_frame(),
        "recovery frame paints fresh output"
    );
    assert_eq!(binding.painted_frame_count(), initial_paint_count + 1);
    assert_eq!(
        binding
            .layer_tree()
            .expect("recovery retains the committed layer tree")
            .root(),
        initial_layer_root,
        "the composited root remains retained across recovery"
    );

    let recovered = snapshot(binding, pipeline, root);
    for slot in 0..CHILD_COUNT {
        if slot == FAILING_SLOT {
            continue;
        }
        assert_eq!(recovered.child_ids[slot], initial.child_ids[slot]);
        assert_eq!(recovered.render_ids[slot], initial.render_ids[slot]);
        assert_eq!(recovered.geometries[slot], initial.geometries[slot]);
    }
    let replacement = recovered.child_ids[FAILING_SLOT];
    let tree = binding.tree_mut();
    let replacement_node = tree.get(replacement).expect("replacement stays live");
    assert_eq!(replacement_node.parent(), Some(root));
    assert_eq!(replacement_node.slot(), FAILING_SLOT);
    assert_eq!(
        replacement_node.element().view_type_id(),
        TypeId::of::<ErrorView>()
    );
    let replacement_size = recovered.geometries[FAILING_SLOT].0;
    assert!(replacement_size.width > px(0.0) && replacement_size.height > px(0.0));
    pipeline.with(|owner| {
        assert!(
            owner
                .render_tree()
                .get(recovered.render_ids[FAILING_SLOT])
                .expect("replacement render id resolves")
                .downcast_render_object::<RenderErrorBox>()
                .is_some(),
            "ErrorView must materialize as RenderErrorBox"
        );
    });
    recovered
}

fn assert_no_stranded_build_work(binding: &mut HeadlessBinding, replacement: ElementId) {
    let owner = binding.build_owner_mut();
    assert!(!owner.has_dirty_elements());
    assert_eq!(owner.dirty_count(), 0);
    assert_eq!(owner.pending_external_builds(), 0);
    assert_eq!(owner.pending_rebuild_reasons(replacement), None);
}

#[test]
fn lifecycle_panic_containment_init_state_paints_exact_error_slot() {
    let (mut binding, pipeline, root, initial) = mount_healthy_row();
    let failed_id = Rc::new(Cell::new(None));
    let init_calls = Rc::new(Cell::new(0));
    let dispose_calls = Rc::new(Cell::new(0));
    binding.swap_root_view(
        root,
        &DenseRow {
            children: children_with_failure(
                PanicsInInitState {
                    failed_id: Rc::clone(&failed_id),
                    init_calls: Rc::clone(&init_calls),
                    dispose_calls: Rc::clone(&dispose_calls),
                }
                .boxed(),
            ),
        },
    );

    let recovered_tree = assert_recovered_frame(&mut binding, &pipeline, root, &initial);
    let failed = failed_id.get().expect("init_state captured its element id");
    assert_eq!((init_calls.get(), dispose_calls.get()), (1, 0));
    assert!(binding.tree_mut().get(failed).is_none());
    assert!(
        binding
            .tree_mut()
            .get(initial.child_ids[FAILING_SLOT])
            .is_none()
    );

    let records = binding.build_owner_mut().take_recovered_panics();
    assert_eq!(records.len(), 1, "one behavior panic records once");
    let record = &records[0];
    assert_eq!(record.hook, LifecycleHook::InitState);
    assert_eq!(record.view_type_id, TypeId::of::<PanicsInInitState>());
    assert!(!record.internal_invariant);
    assert!(matches!(
        record.at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if element == failed
    ));
    assert_ne!(recovered_tree.child_ids[FAILING_SLOT], failed);
    assert!(binding.build_owner_mut().take_recovered_panics().is_empty());
    assert_no_stranded_build_work(&mut binding, recovered_tree.child_ids[FAILING_SLOT]);
}

#[test]
fn lifecycle_panic_containment_create_render_object_paints_exact_error_slot() {
    let (mut binding, pipeline, root, initial) = mount_healthy_row();
    binding.swap_root_view(
        root,
        &DenseRow {
            children: children_with_failure(PanicsOnCreateRenderObject.boxed()),
        },
    );

    let recovered_tree = assert_recovered_frame(&mut binding, &pipeline, root, &initial);
    let substitute = recovered_tree.child_ids[FAILING_SLOT];
    assert!(
        binding
            .tree_mut()
            .get(initial.child_ids[FAILING_SLOT])
            .is_none()
    );
    let records = binding.build_owner_mut().take_recovered_panics();
    assert_eq!(records.len(), 1, "one mount panic records once");
    let record = &records[0];
    assert_eq!(record.hook, LifecycleHook::Mount);
    assert_eq!(
        record.view_type_id,
        TypeId::of::<PanicsOnCreateRenderObject>()
    );
    assert!(!record.internal_invariant);
    let minted = match record.at {
        RecoveredAt::Substituted {
            element: Some(minted),
            substitute: recorded_substitute,
            parent,
            slot,
            ..
        } => {
            assert_eq!(recorded_substitute, substitute);
            assert_eq!(parent, root);
            assert_eq!(slot, FAILING_SLOT);
            minted
        }
        other => panic!("expected exact substituted attribution, got {other:?}"),
    };
    assert!(binding.tree_mut().get(minted).is_none());
    assert!(binding.build_owner_mut().take_recovered_panics().is_empty());
    assert_no_stranded_build_work(&mut binding, substitute);
}
