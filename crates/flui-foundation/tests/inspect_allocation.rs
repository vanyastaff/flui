//! Describing a node allocates only if its sink does (ADR-0242 §1).
//!
//! A counting sink walks a derived description with every field kind, a sequence and an
//! untyped field, under a counting `#[global_allocator]`. That allocator is process-wide,
//! which is why this file is a test target of its own rather than a module of
//! `tests/main.rs`. Counters are per-thread, so libtest's other threads add nothing to the
//! measured window, and the snapshot sink is the positive control.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::num::NonZeroU32;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::{EdgeInsets, Matrix4, Offset, Rect, Size};
use flui_foundation::inspect::{
    Field, Fields, Inspect, InspectId, InspectTree, Links, Meta, NodeView, Rgba, Seq, Sink,
    SnapshotQuery, Value, snapshot,
};

// `Cell<usize>` in a const-initialised thread-local has no drop glue and no lazy init, so
// reading it cannot itself allocate or run during TLS teardown.
thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
}

fn bump() {
    let _ = ALLOC_COUNT.try_with(|count| count.set(count.get().wrapping_add(1)));
}

fn allocations() -> usize {
    ALLOC_COUNT.try_with(Cell::get).unwrap_or(0)
}

struct CountingAllocator;

// SAFETY: `alloc` and `dealloc` forward unchanged to `System`, so every pointer is freed by
// the allocator that produced it. The default `realloc` and `alloc_zeroed` go through
// `self.alloc`, so they are counted too.
#[expect(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump();
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are the caller's own, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Debug)]
struct Opaque;

#[derive(Inspect)]
enum Fit {
    Cover,
}

#[derive(Inspect)]
struct Everything {
    flag: bool,
    count: i32,
    index: usize,
    #[inspect(unit = fraction, default = 1.0)]
    opacity: f64,
    label: String,
    fit: Fit,
    #[inspect(unit = logical_px)]
    size: Size,
    offset: Offset,
    rect: Rect,
    tint: Rgba,
    #[inspect(default)]
    padding: EdgeInsets,
    transform: Matrix4,
    delay: Duration,
    target: RenderId,
    missing: Option<u8>,
    stops: Vec<f64>,
    #[inspect(debug)]
    opaque: Opaque,
}

/// Counts what it receives and touches every value, keeping nothing.
#[derive(Default)]
struct Counting {
    fields: usize,
    items: usize,
}

impl Sink for Counting {
    fn field(&mut self, field: Field<'_>) {
        black_box(field.value);
        black_box(field.meta);
        self.fields += 1;
    }

    fn seq(&mut self, _: &'static str, meta: Meta<'_>, items: &dyn Seq) {
        black_box(meta);
        items.each(&mut |item| {
            black_box(item);
            self.items += 1;
        });
        self.fields += 1;
    }
}

fn everything() -> Everything {
    Everything {
        flag: true,
        count: -3,
        index: 7,
        opacity: 0.5,
        label: "Save".to_owned(),
        fit: Fit::Cover,
        size: Size::new(360.0, 48.0),
        offset: Offset::new(8.0, 8.0),
        rect: Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        tint: Rgba::new(1.0, 0.0, 0.0, 1.0),
        padding: EdgeInsets::all(4.0),
        transform: Matrix4::translation(1.0, 2.0, 0.0),
        delay: Duration::from_millis(16),
        target: RenderId::new_gen(3, NonZeroU32::MIN),
        missing: None,
        stops: vec![0.0, 0.5, 1.0],
        opaque: Opaque,
    }
}

#[test]
fn a_counting_sink_walks_a_description_without_allocating() {
    let object = everything();
    let mut sink = Counting::default();
    let before = allocations();
    for _ in 0..100 {
        object.inspect(&mut Fields::new(black_box(&mut sink)));
        black_box(object.kind());
    }
    let allocated = allocations() - before;
    assert_eq!(
        (sink.fields, sink.items),
        (1_700, 300),
        "every field and sequence item reached the sink"
    );
    assert_eq!(allocated, 0, "describing allocated {allocated} times");
}

struct One(Everything);

impl InspectTree for One {
    type Node = Everything;

    fn root(&self) -> Option<InspectId> {
        Some(RenderId::new_gen(1, NonZeroU32::MIN).into())
    }

    fn node(&self, _: InspectId) -> Option<NodeView<'_, Everything>> {
        Some(NodeView {
            object: &self.0,
            rect: None,
            links: Links::default(),
        })
    }

    fn children(&self, _: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        core::iter::empty()
    }
}

#[test]
fn the_snapshot_sink_is_where_allocation_happens() {
    let tree = One(everything());
    let before = allocations();
    let node = snapshot(&tree, SnapshotQuery::default()).expect("one node");
    assert!(
        allocations() > before,
        "the owned snapshot allocates, so the counter sees this thread's allocations"
    );
    assert_eq!(node.fields.len(), 17);
    assert_eq!(node.get::<f64>("opacity"), Ok(0.5));
    assert_eq!(node.debug_fields, 1);
    let _ = Value::None;
}
