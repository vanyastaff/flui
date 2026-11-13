//! The explicit transitions follow their animation without rebuilding the
//! element tree: `SlideTransition`, `ScaleTransition` and `RotationTransition`
//! move a transform, and paint, hit-testing and semantics all sit where the
//! child is painted; `SizeTransition`, `AlignTransition` and
//! `PositionedTransition` relay out, and `DecoratedBoxTransition` repaints.
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

use crate::common::{LaidOut, lay_out, lay_out_animated, loose, tight};
use flui_animation::Animatable;
use flui_animation::{Animation, AnimationController, Tween};
use flui_foundation::RenderId;
use flui_foundation::geometry::Matrix4;
use flui_foundation::geometry::{RelativeRect, Size};
use flui_objects::TranslationFraction;
use flui_painting::Alignment;
use flui_painting::styling::BoxDecoration;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};

use flui_view::{BoxedView, View, ViewExt};
use flui_view::{FrameBuildReport, RebuildReason};
use flui_widgets::{
    Align, AlignTransition, ColoredBox, Column, DecoratedBoxTransition, GestureDetector,
    PositionedTransition, RotationTransition, ScaleTransition, SizeTransition, SizedBox,
    SlideTransition, Stack, app_root,
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

/// The child's own fill, told apart from a decoration painted around it.
const CHILD_FILL: Color = Color::rgb(10, 20, 30);

fn child_box() -> SizedBox {
    SizedBox::new(SIDE, SIDE).child(ColoredBox::new(CHILD_FILL))
}

/// One 60 Hz frame of virtual time.
const FRAME: Duration = Duration::from_nanos(16_666_667);

/// A tree on a UI runtime. Every frame runs the build phase, so a report of
/// `elements_built == 0` means nothing rebuilt, not that the build phase was
/// skipped.
struct Headless {
    laid: LaidOut,
}

impl Headless {
    fn mount<V: View + Clone + 'static>(view: V) -> Self {
        let mut laid = lay_out(
            app_root(Align::new(Alignment::TOP_LEFT).child(view)),
            tight(200.0, 200.0),
        );
        laid.pump_for(FRAME);
        Self { laid }
    }

    /// Drive one frame `dt` after the last and report what it built.
    fn frame(&mut self, dt: Duration) -> FrameBuildReport {
        self.laid.pump_for(dt);
        self.laid
            .with_build_owner_mut(|owner| owner.last_frame_build_report())
    }

    fn pipeline(&self) -> PipelineCell {
        self.laid.pipeline_owner()
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
            flui_rendering::testing::inspect::node_snapshot(owner, id).kind
                == "RenderAnimatedTransform"
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

fn assert_no_rebuild(report: &FrameBuildReport, what: &str) {
    assert_eq!(
        report.elements_built, 0,
        "{what} must not rebuild any element: {report:?}"
    );
    assert_eq!(report.count(RebuildReason::AnimationTick), 0, "{what}");
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
        controller.set_value(value).expect("no build is running");
        let report = tree.frame(FRAME);
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
    let view =
        SlideTransition::new(fraction(&c, TranslationFraction::new(1.0, 0.5))).child(child_box());
    tick_without_rebuilding(view, &c, 1, |v| slide_by(v, 0.5 * v), "slide");
}

pub(crate) fn slide_rtl_mirrors_dx_without_rebuilding() {
    let c = controller();
    let view = SlideTransition::new(fraction(&c, TranslationFraction::new(1.0, 0.5)))
        .child(child_box())
        .text_direction(TextDirection::Rtl);
    tick_without_rebuilding(view, &c, 1, |v| slide_by(-v, 0.5 * v), "rtl slide");
}

pub(crate) fn scale_ticks_without_rebuilding() {
    let c = controller();
    let view = ScaleTransition::new(scalar(&c)).child(child_box());
    tick_without_rebuilding(view, &c, 1, scale_by, "scale");
}

pub(crate) fn rotation_ticks_without_rebuilding() {
    let c = controller();
    let view = RotationTransition::new(scalar(&c)).child(child_box());
    tick_without_rebuilding(view, &c, 1, rotate_by, "rotation");
}

fn render_nodes(owner: &PipelineOwner, type_name: &str) -> Vec<RenderId> {
    owner
        .render_tree()
        .iter()
        .map(|(id, _)| id)
        .filter(|&id| flui_rendering::testing::inspect::node_snapshot(owner, id).kind == type_name)
        .collect()
}

impl Headless {
    /// The only `type_name` node, its size and its child's offset in it.
    fn node_geometry(&self, type_name: &str) -> (Size, (f64, f64)) {
        self.pipeline().with(|owner| {
            let nodes = render_nodes(owner, type_name);
            assert_eq!(nodes.len(), 1, "one {type_name}");
            let node = nodes[0];
            let child = owner.render_tree().children(node)[0];
            let size = owner
                .render_tree()
                .get(node)
                .and_then(flui_rendering::storage::RenderNode::geometry_box)
                .expect("the transition node is laid out");
            let origin = owner
                .transform_to(child, node)
                .expect("child below the transition is laid out")
                .transform_point(0.0, 0.0);
            (size, origin)
        })
    }

    /// The color of the first painted rectangle that is not the child's fill.
    fn decoration_fill(&self) -> Color {
        use flui_painting::display_list::DrawOp;
        let tree = self.laid.layer_tree().expect("a painted frame");
        tree.iter()
            .filter_map(|(_, node)| match node.layer() {
                flui_rendering::layer::Layer::Picture(picture) => Some(picture.picture().clone()),
                _ => None,
            })
            .flat_map(|picture| picture.iter().cloned().collect::<Vec<_>>())
            .find_map(|command| match command.op {
                DrawOp::Rect { paint, .. } if paint.color != CHILD_FILL => Some(paint.color),
                _ => None,
            })
            .expect("the decoration paints a fill")
    }
}

/// Mounts `view`, then for every value sets the controller and drives one
/// frame: nothing may rebuild, and `check` reads that same frame's layout or
/// paint.
fn ticks_without_rebuilding<V: View + Clone + 'static>(
    view: V,
    controller: &AnimationController,
    what: &str,
    check: impl Fn(&Headless, f64),
) {
    let mut tree = Headless::mount(view);
    for value in VALUES {
        controller.set_value(value).expect("no build is running");
        let report = tree.frame(FRAME);
        assert_no_rebuild(&report, &format!("{what}: a tick to {value}"));
        check(&tree, value);
    }
}

fn assert_between(value: f64, low: f64, high: f64, what: &str) {
    assert!(
        value > low && value < high,
        "{what}: {value} must lie strictly between {low} and {high}"
    );
}

/// The child's height scales with the factor and, centred on the axis, the
/// clipped child overflows the shrunken box equally at both ends.
pub(crate) fn size_ticks_without_rebuilding() {
    let c = controller();
    let view = SizeTransition::new(scalar(&c)).child(child_box());
    ticks_without_rebuilding(view, &c, "size", |tree, v| {
        let (size, origin) = tree.node_geometry("RenderAnimatedAlign");
        assert_eq!(size, Size::new(200.0, SIDE * v), "size at {v}");
        assert_eq!(origin, (0.0, (SIDE * v - SIDE) / 2.0), "child at {v}");
        if v == 0.5 {
            assert_between(size.height, 0.0, SIDE, "half-way height");
        }
    });
}

pub(crate) fn align_ticks_without_rebuilding() {
    let c = controller();
    let alignment: std::rc::Rc<dyn Animation<Alignment>> = std::rc::Rc::new(
        Tween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT).animate(scalar(&c)),
    );
    let view = AlignTransition::new(alignment, child_box());
    ticks_without_rebuilding(view, &c, "align", |tree, v| {
        let (size, origin) = tree.node_geometry("RenderAnimatedAlign");
        assert_eq!(size, Size::new(200.0, 200.0));
        let free = 200.0 - SIDE;
        assert_eq!(origin, (free * v, free * v), "child at {v}");
        if v == 0.5 {
            assert_between(origin.0, 0.0, free, "half-way offset");
        }
    });
}

pub(crate) fn positioned_ticks_without_rebuilding() {
    let c = controller();
    let rect: std::rc::Rc<dyn Animation<RelativeRect>> = std::rc::Rc::new(
        Tween::new(
            RelativeRect::from_ltrb(0.0, 0.0, 0.0, 0.0),
            RelativeRect::from_ltrb(20.0, 40.0, 60.0, 80.0),
        )
        .animate(scalar(&c)),
    );
    let view = Stack::new((PositionedTransition::new(rect, ColoredBox::new(CHILD_FILL)),));
    ticks_without_rebuilding(view, &c, "positioned", |tree, v| {
        let (size, origin) = tree.node_geometry("RenderAnimatedInset");
        assert_eq!(size, Size::new(200.0, 200.0), "fills the stack");
        assert_eq!(origin, (20.0 * v, 40.0 * v), "child at {v}");
        let child = tree.pipeline().with(|owner| {
            let node = render_nodes(owner, "RenderAnimatedInset")[0];
            let child = owner.render_tree().children(node)[0];
            owner
                .render_tree()
                .get(child)
                .and_then(flui_rendering::storage::RenderNode::geometry_box)
                .expect("the child is laid out")
        });
        assert_eq!(child, Size::new(200.0 - 80.0 * v, 200.0 - 120.0 * v));
        if v == 0.5 {
            assert_between(origin.1, 0.0, 40.0, "half-way top");
        }
    });
}

pub(crate) fn decorated_box_ticks_without_rebuilding() {
    let c = controller();
    let decoration: std::rc::Rc<dyn Animation<BoxDecoration>> = std::rc::Rc::new(
        Tween::new(
            BoxDecoration::with_color(Color::RED),
            BoxDecoration::with_color(Color::BLUE),
        )
        .animate(scalar(&c)),
    );
    let view = DecoratedBoxTransition::new(decoration, child_box());
    ticks_without_rebuilding(view, &c, "decorated box", |tree, v| {
        let fill = tree.decoration_fill();
        if v < 1.0 {
            assert_between(f64::from(fill.red_f32()), 0.0, 1.0, "red channel");
            assert_between(f64::from(fill.blue_f32()), 0.0, 1.0, "blue channel");
        } else {
            assert_eq!(fill, Color::BLUE);
        }
    });
}

/// Under reduced motion a registered controller settles on the first tick,
/// and each transition shows its end value on that frame.
pub(crate) fn reduced_motion_lands_on_the_end_value() {
    use crate::common::lay_out_animated;
    use flui_animation::MotionOverride;
    use flui_painting::display_list::DrawOp;

    /// The settled tree and the controller's owner, kept alive with it.
    fn settle<V: View + Clone + 'static>(
        build: impl FnOnce(std::rc::Rc<dyn Animation<f64>>) -> V,
    ) -> (LaidOut, flui_animation::DrivenController) {
        let vsync = flui_animation::Vsync::new();
        let owner = AnimationController::builder(Duration::from_millis(300)).build_on(Some(&vsync));
        let controller = owner.controller();
        let mut laid = lay_out_animated(
            Align::new(Alignment::TOP_LEFT).child(build(scalar(controller))),
            loose(200.0),
            vsync,
        );
        laid.set_motion_override(MotionOverride::Reduce);
        controller.forward().expect("a fresh controller forwards");
        laid.pump_for(FRAME);
        assert_eq!(controller.value(), 1.0, "reduced motion settles the run");
        (laid, owner)
    }

    let (laid, _owner) = settle(|t| SizeTransition::new(t).child(child_box()));
    let node = laid.find_by_render_type("RenderAnimatedAlign");
    assert_eq!(laid.size(node).height, SIDE);

    let (laid, _owner) = settle(|t| {
        AlignTransition::new(
            std::rc::Rc::new(Tween::new(Alignment::TOP_LEFT, Alignment::BOTTOM_RIGHT).animate(t)),
            child_box(),
        )
    });
    let node = laid.find_by_render_type("RenderAnimatedAlign");
    let free = laid.size(node).width - SIDE;
    assert_eq!(laid.offset(laid.only_child(node)).dx, free);

    let (laid, _owner) = settle(|t| {
        Stack::new((PositionedTransition::new(
            std::rc::Rc::new(
                Tween::new(
                    RelativeRect::from_ltrb(0.0, 0.0, 0.0, 0.0),
                    RelativeRect::from_ltrb(20.0, 40.0, 60.0, 80.0),
                )
                .animate(t),
            ),
            ColoredBox::new(CHILD_FILL),
        ),))
    });
    let node = laid.find_by_render_type("RenderAnimatedInset");
    assert_eq!(laid.offset(laid.only_child(node)).dy, 40.0);

    let (laid, _owner) = settle(|t| {
        DecoratedBoxTransition::new(
            std::rc::Rc::new(
                Tween::new(
                    BoxDecoration::with_color(Color::RED),
                    BoxDecoration::with_color(Color::BLUE),
                )
                .animate(t),
            ),
            child_box(),
        )
    });
    let fill = laid
        .draw_ops()
        .into_iter()
        .find_map(|command| match command.op {
            DrawOp::Rect { paint, .. } if paint.color != CHILD_FILL => Some(paint.color),
            _ => None,
        });
    assert_eq!(fill, Some(Color::BLUE));
}

/// Two transitions on one controller both follow the same tick.
pub(crate) fn shared_controller_moves_both_transitions_on_one_tick() {
    let c = controller();
    let view = Column::new((
        ScaleTransition::new(scalar(&c)).child(child_box()).boxed(),
        ScaleTransition::new(scalar(&c)).child(child_box()).boxed(),
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
    let mut tree = Headless::mount(ScaleTransition::new(scalar(c)).child(child_box()));
    tree.laid
        .adopt_vsync(vsync)
        .expect("an open registry without a parent is admitted");
    c.forward().expect("a fresh controller forwards");
    tree.frame(FRAME);
    let report = tree.frame(Duration::from_millis(150));
    assert_no_rebuild(&report, "a time tick");
    assert_matrix(tree.matrix(0), scale_by(c.value()), "mid-run");

    let report = tree.frame(Duration::ZERO);
    assert_no_rebuild(&report, "a zero-dt frame");
    assert!(
        !tree.laid.did_paint_last_frame(),
        "a zero-dt frame paints nothing: {report:?}"
    );
    assert_matrix(tree.matrix(0), scale_by(c.value()), "after a zero-dt frame");

    c.reverse().expect("a running controller reverses");
    tree.frame(FRAME);
    let report = tree.frame(Duration::from_millis(60));
    let value = c.value();
    assert!(value < 0.5, "the reverse run moved back, got {value}");
    assert_no_rebuild(&report, "a reversing tick");
    assert_matrix(tree.matrix(0), scale_by(value), "reverse mid-run");

    tree.frame(Duration::from_secs(10));
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
        .on_tap(move |_ctx| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .child(child_box());
    let c = controller();
    c.set_value(value).expect("no build is running");
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

    use flui_animation::Vsync;
    use flui_foundation::geometry::Point;
    use flui_platform::pointer::PointerEvent;
    use flui_view::environment::VsyncScope;
    use flui_widgets::Listener;

    for (direction, sign) in [(TextDirection::Ltr, 1.0), (TextDirection::Rtl, -1.0)] {
        let vsync = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
        let controller = owner.controller();
        let received = Rc::new(RefCell::new(Vec::new()));
        let delivered = Rc::clone(&received);
        // The innermost listener receives the first Down before the recording
        // one above it and moves the animation there: the hit test already
        // selected the path, so the recording listener must still see that
        // path's mapping, not one recomputed from the newer sample.
        let armed = Rc::new(Cell::new(false));
        let mutate = Rc::clone(&armed);
        let mover = controller.clone();
        let mutating = Listener::new()
            .on_pointer_down(move |_, _| {
                if mutate.replace(false) {
                    mover.set_value(0.5).expect("no build is running");
                }
            })
            .child(child_box());
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
            .child(mutating);
        let slide = SlideTransition::new(fraction(controller, TranslationFraction::new(2.0, 0.0)))
            .child(listener)
            .text_direction(direction);
        let mut laid = lay_out_animated(
            app_root(VsyncScope::new(
                vsync.clone(),
                Align::new(Alignment::CENTER).child(slide),
            )),
            tight(200.0, 200.0),
            vsync,
        );
        let built =
            |laid: &mut LaidOut| laid.with_build_owner_mut(|owner| owner.last_frame_build_report());
        laid.pump_for(Duration::ZERO);
        controller.forward().expect("fresh slide run");
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(250));
        assert!((controller.value() - 0.25).abs() < 1e-9);
        assert_no_rebuild(&built(&mut laid), "running slide before selection");

        let first = (90.0 + sign * 20.0, 90.0);
        armed.set(true);
        laid.dispatch_pointer_down(first.0, first.1);
        laid.dispatch_pointer_up(first.0, first.1);
        assert!(
            !armed.get(),
            "{direction:?}: the inner listener moved the animation during delivery"
        );
        laid.pump_for(Duration::ZERO);
        assert_eq!(
            &*received.borrow(),
            &[(Point::new(10.0, 10.0), Point::new(first.0, first.1))],
            "{direction:?}: selected coordinates survive a newer animation sample"
        );
        assert_no_rebuild(&built(&mut laid), "committing the replacement sample");

        let second = (90.0 + sign * 40.0, 90.0);
        laid.dispatch_pointer_down(second.0, second.1);
        laid.dispatch_pointer_up(second.0, second.1);
        laid.pump_for(Duration::ZERO);
        assert_eq!(
            &*received.borrow(),
            &[
                (Point::new(10.0, 10.0), Point::new(first.0, first.1)),
                (Point::new(10.0, 10.0), Point::new(second.0, second.1)),
            ],
            "{direction:?}: the next selection uses the current sample"
        );
        assert_no_rebuild(&built(&mut laid), "the next pointer selection");
    }
}

pub(crate) fn hit_test_follows_the_painted_transform() {
    let slide = |transform_hit_tests: bool| {
        move |c: &AnimationController, child: GestureDetector| -> BoxedView {
            let view = SlideTransition::new(fraction(c, TranslationFraction::new(1.0, 0.0)))
                .child(child)
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
        let view = ScaleTransition::new(scalar(c)).child(child);
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
    first.set_value(0.5).expect("no build is running");
    let second = controller();
    second.set_value(1.0).expect("no build is running");
    let mut laid = lay_out(
        ScaleTransition::new(scalar(&first)).child(child_box()),
        loose(200.0),
    );
    let before = transform_node(&laid, 0);
    laid.pump_widget(ScaleTransition::new(scalar(&second)).child(child_box()));
    assert_eq!(
        transform_node(&laid, 0),
        before,
        "the swap must not recreate the node"
    );
    assert_maps(&laid, 0, scale_by(1.0), "after the swap");

    first.set_value(0.25).expect("no build is running");
    laid.tick();
    assert_maps(
        &laid,
        0,
        scale_by(1.0),
        "the old animation no longer drives the node",
    );
    second.set_value(0.75).expect("no build is running");
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
            ScaleTransition::new(scalar(&c)).child(child_box()).boxed(),
            ScaleTransition::new(scalar(&c)).child(child_box()).boxed(),
        )),
        loose(200.0),
    );
    c.set_value(0.5).expect("no build is running");
    laid.tick();
    laid.pump_widget(Column::new((ScaleTransition::new(scalar(&c))
        .child(child_box())
        .boxed(),)));
    assert_eq!(
        laid.find_all_by_render_type("RenderAnimatedTransform")
            .len(),
        1
    );
    c.set_value(0.25).expect("no build is running");
    laid.tick();
    assert_maps(&laid, 0, scale_by(0.25), "the surviving transition");
}

/// The tree keeps the animation alive while mounted, even after the caller
/// dropped its own handle, and releases it on unmount.
pub(crate) fn the_animation_is_released_after_unmount() {
    let c = controller();
    c.set_value(0.5).expect("no build is running");
    let animation = fraction(&c, TranslationFraction::new(1.0, 0.0));
    let weak: std::rc::Weak<dyn Animation<TranslationFraction>> =
        std::rc::Rc::downgrade(&animation);
    let mut laid = lay_out(
        SlideTransition::new(animation).child(child_box()),
        loose(200.0),
    );
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
