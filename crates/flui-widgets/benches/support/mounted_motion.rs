//! Shared mounted-frame fixture for timing and allocation observations.
//! Scene inspection and frame telemetry allocate; keep them outside measurement.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_animation::{Linear, SharedCurve, Vsync, curves};
use flui_foundation::geometry::{Matrix4, Rect};
use flui_painting::{display_list::DrawOp, styling::Color};
use flui_rendering::{layer::Layer, pipeline::PipelineOwner};
use flui_testing::widgets::{LaidOut, lay_out_animated, tight};
use flui_view::{StateHandle, ViewExt};
use flui_widgets::prelude::*;
use flui_widgets::{AnimatedContainer, AnimatedOpacity, ScrollController, Scrollable, VsyncScope};

pub(super) const FRAME: Duration = Duration::from_millis(17);
pub(super) const RUN: Duration = Duration::from_secs(2);
pub(super) const BATCH: u64 = 32;

#[derive(Clone, Copy, Debug)]
pub(super) enum Workload {
    Color,
    Opacity,
    ColorAndOpacity,
    FractionalAlpha,
}

impl Workload {
    fn changes_color(self) -> bool {
        !matches!(self, Self::Opacity)
    }

    fn changes_opacity(self) -> bool {
        matches!(self, Self::Opacity | Self::ColorAndOpacity)
    }

    fn color(self, target: bool) -> Color {
        if matches!(self, Self::FractionalAlpha) {
            Color::from_rgba_f32_array([1.0, 0.0, 0.0, if target { 0.0018 } else { 0.001 }])
        } else if target {
            Color::RED
        } else {
            Color::BLUE
        }
    }
}

#[derive(Clone, StatefulView)]
struct Population {
    count: usize,
    workload: Workload,
    target: StateHandle<bool>,
}

struct PopulationState {
    target: StateHandle<bool>,
}

impl StatefulView for Population {
    type State = PopulationState;

    fn create_state(&self) -> Self::State {
        PopulationState {
            target: self.target.clone(),
        }
    }
}

/// Fixture populations are at most 1000, so both indices fit exactly in f64.
#[expect(
    clippy::cast_precision_loss,
    reason = "bounded visible fixture indices"
)]
fn cell_bounds(index: usize) -> Rect {
    Rect::from_ltwh(
        (index % 40) as f64 * 12.0,
        (index / 40) as f64 * 12.0,
        10.0,
        10.0,
    )
}

impl ViewState<Population> for PopulationState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.target.bind(ctx);
    }

    fn build(&self, view: &Population, _ctx: &dyn BuildContext) -> impl IntoView {
        let target = self.target.with(|target| *target);
        let children = (0..view.count)
            .map(|index| {
                let cell = if view.workload.changes_color() {
                    AnimatedContainer::new(SizedBox::square(10.0))
                        .color(view.workload.color(target))
                        .duration(RUN)
                        .curve(Linear)
                        .boxed()
                } else {
                    ColoredBox::new(Color::BLUE)
                        .child(SizedBox::square(10.0))
                        .boxed()
                };
                let cell = if view.workload.changes_opacity() {
                    AnimatedOpacity::new(if target { 0.85 } else { 0.25 }, cell)
                        .duration(RUN)
                        .curve(Linear)
                        .boxed()
                } else {
                    cell
                };
                let bounds = cell_bounds(index);
                Positioned::new(
                    Semantics::new()
                        .container(true)
                        .label(format!("Cell {index}"))
                        .child(cell),
                )
                .left(bounds.min.x)
                .top(bounds.min.y)
                .width(10.0)
                .height(10.0)
                .boxed()
            })
            .collect::<Vec<_>>();
        Stack::new(children)
    }
}

#[derive(Debug)]
pub(super) struct CellSample {
    bounds: Rect,
    color: [f32; 4],
    opacity: f64,
}

#[derive(Default, Debug)]
pub(super) struct FrameWork {
    pub(super) frames: u64,
    pub(super) submitted: u64,
    pub(super) builds: usize,
    pub(super) layout_roots: u64,
}

impl FrameWork {
    pub(super) fn add(&mut self, other: Self) {
        self.frames += other.frames;
        self.submitted += other.submitted;
        self.builds += other.builds;
        self.layout_roots += other.layout_roots;
    }
}

pub(super) struct FrameSnapshot {
    frames: u64,
    builds: usize,
    submitted: u64,
    layout_roots: u64,
}

pub(super) struct MountedMotion {
    pub(super) laid: LaidOut,
    target: StateHandle<bool>,
    builds: Arc<BuildCounter>,
}

#[derive(Default)]
struct BuildCounter(AtomicUsize);

impl flui_foundation::observe::TreeObserver for BuildCounter {
    fn element_rebuilt(&self, _event: &flui_foundation::observe::ElementRebuilt) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

impl MountedMotion {
    pub(super) fn new(count: usize, workload: Workload) -> Self {
        assert!((1..=1000).contains(&count));
        let target = StateHandle::new(false);
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                Population {
                    count,
                    workload,
                    target: target.clone(),
                },
            ),
            tight(800.0, 600.0),
            vsync,
        );
        laid.enable_semantics();
        laid.tick();
        Self {
            laid,
            target,
            builds: Arc::default(),
        }
    }

    /// Count completed builds through the existing tree-observation contract.
    /// Installed only for untimed verification; timing fixtures have no observer.
    pub(super) fn observe_builds(&mut self) {
        let observer = self.builds.clone();
        self.laid
            .with_build_owner_mut(|owner| owner.set_tree_observer(observer));
    }

    /// Author update plus its admission frame. A newly admitted generation can
    /// establish its time origin on the following frame; steady setup primes it.
    pub(super) fn retarget(&mut self) {
        self.target.update(|target| *target = !*target);
        self.laid.tick();
    }

    /// Identical fresh interior runs: settle old motion, admit the next target,
    /// establish its origin, then advance 400 ms before measuring 32 x 17 ms.
    /// Setup and completion belong outside the steady-frame interval.
    pub(super) fn prepare_active(&mut self) {
        self.laid.pump_for(RUN + Duration::from_secs(1));
        self.retarget();
        self.laid.tick();
        self.laid.pump_for(Duration::from_millis(400));
    }

    pub(super) fn work_before(&self) -> FrameSnapshot {
        FrameSnapshot {
            frames: self.laid.binding_scheduler().frame_count(),
            builds: self.builds.0.load(Ordering::Relaxed),
            submitted: self.laid.painted_frame_count(),
            layout_roots: self
                .laid
                .pipeline_owner()
                .with(PipelineOwner::layout_roots_total),
        }
    }

    pub(super) fn work_since(&mut self, before: FrameSnapshot) -> FrameWork {
        let frames = self.laid.binding_scheduler().frame_count() - before.frames;
        FrameWork {
            frames,
            submitted: self.laid.painted_frame_count() - before.submitted,
            builds: self.builds.0.load(Ordering::Relaxed) - before.builds,
            layout_roots: self
                .laid
                .pipeline_owner()
                .with(PipelineOwner::layout_roots_total)
                - before.layout_roots,
        }
    }

    /// Fixture transforms are translations. Apply picture and ancestor offsets
    /// to inspect actual committed screen bounds, rather than authored positions.
    pub(super) fn scene(&self) -> Vec<CellSample> {
        painted_cells(&self.laid)
    }
}

fn painted_cells(laid: &LaidOut) -> Vec<CellSample> {
    let tree = laid.layer_tree().expect("submitted scene");
    let mut cells = Vec::new();
    for (id, node) in tree.iter() {
        let Layer::Picture(picture) = node.layer() else {
            continue;
        };
        for command in picture.picture() {
            let DrawOp::Rect { rect, paint } = &command.op else {
                continue;
            };
            if rect.width() != 10.0 || rect.height() != 10.0 {
                continue;
            }
            let mut bounds = command.transform.transform_rect(rect);
            let mut opacity = 1.0;
            let mut parent = tree.parent(id);
            while let Some(id) = parent {
                let layer = tree.get_layer(id).expect("live scene ancestor");
                let transform = match layer {
                    Layer::Offset(offset) => Some(Matrix4::translation(
                        offset.offset().dx,
                        offset.offset().dy,
                        0.0,
                    )),
                    Layer::Transform(transform) => Some(*transform.transform()),
                    Layer::Opacity(layer) => {
                        opacity *= layer.alpha();
                        Some(Matrix4::translation(
                            layer.offset().dx,
                            layer.offset().dy,
                            0.0,
                        ))
                    }
                    _ => None,
                };
                if let Some(transform) = transform {
                    bounds = transform.transform_rect(&bounds);
                }
                parent = tree.parent(id);
            }
            cells.push(CellSample {
                bounds,
                color: paint.color.to_rgba_f32_array(),
                opacity,
            });
        }
    }
    cells.sort_by(|a, b| {
        a.bounds
            .min
            .y
            .total_cmp(&b.bounds.min.y)
            .then(a.bounds.min.x.total_cmp(&b.bounds.min.x))
    });
    cells
}

/// Real controller commands, cancellation hook and viewport publication.
/// A marker stays visible throughout the measured 20%..47.2% scroll interval.
pub(super) struct MountedScroll {
    pub(super) laid: LaidOut,
    controller: ScrollController,
}

impl MountedScroll {
    pub(super) fn new() -> Self {
        let controller = ScrollController::new();
        let vsync = Vsync::new();
        let marker = Positioned::new(
            Semantics::new()
                .container(true)
                .label("Scroll marker")
                .child(ColoredBox::new(Color::RED).child(SizedBox::square(10.0))),
        )
        .left(0.0)
        .top(500.0)
        .width(10.0)
        .height(10.0);
        let mut laid = lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(800.0, 5000.0).child(Stack::new(vec![marker.boxed()]))),
            ),
            tight(800.0, 600.0),
            vsync,
        );
        laid.enable_semantics();
        laid.tick();
        Self { laid, controller }
    }

    pub(super) fn prepare_active(&mut self) {
        self.controller.jump_to(0.0).expect("no build is running");
        self.laid.tick();
        self.animate();
        self.laid.tick();
        self.laid.pump_for(Duration::from_millis(400));
    }

    pub(super) fn pixels(&self) -> f64 {
        self.controller.pixels()
    }

    pub(super) fn animate(&mut self) {
        self.controller
            .animate_to(1000.0, RUN, SharedCurve::new(curves::LINEAR))
            .expect("no build is running");
        self.laid.tick();
    }

    pub(super) fn interrupt(&mut self) {
        self.controller
            .jump_to(self.pixels())
            .expect("no build is running");
        self.laid.tick();
    }

    pub(super) fn assert_marker(&self) {
        let cells = painted_cells(&self.laid);
        assert_eq!(cells.len(), 1, "one painted scroll marker");
        let expected = Rect::from_ltwh(0.0, 500.0 - self.pixels(), 10.0, 10.0);
        assert_eq!(
            cells[0].bounds, expected,
            "controller position reaches committed paint"
        );
        assert_eq!(cells[0].color, Color::RED.to_rgba_f32_array());
        assert!(
            expected.min.y >= 0.0 && expected.max.y <= 600.0,
            "marker is visible"
        );
    }
}

pub(super) fn verify_scroll() {
    let mut scroll = MountedScroll::new();
    let builds = Arc::new(BuildCounter::default());
    scroll
        .laid
        .with_build_owner_mut(|owner| owner.set_tree_observer(builds.clone()));
    let mut submitted = 0;
    for _ in 0..3 {
        scroll.prepare_active();
        scroll.assert_marker();
        for _ in 0..BATCH {
            let before = scroll.laid.painted_frame_count();
            let pixels = scroll.pixels();
            scroll.laid.pump_for(FRAME);
            assert!(scroll.pixels() > pixels, "active scroll advances");
            assert_eq!(scroll.laid.painted_frame_count() - before, 1);
            submitted += 1;
            scroll.assert_marker();
        }
    }
    scroll.prepare_active();
    let before = scroll.pixels();
    scroll.animate();
    assert_eq!(
        scroll.pixels(),
        before,
        "admission preserves the accepted position"
    );
    for _ in 0..3 {
        scroll.laid.pump_for(FRAME);
    }
    assert!(
        scroll.pixels() > before,
        "replacement animate command progresses"
    );
    scroll.assert_marker();
    scroll.prepare_active();
    let stopped = scroll.pixels();
    scroll.interrupt();
    scroll.assert_marker();
    let idle_paints = scroll.laid.painted_frame_count();
    let idle_builds = builds.0.load(Ordering::Relaxed);
    let idle_layout = scroll
        .laid
        .pipeline_owner()
        .with(PipelineOwner::layout_roots_total);
    for _ in 0..8 {
        scroll.laid.pump_for(FRAME);
        assert_eq!(
            scroll.pixels(),
            stopped,
            "equal-position jump cancels the actual run"
        );
        scroll.assert_marker();
    }
    assert_eq!(scroll.laid.painted_frame_count(), idle_paints);
    assert_eq!(builds.0.load(Ordering::Relaxed), idle_builds);
    assert_eq!(
        scroll
            .laid
            .pipeline_owner()
            .with(PipelineOwner::layout_roots_total),
        idle_layout
    );
    scroll.prepare_active();
    let before = scroll.pixels();
    scroll.laid.pump_for(FRAME);
    assert!(
        scroll.pixels() > before,
        "new command recovers after cancellation"
    );
    scroll.assert_marker();
    println!(
        "Scroll: active96 submitted={submitted}; equal-position cancellation idle8 has no paint/build/layout; next command advances"
    );
}

pub(super) fn assert_population(cells: &[CellSample], count: usize) {
    assert_eq!(cells.len(), count, "exact visible painted population");
    for (index, cell) in cells.iter().enumerate() {
        assert_eq!(
            cell.bounds,
            cell_bounds(index),
            "cell {index} screen geometry"
        );
        assert!(cell.color.iter().all(|value| value.is_finite()));
        assert!(cell.opacity.is_finite() && cell.opacity > 0.0 && cell.opacity <= 1.0);
    }
}

pub(super) fn assert_progress(before: &[CellSample], after: &[CellSample], workload: Workload) {
    assert_eq!(before.len(), after.len());
    for (index, (before, after)) in before.iter().zip(after).enumerate() {
        if workload.changes_color() {
            assert_ne!(before.color, after.color, "cell {index} color must advance");
        }
        if workload.changes_opacity() {
            assert_ne!(
                before.opacity, after.opacity,
                "cell {index} opacity must advance"
            );
        }
        if matches!(workload, Workload::FractionalAlpha) {
            assert!(
                after.color[3] > 0.0 && after.color[3] < 0.0019,
                "fractional control stays in the alpha byte-zero interval"
            );
        }
    }
}
