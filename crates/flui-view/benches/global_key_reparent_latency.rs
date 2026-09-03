//! Production GlobalKey render-relocation latency.
//!
//! The timed operation updates a destination render parent, retakes an active
//! keyed render child through `BuildOwner::build_scope`, performs its targeted
//! provisional synchronization, and commits the coalesced authoritative render
//! order at the build boundary. Fixture construction is outside the timed
//! closure.

// Bench harness, not public API; `criterion_group!` generates the entry point.
#![allow(missing_docs)]

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::{ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{BoxedView, BuildOwner, ElementTree, GlobalKey, RenderView, View, ViewExt};

#[derive(Clone)]
struct Leaf {
    key: ValueKey<usize>,
}

impl RenderView for Leaf {
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

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct GlobalLeaf {
    key: GlobalKey<()>,
}

impl RenderView for GlobalLeaf {
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

impl View for GlobalLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct Host {
    key: ValueKey<usize>,
    children: Vec<BoxedView>,
}

impl Host {
    fn new(key: usize, children: Vec<BoxedView>) -> Self {
        Self {
            key: ValueKey::new(key),
            children,
        }
    }
}

impl RenderView for Host {
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

impl View for Host {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

struct Fixture {
    tree: ElementTree,
    owner: BuildOwner,
    destination: flui_foundation::ElementId,
    destination_update: Host,
    moved_render: flui_foundation::RenderId,
    pipeline: PipelineCell,
}

fn direct_children(
    tree: &ElementTree,
    parent: flui_foundation::ElementId,
) -> Vec<flui_foundation::ElementId> {
    let mut children = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect::<Vec<_>>();
    children.sort_by_key(|&(slot, _)| slot);
    children.into_iter().map(|(_, id)| id).collect()
}

fn setup(unrelated_count: usize) -> Fixture {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let global = GlobalKey::<()>::new();
    let mut root_children = vec![
        Host::new(
            1,
            vec![
                GlobalLeaf {
                    key: global.clone(),
                }
                .boxed(),
            ],
        )
        .boxed(),
        Host::new(2, Vec::new()).boxed(),
    ];
    root_children.extend((0..unrelated_count).map(|index| {
        Leaf {
            key: ValueKey::new(index + 10),
        }
        .boxed()
    }));
    let root = Host::new(0, root_children);
    let root_id = tree.mount_root_with_pipeline_owner(
        &root,
        Some(pipeline.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let parents = direct_children(&tree, root_id);
    let donor = parents[0];
    let destination = parents[1];
    let moved = direct_children(&tree, donor)[0];
    let moved_render = tree
        .get(moved)
        .and_then(|node| node.element().render_id())
        .expect("moved render child");

    Fixture {
        tree,
        owner,
        destination,
        destination_update: Host::new(
            2,
            vec![
                GlobalLeaf {
                    key: global.clone(),
                }
                .boxed(),
            ],
        ),
        moved_render,
        pipeline,
    }
}

fn bench_reparent(c: &mut Criterion) {
    let mut group = c.benchmark_group("global_key_render_relocation");
    for unrelated_count in [32_usize, 256, 1_024] {
        group.bench_with_input(
            BenchmarkId::new("unrelated_render_nodes", unrelated_count),
            &unrelated_count,
            |bencher, &count| {
                bencher.iter_batched_ref(
                    || setup(count),
                    |fixture| {
                        fixture.tree.update(
                            fixture.destination,
                            &fixture.destination_update,
                            &mut fixture.owner.element_owner_mut(),
                        );
                        fixture.owner.schedule_build_for(
                            fixture.destination,
                            fixture
                                .tree
                                .get(fixture.destination)
                                .expect("destination")
                                .depth(),
                            flui_view::RebuildReason::ParentUpdate,
                        );
                        fixture.owner.build_scope(&mut fixture.tree);
                        fixture.pipeline.with(|owner| {
                            std::hint::black_box(
                                owner
                                    .render_tree()
                                    .parent(fixture.moved_render)
                                    .expect("moved child remains attached"),
                            );
                        });
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_reparent);
criterion_main!(benches);
