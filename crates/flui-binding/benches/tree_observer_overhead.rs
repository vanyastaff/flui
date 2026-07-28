//! Overhead check on ADR-0040's zero-cost-when-off claim: what does the
//! tree-observation seam cost a frame with no observer installed, and what
//! does a live one add?
//!
//! Three scenarios over the same rebuild-heavy drive (keyed rotation of 64
//! render children through the production `build_scope` path each
//! iteration): observer absent (the `Option` null-check off path), a no-op
//! observer (emission + `catch_unwind` frame, empty body), and the
//! devtools counting inspector (the first real consumer).

// Bench binary: criterion's generated main has no docs (house precedent:
// the flui-view benches carry the same allow).
#![allow(missing_docs)]

use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use flui_devtools::inspector::InspectorCounters;
use flui_foundation::observe::TreeObserver;
use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_view::{BuildOwner, ElementTree, RebuildReason, RenderView, View, ViewExt};

#[derive(Clone)]
struct KeyedLeafBox {
    key: ValueKey<u32>,
}

impl KeyedLeafBox {
    fn new(tag: u32) -> Self {
        Self {
            key: ValueKey::new(tag),
        }
    }
}

impl RenderView for KeyedLeafBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
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
    ) {
    }
}

impl View for KeyedLeafBox {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct MultiBox {
    children: Vec<flui_view::BoxedView>,
}

impl MultiBox {
    fn keyed(order: &[u32]) -> Self {
        Self {
            children: order
                .iter()
                .copied()
                .map(|tag| KeyedLeafBox::new(tag).boxed())
                .collect(),
        }
    }
}

impl RenderView for MultiBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
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
    ) {
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

impl View for MultiBox {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

struct NoOpObserver;
impl TreeObserver for NoOpObserver {}

/// One rebuild-heavy iteration: rotate the 64 keyed children and drain
/// through `build_scope` (root rebuild + 64 keyed reconciles + reorders).
fn drive_rotation(
    owner: &mut BuildOwner,
    tree: &mut ElementTree,
    root: ElementId,
    order: &mut [u32],
) {
    order.rotate_left(1);
    tree.update(
        root,
        &MultiBox::keyed(order),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root, 0, RebuildReason::ParentUpdate);
    owner.build_scope(tree);
}

fn setup(
    observer: Option<Arc<dyn TreeObserver>>,
) -> (BuildOwner, ElementTree, ElementId, Vec<u32>) {
    let mut owner = BuildOwner::new();
    if let Some(observer) = observer {
        owner.set_tree_observer(observer);
    }
    let mut tree = ElementTree::new();
    let order: Vec<u32> = (0..64).collect();
    let root = tree.mount_root(&MultiBox::keyed(&order), &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    (owner, tree, root, order)
}

fn bench_observer_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("tree_observer/rebuild_heavy_64_children");

    group.bench_function("observer_absent", |b| {
        let (mut owner, mut tree, root, mut order) = setup(None);
        b.iter(|| {
            drive_rotation(&mut owner, &mut tree, root, &mut order);
            black_box(tree.len());
        });
    });

    group.bench_function("noop_observer", |b| {
        let (mut owner, mut tree, root, mut order) = setup(Some(Arc::new(NoOpObserver)));
        b.iter(|| {
            drive_rotation(&mut owner, &mut tree, root, &mut order);
            black_box(tree.len());
        });
    });

    group.bench_function("counting_inspector", |b| {
        let counters = Arc::new(InspectorCounters::new());
        let (mut owner, mut tree, root, mut order) =
            setup(Some(Arc::clone(&counters) as Arc<dyn TreeObserver>));
        b.iter(|| {
            drive_rotation(&mut owner, &mut tree, root, &mut order);
            black_box(counters.snapshot().rebuilds);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_observer_overhead);
criterion_main!(benches);
