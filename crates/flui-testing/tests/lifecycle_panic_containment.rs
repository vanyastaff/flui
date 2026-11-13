//! Full-frame containment for panics raised while reconciling a dense row.
//!
//! The case starts from a healthy, painted tree and fails only after a root
//! swap, so the assertions exercise a frame of the UI runtime rather than the
//! first mount.

use std::{cell::Cell, rc::Rc};

use flui_foundation::geometry::{Offset, Size};
use flui_foundation::{ElementId, RenderId};
use flui_objects::{RenderFlex, RenderSizedBox};
use flui_rendering::protocol::BoxProtocol;
use flui_testing::widgets::{LaidOut, lay_out, tight};
use flui_view::{
    BoxedView, BuildContext, ErrorView, IntoView, LifecycleContext, RenderView, StatefulView, View,
    ViewExt, ViewState,
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
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
            Size::new(48.0, 80.0)
        } else {
            Size::new(10.0 + self.slot as f64, 20.0 + self.slot as f64)
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        PanickingState {
            failed_id: Rc::clone(&self.failed_id),
            init_calls: Rc::clone(&self.init_calls),
            dispose_calls: Rc::clone(&self.dispose_calls),
        }
    }
}

impl ViewState<PanicsInInitState> for PanickingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

#[derive(Debug)]
struct DenseSnapshot {
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

fn snapshot(laid: &LaidOut) -> DenseSnapshot {
    let row = laid.find_by_render_type("RenderFlex");
    let render_ids = laid.children(row);
    assert_eq!(render_ids.len(), CHILD_COUNT, "render child count");
    let geometries: Vec<(Size, Offset)> = render_ids
        .iter()
        .map(|id| (laid.size(*id), laid.offset(*id)))
        .collect();
    DenseSnapshot {
        render_ids,
        geometries,
    }
}

fn assert_no_stranded_build_work(laid: &mut LaidOut) {
    laid.with_build_owner_mut(|owner| {
        assert!(!owner.has_dirty_elements());
        assert_eq!(owner.dirty_count(), 0);
        assert_eq!(owner.pending_external_builds(), 0);
    });
}

pub(crate) fn lifecycle_panic_containment_init_state_paints_exact_error_slot() {
    let mut laid = lay_out(
        DenseRow {
            children: healthy_children(),
        },
        tight(300.0, 80.0),
    );
    assert!(laid.did_paint_last_frame(), "the healthy mount paints");
    let initial = snapshot(&laid);
    let initial_paint_count = laid.painted_frame_count();
    let initial_layer_root = laid
        .layer_tree()
        .expect("the mount retained a layer tree")
        .root();

    let failed_id = Rc::new(Cell::new(None));
    let init_calls = Rc::new(Cell::new(0));
    let dispose_calls = Rc::new(Cell::new(0));
    laid.pump_widget(DenseRow {
        children: children_with_failure(
            PanicsInInitState {
                failed_id: Rc::clone(&failed_id),
                init_calls: Rc::clone(&init_calls),
                dispose_calls: Rc::clone(&dispose_calls),
            }
            .boxed(),
        ),
    });

    assert!(
        laid.did_paint_last_frame(),
        "the recovery frame paints fresh output"
    );
    assert_eq!(laid.painted_frame_count(), initial_paint_count + 1);
    assert_eq!(
        laid.layer_tree()
            .expect("recovery retains the committed layer tree")
            .root(),
        initial_layer_root,
        "the composited root remains retained across recovery"
    );

    let recovered = snapshot(&laid);
    for slot in 0..CHILD_COUNT {
        if slot == FAILING_SLOT {
            continue;
        }
        assert_eq!(recovered.render_ids[slot], initial.render_ids[slot]);
        assert_eq!(recovered.geometries[slot], initial.geometries[slot]);
    }
    assert_ne!(
        recovered.render_ids[FAILING_SLOT],
        initial.render_ids[FAILING_SLOT]
    );
    assert_eq!(
        laid.find_by_render_type("RenderErrorBox"),
        recovered.render_ids[FAILING_SLOT],
        "ErrorView materializes as RenderErrorBox in the failed slot"
    );
    let replacement_size = recovered.geometries[FAILING_SLOT].0;
    assert!(replacement_size.width > 0.0 && replacement_size.height > 0.0);

    failed_id.get().expect("init_state captured its element id");
    assert_eq!((init_calls.get(), dispose_calls.get()), (1, 0));
    assert_eq!(laid.count_elements_by_view_type::<ErrorView>(), 1);
    assert_eq!(
        laid.count_elements_by_view_type::<PanicsInInitState>(),
        0,
        "the failed element is gone"
    );
    assert_no_stranded_build_work(&mut laid);
}
