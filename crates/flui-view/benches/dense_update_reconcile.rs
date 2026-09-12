//! Production dense-child update latency for stable and reordered keyed fan-outs.
//!
//! Fixture construction and the initial build are outside the timed closure. The
//! measured operation updates the real render parent and drains `BuildOwner`, so
//! every child crosses the production dense `update_child` path.

// Bench harness, not public API; `criterion_group!` generates the entry point.
#![allow(missing_docs)]

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::{
    RenderUpdateImpact,
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_types::geometry::px;
use flui_view::{BoxedView, BuildOwner, ElementTree, RenderView, View, ViewExt};

#[derive(Clone)]
struct KeyedLeaf {
    key: ValueKey<usize>,
    revision: usize,
}

impl RenderView for KeyedLeaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(1.0)), Some(px(1.0)))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        let extent = px(1.0 + (self.revision % 2) as f32);
        render_object.set_size(Some(extent), Some(extent))
    }
}

impl View for KeyedLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct DenseHost {
    children: Vec<BoxedView>,
}

impl RenderView for DenseHost {
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
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
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

impl View for DenseHost {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone, Copy)]
enum UpdateOrder {
    Stable,
    Reversed,
}

struct Fixture {
    tree: ElementTree,
    owner: BuildOwner,
    parent: ElementId,
    update: DenseHost,
}

fn children(fan_out: usize, revision: usize, order: UpdateOrder) -> Vec<BoxedView> {
    let keys: Box<dyn Iterator<Item = usize>> = match order {
        UpdateOrder::Stable => Box::new(0..fan_out),
        UpdateOrder::Reversed => Box::new((0..fan_out).rev()),
    };
    keys.map(|key| {
        KeyedLeaf {
            key: ValueKey::new(key),
            revision,
        }
        .boxed()
    })
    .collect()
}

fn setup(fan_out: usize, update_order: UpdateOrder) -> Fixture {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let initial = DenseHost {
        children: children(fan_out, 0, UpdateOrder::Stable),
    };
    let parent = tree.mount_root_with_pipeline_owner(
        &initial,
        Some(pipeline),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(parent, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    Fixture {
        tree,
        owner,
        parent,
        update: DenseHost {
            children: children(fan_out, 1, update_order),
        },
    }
}

fn run_update(fixture: &mut Fixture) {
    fixture.tree.update(
        fixture.parent,
        &fixture.update,
        &mut fixture.owner.element_owner_mut(),
    );
    fixture
        .owner
        .schedule_build_for(fixture.parent, 0, flui_view::RebuildReason::ParentUpdate);
    fixture.owner.build_scope(&mut fixture.tree);
    std::hint::black_box(
        fixture
            .tree
            .get(fixture.parent)
            .expect("the dense benchmark parent remains live")
            .child_ids(),
    );
}

fn bench_dense_update_reconcile(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("dense_update_reconcile");
    group.sample_size(20);
    for fan_out in [32_usize, 256, 1_024] {
        for (label, order) in [
            ("same_slot", UpdateOrder::Stable),
            ("reordered", UpdateOrder::Reversed),
        ] {
            group.bench_with_input(
                BenchmarkId::new(label, fan_out),
                &fan_out,
                |bencher, &fan_out| {
                    bencher.iter_batched_ref(
                        || setup(fan_out, order),
                        run_update,
                        BatchSize::PerIteration,
                    );
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench_dense_update_reconcile);
criterion_main!(benches);
