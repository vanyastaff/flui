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
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system and reads the environment. It lives here rather
//! than in `flui-testing`, which owns the frame driver, because the app is a
//! widget tree: a `flui-testing` → `flui-widgets` dev edge would make every
//! crate that dev-depends on `flui-testing` a dependent of the widget catalog,
//! widening every change's CI scope.

use std::fmt::Write as _;
use std::sync::Once;
use std::time::Duration;

use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::view::ScrollPosition;
use flui_testing::{FrameReport, HeadlessBinding, MountOptions, MountOwners, pin_font_faces};
use flui_types::Color;
use flui_view::RebuildReason;
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, Column, Expanded, FocusRoot, GestureArenaScope, ListView, SizedBox, Text,
};

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);
const ROWS: usize = 10_000;
const ROW_HEIGHT: f32 = 20.0;
const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 600.0;
/// The label's fixed height: the list below it is `HEIGHT - LABEL_HEIGHT`
/// tall whatever the text measures, so font metrics cannot move the band.
const LABEL_HEIGHT: f32 = 20.0;

/// The one stateful piece: a label whose change rebuilds only itself and the
/// text below it.
#[derive(Clone, StatefulView)]
struct Label {
    text: StateHandle<String>,
}

struct LabelState {
    text: StateHandle<String>,
}

impl StatefulView for Label {
    type State = LabelState;
    fn create_state(&self) -> Self::State {
        LabelState {
            text: self.text.clone(),
        }
    }
}

impl ViewState<Label> for LabelState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.text.bind(ctx);
    }
    fn build(&self, _view: &Label, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.text.with(Clone::clone))
    }
}

/// The app: a fixed-height label over a lazy list of fixed-height rows,
/// scrolled by a shared position.
#[derive(Clone, StatelessView)]
struct PerfApp {
    label: StateHandle<String>,
    position: ScrollPosition,
}

impl StatelessView for PerfApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Each row paints, so a retained row carries a picture layer that a
        // graft reuses; an empty row would retain nothing to count.
        let list = ListView::builder(ROWS, ROW_HEIGHT, |index| {
            (index < ROWS).then(|| {
                SizedBox::new(WIDTH, ROW_HEIGHT)
                    .child(ColoredBox::new(Color::rgb(40, 80, 120)))
                    .boxed()
            })
        })
        .position(self.position.clone());
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
    label: StateHandle<String>,
    position: ScrollPosition,
}

impl Mounted {
    fn pump(&mut self) -> FrameReport {
        self.binding.pump_frame(FRAME);
        self.binding.last_frame_report().clone()
    }

    /// Pumps `frames` frames and sums their reports.
    fn pump_many(&mut self, frames: usize) -> FrameReport {
        let mut total = FrameReport::default();
        for _ in 0..frames {
            total += &self.pump();
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

fn pin_fonts() {
    static PIN: Once = Once::new();
    PIN.call_once(|| {
        pin_font_faces(&[flui_painting::fonts::ROBOTO_REGULAR], "Roboto");
    });
}

/// Mounts [`PerfApp`] at 800×600 with semantics on.
fn mount() -> Mounted {
    pin_fonts();
    let label = StateHandle::new(String::from("label"));
    let position = ScrollPosition::new(0.0);
    let app = PerfApp {
        label: label.clone(),
        position: position.clone(),
    };
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(binding.arena().clone(), FocusRoot::new(app));
    let _ = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(WIDTH, HEIGHT),
    );
    binding.enable_semantics().expect("tree-bound");
    Mounted {
        binding,
        label,
        position,
    }
}

/// Writes `report` for `xtask perf` when `FLUI_PERF_OUT` is set. Called
/// before a scenario asserts, so a failing budget still reports its numbers.
fn record(scenario: &str, report: &FrameReport) {
    let Some(dir) = std::env::var_os("FLUI_PERF_OUT") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("create FLUI_PERF_OUT");
    let mut body = format!("[{scenario}]\n");
    for (name, value) in report.counters() {
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
    let report = app.pump();
    record("full_reassemble", &report);

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

/// Ten seconds with nothing changing costs nothing: no build, no layout, no
/// paint, no semantics, no frame. The control step on the same binding shows
/// the counters are live, so a counter stuck at zero cannot pass as idle.
#[test]
fn perf_idle_ten_seconds_produces_no_frames() {
    let mut app = mount();
    let _ = app.pump();

    let idle = app.pump_many(600);
    record("idle_10s", &idle);

    for (name, value) in idle.counters() {
        assert_eq!(
            value, 0,
            "{name} must stay at 0 over an idle 10 s: {idle:#?}"
        );
    }

    app.label.update(|text| text.push('!'));
    let control = app.pump();
    assert_eq!(
        control.pipeline.frames_produced, 1,
        "control: a change produces a frame: {control:#?}"
    );
    assert!(
        control.build.elements_built >= 1,
        "control: a change rebuilds: {control:#?}"
    );
}

/// Scrolling one screen of a 10 000-row lazy list lays out the newly visible
/// band, not the list.
#[test]
fn perf_scrolling_a_10k_list_one_screen_lays_out_only_the_band() {
    let mut app = mount();
    let band = app.render_nodes();
    assert!(
        band < 400,
        "sanity: the mount built only a band of the 10 000 rows, got {band} render nodes"
    );

    app.position.set_pixels(HEIGHT);
    // Two frames: a lazy band that first appears is built after that frame's
    // paint and lands on the next (the ListView first-frame divergence).
    let report = app.pump_many(2);
    record("list_10k_scroll_one_screen", &report);

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

/// Changing one label rebuilds the label's holder, its `Text` and the text's
/// render view — nothing else — and the list's rows are grafted from their
/// retained layers rather than repainted.
#[test]
fn perf_one_text_change_rebuilds_at_most_three_elements() {
    let mut app = mount();
    let _ = app.pump();

    app.label.update(|text| text.push('!'));
    let report = app.pump();
    record("text_change", &report);

    assert_eq!(
        report.build.count(RebuildReason::StateChange),
        1,
        "only the label's holder was changed: {report:#?}"
    );
    assert!(
        (2..=3).contains(&report.build.elements_built),
        "holder, Text and its render view at most: {report:#?}"
    );
    assert_eq!(
        report.build.builds_run, report.build.elements_built,
        "no element re-enters: {report:#?}"
    );
    assert!(report.pipeline.nodes_laid_out >= 1, "{report:#?}");
    assert!(
        report.pipeline.semantics_nodes_updated >= 1,
        "the label's accessible text changed: {report:#?}"
    );
    assert!(
        report.pipeline.layers_reused >= 1,
        "the rows sit behind repaint boundaries and are grafted: {report:#?}"
    );
}
