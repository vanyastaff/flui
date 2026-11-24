//! Process-isolated allocator witness for warm committed transform queries
//! and hit tests.

use std::hint::black_box;

use flui_foundation::geometry::Size;
use flui_foundation::testing::heap::{CountingAllocator, HeapCounts};
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner, testing::SceneHost};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn committed_transform_queries_reuse_ancestry_storage() {
    for depth in [1, 12, 100] {
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let root = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
        let mut parent = root;
        for _ in 1..depth {
            parent = owner
                .insert_child_render_object(parent, Box::new(RenderPadding::all(1.0)))
                .expect("padding attaches");
        }
        let leaf = owner
            .insert_child_render_object(parent, Box::new(RenderColoredBox::red(40.0, 40.0)))
            .expect("leaf attaches");
        assert_eq!(
            owner.transform_to(leaf, root),
            None,
            "a query cannot invent a committed size"
        );
        owner.set_root_constraints(Some(BoxConstraints::loose(Size::new(1024.0, 1024.0))));
        owner
            .layout_pass()
            .run_layout()
            .expect("deep tree lays out");
        let transform = owner.transform_to(leaf, root).expect("committed ancestry");
        assert_eq!(
            transform.transform_point(3.0, 5.0),
            (3.0 + f64::from(depth), 5.0 + f64::from(depth))
        );
        // Storage serves a query made through any phase's pass, as input
        // queries do between frames; no fresh owner or test-only seam.
        let owner = owner
            .layout_pass()
            .into_compositing()
            .into_paint()
            .into_semantics();
        let before = HeapCounts::now().allocations;
        for _ in 0..32 {
            black_box(owner.transform_to(black_box(leaf), black_box(root)));
            black_box(owner.transform_to(black_box(root), black_box(leaf)));
        }
        let requests = HeapCounts::now().allocations - before;
        assert_eq!(
            requests, 0,
            "warm transform queries allocate at depth {depth}"
        );
    }
}

/// Places child `i` at `(i * 50, 0)`.
#[derive(Debug)]
struct RowFlowDelegate;

impl flui_rendering::delegates::FlowDelegate for RowFlowDelegate {
    fn size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn constraints_for_child(&self, _index: usize, constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::loose(constraints.biggest())
    }

    fn paint_children(&self, context: &mut flui_rendering::delegates::FlowPaintingContext<'_, '_>) {
        for index in 0..context.child_count() {
            context.paint_child(
                index,
                flui_foundation::geometry::Matrix4::translation(index as f64 * 50.0, 0.0, 0.0),
            );
        }
    }

    fn should_relayout(&self, _old: &dyn flui_rendering::delegates::FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old: &dyn flui_rendering::delegates::FlowDelegate) -> bool {
        false
    }
}

/// A pointer moving over a painted tree hit-tests it without allocating: the
/// walk borrows each node's children, a flow reads what it painted, and a
/// result cleared between moves keeps its storage — through a flow, a
/// padding, a transform and a row of boxes.
#[test]
fn warm_hit_tests_reuse_their_result_storage() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::PointerKind;
    use flui_interaction::pointer::HitTestResult;
    use flui_objects::{RenderFlex, RenderFlow, RenderTransform};

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root =
        owner.set_root_render_object(Box::new(RenderFlow::new(std::rc::Rc::new(RowFlowDelegate))));
    owner
        .insert_child_render_object(root, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("first flow child attaches");
    let padding = owner
        .insert_child_render_object(root, Box::new(RenderPadding::all(1.0)))
        .expect("padding attaches");
    let transform = owner
        .insert_child_render_object(
            padding,
            Box::new(
                RenderTransform::scale(2.0, 2.0).with_alignment(flui_painting::Alignment::TOP_LEFT),
            ),
        )
        .expect("transform attaches");
    let row = owner
        .insert_child_render_object(transform, Box::new(RenderFlex::row()))
        .expect("row attaches");
    let mut leaf = None;
    for _ in 0..4 {
        leaf = Some(
            owner
                .insert_child_render_object(row, Box::new(RenderColoredBox::red(10.0, 10.0)))
                .expect("row child attaches"),
        );
    }
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(400.0, 400.0))));
    let mut scene = SceneHost::new();
    let frame = scene.run_frame(&mut owner);
    frame.expect("the tree paints");

    // The last row box, scaled 2x from the padding's corner at (51, 1).
    let position = Offset::new(51.0 + 2.0 * 35.0, 1.0 + 2.0 * 5.0);
    let mut result = HitTestResult::new();
    assert!(owner.hit_test(position, PointerKind::Mouse, &mut result));
    assert_eq!(
        result.path().first().map(|entry| entry.target),
        leaf,
        "the position is over the last row box"
    );

    let before = HeapCounts::now().allocations;
    for _ in 0..32 {
        black_box(HitTestResult::new());
        result.clear();
        black_box(owner.hit_test(black_box(position), PointerKind::Mouse, &mut result));
    }
    let requests = HeapCounts::now().allocations - before;
    assert_eq!(requests, 0, "warm hit tests allocate");
}

/// Removing a subtree reuses the id lists and the node buffer the last removal
/// grew: an item scrolled off a list, removed frame after frame, asks the heap
/// for nothing once its first removal has sized them.
#[test]
fn a_warm_subtree_removal_allocates_nothing() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
    let mut removals = Vec::new();
    for _ in 0..3 {
        let item = owner
            .insert_child_render_object(root, Box::new(RenderPadding::all(2.0)))
            .expect("item attaches");
        let mut parent = item;
        for _ in 0..6 {
            parent = owner
                .insert_child_render_object(parent, Box::new(RenderPadding::all(1.0)))
                .expect("padding attaches");
        }
        owner
            .insert_child_render_object(parent, Box::new(RenderColoredBox::red(4.0, 4.0)))
            .expect("leaf attaches");

        let before = HeapCounts::now().allocations;
        let removed = owner.remove_render_object(black_box(item));
        removals.push((removed, HeapCounts::now().allocations - before));
    }
    assert!(
        removals.iter().all(|&(removed, _)| removed == 8),
        "{removals:?}"
    );
    assert_eq!(
        removals.last().map(|&(_, allocations)| allocations),
        Some(0),
        "a warm removal allocated: {removals:?}"
    );
}
