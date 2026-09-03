//! Layout-builder scoped drains scale with dirty work rather than the number
//! of foreign scope/heap combinations.

// Bench harness, not public API; Criterion generates the entry function.

use std::sync::Arc;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::{ElementId, RenderId};
use flui_objects::{LayoutConstraintsCell, RenderSizedBox};
use flui_rendering::{
    RenderUpdateImpact,
    constraints::BoxConstraints,
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_types::geometry::px;
use flui_view::{
    BuildOwner, RebuildReason, RenderObjectContext, RenderView, View, element::ElementKind,
    tree::ElementTree,
};

#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> ElementKind {
        ElementKind::render_variable(self)
    }
}

struct Fixture {
    owner: BuildOwner,
    tree: ElementTree,
    pipeline: PipelineCell,
}

fn constraints() -> BoxConstraints {
    BoxConstraints::tight_for(Some(px(100.0)), Some(px(100.0)))
}

fn insert_child(
    owner: &mut BuildOwner,
    tree: &mut ElementTree,
    parent: ElementId,
    slot: usize,
) -> ElementId {
    tree.insert(&Leaf, parent, slot, &mut owner.element_owner_mut())
}

fn insert_render_node(pipeline: &PipelineCell) -> RenderId {
    pipeline.with_mut(|owner| owner.insert::<BoxProtocol>(Box::new(RenderSizedBox::shrink())))
}

fn register_ready_scope(
    owner: &mut BuildOwner,
    pipeline: &PipelineCell,
    scope: ElementId,
) -> Arc<LayoutConstraintsCell> {
    let cell = owner.register_layout_builder_for_test(insert_render_node(pipeline), scope);
    cell.publish(constraints());
    cell.commit();
    cell
}

fn no_scope_fixture(dirty_count: usize) -> Fixture {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let root = tree.mount_root(&Leaf, &mut owner.element_owner_mut());
    owner.build_scope(&mut tree);
    for slot in 0..dirty_count {
        let child = insert_child(&mut owner, &mut tree, root, slot);
        owner.build_scope(&mut tree);
        tree.mark_needs_build(child);
        owner.schedule_build_for(child, 1, RebuildReason::StateChange);
    }
    Fixture {
        owner,
        tree,
        pipeline,
    }
}

fn sibling_scope_fixture(scope_count: usize) -> Fixture {
    let mut fixture = no_scope_fixture(0);
    let root = fixture.tree.root().expect("fixture root");
    for slot in 0..scope_count {
        let scope = insert_child(&mut fixture.owner, &mut fixture.tree, root, slot);
        fixture.owner.build_scope(&mut fixture.tree);
        let descendant = insert_child(&mut fixture.owner, &mut fixture.tree, scope, 0);
        fixture.owner.build_scope(&mut fixture.tree);
        register_ready_scope(&mut fixture.owner, &fixture.pipeline, scope);
        fixture.tree.mark_needs_build(descendant);
        fixture
            .owner
            .schedule_build_for(descendant, 2, RebuildReason::StateChange);
    }
    fixture.owner.build_scope(&mut fixture.tree);
    fixture
}

fn nested_scope_fixture(scope_count: usize) -> Fixture {
    let mut fixture = no_scope_fixture(0);
    let mut parent = fixture.tree.root().expect("fixture root");
    for _ in 0..scope_count {
        let scope = insert_child(&mut fixture.owner, &mut fixture.tree, parent, 0);
        fixture.owner.build_scope(&mut fixture.tree);
        let descendant = insert_child(&mut fixture.owner, &mut fixture.tree, scope, 1);
        fixture.owner.build_scope(&mut fixture.tree);
        register_ready_scope(&mut fixture.owner, &fixture.pipeline, scope);
        fixture.tree.mark_needs_build(descendant);
        let depth = fixture.tree.get(descendant).expect("descendant").depth();
        fixture
            .owner
            .schedule_build_for(descendant, depth, RebuildReason::StateChange);
        parent = scope;
    }
    fixture.owner.build_scope(&mut fixture.tree);
    fixture
}

fn bench_scoped_build_drain(c: &mut Criterion) {
    let mut group = c.benchmark_group("scoped_build_drain");
    group.sample_size(20);

    group.bench_function("no_scopes/256_dirty", |b| {
        b.iter_batched(
            || no_scope_fixture(256),
            |mut fixture| fixture.owner.build_scope(&mut fixture.tree),
            BatchSize::SmallInput,
        );
    });

    for scope_count in [32, 256] {
        group.bench_with_input(
            BenchmarkId::new("sibling_scopes", scope_count),
            &scope_count,
            |b, &scope_count| {
                b.iter_batched(
                    || sibling_scope_fixture(scope_count),
                    |mut fixture| {
                        fixture
                            .owner
                            .service_layout_builders(&mut fixture.tree, &fixture.pipeline)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.bench_function("nested_published_scopes/13", |b| {
        b.iter_batched(
            || nested_scope_fixture(13),
            |mut fixture| {
                fixture
                    .owner
                    .service_layout_builders(&mut fixture.tree, &fixture.pipeline)
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, bench_scoped_build_drain);
criterion_main!(benches);
