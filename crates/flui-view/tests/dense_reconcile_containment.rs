//! Dense reconciliation containment: topology, layout, and observation stay
//! one committed version when a mount or `GlobalKey` retake hook panics.

use std::{
    any::TypeId,
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId, ViewKey};
use flui_objects::{RenderFlex, RenderSizedBox};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Size, geometry::px};
use flui_view::{
    BoxedView, BuildContext, BuildOwner, ElementTree, ErrorView, GlobalKey, IntoView,
    LifecycleHook, RebuildReason, RecoveredAt, RenderView, StatefulView, View, ViewExt, ViewState,
};

pub(super) const DENSE_CHILD_COUNT: usize = 10;
pub(super) const PANICKING_SLOT: usize = 7;

#[derive(Clone)]
pub(super) struct DenseRow {
    pub(super) children: Vec<BoxedView>,
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
pub(super) struct DenseHealthyLeaf {
    pub(super) marker: usize,
}

impl DenseHealthyLeaf {
    fn size(&self) -> Size {
        Size::new(px(8.0 + self.marker as f32), px(12.0))
    }
}

impl RenderView for DenseHealthyLeaf {
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

impl View for DenseHealthyLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
pub(super) struct DensePanicsOnCreate;

impl RenderView for DensePanicsOnCreate {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        panic!("dense create_render_object panic")
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for DensePanicsOnCreate {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DenseObservation {
    Mount {
        element: ElementId,
        parent: Option<ElementId>,
        slot: usize,
        view_type_id: TypeId,
    },
    Unmount {
        element: ElementId,
    },
    Move {
        element: ElementId,
        parent: ElementId,
        slot: usize,
    },
    Rebuilt {
        element: ElementId,
        view_type_id: TypeId,
        reasons: flui_foundation::RebuildReasons,
    },
}

#[derive(Default)]
pub(super) struct DenseObserver {
    events: parking_lot::Mutex<Vec<DenseObservation>>,
}

impl flui_foundation::observe::TreeObserver for DenseObserver {
    fn element_mounted(&self, event: &flui_foundation::observe::ElementMounted) {
        self.events.lock().push(DenseObservation::Mount {
            element: event.element,
            parent: event.parent,
            slot: event.slot,
            view_type_id: event.view_type_id,
        });
    }

    fn element_unmounted(&self, event: &flui_foundation::observe::ElementUnmounted) {
        self.events.lock().push(DenseObservation::Unmount {
            element: event.element,
        });
    }

    fn element_moved(&self, event: &flui_foundation::observe::ElementMoved) {
        self.events.lock().push(DenseObservation::Move {
            element: event.element,
            parent: event.parent,
            slot: event.slot,
        });
    }

    fn element_rebuilt(&self, event: &flui_foundation::observe::ElementRebuilt) {
        self.events.lock().push(DenseObservation::Rebuilt {
            element: event.element,
            view_type_id: event.view_type_id,
            reasons: event.reasons,
        });
    }
}

impl DenseObserver {
    pub(super) fn events(&self) -> Vec<DenseObservation> {
        self.events.lock().clone()
    }

    fn lifecycle_events_for(&self, element: ElementId) -> Vec<DenseObservation> {
        self.events()
            .into_iter()
            .filter(|event| match event {
                DenseObservation::Mount {
                    element: observed, ..
                }
                | DenseObservation::Unmount { element: observed } => *observed == element,
                DenseObservation::Move { .. } | DenseObservation::Rebuilt { .. } => false,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DenseSnapshot {
    pub(super) child_ids: Vec<ElementId>,
    pub(super) render_children: Vec<RenderId>,
    pub(super) live_ids: HashSet<ElementId>,
}

fn dense_children_with(slot: usize, child: BoxedView) -> Vec<BoxedView> {
    (0..DENSE_CHILD_COUNT)
        .map(|index| {
            if index == slot {
                child.clone()
            } else {
                DenseHealthyLeaf { marker: index }.boxed()
            }
        })
        .collect()
}

fn dense_healthy_children() -> Vec<BoxedView> {
    (0..DENSE_CHILD_COUNT)
        .map(|marker| DenseHealthyLeaf { marker }.boxed())
        .collect()
}

pub(super) fn first_render_descendant(tree: &ElementTree, element: ElementId) -> Option<RenderId> {
    let node = tree.get(element)?;
    node.element().render_id().or_else(|| {
        node.child_ids()
            .iter()
            .find_map(|child| first_render_descendant(tree, *child))
    })
}

fn subtree_element_ids(tree: &ElementTree, root: ElementId) -> Vec<ElementId> {
    fn collect(tree: &ElementTree, element: ElementId, ids: &mut Vec<ElementId>) {
        ids.push(element);
        for child in tree
            .get(element)
            .expect("a captured subtree element must resolve")
            .child_ids()
        {
            collect(tree, *child, ids);
        }
    }

    let mut ids = Vec::new();
    collect(tree, root, &mut ids);
    ids
}

pub(super) fn run_real_pipeline_frame(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    expected_root: ElementId,
) {
    let expected_render_root = tree
        .get(expected_root)
        .expect("the expected pipeline root element must resolve")
        .element()
        .render_id()
        .expect("the expected pipeline root element must own a render object");
    pipeline.with_mut(|owner| {
        let render_root = {
            let render_tree = owner.render_tree();
            let mut parentless = render_tree
                .iter()
                .map(|(id, _)| id)
                .filter(|id| render_tree.parent(*id).is_none());
            let render_root = parentless
                .next()
                .expect("the dense containment topology must have one render root");
            assert!(
                parentless.next().is_none(),
                "the dense containment topology must have exactly one render root"
            );
            render_root
        };
        assert_eq!(
            render_root, expected_render_root,
            "the sole parentless render node must be the expected dense parent frontier"
        );
        owner.set_root_id(Some(render_root));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(320.0), px(80.0)))));
        let (idle, result) = std::mem::take(owner).run_frame();
        *owner = idle;
        result.expect("the dense containment topology must complete a real pipeline frame");
    });
}

fn assert_observer_conservation(
    tree: &ElementTree,
    observer: &DenseObserver,
) -> HashSet<ElementId> {
    let mut active = HashSet::new();
    let mut mounted_before_unmount = HashSet::new();
    for event in observer.events() {
        match event {
            DenseObservation::Mount { element, .. } => {
                assert!(
                    mounted_before_unmount.insert(element),
                    "a generational element id may be announced as mounted only once: {element:?}"
                );
                assert!(
                    active.insert(element),
                    "a mounted element id cannot already be active: {element:?}"
                );
            }
            DenseObservation::Unmount { element } => {
                assert!(
                    mounted_before_unmount.contains(&element),
                    "an unmount must follow that exact generational id's mount: {element:?}"
                );
                assert!(
                    active.remove(&element),
                    "an element may be unmounted only while its announced mount is unmatched: \
                     {element:?}"
                );
            }
            DenseObservation::Move { .. } | DenseObservation::Rebuilt { .. } => {}
        }
    }

    let live_ids: HashSet<_> = tree.iter_nodes().map(|(id, _)| id).collect();
    assert_eq!(
        active, live_ids,
        "the observer's exact active-id set must equal the slab's live generational ids"
    );
    live_ids
}

pub(super) fn assert_dense_snapshot_with_count(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    parent: ElementId,
    observer: &DenseObserver,
    expected_child_count: usize,
) -> DenseSnapshot {
    let parent_node = tree.get(parent).expect("the dense parent must remain live");
    let child_ids = parent_node.child_ids().to_vec();
    assert_eq!(
        child_ids.len(),
        expected_child_count,
        "the parent must store exactly one child id per declared dense slot"
    );
    assert_eq!(
        child_ids.iter().copied().collect::<HashSet<_>>().len(),
        expected_child_count,
        "the parent's stored generational child ids must be unique"
    );

    for (slot, child) in child_ids.iter().copied().enumerate() {
        let child_node = tree
            .get(child)
            .expect("every id stored by the dense parent must resolve");
        assert_eq!(
            child_node.parent(),
            Some(parent),
            "the child-side parent edge must agree with the parent's stored edge"
        );
        assert_eq!(
            child_node.slot(),
            slot,
            "the child's stored slot must equal its index in parent.child_ids"
        );
    }

    let expected_render_children: Vec<_> = child_ids
        .iter()
        .map(|child| {
            first_render_descendant(tree, *child)
                .expect("every dense logical child must have a first render descendant")
        })
        .collect();
    let parent_render = parent_node
        .element()
        .render_id()
        .expect("the DenseRow parent must own its RenderFlex");
    let render_children = pipeline.with(|owner| {
        owner
            .render_tree()
            .get(parent_render)
            .expect("the parent render id must resolve")
            .children()
            .to_vec()
    });
    assert_eq!(
        render_children, expected_render_children,
        "logical child first-render-descendant ids must exactly equal the RenderFlex children in \
         slot order"
    );

    DenseSnapshot {
        child_ids,
        render_children,
        live_ids: assert_observer_conservation(tree, observer),
    }
}

fn assert_dense_snapshot(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    parent: ElementId,
    observer: &DenseObserver,
) -> DenseSnapshot {
    assert_dense_snapshot_with_count(tree, pipeline, parent, observer, DENSE_CHILD_COUNT)
}

fn assert_healthy_dense_layout(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    snapshot: &DenseSnapshot,
    excluded_slot: usize,
) {
    for (slot, child) in snapshot.child_ids.iter().copied().enumerate() {
        if slot == excluded_slot {
            continue;
        }
        assert_eq!(
            tree.get(child)
                .expect("healthy dense child")
                .element()
                .view_type_id(),
            TypeId::of::<DenseHealthyLeaf>(),
            "slot {slot} must retain its healthy view"
        );
        let render_id = first_render_descendant(tree, child).expect("healthy leaf render id");
        let geometry = pipeline.with(|owner| {
            owner
                .render_tree()
                .get(render_id)
                .expect("healthy render id must resolve")
                .geometry_box()
        });
        assert_eq!(
            geometry,
            Some(DenseHealthyLeaf { marker: slot }.size()),
            "healthy slot {slot} must complete layout at its exact nonzero configured size"
        );
    }
}

pub(super) fn mount_dense_root(
    root: DenseRow,
) -> (
    ElementTree,
    BuildOwner,
    PipelineCell,
    Arc<DenseObserver>,
    ElementId,
) {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let observer = Arc::new(DenseObserver::default());
    let mut owner = BuildOwner::new();
    owner.set_tree_observer(
        Arc::clone(&observer) as Arc<dyn flui_foundation::observe::TreeObserver>
    );
    let mut tree = ElementTree::new();
    let root_id = tree.mount_root_with_pipeline_owner(
        &root,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    (tree, owner, pipeline, observer, root_id)
}

#[test]
fn dense_mount_panic_substitutes_at_exact_slot_and_preserves_topology() {
    let (mut tree, mut owner, pipeline, observer, parent) = mount_dense_root(DenseRow {
        children: dense_children_with(PANICKING_SLOT, DensePanicsOnCreate.boxed()),
    });

    owner.build_scope(&mut tree);

    let snapshot = assert_dense_snapshot(&tree, &pipeline, parent, &observer);
    let substitute = snapshot.child_ids[PANICKING_SLOT];
    assert_eq!(
        tree.get(substitute)
            .expect("the recovered slot must resolve")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>(),
        "only the failing slot is replaced by ErrorView"
    );

    run_real_pipeline_frame(&tree, &pipeline, parent);
    assert_healthy_dense_layout(&tree, &pipeline, &snapshot, PANICKING_SLOT);

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "one failing mount records once");
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Mount);
    assert_eq!(panic.view_type_id, TypeId::of::<DensePanicsOnCreate>());
    let minted = match panic.at {
        RecoveredAt::Substituted {
            element: Some(minted),
            substitute: recorded_substitute,
            parent: recorded_parent,
            slot,
            ..
        } => {
            assert_eq!(recorded_substitute, substitute);
            assert_eq!(recorded_parent, parent);
            assert_eq!(slot, PANICKING_SLOT);
            minted
        }
        other => panic!("expected an exact failed-mount substitution record, got {other:?}"),
    };
    assert!(
        tree.get(minted).is_none(),
        "the discarded minted generational id must not resolve"
    );
    assert!(
        observer.lifecycle_events_for(minted).is_empty(),
        "an abandoned mint is never announced to the observer"
    );
    assert_eq!(
        observer.lifecycle_events_for(substitute),
        vec![DenseObservation::Mount {
            element: substitute,
            parent: Some(parent),
            slot: PANICKING_SLOT,
            view_type_id: TypeId::of::<ErrorView>(),
        }],
        "the substitute has exactly one announced Mount"
    );
    assert!(
        owner.take_recovered_panics().is_empty(),
        "the recovery drain is consuming and cannot report the same panic twice"
    );
}

#[test]
fn repeated_dense_mount_panics_do_not_accumulate_ghosts() {
    let (mut tree, mut owner, pipeline, observer, parent) = mount_dense_root(DenseRow {
        children: dense_healthy_children(),
    });
    owner.build_scope(&mut tree);
    run_real_pipeline_frame(&tree, &pipeline, parent);
    let initial = assert_dense_snapshot(&tree, &pipeline, parent, &observer);
    let healthy_ids: HashMap<_, _> = initial
        .child_ids
        .iter()
        .copied()
        .enumerate()
        .filter(|(slot, _)| *slot != PANICKING_SLOT)
        .collect();
    let stable_live_count = initial.live_ids.len();
    let mut previous_substitute = None;

    for attempt in 0..2 {
        tree.update(
            parent,
            &DenseRow {
                children: dense_children_with(PANICKING_SLOT, DensePanicsOnCreate.boxed()),
            },
            &mut owner.element_owner_mut(),
        );
        owner.schedule_build_for(parent, 0, RebuildReason::ParentUpdate);
        owner.build_scope(&mut tree);

        let snapshot = assert_dense_snapshot(&tree, &pipeline, parent, &observer);
        assert_eq!(
            snapshot.live_ids.len(),
            stable_live_count,
            "attempt {attempt} must replace one element with one substitute, without slab growth"
        );
        for (slot, expected_id) in &healthy_ids {
            assert_eq!(
                snapshot.child_ids[*slot], *expected_id,
                "healthy slot {slot} must keep its generational identity across every failed \
                 parent update"
            );
        }

        let current_substitute = snapshot.child_ids[PANICKING_SLOT];
        assert_eq!(
            tree.get(current_substitute)
                .expect("current substitute")
                .element()
                .view_type_id(),
            TypeId::of::<ErrorView>()
        );
        if let Some(previous) = previous_substitute {
            assert!(
                tree.get(previous).is_none(),
                "the prior attempt's substitute must be retired before the next one commits"
            );
        }

        run_real_pipeline_frame(&tree, &pipeline, parent);
        assert_healthy_dense_layout(&tree, &pipeline, &snapshot, PANICKING_SLOT);

        let mut recovered = owner.take_recovered_panics();
        assert_eq!(
            recovered.len(),
            1,
            "each failed parent update contributes one Mount recovery"
        );
        let panic = recovered.remove(0);
        assert_eq!(panic.hook, LifecycleHook::Mount);
        assert!(matches!(
            panic.at,
            RecoveredAt::Substituted {
                substitute,
                parent: recorded_parent,
                slot: PANICKING_SLOT,
                ..
            } if substitute == current_substitute && recorded_parent == parent
        ));
        previous_substitute = Some(current_substitute);
    }
}

#[derive(Clone)]
pub(super) struct DenseGlobalKeyUpdatePanicSubtree {
    key: GlobalKey<Self>,
    armed: Rc<Cell<bool>>,
}

impl DenseGlobalKeyUpdatePanicSubtree {
    pub(super) fn new(key: GlobalKey<Self>, armed: Rc<Cell<bool>>) -> Self {
        Self { key, armed }
    }
}

impl RenderView for DenseGlobalKeyUpdatePanicSubtree {
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
        assert!(!self.armed.get(), "dense retake update_render_object panic");
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&DenseHealthyLeaf { marker: 3 });
    }
}

impl View for DenseGlobalKeyUpdatePanicSubtree {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

fn mount_two_dense_hosts(
    child_under_a: BoxedView,
) -> (
    ElementTree,
    BuildOwner,
    PipelineCell,
    Arc<DenseObserver>,
    ElementId,
    ElementId,
) {
    let (mut tree, mut owner, pipeline, observer, root) = mount_dense_root(DenseRow {
        children: vec![
            DenseRow {
                children: vec![child_under_a],
            }
            .boxed(),
            DenseRow {
                children: Vec::new(),
            }
            .boxed(),
        ],
    });
    owner.build_scope(&mut tree);
    run_real_pipeline_frame(&tree, &pipeline, root);
    let hosts = tree.get(root).expect("two-host root").child_ids().to_vec();
    assert_eq!(hosts.len(), 2);
    (tree, owner, pipeline, observer, hosts[0], hosts[1])
}

fn soft_remove_from_a(tree: &mut ElementTree, owner: &mut BuildOwner, parent_a: ElementId) {
    let depth = tree.get(parent_a).expect("parent A").depth();
    tree.update(
        parent_a,
        &DenseRow {
            children: Vec::new(),
        },
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent_a, depth, RebuildReason::ParentUpdate);
    owner.build_scope(tree);
    assert!(
        tree.get(parent_a)
            .expect("parent A remains live")
            .child_ids()
            .is_empty(),
        "parent A must commit the soft removal before B claims the key"
    );
}

fn rebuild_dense_destination(
    tree: &mut ElementTree,
    owner: &mut BuildOwner,
    parent_b: ElementId,
    claimed_child: BoxedView,
) {
    let depth = tree.get(parent_b).expect("parent B").depth();
    tree.update(
        parent_b,
        &DenseRow {
            children: dense_children_with(PANICKING_SLOT, claimed_child),
        },
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent_b, depth, RebuildReason::ParentUpdate);
    owner.build_scope(tree);
}

#[test]
fn dense_inactive_retake_update_panic_finalizes_original() {
    let key = GlobalKey::<DenseGlobalKeyUpdatePanicSubtree>::new();
    let armed = Rc::new(Cell::new(false));
    let keyed = DenseGlobalKeyUpdatePanicSubtree::new(key.clone(), armed.clone());
    let (mut tree, mut owner, pipeline, observer, parent_a, parent_b) =
        mount_two_dense_hosts(keyed.clone().boxed());
    let original = owner
        .element_for_global_key(&key)
        .expect("the original keyed subtree must register under A");
    let original_subtree_ids = subtree_element_ids(&tree, original);
    let original_render_ids: Vec<_> = original_subtree_ids
        .iter()
        .filter_map(|id| tree.get(*id).and_then(|node| node.element().render_id()))
        .collect();
    assert!(
        original_subtree_ids.len() > 1 && original_render_ids.len() > 1,
        "the retake fixture must contain a real multi-element render subtree"
    );

    soft_remove_from_a(&mut tree, &mut owner, parent_a);
    assert!(
        tree.get(original).is_some(),
        "soft removal preserves the retake window"
    );
    armed.set(true);
    rebuild_dense_destination(&mut tree, &mut owner, parent_b, keyed.boxed());

    let snapshot_before_finalize = assert_dense_snapshot(&tree, &pipeline, parent_b, &observer);
    let substitute = snapshot_before_finalize.child_ids[PANICKING_SLOT];
    assert_eq!(
        tree.get(substitute)
            .expect("destination substitute")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>()
    );
    for element in &original_subtree_ids {
        assert!(
            tree.get(*element).is_none(),
            "failed retake finalizes every original subtree element immediately: {element:?}"
        );
    }
    assert_eq!(owner.element_for_global_key(&key), None);
    pipeline.with(|pipeline_owner| {
        for render_id in &original_render_ids {
            assert!(
                pipeline_owner.render_tree().get(*render_id).is_none(),
                "every original retake render id must be removed immediately: {render_id:?}"
            );
        }
    });
    assert_eq!(
        observer.lifecycle_events_for(original),
        vec![
            DenseObservation::Mount {
                element: original,
                parent: Some(parent_a),
                slot: 0,
                view_type_id: TypeId::of::<DenseGlobalKeyUpdatePanicSubtree>(),
            },
            DenseObservation::Unmount { element: original },
        ],
        "the original generational id has one causally ordered Mount then Unmount"
    );

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "the retake update is recorded once");
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Update);
    assert_eq!(
        panic.view_type_id,
        TypeId::of::<DenseGlobalKeyUpdatePanicSubtree>()
    );
    assert!(matches!(
        panic.at,
        RecoveredAt::Substituted {
            element: Some(element),
            substitute: recorded_substitute,
            parent: recorded_parent,
            slot: PANICKING_SLOT,
            ..
        } if element == original
            && recorded_substitute == substitute
            && recorded_parent == parent_b
    ));

    owner.finalize_tree(&mut tree);
    let snapshot_after_finalize = assert_dense_snapshot(&tree, &pipeline, parent_b, &observer);
    assert_eq!(
        snapshot_after_finalize, snapshot_before_finalize,
        "finalization must not mutate topology already cleaned by the failed retake"
    );
    assert!(
        owner.take_recovered_panics().is_empty(),
        "finalization must not duplicate the retake cleanup record"
    );
}

#[derive(Clone)]
pub(super) struct DenseRetakeRoot {
    key: GlobalKey<DenseRetakeRootState>,
    descendant_armed: Rc<Cell<bool>>,
}

impl DenseRetakeRoot {
    pub(super) fn new(
        key: GlobalKey<DenseRetakeRootState>,
        descendant_armed: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            key,
            descendant_armed,
        }
    }
}

pub(super) struct DenseRetakeRootState {
    descendant_armed: Rc<Cell<bool>>,
}

impl StatefulView for DenseRetakeRoot {
    type State = DenseRetakeRootState;

    fn create_state(&self) -> Self::State {
        DenseRetakeRootState {
            descendant_armed: self.descendant_armed.clone(),
        }
    }
}

impl ViewState<DenseRetakeRoot> for DenseRetakeRootState {
    fn build(&self, _view: &DenseRetakeRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        DenseActivatePanicDescendant {
            armed: self.descendant_armed.clone(),
        }
    }
}

impl View for DenseRetakeRoot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct DenseActivatePanicDescendant {
    armed: Rc<Cell<bool>>,
}

struct DenseActivatePanicDescendantState {
    armed: Rc<Cell<bool>>,
}

impl StatefulView for DenseActivatePanicDescendant {
    type State = DenseActivatePanicDescendantState;

    fn create_state(&self) -> Self::State {
        DenseActivatePanicDescendantState {
            armed: self.armed.clone(),
        }
    }
}

impl ViewState<DenseActivatePanicDescendant> for DenseActivatePanicDescendantState {
    fn build(
        &self,
        _view: &DenseActivatePanicDescendant,
        _ctx: &dyn BuildContext,
    ) -> impl IntoView {
        DenseHealthyLeaf { marker: 1 }
    }

    fn activate(&mut self) {
        assert!(!self.armed.get(), "dense retake descendant activate panic");
    }
}

impl View for DenseActivatePanicDescendant {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

#[test]
fn dense_retake_descendant_activate_panic_records_once_and_frees_subtree() {
    let key = GlobalKey::<DenseRetakeRootState>::new();
    let descendant_armed = Rc::new(Cell::new(false));
    let keyed_root = DenseRetakeRoot::new(key.clone(), descendant_armed.clone());
    let (mut tree, mut owner, pipeline, observer, parent_a, parent_b) =
        mount_two_dense_hosts(keyed_root.clone().boxed());
    let retake_root = owner
        .element_for_global_key(&key)
        .expect("the keyed retake root must register under A");
    let subtree_ids = subtree_element_ids(&tree, retake_root);
    let descendant = *tree
        .get(retake_root)
        .expect("retake root")
        .child_ids()
        .first()
        .expect("retake root builds the panicking stateful descendant");
    assert_eq!(
        tree.get(descendant)
            .expect("activate descendant")
            .element()
            .view_type_id(),
        TypeId::of::<DenseActivatePanicDescendant>()
    );
    let subtree_render_ids: Vec<_> = subtree_ids
        .iter()
        .filter_map(|id| tree.get(*id).and_then(|node| node.element().render_id()))
        .collect();
    assert!(
        !subtree_render_ids.is_empty(),
        "fixture must own a render frontier"
    );

    soft_remove_from_a(&mut tree, &mut owner, parent_a);
    descendant_armed.set(true);
    rebuild_dense_destination(&mut tree, &mut owner, parent_b, keyed_root.boxed());

    let snapshot = assert_dense_snapshot(&tree, &pipeline, parent_b, &observer);
    let substitute = snapshot.child_ids[PANICKING_SLOT];
    assert_eq!(
        tree.get(substitute)
            .expect("activate recovery substitute")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>()
    );
    for element in &subtree_ids {
        assert!(
            tree.get(*element).is_none(),
            "every original subtree element must be freed after failed activation: {element:?}"
        );
    }
    assert_eq!(owner.element_for_global_key(&key), None);
    pipeline.with(|pipeline_owner| {
        for render_id in &subtree_render_ids {
            assert!(
                pipeline_owner.render_tree().get(*render_id).is_none(),
                "every original subtree render id must be removed: {render_id:?}"
            );
        }
    });

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "the innermost activate seam records once without a coarse substitute duplicate"
    );
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Activate);
    assert_eq!(
        panic.view_type_id,
        TypeId::of::<DenseActivatePanicDescendant>()
    );
    assert!(matches!(
        panic.at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if element == descendant
    ));
    assert!(
        owner.take_recovered_panics().is_empty(),
        "the exact descendant record is never followed by a coarser duplicate"
    );
}
