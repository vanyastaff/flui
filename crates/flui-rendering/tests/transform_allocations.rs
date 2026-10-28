//! Process-isolated allocator witness for warm committed transform queries
//! and hit tests.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};

thread_local! {
    #[cfg_attr(target_os = "android", expect(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

fn record() {
    let _ = ALLOCATIONS.try_with(|counter| counter.set(counter.get().saturating_add(1)));
}

// SAFETY: every operation delegates the caller's original pointer, layout and
// requested size to System; recording does not allocate or retain any pointer.
#[expect(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the caller supplied the allocation layout required by System.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the caller supplied the allocation layout required by System.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        // SAFETY: System owns the pointer; the caller supplied its layout and
        // valid replacement size, forwarded without modification.
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: System owns the pointer with this exact layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

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
        let mut owner = owner.into_layout();
        owner.run_layout().expect("deep tree lays out");
        let transform = owner.transform_to(leaf, root).expect("committed ancestry");
        assert_eq!(
            transform.transform_point(3.0, 5.0),
            (3.0 + f64::from(depth), 5.0 + f64::from(depth))
        );
        // Storage follows this presentation through phase changes, just as
        // input queries do between frames; no fresh owner or test-only seam.
        let owner = owner
            .into_compositing()
            .into_paint()
            .into_semantics()
            .finish();
        let before = ALLOCATIONS.with(Cell::get);
        for _ in 0..32 {
            black_box(owner.transform_to(black_box(leaf), black_box(root)));
            black_box(owner.transform_to(black_box(root), black_box(leaf)));
        }
        let requests = ALLOCATIONS.with(Cell::get) - before;
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
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
    use flui_objects::{RenderFlex, RenderFlow, RenderTransform};
    use flui_rendering::hit_testing::HitTestResult;

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
    let (owner, frame) = owner.run_frame();
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

    let before = ALLOCATIONS.with(Cell::get);
    for _ in 0..32 {
        black_box(HitTestResult::new());
        result.clear();
        black_box(owner.hit_test(black_box(position), PointerKind::Mouse, &mut result));
    }
    let requests = ALLOCATIONS.with(Cell::get) - before;
    assert_eq!(requests, 0, "warm hit tests allocate");
}
