//! Process-isolated allocator witness for public committed transform queries.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};

thread_local! {
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
