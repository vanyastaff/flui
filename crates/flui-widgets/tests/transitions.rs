//! `SlideTransition`, `ScaleTransition` and `RotationTransition` follow their
//! animation without rebuilding the element tree, and paint, hit-testing and
//! semantics all sit where the child is painted.
//!
//! Rebuilds are read from the build owner's public per-frame report after a
//! frame the test does not dirty itself (`LaidOut::tick`, `pump_for`); the
//! expected geometry is analytic — a translation by `(dx·w, dy·h)`, a scale or
//! a `2π·turns` rotation about the child's centre — compared through
//! `PipelineOwner::transform_to`.

use std::f64::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{LaidOut, lay_out, loose};
use flui_animation::ext::AnimatableExt;
use flui_animation::{Animation, AnimationController, Tween};
use flui_foundation::RenderId;
use flui_foundation::geometry::Matrix4;
use flui_objects::TranslationFraction;
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_testing::{FrameReport, HeadlessBinding, MountOptions, MountOwners};
use flui_view::RebuildReason;
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{
    Align, ColoredBox, Column, FocusRoot, GestureArenaScope, GestureDetector, RotationTransition,
    ScaleTransition, SizedBox, SlideTransition,
};

/// The child's side: every transition here wraps a 40×40 box.
const SIDE: f64 = 40.0;
const VALUES: [f64; 3] = [0.25, 0.5, 1.0];

fn controller() -> AnimationController {
    AnimationController::builder(Duration::from_millis(300)).build()
}

fn scalar(controller: &AnimationController) -> std::rc::Rc<dyn Animation<f64>> {
    std::rc::Rc::new(controller.clone())
}

/// `controller` mapped onto `ZERO → end`.
fn fraction(
    controller: &AnimationController,
    end: TranslationFraction,
) -> std::rc::Rc<dyn Animation<TranslationFraction>> {
    std::rc::Rc::new(Tween::new(TranslationFraction::ZERO, end).animate(scalar(controller)))
}

fn child_box() -> SizedBox {
    SizedBox::new(SIDE, SIDE).child(ColoredBox::new(Color::rgb(10, 20, 30)))
}

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// A tree on the headless binding, whose per-frame report runs the build
/// phase on every frame — so `elements_built == 0` means nothing rebuilt,
/// not that the build phase was skipped.
struct Headless {
    binding: HeadlessBinding,
}

impl Headless {
    fn mount<V: View + Clone + 'static>(view: V) -> Self {
        let mut binding = HeadlessBinding::new();
        let root = GestureArenaScope::new(
            binding.arena().clone(),
            FocusRoot::new(Align::new(Alignment::TOP_LEFT).child(view)),
        );
        let _ = binding.mount_root(
            &root,
            MountOwners::fresh(),
            MountOptions::tight(200.0, 200.0),
        );
        binding.pump_frame(FRAME);
        Self { binding }
    }

    fn pump(&mut self, dt: Duration) -> FrameReport {
        self.binding.pump_frame(dt);
        self.binding.last_frame_report().clone()
    }

    fn pipeline(&self) -> &PipelineCell {
        self.binding.pipeline_owner().expect("tree-bound")
    }

    /// The child of the `index`-th transition node, mapped into that node.
    fn matrix(&self, index: usize) -> Matrix4 {
        self.pipeline().with(|owner| {
            let node = transform_nodes(owner)[index];
            let child = owner.render_tree().children(node)[0];
            owner
                .transform_to(child, node)
                .expect("child below the transition is laid out")
        })
    }
}

fn transform_nodes(owner: &PipelineOwner) -> Vec<RenderId> {
    owner
        .render_tree()
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| {
            owner
                .debug_node_diagnostics(id)
                .and_then(|diagnostics| diagnostics.name().map(str::to_owned))
                .is_some_and(|name| name.ends_with("RenderAnimatedTransform"))
        })
        .collect()
}

fn transform_node(laid: &LaidOut, index: usize) -> RenderId {
    laid.find_all_by_render_type("RenderAnimatedTransform")[index]
}

/// Asserts that `matrix` maps child-local points the way `expected` does.
fn assert_matrix(matrix: Matrix4, expected: impl Fn(f64, f64) -> (f64, f64), what: &str) {
    for (x, y) in [(0.0, 0.0), (SIDE, SIDE / 2.0), (10.0, 30.0)] {
        let actual = matrix.transform_point(x, y);
        let wanted = expected(x, y);
        assert!(
            (actual.0 - wanted.0).abs() < 1e-9 && (actual.1 - wanted.1).abs() < 1e-9,
            "{what}: ({x}, {y}) maps to {actual:?}, expected {wanted:?}"
        );
    }
}

/// [`assert_matrix`] for the `index`-th transition of a [`LaidOut`] tree.
fn assert_maps(
    laid: &LaidOut,
    index: usize,
    expected: impl Fn(f64, f64) -> (f64, f64),
    what: &str,
) {
    let node = transform_node(laid, index);
    let child = laid.only_child(node);
    let matrix = laid
        .pipeline_owner()
        .with(|owner| owner.transform_to(child, node))
        .expect("child below the transition is laid out");
    assert_matrix(matrix, expected, what);
}

fn slide_by(dx: f64, dy: f64) -> impl Fn(f64, f64) -> (f64, f64) {
    move |x, y| (x + dx * SIDE, y + dy * SIDE)
}

fn scale_by(s: f64) -> impl Fn(f64, f64) -> (f64, f64) {
    let c = SIDE / 2.0;
    move |x, y| (c + (x - c) * s, c + (y - c) * s)
}

fn rotate_by(turns: f64) -> impl Fn(f64, f64) -> (f64, f64) {
    let c = SIDE / 2.0;
    let (sin, cos) = (turns * TAU).sin_cos();
    move |x, y| {
        let (rx, ry) = (x - c, y - c);
        (c + rx * cos - ry * sin, c + rx * sin + ry * cos)
    }
}

fn assert_no_rebuild(report: &FrameReport, what: &str) {
    assert_eq!(
        report.build.elements_built, 0,
        "{what} must not rebuild any element: {:?}",
        report.build
    );
    assert_eq!(
        report.build.count(RebuildReason::AnimationTick),
        0,
        "{what}"
    );
}

/// Mounts `view`, then for every value sets the controller and drives one
/// frame: nothing may rebuild, and every node must map its child as
/// `expected(value)` on that same frame.
fn tick_without_rebuilding<V, E, F>(
    view: V,
    controller: &AnimationController,
    nodes: usize,
    expected: E,
    what: &str,
) where
    V: View + Clone + 'static,
    E: Fn(f64) -> F,
    F: Fn(f64, f64) -> (f64, f64),
{
    let mut tree = Headless::mount(view);
    for value in VALUES {
        controller.set_value(value);
        let report = tree.pump(FRAME);
        assert_no_rebuild(&report, &format!("{what}: a tick to {value}"));
        for index in 0..nodes {
            assert_matrix(
                tree.matrix(index),
                expected(value),
                &format!("{what} at {value}"),
            );
        }
    }
}

pub(crate) fn slide_ticks_without_rebuilding() {
    let c = controller();
    let view = SlideTransition::new(
        fraction(&c, TranslationFraction::new(1.0, 0.5)),
        child_box(),
    );
    tick_without_rebuilding(view, &c, 1, |v| slide_by(v, 0.5 * v), "slide");
}

pub(crate) fn slide_rtl_mirrors_dx_without_rebuilding() {
    let c = controller();
    let view = SlideTransition::new(
        fraction(&c, TranslationFraction::new(1.0, 0.5)),
        child_box(),
    )
    .text_direction(TextDirection::Rtl);
    tick_without_rebuilding(view, &c, 1, |v| slide_by(-v, 0.5 * v), "rtl slide");
}

pub(crate) fn scale_ticks_without_rebuilding() {
    let c = controller();
    let view = ScaleTransition::new(scalar(&c), child_box());
    tick_without_rebuilding(view, &c, 1, scale_by, "scale");
}

pub(crate) fn rotation_ticks_without_rebuilding() {
    let c = controller();
    let view = RotationTransition::new(scalar(&c), child_box());
    tick_without_rebuilding(view, &c, 1, rotate_by, "rotation");
}

/// Two transitions on one controller both follow the same tick.
pub(crate) fn shared_controller_moves_both_transitions_on_one_tick() {
    let c = controller();
    let view = Column::new((
        ScaleTransition::new(scalar(&c), child_box()).boxed(),
        ScaleTransition::new(scalar(&c), child_box()).boxed(),
    ));
    tick_without_rebuilding(view, &c, 2, scale_by, "shared controller");
}

/// A registered controller driven by virtual time: a repeated frame time
/// paints nothing, a reverse mid-run shows the controller's current value,
/// and a jump past the end lands on the end value in one frame.
pub(crate) fn virtual_time_ticks_follow_the_controller_value() {
    let vsync = flui_animation::Vsync::new();
    let owner = AnimationController::builder(Duration::from_millis(300)).build_on(Some(&vsync));
    let c = owner.controller();
    let mut tree = Headless::mount(ScaleTransition::new(scalar(c), child_box()));
    tree.binding.adopt_vsync(vsync);
    c.forward().expect("a fresh controller forwards");
    tree.pump(FRAME);
    let report = tree.pump(Duration::from_millis(150));
    assert_no_rebuild(&report, "a time tick");
    assert_matrix(tree.matrix(0), scale_by(c.value()), "mid-run");

    let report = tree.pump(Duration::ZERO);
    assert_no_rebuild(&report, "a zero-dt frame");
    assert_eq!(
        report.pipeline.nodes_painted, 0,
        "a zero-dt frame paints nothing: {report:?}"
    );
    assert_matrix(tree.matrix(0), scale_by(c.value()), "after a zero-dt frame");

    c.reverse().expect("a running controller reverses");
    tree.pump(FRAME);
    let report = tree.pump(Duration::from_millis(60));
    let value = c.value();
    assert!(value < 0.5, "the reverse run moved back, got {value}");
    assert_no_rebuild(&report, "a reversing tick");
    assert_matrix(tree.matrix(0), scale_by(value), "reverse mid-run");

    tree.pump(Duration::from_secs(10));
    assert_eq!(c.value(), 0.0, "a 10 s jump completes the reverse run");
    assert_matrix(tree.matrix(0), scale_by(0.0), "after a 10 s jump");
}

/// Builds a tap-counting child inside `wrap`, sets the controller to
/// `value`, and returns `(taps at the moved position, taps at the laid-out
/// position)`.
fn taps_at(
    wrap: impl FnOnce(&AnimationController, GestureDetector) -> BoxedView,
    value: f64,
    moved: (f64, f64),
    original: (f64, f64),
) -> (usize, usize) {
    let taps = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&taps);
    let detector = GestureDetector::new()
        .on_tap(move |_cx| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .child(child_box());
    let c = controller();
    c.set_value(value);
    let laid = lay_out(wrap(&c, detector), loose(200.0));
    let tap = |(x, y): (f64, f64)| {
        let before = taps.load(Ordering::SeqCst);
        laid.dispatch_pointer_down(x, y);
        laid.dispatch_pointer_up(x, y);
        taps.load(Ordering::SeqCst) - before
    };
    (tap(moved), tap(original))
}

/// A selected path owns its coordinate mapping through delivery, while a
/// later selection observes the newly committed animation sample.
pub(crate) fn a_moving_slide_preserves_selected_pointer_coordinates() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use flui_foundation::geometry::{Offset, Point};
    use flui_platform_api::pointer::PointerEvent;
    use flui_testing::PointerScript;
    use flui_widgets::Listener;

    for (direction, sign) in [(TextDirection::Ltr, 1.0), (TextDirection::Rtl, -1.0)] {
        let mut binding = HeadlessBinding::new();
        let owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(binding.vsync()));
        let controller = owner.controller();
        let received = Rc::new(RefCell::new(Vec::new()));
        let delivered = Rc::clone(&received);
        let listener = Listener::new()
            .on_pointer_down(move |_, dispatch| {
                let (PointerEvent::Down(local), PointerEvent::Down(global)) =
                    (dispatch.local, dispatch.global)
                else {
                    panic!("pointer-down callback receives a Down in both spaces");
                };
                delivered
                    .borrow_mut()
                    .push((local.sample.position.get(), global.sample.position.get()));
            })
            .child(child_box());
        let slide = SlideTransition::new(
            fraction(controller, TranslationFraction::new(2.0, 0.0)),
            listener,
        )
        .text_direction(direction);
        let root = GestureArenaScope::new(
            binding.arena().clone(),
            FocusRoot::new(Align::new(Alignment::CENTER).child(slide)),
        );
        let _mounted = binding.mount_root(
            &root,
            MountOwners::fresh(),
            MountOptions::tight(200.0, 200.0),
        );
        binding.pump_frame(Duration::ZERO);
        controller.forward().expect("fresh slide run");
        binding.pump_frame(Duration::ZERO);
        binding.pump_frame(Duration::from_millis(250));
        assert!((controller.value() - 0.25).abs() < 1e-9);
        assert_no_rebuild(
            binding.last_frame_report(),
            "running slide before selection",
        );

        let first = Offset::new(90.0 + sign * 20.0, 90.0);
        let selected = Cell::new(false);
        binding.replay_with(&PointerScript::tap(first), |binding, position| {
            let path = binding.hit_test(position);
            if !selected.replace(true) {
                // Change the public animation after choosing the path, before
                // the owner lane receives it. Its mapping must remain admitted.
                controller.set_value(0.5);
            }
            path
        });
        assert_eq!(
            &*received.borrow(),
            &[(Point::new(10.0, 10.0), Point::new(first.dx, first.dy))],
            "{direction:?}: selected coordinates survive a newer animation sample"
        );
        assert_no_rebuild(
            binding.last_frame_report(),
            "committing the replacement sample",
        );

        let second = Offset::new(90.0 + sign * 40.0, 90.0);
        binding.replay(&PointerScript::tap(second));
        assert_eq!(
            &*received.borrow(),
            &[
                (Point::new(10.0, 10.0), Point::new(first.dx, first.dy)),
                (Point::new(10.0, 10.0), Point::new(second.dx, second.dy)),
            ],
            "{direction:?}: the next selection uses the current sample"
        );
        assert_no_rebuild(binding.last_frame_report(), "the next pointer selection");
    }
}

pub(crate) fn hit_test_follows_the_painted_transform() {
    let slide = |transform_hit_tests: bool| {
        move |c: &AnimationController, child: GestureDetector| -> BoxedView {
            let view = SlideTransition::new(fraction(c, TranslationFraction::new(1.0, 0.0)), child)
                .transform_hit_tests(transform_hit_tests);
            view.boxed()
        }
    };
    assert_eq!(
        taps_at(slide(true), 1.0, (60.0, 20.0), (20.0, 20.0)),
        (1, 0),
        "a slid child is hit where it is painted, not where it was laid out"
    );
    assert_eq!(
        taps_at(slide(false), 1.0, (60.0, 20.0), (20.0, 20.0)),
        (0, 1),
        "transform_hit_tests(false) keeps hits at the laid-out position"
    );
    let scale = |c: &AnimationController, child: GestureDetector| -> BoxedView {
        let view = ScaleTransition::new(scalar(c), child);
        view.boxed()
    };
    assert_eq!(
        taps_at(scale, 0.0, (20.0, 20.0), (20.0, 20.0)),
        (0, 0),
        "a child scaled to zero is not hit anywhere"
    );
    assert_eq!(
        taps_at(scale, 0.5, (15.0, 15.0), (2.0, 2.0)),
        (1, 0),
        "a half-scaled child is hit inside its painted square only"
    );
}

/// A parent rebuild that hands the transition a new animation retargets the
/// same render object, and the new value shows on that frame.
pub(crate) fn swapping_the_animation_keeps_the_render_object() {
    let first = controller();
    first.set_value(0.5);
    let second = controller();
    second.set_value(1.0);
    let mut laid = lay_out(
        ScaleTransition::new(scalar(&first), child_box()),
        loose(200.0),
    );
    let before = transform_node(&laid, 0);
    laid.pump_widget(ScaleTransition::new(scalar(&second), child_box()));
    assert_eq!(
        transform_node(&laid, 0),
        before,
        "the swap must not recreate the node"
    );
    assert_maps(&laid, 0, scale_by(1.0), "after the swap");

    first.set_value(0.25);
    laid.tick();
    assert_maps(
        &laid,
        0,
        scale_by(1.0),
        "the old animation no longer drives the node",
    );
    second.set_value(0.75);
    laid.tick();
    assert_maps(
        &laid,
        0,
        scale_by(0.75),
        "the new animation drives the node",
    );
}

/// Removing one of two transitions on a shared controller leaves the other
/// subscribed, and ticks after the removal neither panic nor rebuild.
pub(crate) fn unmounted_transition_leaves_its_sibling_ticking() {
    let c = controller();
    let mut laid = lay_out(
        Column::new((
            ScaleTransition::new(scalar(&c), child_box()).boxed(),
            ScaleTransition::new(scalar(&c), child_box()).boxed(),
        )),
        loose(200.0),
    );
    c.set_value(0.5);
    laid.tick();
    laid.pump_widget(Column::new((
        ScaleTransition::new(scalar(&c), child_box()).boxed(),
    )));
    assert_eq!(
        laid.find_all_by_render_type("RenderAnimatedTransform")
            .len(),
        1
    );
    c.set_value(0.25);
    laid.tick();
    assert_maps(&laid, 0, scale_by(0.25), "the surviving transition");
}

/// The tree keeps the animation alive while mounted, even after the caller
/// dropped its own handle, and releases it on unmount.
pub(crate) fn the_animation_is_released_after_unmount() {
    let c = controller();
    c.set_value(0.5);
    let animation = fraction(&c, TranslationFraction::new(1.0, 0.0));
    let weak: std::rc::Weak<dyn Animation<TranslationFraction>> =
        std::rc::Rc::downgrade(&animation);
    let mut laid = lay_out(SlideTransition::new(animation, child_box()), loose(200.0));
    assert!(
        weak.upgrade().is_some(),
        "the mounted transition keeps its animation"
    );
    assert_maps(&laid, 0, slide_by(0.5, 0.0), "while mounted");
    laid.pump_widget(child_box());
    assert!(
        weak.upgrade().is_none(),
        "unmounting the transition must release its animation"
    );
}
