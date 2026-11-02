//! Process-isolated allocator witness: the frame path's tracing costs nothing at a level that
//! is not enabled.
//!
//! Spans and events at DEBUG and TRACE sit on the layout path. A subscriber that admits INFO
//! only must leave the allocation count of a relayout where a subscriber-free run puts it, and a
//! subscriber that admits TRACE and formats the fields must raise it, which shows the witness
//! sees the instrumentation at all.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fmt::Write as _,
};

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Dispatch, Event, Level, Metadata, Subscriber};

thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
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

/// Admits events and spans up to `ceiling`; a subscriber that formats what it admits allocates.
struct FormattingSubscriber {
    ceiling: Level,
}

struct Format(String);

impl Visit for Format {
    fn record_debug(&mut self, field: &Field, value: &dyn core::fmt::Debug) {
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl Subscriber for FormattingSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= self.ceiling
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let mut text = Format(String::new());
        attributes.record(&mut text);
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut text = Format(String::new());
        event.record(&mut text);
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// Allocations of eight relayouts of a warm three-deep tree under `dispatch`.
fn relayout_allocations(dispatch: &Dispatch) -> usize {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
    let middle = owner
        .insert_child_render_object(root, Box::new(RenderPadding::all(1.0)))
        .expect("padding attaches");
    owner
        .insert_child_render_object(middle, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("leaf attaches");
    owner.set_root_constraints(Some(BoxConstraints::loose(Size::new(1024.0, 1024.0))));
    let mut owner = owner.into_layout();
    tracing::dispatcher::with_default(dispatch, || {
        for _ in 0..2 {
            owner.mark_needs_layout(root);
            owner.run_layout().expect("the tree lays out");
        }
        let mut total = 0;
        for _ in 0..8 {
            owner.mark_needs_layout(root);
            let before = ALLOCATIONS.with(Cell::get);
            owner.run_layout().expect("the tree lays out");
            total += ALLOCATIONS.with(Cell::get) - before;
        }
        total
    })
}

#[test]
fn a_level_that_is_not_enabled_adds_no_allocation_to_a_relayout() {
    let none = relayout_allocations(&Dispatch::none());
    let info = relayout_allocations(&Dispatch::new(FormattingSubscriber {
        ceiling: Level::INFO,
    }));
    let trace = relayout_allocations(&Dispatch::new(FormattingSubscriber {
        ceiling: Level::TRACE,
    }));
    assert_eq!(
        info, none,
        "DEBUG and TRACE instrumentation allocated while only INFO was enabled"
    );
    assert!(
        trace > none,
        "the witness did not see the instrumentation: {trace} allocations with TRACE, {none} without"
    );
}
