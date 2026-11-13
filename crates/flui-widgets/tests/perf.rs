//! Perf scenarios: what a frame costs, counted rather than timed.
//!
//! Each scenario mounts the same small app — a fixed-height label over a lazy
//! 10 000-row list — applies one change, pumps, and asserts bounds on the
//! frame's [`FrameReport`]. The bounds are the budget and hold on every host;
//! the exact values are what `cargo xtask perf` compares against the checked-in
//! baseline (one file per scenario in `crates/flui-widgets/perf/baseline/`),
//! and it collects them
//! through `FLUI_PERF_OUT`: when that variable names a directory, each scenario
//! writes `<dir>/<scenario>.toml` **before** asserting, so a broken budget still
//! reports its numbers.
//!
//! Every record also carries what the scenario's measured frames asked of the
//! heap on the test thread, counted by this target's `#[global_allocator]`:
//! `allocations` (a `realloc` included), `reallocations` (the buffers among
//! them that outgrew their capacity) and `allocated_bytes`. Like the frame
//! counters they are a function of the tree and the change, not of the host;
//! they only have to stay at or under the baseline, which `--bless` ratchets.
//!
//! A `perf_scaling_` scenario runs the same change on a small and a large tree
//! and asserts that the two cost the same: equal allocations here, and equal
//! instruction counts within a tolerance under `cargo xtask perf --instructions`,
//! which runs each size alone under callgrind (`FLUI_PERF_SIZE`) and counts
//! only what runs inside [`perf_measured`]. A frame whose cost grows with nodes
//! it does not touch fails there, though no counter of its work moves.
//!
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system, reads the environment and installs the kit's counting
//! global allocator (`flui_testing::heap`). It lives here rather
//! than in `flui-testing`, which owns the frame driver, because the app is a
//! widget tree: a `flui-testing` → `flui-widgets` dev edge would make every
//! crate that dev-depends on `flui-testing` a dependent of the widget catalog,
//! widening every change's CI scope.

use std::fmt::Write as _;
use std::time::Duration;

use flui_painting::styling::Color;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::view::{OffsetWriteCause, ScrollOffsetWriter, ScrollPosition};
use flui_testing::control_cost::control_over_text;
use flui_testing::heap::{CountingAllocator, HeapCounts};
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

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// The heap counters a frame scenario records, under their baseline names.
fn heap_counts(heap: HeapCounts) -> [(&'static str, u64); 3] {
    [
        ("allocations", heap.allocations),
        ("reallocations", heap.reallocations),
        ("allocated_bytes", heap.allocated_bytes),
    ]
}

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
    // A frame scenario compares what its frames ask of the heap; what they
    // leave live is a mount figure, so it is left out of every comparison.
    let heap = HeapCounts {
        live_bytes: 0,
        ..HeapCounts::now().since(before)
    };
    (result, heap)
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

thread_local! {
    /// Rows [`PerfApp`]'s list has built on this thread, so a scenario can see
    /// that a scroll reached new rows. A thread-local rather than a captured
    /// handle: the row builder stays a zero-sized closure, so counting adds
    /// nothing to what a rebuild of the app allocates.
    static ROWS_BUILT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
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
                    ROWS_BUILT.with(|built| built.set(built.get() + 1));
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
    counts.extend(heap_counts(heap));
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
/// every counter moves but three: the retained-layer one, the full-damage
/// one, which only a frame with no base on screen moves, and the depth one,
/// which only a reparented boundary moves. Removing any other increment
/// site fails here.
#[test]
fn perf_counters_are_live_on_a_full_reassemble() {
    let mut app = mount();
    app.binding.perform_reassemble();
    app.binding.reassemble_render_tree();
    let (report, allocations) = perf_measured(|| app.binding.pump_frame_report(FRAME));
    record("full_reassemble", &report, allocations);

    for (name, value) in report.counters() {
        if matches!(
            name,
            "layers_reused" | "damage_full" | "scene_depth_updates"
        ) {
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

/// One-screen scrolls a scenario pumps before it measures a steady one. A
/// margin, not a measured threshold: the scenario asserts that the two scrolls
/// after it cost the same within [`STEADY_SLACK`], so a warm-up that runs past
/// it fails there.
const WARM_SCROLLS: u32 = 8;

/// How many allocations or reallocations two steady scrolls may differ by: the
/// rows a band rebuilds are not the same rows from one screen to the next.
const STEADY_SLACK: u64 = 2;

/// A one-screen scroll after [`WARM_SCROLLS`] others, driven through the UI
/// runtime's frame transaction: what every scroll after the first few asks of
/// the heap, and the baseline's ratchet fails a change that makes it cost
/// more. The first-scroll row above runs on another driver, so the two rows'
/// counts are not comparable.
#[test]
fn perf_a_warm_scroll_reuses_what_the_earlier_ones_kept() {
    let (position, writer) = ScrollPosition::with_writer(0.0);
    let app = PerfApp {
        label: SharedSignal::new(String::from("label")),
        position,
    };
    let mut host = HeadlessHost::new(HeadlessWindow::new(WIDTH as u32, HEIGHT as u32));
    host.attach(&app).expect("a fresh host takes a root");
    host.enable_semantics();
    let visible_rows = ((HEIGHT - LABEL_HEIGHT) / ROW_HEIGHT) as u64;
    // One frame a screen: the runtime's frame transaction lands the lazy band
    // in the frame that shows it. The frame after must cost what a later, idle
    // one does and build no row, so no work of the scroll slips past the frame
    // it measures.
    let rows_built = || ROWS_BUILT.with(std::cell::Cell::get);
    let mut scroll_to = |screen: u32| {
        writer.set_pixels(HEIGHT * f64::from(screen), OffsetWriteCause::Programmatic);
        let before = rows_built();
        let ((), heap) = perf_measured(|| {
            let _ = host.pump(FRAME);
        });
        let built = rows_built() - before;
        let ((), after) = perf_measured(|| {
            let _ = host.pump(FRAME);
        });
        let ((), idle) = perf_measured(|| {
            let _ = host.pump(FRAME);
        });
        assert!(
            rows_built() - before == built
                && after.allocations == idle.allocations
                && after.allocated_bytes == idle.allocated_bytes,
            "screen {screen}'s scroll did work after the frame it measures: \
             the next frame asked {after:?} of the heap against an idle {idle:?}"
        );
        (heap, built)
    };
    for screen in 0..=WARM_SCROLLS {
        scroll_to(screen);
    }
    let (first, first_built) = scroll_to(WARM_SCROLLS + 1);
    let (next, next_built) = scroll_to(WARM_SCROLLS + 2);
    record_counts("list_10k_scroll_warm", &heap_counts(first));

    assert!(
        first_built >= visible_rows && next_built >= visible_rows,
        "a warm scroll builds every newly visible row ({visible_rows}): built {first_built}, then {next_built}"
    );
    assert!(
        first.allocations.abs_diff(next.allocations) <= STEADY_SLACK
            && first.reallocations.abs_diff(next.reallocations) <= STEADY_SLACK,
        "a warm scroll is still warming up: one asked {first:?} of the heap, the next {next:?}"
    );
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
        .child(RepaintBoundary::new().child(SlideTransition::new(position).child(panel)));
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(binding.arena().clone(), FocusTreeRoot::new(app_root(app)));
    let _ = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(WIDTH, HEIGHT),
    );
    binding.enable_semantics().expect("tree-bound");
    // The scenario counts the tick itself, not the debug check that measures
    // a re-placement against a rebuild.
    binding
        .pipeline_owner()
        .expect("mounted")
        .with_mut(|owner| owner.set_semantics_placement_check(false));
    // The first frame assembles the semantics tree; the first tick after it
    // grows the buffers a moved subtree's pass keeps. The measured tick is
    // the steady one every later frame of the slide repeats.
    for value in [0.25, 0.375] {
        controller.set_value(value).expect("no build is running");
        binding.pump_frame(FRAME);
    }

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
    assert_eq!(
        report.scene.scene_depth_updates, 0,
        "a slide moves the panel's node without reparenting it: {report:#?}"
    );
    assert_eq!(report.scene.damage_full, 0, "{report:#?}");
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
                    .on_pressed(|_ctx| {})
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
        record_counts(&format!("label_change_{name}"), &heap_counts(heap));
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

/// What a `RawButton` adds to the `Text` it holds, per button: the four
/// numbers `docs/gaps.md` closes "An interactive widget costs eighteen
/// elements" on (ADR-0322). The `Text` alone is recorded beside them.
#[test]
fn perf_raw_button_over_text() {
    let cost = control_over_text(|row| {
        RawButton::new(Text::new(format!("Row {row}")))
            .on_pressed(|_ctx| {})
            .boxed()
    });
    let mut counts = cost.counts().to_vec();
    counts.extend([
        ("text_allocations", cost.bare.allocations),
        ("text_reallocations", cost.bare.reallocations),
        ("text_bytes", cost.bare.live_bytes),
    ]);
    record_counts("raw_button_over_text", &counts);

    // One element for the button and one for its behaviours; one render
    // object. The allocations and bytes are held by the baseline, which
    // only falls: the entry's numbers for them are 40 and 3 KB.
    assert!(
        cost.elements <= 2,
        "a button adds {} elements to its child",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 1,
        "a button adds {} render nodes to its child",
        cost.render_nodes
    );
}

/// A caret standing in for a text field's: a thin box under a repaint
/// boundary of its own, shown or hidden by a signal, as a blink toggles it.
#[derive(Clone, StatefulView)]
struct Caret {
    shown: SharedSignal<bool>,
}

struct CaretState {
    shown: SharedSignal<bool>,
}

impl StatefulView for Caret {
    type State = CaretState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        CaretState {
            shown: self.shown.clone(),
        }
    }
}

impl ViewState<Caret> for CaretState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shown.bind(ctx);
    }
    fn build(&self, _view: &Caret, ctx: &dyn BuildContext) -> impl IntoView {
        let color = if self.shown.get(ctx) {
            Color::rgb(0, 0, 0)
        } else {
            Color::rgb(255, 255, 255)
        };
        SizedBox::new(2.0, LABEL_HEIGHT).child(ColoredBox::new(color))
    }
}

/// A caret over `rows` painted rows in one eager column under a boundary of
/// their own: every row is mounted, laid out and painted once.
#[derive(Clone, StatelessView)]
struct CaretOverRows {
    shown: SharedSignal<bool>,
    rows: usize,
}

impl StatelessView for CaretOverRows {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let rows: Vec<BoxedView> = (0..self.rows)
            .map(|row| {
                let shade = (row % 200) as u8;
                SizedBox::new(WIDTH, ROW_HEIGHT)
                    .child(ColoredBox::new(Color::rgb(shade, 80, 120)))
                    .boxed()
            })
            .collect();
        Column::new(vec![
            RepaintBoundary::new()
                .child(Caret {
                    shown: self.shown.clone(),
                })
                .boxed(),
            Expanded::new(
                SingleChildScrollView::new().child(RepaintBoundary::new().child(Column::new(rows))),
            )
            .boxed(),
        ])
    }
}

/// The report of [`SCALING_FRAMES`] caret blinks over `rows` rows, with the
/// heap counts they made.
fn caret_blinks(rows: usize) -> (FrameReport, HeapCounts) {
    let shown = SharedSignal::new(true);
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(
        binding.arena().clone(),
        FocusTreeRoot::new(app_root(CaretOverRows {
            shown: shown.clone(),
            rows,
        })),
    );
    let _ = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(WIDTH, HEIGHT),
    );
    for _ in 0..2 {
        let _ = binding.pump_frame_report(FRAME);
    }
    perf_measured(|| {
        let mut total = FrameReport::default();
        for blink in 0..SCALING_FRAMES {
            shown.set(blink % 2 == 1);
            total += &binding.pump_frame_report(FRAME);
        }
        total
    })
}

/// A caret blink repaints the caret's boundary alone and patches one node,
/// over 1 000 rows as over 10 000: the frame names no other boundary, so
/// nothing it does grows with the tree (ADR-0304 §8).
#[test]
fn perf_a_caret_blink_in_a_10k_list_costs_the_same_on_any_tree_size() {
    let (small, small_heap) = caret_blinks(1_000);
    let (large, large_heap) = caret_blinks(10_000);
    record("caret_blink_in_10k_list", &large, large_heap);

    for name in [
        "nodes_painted",
        "scene_ops",
        "scene_nodes_visited",
        "damage_rects",
    ] {
        assert_eq!(
            counter(&small, name),
            counter(&large, name),
            "{name}: {small:#?} over 1 000 rows, {large:#?} over 10 000"
        );
    }
    let blinks = SCALING_FRAMES as u64;
    assert_eq!(
        counter(&large, "scene_ops"),
        blinks,
        "one Publish of the caret's boundary per blink: {large:#?}"
    );
    assert_eq!(counter(&large, "damage_full"), 0, "{large:#?}");
    assert_eq!(
        small_heap, large_heap,
        "{small_heap:?} over 1 000 rows, {large_heap:?} over 10 000"
    );
}

/// Frames requested with nothing changed in the tree, as a ticker that
/// changes nothing asks for them, driven through the UI runtime's frame
/// transaction: none publishes or submits a scene, because the retained scene
/// already holds what the window shows (ADR-0304 §8). What the frames still
/// ask of the heap is held to its baseline, which `cargo xtask perf` ratchets
/// down as the frame's per-frame buffers become kept ones.
#[test]
fn perf_an_idle_frame_leaves_the_retained_scene_alone() {
    const IDLE_FRAMES: usize = 4;
    let (position, _writer) = ScrollPosition::with_writer(0.0);
    let app = PerfApp {
        label: SharedSignal::new(String::from("label")),
        position,
    };
    let mut host = HeadlessHost::new(HeadlessWindow::new(WIDTH as u32, HEIGHT as u32));
    host.attach(&app).expect("a fresh host takes a root");
    host.enable_semantics();
    // The lazy band lands a frame after the mount's paint.
    for _ in 0..3 {
        let _ = host.pump(FRAME);
    }
    let shown = host
        .sink()
        .version()
        .expect("the mount submitted a scene")
        .id();
    let submits = host.sink().submits();
    let ((), allocations) = perf_measured(|| {
        for _ in 0..IDLE_FRAMES {
            host.request_frame();
            let _ = host.pump(FRAME);
        }
    });
    record_counts("idle_frame_retained_scene", &heap_counts(allocations));

    assert!(
        host.sink()
            .version()
            .is_some_and(|version| version.id() == shown),
        "an idle frame publishes no new scene version"
    );
    assert_eq!(
        host.sink().submits(),
        submits,
        "an idle frame submits nothing"
    );
}

/// What a `Semantics` with one action adds to the `Text` it holds in a
/// window that collects no semantics: its element and render object, and no
/// action route (ADR-0322 §8).
#[test]
fn perf_semantics_action_over_text() {
    let cost = control_over_text(|row| {
        Semantics::new()
            .on_click(|_ctx| {})
            .child(Text::new(format!("Row {row}")))
            .boxed()
    });
    record_counts("semantics_action_over_text", &cost.counts());
}
