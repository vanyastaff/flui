//! Perf scenarios: what a frame costs, counted rather than timed.
//!
//! Each scenario mounts the same small app — a fixed-height label over a lazy
//! 10 000-row list — applies one change, pumps, and asserts bounds on the
//! frame's [`FrameReport`]. The bounds are the budget and hold on every host;
//! the exact values are what `cargo xtask perf` compares against the checked-in
//! baseline (`crates/flui-widgets/perf/baseline.toml`), and it collects them
//! through `FLUI_PERF_OUT`: when that variable names a directory, each scenario
//! writes `<dir>/<scenario>.toml` **before** asserting, so a broken budget still
//! reports its numbers.
//!
//! Every record also carries what the scenario's measured frames asked of the
//! heap on the test thread, counted by this target's `#[global_allocator]`:
//! `allocations` (a `realloc` included), `reallocations` (the buffers among
//! them that outgrew their capacity) and `allocated_bytes`. Like the frame
//! counters they are a function of the tree and the change, not of the host,
//! so they ratchet the same way.
//!
//! A `perf_scaling_` scenario runs the same change on a small and a large tree
//! and asserts that the two cost the same: equal allocations here, and equal
//! instruction counts within a tolerance under `cargo xtask perf --instructions`,
//! which runs each size alone under callgrind (`FLUI_PERF_SIZE`) and counts
//! only what runs inside [`perf_measured`]. A frame whose cost grows with nodes
//! it does not touch fails there, though no counter of its work moves.
//!
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system, reads the environment and installs a counting
//! global allocator. It lives here rather
//! than in `flui-testing`, which owns the frame driver, because the app is a
//! widget tree: a `flui-testing` → `flui-widgets` dev edge would make every
//! crate that dev-depends on `flui-testing` a dependent of the widget catalog,
//! widening every change's CI scope.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fmt::Write as _;
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_painting::styling::Color;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::view::{OffsetWriteCause, ScrollOffsetWriter, ScrollPosition};
use flui_testing::widgets::SharedSignal;
use flui_testing::{
    FrameReport, HeadlessBinding, HeadlessHost, HeadlessWindow, MountOptions, MountOwners,
};
use flui_view::environment::{FocusTreeRoot, GestureArenaScope};
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, Column, Expanded, RawButton, RepaintBoundary, SingleChildScrollView, SizedBox,
    SliverList, Text, Viewport, app_root,
};

// `Cell<u64>` in a const-initialised thread-local has no drop glue and no
// lazy init, so the allocator reading it cannot itself allocate or run during
// TLS teardown. Per thread, so libtest's harness thread and any background
// thread a host starts add nothing to a scenario's count.
thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static REALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
}

/// What this thread has asked of the heap so far; zeros once its TLS is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeapCounts {
    /// Allocations, a growth or shrink by `realloc` included.
    allocations: u64,
    /// The `realloc`s among them: a buffer that outgrew its capacity.
    reallocations: u64,
    /// The bytes those allocations asked for.
    allocated_bytes: u64,
}

impl HeapCounts {
    fn now() -> Self {
        let read = |counter: &'static std::thread::LocalKey<Cell<u64>>| {
            counter.try_with(Cell::get).unwrap_or(0)
        };
        Self {
            allocations: read(&ALLOCATIONS),
            reallocations: read(&REALLOCATIONS),
            allocated_bytes: read(&ALLOCATED_BYTES),
        }
    }

    /// What happened between `earlier` and `self`.
    fn since(self, earlier: Self) -> Self {
        Self {
            allocations: self.allocations - earlier.allocations,
            reallocations: self.reallocations - earlier.reallocations,
            allocated_bytes: self.allocated_bytes - earlier.allocated_bytes,
        }
    }

    /// The counters a scenario records, under their baseline names.
    fn counts(self) -> [(&'static str, u64); 3] {
        [
            ("allocations", self.allocations),
            ("reallocations", self.reallocations),
            ("allocated_bytes", self.allocated_bytes),
        ]
    }
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
// `self.dealloc`, still `System`, and a growth counts as one allocation and
// one reallocation. `alloc_zeroed` keeps the default, through `self.alloc`.
#[expect(unsafe_code, reason = "a counting global allocator forwards to System")]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
        let size = layout.size() as u64;
        let _ = ALLOCATED_BYTES.try_with(|bytes| bytes.set(bytes.get().saturating_add(size)));
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
        let _ = REALLOCATIONS.try_with(|count| count.set(count.get().saturating_add(1)));
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

/// Runs a scenario's measured frames and returns their result with what
/// they asked of the heap.
///
/// The one window `cargo xtask perf --instructions` counts: it collects
/// instructions only while this function runs (callgrind's `--toggle-collect`
/// on its name), so it must stay out of line and keep its name.
#[inline(never)]
fn perf_measured<R>(frames: impl FnOnce() -> R) -> (R, HeapCounts) {
    let before = HeapCounts::now();
    let result = frames();
    (result, HeapCounts::now().since(before))
}

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);
const ROWS: usize = 10_000;
const ROW_HEIGHT: f64 = 20.0;
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;
/// The label's fixed height: the list below it is `HEIGHT - LABEL_HEIGHT`
/// tall whatever the text measures, so font metrics cannot move the band.
const LABEL_HEIGHT: f64 = 20.0;

/// The one stateful piece: a label whose change rebuilds only itself and the
/// text below it.
#[derive(Clone, StatefulView)]
struct Label {
    text: SharedSignal<String>,
}

struct LabelState {
    text: SharedSignal<String>,
}

impl StatefulView for Label {
    type State = LabelState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        LabelState {
            text: self.text.clone(),
        }
    }
}

impl ViewState<Label> for LabelState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.text.bind(ctx);
    }
    fn build(&self, _view: &Label, ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.text.get(ctx))
    }
}

/// The app: a fixed-height label over a lazy list of fixed-height rows,
/// scrolled by a shared position.
#[derive(Clone, StatelessView)]
struct PerfApp {
    label: SharedSignal<String>,
    position: ScrollPosition,
}

impl StatelessView for PerfApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Each row paints, so a retained row carries a picture layer that a
        // graft reuses; an empty row would retain nothing to count. The
        // viewport is passive: the scenario moves the shared position itself.
        let rows = SliverList::new(
            ROWS,
            ROW_HEIGHT,
            std::rc::Rc::new(|index| {
                (index < ROWS).then(|| {
                    RepaintBoundary::new()
                        .child(
                            SizedBox::new(WIDTH, ROW_HEIGHT)
                                .child(ColoredBox::new(Color::rgb(40, 80, 120))),
                        )
                        .boxed()
                })
            }),
        );
        let list = Viewport::new(self.position.clone(), (rows,));
        Column::new(vec![
            SizedBox::height(LABEL_HEIGHT)
                .child(Label {
                    text: self.label.clone(),
                })
                .boxed(),
            Expanded::new(list).boxed(),
        ])
    }
}

struct Mounted {
    binding: HeadlessBinding,
    writer: ScrollOffsetWriter,
}
impl Mounted {
    /// Pumps `frames` frames and sums their reports.
    fn pump_many(&mut self, frames: usize) -> FrameReport {
        let mut total = FrameReport::default();
        for _ in 0..frames {
            total += &self.binding.pump_frame_report(FRAME);
        }
        total
    }

    fn render_nodes(&self) -> usize {
        self.binding
            .pipeline_owner()
            .expect("tree-bound")
            .with(|owner: &PipelineOwner| owner.render_tree().len())
    }
}

/// Mounts [`PerfApp`] at 800×600 with semantics on.
fn mount() -> Mounted {
    let label = SharedSignal::new(String::from("label"));
    let (position, writer) = ScrollPosition::with_writer(0.0);
    let app = PerfApp { label, position };
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(binding.arena().clone(), FocusTreeRoot::new(app_root(app)));
    let _ = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(WIDTH, HEIGHT),
    );
    binding.enable_semantics().expect("tree-bound");
    Mounted { binding, writer }
}

/// Writes `report` and what the measured frames asked of the heap for `xtask perf`
/// when `FLUI_PERF_OUT` is set. Called before a scenario asserts, so a failing
/// budget still reports its numbers.
fn record(scenario: &str, report: &FrameReport, heap: HeapCounts) {
    let mut counts: Vec<(&str, u64)> = report.counters().into_iter().collect();
    counts.extend(heap.counts());
    record_counts(scenario, &counts);
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

fn counter(report: &FrameReport, name: &str) -> u64 {
    report
        .counters()
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| *value)
        .expect("a known counter name")
}

/// A full reassemble — every element rebuilt and every render object
/// re-laid-out and repainted, with semantics on — touches every phase, so
/// every counter but the retained-layer one moves. Removing any increment
/// site fails here.
#[test]
fn perf_counters_are_live_on_a_full_reassemble() {
    let mut app = mount();
    app.binding.perform_reassemble();
    app.binding.reassemble_render_tree();
    let (report, allocations) = perf_measured(|| app.binding.pump_frame_report(FRAME));
    record("full_reassemble", &report, allocations);

    for (name, value) in report.counters() {
        if name == "layers_reused" {
            continue;
        }
        assert!(
            value > 0,
            "{name} must move on a full reassemble: {report:#?}"
        );
    }
    assert_eq!(report.pipeline.frames_produced, 1, "{report:#?}");
}

/// Scrolling one screen of a 10 000-row lazy list lays out the newly visible
/// band, not the list.
#[test]
fn perf_scrolling_a_10k_list_one_screen_lays_out_only_the_band() {
    let mut app = mount();
    let band = app.render_nodes();

    app.writer
        .set_pixels(HEIGHT, OffsetWriteCause::Programmatic);
    // Two frames: a lazy band that first appears is built after that frame's
    // paint and lands on the next (the ListView first-frame divergence).
    let (report, allocations) = perf_measured(|| app.pump_many(2));
    record("list_10k_scroll_one_screen", &report, allocations);

    assert!(
        band < 400,
        "sanity: the mount built only a band of the 10 000 rows, got {band} render nodes"
    );
    let visible_rows = ((HEIGHT - LABEL_HEIGHT) / ROW_HEIGHT) as u64;
    let laid_out = counter(&report, "nodes_laid_out");
    assert!(
        laid_out >= visible_rows,
        "every newly visible row lays out ({visible_rows}), got {laid_out}: {report:#?}"
    );
    assert!(
        laid_out <= band as u64,
        "no more nodes lay out than one mounted band holds ({band}), got {laid_out}: {report:#?}"
    );
    assert!(report.pipeline.layout_passes >= 1, "{report:#?}");
    assert!(report.pipeline.nodes_painted >= 1, "{report:#?}");
    assert!(report.pipeline.frames_produced >= 1, "{report:#?}");
}

/// Rows under the sliding panel: enough painted content that a subtree
/// repaint and a layer patch differ visibly in `nodes_painted`.
const PANEL_ROWS: usize = 30;

/// One tick of a `SlideTransition` over a painted panel: the transition
/// follows the animation through a transform layer, so the tick rebuilds no
/// element and does not repaint the panel — only the layer moves.
#[test]
fn perf_a_slide_transition_tick_moves_a_layer_without_rebuilding() {
    use flui_animation::Animatable;
    use flui_animation::{Animation, AnimationController, Tween};
    use flui_objects::TranslationFraction;
    use flui_widgets::{Align, RepaintBoundary, SlideTransition};

    let controller = AnimationController::builder(Duration::from_millis(300)).build();
    let position: std::rc::Rc<dyn Animation<TranslationFraction>> = std::rc::Rc::new(
        Tween::new(
            TranslationFraction::new(-1.0, 0.0),
            TranslationFraction::ZERO,
        )
        .animate(std::rc::Rc::new(controller.clone()) as std::rc::Rc<dyn Animation<f64>>),
    );
    let panel = Column::new(
        (0..PANEL_ROWS)
            .map(|_| {
                SizedBox::new(WIDTH / 2.0, ROW_HEIGHT)
                    .child(ColoredBox::new(Color::rgb(40, 80, 120)))
                    .boxed()
            })
            .collect::<Vec<_>>(),
    );
    let app = Align::new(flui_painting::Alignment::TOP_LEFT)
        .child(RepaintBoundary::new().child(SlideTransition::new(position, panel)));
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(binding.arena().clone(), FocusTreeRoot::new(app_root(app)));
    let _ = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(WIDTH, HEIGHT),
    );
    binding.enable_semantics().expect("tree-bound");
    controller.set_value(0.25).expect("no build is running");
    binding.pump_frame(FRAME);

    let ((), allocations) = perf_measured(|| {
        controller.set_value(0.5).expect("no build is running");
        binding.pump_frame(FRAME);
    });
    let report = binding.last_frame_report().clone();
    record("slide_transition_tick", &report, allocations);

    assert_eq!(report.build.elements_built, 0, "{report:#?}");
    assert!(
        report.pipeline.nodes_painted < PANEL_ROWS as u64,
        "a tick must not repaint the panel's rows: {report:#?}"
    );
    assert_eq!(report.pipeline.nodes_laid_out, 0, "{report:#?}");
}

/// The two tree sizes a `perf_scaling_` scenario compares, by the name
/// `FLUI_PERF_SIZE` selects one with. Ten times apart, so a cost that follows
/// the tree shows up as a multiple rather than as noise.
const SCALING_SIZES: [(&str, usize); 2] = [("small", 500), ("large", 5_000)];

/// The frames a scaling scenario measures.
const SCALING_FRAMES: usize = 4;

/// A fixed-height label over `buttons` pressable rows in one eager column:
/// every row is mounted, laid out and painted once, so a frame that changes
/// only the label has the whole tree to pay for if it walks it.
#[derive(Clone, StatelessView)]
struct ButtonColumn {
    label: SharedSignal<String>,
    buttons: usize,
}

impl StatelessView for ButtonColumn {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let rows: Vec<_> = (0..self.buttons)
            .map(|row| {
                RawButton::new(Text::new(format!("Row {row}")))
                    .on_press(|_cx| {})
                    .boxed()
            })
            .collect();
        Column::new(vec![
            SizedBox::height(LABEL_HEIGHT)
                .child(Label {
                    text: self.label.clone(),
                })
                .boxed(),
            Expanded::new(
                SingleChildScrollView::new().child(RepaintBoundary::new().child(Column::new(rows))),
            )
            .boxed(),
        ])
    }
}

/// Mounts [`ButtonColumn`] on a headless host, settles it, and returns the
/// heap counts of [`SCALING_FRAMES`] label changes.
fn label_change_heap(buttons: usize) -> HeapCounts {
    let label = SharedSignal::new(String::from("label 0"));
    let mut host = HeadlessHost::new(HeadlessWindow::new(WIDTH as u32, HEIGHT as u32));
    host.attach(&ButtonColumn {
        label: label.clone(),
        buttons,
    })
    .expect("a fresh host takes a root");
    for _ in 0..3 {
        let _ = host.pump(FRAME);
    }
    let texts: Vec<String> = (0..SCALING_FRAMES)
        .map(|frame| format!("label {}", frame % 2 + 1))
        .collect();
    let ((), heap) = perf_measured(|| {
        for text in texts {
            label.set(text);
            let _ = host.pump(FRAME);
        }
    });
    heap
}

/// A label change costs the same over 500 rows as over 5 000: equal heap
/// counts here, and equal instructions within a tolerance under
/// `cargo xtask perf --instructions`. A frame that clears a table the mount
/// grew, or walks the tree to collect something only a few nodes report,
/// fails the second while every counter of its work stays put.
#[test]
fn perf_scaling_a_label_change_costs_the_same_on_any_tree_size() {
    let only = std::env::var("FLUI_PERF_SIZE").ok();
    let mut measured = Vec::new();
    for (name, buttons) in SCALING_SIZES {
        if only.as_deref().is_some_and(|selected| selected != name) {
            continue;
        }
        let heap = label_change_heap(buttons);
        record_counts(&format!("label_change_{name}"), &heap.counts());
        measured.push((buttons, heap));
    }
    assert!(
        !measured.is_empty(),
        "FLUI_PERF_SIZE names no size of {SCALING_SIZES:?}"
    );
    if let [(small_rows, small), (large_rows, large)] = measured[..] {
        assert_eq!(
            small, large,
            "{SCALING_FRAMES} label changes asked {small:?} of the heap over {small_rows} rows \
             and {large:?} over {large_rows}"
        );
    }
}

/// The rows a mount-cost scenario mounts: enough that the app around them
/// is noise in a per-row figure.
const MOUNT_ROWS: usize = 1_000;

/// What `MOUNT_ROWS` rows of one kind cost to mount.
struct MountCost {
    elements: usize,
    render_nodes: usize,
    heap: HeapCounts,
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
            BoxConstraints::tight(Size::new(WIDTH, HEIGHT)),
        );
        (laid.element_node_count(), laid.render_node_count())
    };
    let (live_before, heap_before) = (live_bytes(), HeapCounts::now());
    let mut host = HeadlessHost::new(HeadlessWindow::new(WIDTH as u32, HEIGHT as u32));
    host.attach(&Rows { row })
        .expect("a fresh host takes a root");
    let _ = host.pump(FRAME);
    let cost = MountCost {
        elements,
        render_nodes,
        heap: HeapCounts::now().since(heap_before),
        live_bytes: live_bytes().wrapping_sub(live_before),
    };
    drop(host);
    cost
}

/// What a `RawButton` adds to the `Text` it holds, per button: the four
/// numbers `docs/gaps.md` closes "An interactive widget costs eighteen
/// elements" on (ADR-0322). The `Text` alone is recorded beside them.
#[test]
fn perf_raw_button_over_text() {
    let button_row = |row: usize| {
        RawButton::new(Text::new(format!("Row {row}")))
            .on_press(|_cx| {})
            .boxed()
    };
    // What the process and the thread set up once (the font system, lazy
    // tables) is paid by a mount that is not measured.
    let _warm_up = mount_cost(button_row);
    let text = mount_cost(|row| Text::new(format!("Row {row}")).boxed());
    let button = mount_cost(button_row);
    let rows = MOUNT_ROWS as u64;
    let per_row = |with: u64, without: u64| with.saturating_sub(without).div_ceil(rows);
    let elements = per_row(button.elements as u64, text.elements as u64);
    let render_nodes = per_row(button.render_nodes as u64, text.render_nodes as u64);
    let allocations = per_row(button.heap.allocations, text.heap.allocations);
    let reallocations = per_row(button.heap.reallocations, text.heap.reallocations);
    let bytes = per_row(button.live_bytes, text.live_bytes);
    record_counts(
        "raw_button_over_text",
        &[
            ("elements", elements),
            ("render_nodes", render_nodes),
            ("allocations", allocations),
            ("reallocations", reallocations),
            ("bytes", bytes),
            ("text_allocations", text.heap.allocations / rows),
            ("text_reallocations", text.heap.reallocations / rows),
            ("text_bytes", text.live_bytes / rows),
        ],
    );

    // One element for the button and one for its behaviours; one render
    // object. The allocations and bytes are held by the baseline, which
    // only falls: the entry's numbers for them are 40 and 3 KB.
    assert!(
        elements <= 2,
        "a button adds {elements} elements to its child"
    );
    assert!(
        render_nodes <= 1,
        "a button adds {render_nodes} render nodes to its child"
    );
}

/// What a `Semantics` with one action adds to the `Text` it holds in a
/// window that collects no semantics: its element and render object, and no
/// action route (ADR-0322 §8).
#[test]
fn perf_semantics_action_over_text() {
    let annotated_row = |row: usize| {
        Semantics::new()
            .on_click(|_cx| {})
            .child(Text::new(format!("Row {row}")))
            .boxed()
    };
    let _warm_up = mount_cost(annotated_row);
    let text = mount_cost(|row| Text::new(format!("Row {row}")).boxed());
    let annotated = mount_cost(annotated_row);
    let rows = MOUNT_ROWS as u64;
    let per_row = |with: u64, without: u64| with.saturating_sub(without).div_ceil(rows);
    record_counts(
        "semantics_action_over_text",
        &[
            (
                "elements",
                per_row(annotated.elements as u64, text.elements as u64),
            ),
            (
                "render_nodes",
                per_row(annotated.render_nodes as u64, text.render_nodes as u64),
            ),
            (
                "allocations",
                per_row(annotated.heap.allocations, text.heap.allocations),
            ),
            (
                "reallocations",
                per_row(annotated.heap.reallocations, text.heap.reallocations),
            ),
            ("bytes", per_row(annotated.live_bytes, text.live_bytes)),
        ],
    );
}
