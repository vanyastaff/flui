//! What a Material control costs to mount, counted rather than timed.
//!
//! The control-cost scenarios of `crates/flui-widgets/tests/perf.rs`
//! (`perf_raw_button_over_text`), for the controls of this catalog: each one
//! mounts `MOUNT_ROWS` controls over a `Text` and records, per control, what
//! it adds to the `Text` it holds. ADR-0322 sets the threshold a control on
//! the catalogs' interactive node stays within: 2 elements and 1 render node
//! for its behaviours, beside the elements of its own decoration.
//!
//! The allocations and bytes are held by `perf/baseline/<scenario>.toml`,
//! which only falls: a scenario fails when it costs more than its file says.
//! `FLUI_PERF_OUT=<dir>` writes each record to `<dir>/<scenario>.toml`
//! before asserting, and a lower record is copied into the baseline by hand.
//!
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system and installs a counting global allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fmt::Write as _;
use std::time::Duration;

use flui_material::InkWell;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Column, SingleChildScrollView, Text};
use flui_testing::{HeadlessHost, HeadlessWindow};

// `Cell<u64>` in a const-initialised thread-local has no drop glue and no
// lazy init, so the allocator reading it cannot itself allocate or run during
// TLS teardown. Per thread, so libtest's harness thread and any background
// thread a host starts add nothing to a scenario's count.
thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn allocations() -> u64 {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}

/// This thread's live heap bytes: allocated minus freed, wrapping, so only a
/// difference of two readings means anything.
fn live_bytes() -> u64 {
    LIVE_BYTES.try_with(Cell::get).unwrap_or(0)
}

struct CountingAllocator;

// SAFETY: `alloc`/`dealloc` forward unchanged to `System`, so every pointer
// is freed by the allocator that produced it. `realloc` is `GlobalAlloc`'s
// default spelled out so it can be counted: `self.alloc` + copy +
// `self.dealloc`, still `System`. `alloc_zeroed` keeps the default, through
// `self.alloc`.
#[expect(unsafe_code, reason = "a counting global allocator forwards to System")]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
        let size = layout.size() as u64;
        let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_add(size)));
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = LIVE_BYTES.try_with(|live| live.set(live.get().wrapping_sub(layout.size() as u64)));
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `realloc`'s contract makes `new_size` with `layout.align()`
        // a valid layout (non-zero, not overflowing `isize` once rounded).
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        // SAFETY: `new_layout` is valid, as above.
        let new_ptr = unsafe { self.alloc(new_layout) };
        if !new_ptr.is_null() {
            // SAFETY: both blocks are live and distinct, each at least
            // `min(old, new)` bytes long; `ptr` was allocated here with
            // `layout`, as `realloc`'s contract requires.
            unsafe {
                std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

/// The rows a scenario mounts: enough that the app around them is noise in
/// a per-row figure.
const MOUNT_ROWS: usize = 1_000;

/// What `MOUNT_ROWS` rows of one kind cost to mount.
struct MountCost {
    elements: usize,
    render_nodes: usize,
    allocations: u64,
    live_bytes: u64,
}

/// `MOUNT_ROWS` rows in one eager column, each made by `row`.
#[derive(Clone, StatelessView)]
struct Rows {
    row: fn(usize) -> BoxedView,
}

impl StatelessView for Rows {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SingleChildScrollView::new().child(Column::new(
            (0..MOUNT_ROWS).map(self.row).collect::<Vec<BoxedView>>(),
        ))
    }
}

/// Counts what a tree of [`Rows`] holds, then mounts it on a headless host
/// with semantics off, pumps its first frame, and measures what that
/// allocated and what stays live.
fn mount_cost(row: fn(usize) -> BoxedView) -> MountCost {
    let (elements, render_nodes) = {
        let mut laid = flui_testing::widgets::lay_out(
            Rows { row },
            flui_testing::widgets::tight(WIDTH, HEIGHT),
        );
        (laid.element_node_count(), laid.render_node_count())
    };
    let (live_before, allocations_before) = (live_bytes(), allocations());
    let mut host = HeadlessHost::new(HeadlessWindow::new(WIDTH as u32, HEIGHT as u32));
    host.attach(&Rows { row })
        .expect("a fresh host takes a root");
    let _ = host.pump(FRAME);
    let cost = MountCost {
        elements,
        render_nodes,
        allocations: allocations() - allocations_before,
        live_bytes: live_bytes().wrapping_sub(live_before),
    };
    drop(host);
    cost
}

/// What `control` adds to the `Text` it holds, per control, recorded as
/// `scenario` and held to its baseline file.
fn control_over_text(scenario: &str, control: fn(usize) -> BoxedView) -> [(&'static str, u64); 4] {
    // What the process and the thread set up once (the font system, lazy
    // tables) is paid by a mount that is not measured.
    let _warm_up = mount_cost(control);
    let text = mount_cost(|row| Text::new(format!("Row {row}")).boxed());
    let with = mount_cost(control);
    let rows = MOUNT_ROWS as u64;
    let per_row = |with: u64, without: u64| with.saturating_sub(without).div_ceil(rows);
    let counts = [
        (
            "elements",
            per_row(with.elements as u64, text.elements as u64),
        ),
        (
            "render_nodes",
            per_row(with.render_nodes as u64, text.render_nodes as u64),
        ),
        ("allocations", per_row(with.allocations, text.allocations)),
        ("bytes", per_row(with.live_bytes, text.live_bytes)),
    ];
    record_counts(scenario, &counts);
    let baseline = baseline(scenario);
    for (name, value) in counts {
        let held = baseline
            .iter()
            .find(|(counter, _)| *counter == name)
            .map_or_else(
                || panic!("{scenario}.toml holds no `{name}`"),
                |(_, held)| *held,
            );
        assert!(
            value <= held,
            "{scenario}: {name} rose to {value} over the baseline's {held}"
        );
    }
    counts
}

/// The counters of `perf/baseline/<scenario>.toml`.
fn baseline(scenario: &str) -> Vec<(String, u64)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("perf/baseline")
        .join(format!("{scenario}.toml"));
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    body.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            let value = value.trim().parse::<u64>().ok()?;
            Some((name.trim().to_owned(), value))
        })
        .collect::<Vec<(String, u64)>>()
}

/// Writes `counts` as `<FLUI_PERF_OUT>/<scenario>.toml` when that variable is set.
fn record_counts(scenario: &str, counts: &[(&str, u64)]) {
    let Some(dir) = std::env::var_os("FLUI_PERF_OUT") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("create FLUI_PERF_OUT");
    let mut body = format!("[{scenario}]\n");
    for (name, value) in counts {
        writeln!(body, "{name} = {value}").expect("writing to a String cannot fail");
    }
    std::fs::write(dir.join(format!("{scenario}.toml")), body).expect("write perf record");
}

/// What an `InkWell` adds to the `Text` it holds: its behaviours on the
/// interactive node, and its ink, an idle `CustomPaint`.
#[test]
fn perf_ink_well_over_text() {
    let [elements, render_nodes, ..] = control_over_text("ink_well_over_text", |row| {
        InkWell::new(Text::new(format!("Row {row}")))
            .on_tap(|_ctx| {})
            .boxed()
    });
    // The well's element, its node's element and the ink's; the node's
    // render object and the ink's.
    assert!(elements.1 <= 3, "a well adds {} elements", elements.1);
    assert!(
        render_nodes.1 <= 2,
        "a well adds {} render nodes",
        render_nodes.1
    );
}
