//! Lazy sliver update containment: a resident a delegate returns as a render view directly,
//! whose `update_render_object` panics as it rebuilds, is replaced by an error view at its
//! logical index while the rest of the band and the frame carry on.

use std::{any::TypeId, cell::Cell, rc::Rc};

use flui_foundation::geometry::{AxisDirection, Size};
use flui_foundation::{ElementId, RebuildReason};
use flui_objects::{RenderSizedBox, RenderViewport};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_rendering::testing::SceneHost;
use flui_view::element::SliverList;
use flui_view::{
    BoxedView, BuildOwner, ElementTree, ErrorView, LifecycleHook, RecoveredAt, RenderView, View,
    ViewExt,
};

use crate::dense_reconcile_containment::first_render_descendant;

const ITEM_COUNT: usize = 3;
const PANICKING_INDEX: usize = 1;
const ITEM_EXTENT: f64 = 20.0;

/// A box viewport over one sliver.
#[derive(Clone)]
struct LazyViewport {
    sliver: SliverList,
}

impl RenderView for LazyViewport {
    type Protocol = BoxProtocol;
    type RenderObject = RenderViewport;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderViewport::new(AxisDirection::TopToBottom)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.sliver);
    }
}

impl View for LazyViewport {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// The item the delegate returns: a render view with no composite above it, whose
/// `update_render_object` panics once armed when it is the panicking index.
#[derive(Clone)]
struct LazyRenderItem {
    should_panic: bool,
    armed: Rc<Cell<bool>>,
}

impl RenderView for LazyRenderItem {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::new(None, Some(ITEM_EXTENT))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        assert!(
            !(self.should_panic && self.armed.get()),
            "lazy update_render_object panic"
        );
        render_object.set_size(None, Some(ITEM_EXTENT))
    }
}

impl View for LazyRenderItem {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// A fresh builder delegate on every call, so each root update refreshes the residents.
fn lazy_viewport(armed: &Rc<Cell<bool>>) -> LazyViewport {
    let armed = Rc::clone(armed);
    let builder = Rc::new(move |index: usize| -> Option<BoxedView> {
        (index < ITEM_COUNT).then(|| {
            LazyRenderItem {
                should_panic: index == PANICKING_INDEX,
                armed: Rc::clone(&armed),
            }
            .boxed()
        })
    });
    LazyViewport {
        sliver: SliverList::new(ITEM_COUNT, ITEM_EXTENT, builder),
    }
}

fn run_frame(
    scene: &mut SceneHost,
    tree: &mut ElementTree,
    owner: &mut BuildOwner,
    pipeline: &PipelineCell,
) {
    scene
        .drive_cell(pipeline, |writer| {
            owner.run_frame_with_layout_builders(tree, pipeline, writer)
        })
        .0
        .expect("the lazy viewport frame must complete");
}

fn update_root(
    tree: &mut ElementTree,
    owner: &mut BuildOwner,
    root: ElementId,
    view: &LazyViewport,
) {
    tree.update(root, view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::ParentUpdate);
    owner.build_scope(tree);
}

/// The sliver host: the viewport's one dense child.
fn sliver_host(tree: &ElementTree, root: ElementId) -> ElementId {
    let children = tree.get(root).expect("live viewport").child_ids();
    assert_eq!(children.len(), 1, "the viewport hosts exactly one sliver");
    children[0]
}

/// `(logical index, element, view type)` for every element parented on the sliver host.
fn residents(tree: &ElementTree, host: ElementId) -> Vec<(usize, ElementId, TypeId)> {
    let mut residents: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(host))
        .map(|(id, node)| (node.slot(), id, node.element().view_type_id()))
        .collect();
    residents.sort_by_key(|(index, _, _)| *index);
    residents
}

fn assert_item_residents(tree: &ElementTree, host: ElementId) -> Vec<ElementId> {
    let residents = residents(tree, host);
    assert_eq!(
        residents
            .iter()
            .map(|(index, _, view_type_id)| (*index, *view_type_id))
            .collect::<Vec<_>>(),
        (0..ITEM_COUNT)
            .map(|index| (index, TypeId::of::<LazyRenderItem>()))
            .collect::<Vec<_>>(),
        "every logical index holds exactly one item resident"
    );
    residents.into_iter().map(|(_, id, _)| id).collect()
}

pub(crate) fn lazy_render_update_panic_substitutes_at_its_logical_index() {
    let mut scene = SceneHost::new();
    let pipeline = PipelineCell::new(PipelineOwner::new(
        flui_rendering::TextContextHandle::standalone(),
    ));
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let armed = Rc::new(Cell::new(false));
    let root = tree.mount_root_with_pipeline_owner(
        &lazy_viewport(&armed),
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let render_root = tree
        .get(root)
        .and_then(|node| node.element().render_id())
        .expect("the viewport owns its render object");
    pipeline.with_mut(|pipeline_owner| {
        pipeline_owner.set_root_id(Some(render_root));
        pipeline_owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 200.0))));
    });
    run_frame(&mut scene, &mut tree, &mut owner, &pipeline);
    let host = sliver_host(&tree, root);
    let before = assert_item_residents(&tree, host);
    assert!(owner.take_recovered_panics().is_empty());

    // A new delegate refreshes every resident; the panicking item's configuration push runs
    // as it rebuilds, inside this frame's lazy service pass.
    armed.set(true);
    update_root(&mut tree, &mut owner, root, &lazy_viewport(&armed));
    run_frame(&mut scene, &mut tree, &mut owner, &pipeline);

    let after = residents(&tree, host);
    assert_eq!(
        after.len(),
        ITEM_COUNT,
        "the band keeps one resident per index"
    );
    for (index, id, view_type_id) in &after {
        if *index == PANICKING_INDEX {
            assert_eq!(*view_type_id, TypeId::of::<ErrorView>());
        } else {
            assert_eq!(
                *id, before[*index],
                "healthy index {index} keeps its element"
            );
            assert_eq!(*view_type_id, TypeId::of::<LazyRenderItem>());
        }
    }
    let resident = before[PANICKING_INDEX];
    let substitute = after[PANICKING_INDEX].1;
    assert!(tree.get(resident).is_none(), "the failed resident is freed");
    let substitute_render =
        first_render_descendant(&tree, substitute).expect("the error view renders");
    let substitute_size = pipeline.with(|pipeline_owner| {
        pipeline_owner
            .render_tree()
            .get(substitute_render)
            .and_then(flui_rendering::storage::RenderNode::geometry_box)
    });
    assert!(
        substitute_size.is_some_and(|size| size.is_finite() && !size.is_zero_area()),
        "the error view is laid out in the frame that recovered: {substitute_size:?}"
    );

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "one update hook panic records once");
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Update);
    assert_eq!(panic.view_type_id, TypeId::of::<LazyRenderItem>());
    assert!(
        matches!(
            panic.at,
            RecoveredAt::Substituted {
                element: Some(recorded_resident),
                substitute: recorded_substitute,
                parent: recorded_parent,
                slot: PANICKING_INDEX,
                ..
            } if recorded_resident == resident
                && recorded_substitute == substitute
                && recorded_parent == host
        ),
        "the panic names the resident, its substitute and its logical index: {:?}",
        panic.at
    );

    // The residency map follows the substitute: once the item stops panicking, the next
    // refresh replaces the error view at that index and leaves no orphan behind.
    armed.set(false);
    update_root(&mut tree, &mut owner, root, &lazy_viewport(&armed));
    run_frame(&mut scene, &mut tree, &mut owner, &pipeline);
    let healed = assert_item_residents(&tree, host);
    assert!(tree.get(substitute).is_none(), "the substitute is evicted");
    for index in (0..ITEM_COUNT).filter(|index| *index != PANICKING_INDEX) {
        assert_eq!(
            healed[index], before[index],
            "healthy index {index} keeps its element"
        );
    }
    assert!(owner.take_recovered_panics().is_empty());
}
