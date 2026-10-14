//! `Effects`, `Transition` and `Animate`: the timeline arithmetic as a pure
//! table, and every trigger through a mounted tree, read back from the laid-out
//! opacity and the composed transform of the child. `Shimmer` and `Skeleton`:
//! the shared band read back from the painted gradients.

use std::rc::Rc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, loose};
use flui_animation::{
    Animation, AnimationController, MotionOverride, Stagger, StaggerOrigin, Vsync, curves,
};
use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, Size};
use flui_objects::TranslationFraction;
use flui_painting::Alignment;
use flui_painting::display_list::DrawOp;
use flui_painting::paint::Shader;
use flui_painting::styling::Color;
use flui_testing::a11y::Role;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::motion::{
    Animate, AnimateExt, Effect, Effects, Shimmer, ShimmerBand, Skeleton, Transition, Trigger,
};
use flui_widgets::{Column, Row, SizedBox, Text, TickerMode, VsyncScope};

/// The animated child's side.
const SIDE: f64 = 40.0;

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn child() -> SizedBox {
    SizedBox::new(SIDE, SIDE)
}

/// Linear 100 ms entries, so a sample's expected value is its elapsed share.
fn linear(duration: u64) -> Effects {
    Effects::new().duration(ms(duration)).curve(curves::LINEAR)
}

fn mount(registry: &Vsync, view: impl flui_view::View) -> LaidOut {
    lay_out_animated(
        VsyncScope::new(registry.clone(), view),
        loose(400.0),
        registry.clone(),
    )
}

/// The single animated opacity in the tree.
fn opacity(laid: &LaidOut) -> f64 {
    let nodes = laid.find_all_by_render_type("RenderAnimatedOpacity");
    assert_eq!(nodes.len(), 1, "one fade stage");
    laid.opacity(nodes[0])
}

/// Where the composed stages map the child's local point `(x, y)` in the
/// coordinate space of `top`.
fn map_from(laid: &LaidOut, top: RenderId, (x, y): (f64, f64)) -> (f64, f64) {
    let mut leaf = top;
    while !laid.children(leaf).is_empty() {
        leaf = laid.only_child(leaf);
    }
    laid.pipeline_owner()
        .with(|owner| owner.transform_to(leaf, top))
        .expect("the child is laid out below the stages")
        .transform_point(x, y)
}

fn map(laid: &LaidOut, point: (f64, f64)) -> (f64, f64) {
    map_from(laid, laid.current_root(), point)
}

fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} ± {tolerance}, got {actual}"
    );
}

/// Entries inherit delay, duration and curve; `then` moves the base to the
/// previous end; the stagger shifts the whole timeline; the total is the
/// latest end, not the last entry's.
pub(crate) fn effects_timeline_places_each_entry() {
    let stagger = Stagger::new(ms(30), StaggerOrigin::First);
    // (case, timeline, expected spans in ms, expected total in ms)
    type Row = (&'static str, Effects, &'static [(u64, u64)], u64);
    let rows: [Row; 9] = [
        ("empty", Effects::new(), &[], 0),
        ("default timing", Effects::new().fade_in(), &[(0, 200)], 200),
        (
            "duration carries to later entries",
            Effects::new().duration(ms(300)).fade_in().slide_y(0.2),
            &[(0, 300), (0, 300)],
            300,
        ),
        (
            "delay carries to later entries",
            Effects::new().delay(ms(100)).fade_in().scale(0.9),
            &[(100, 300), (100, 300)],
            300,
        ),
        (
            "then starts at the previous end",
            Effects::new().duration(ms(300)).fade_in().then().scale(0.9),
            &[(0, 300), (300, 600)],
            600,
        ),
        (
            "then resets the delay and keeps the duration",
            Effects::new()
                .delay(ms(50))
                .duration(ms(100))
                .fade_in()
                .then()
                .scale(0.9)
                .delay(ms(10))
                .rotate(0.25),
            &[(50, 150), (150, 250), (160, 260)],
            260,
        ),
        (
            "then before any entry keeps the start",
            Effects::new().then().fade_in(),
            &[(0, 200)],
            200,
        ),
        (
            "total is the latest end",
            Effects::new()
                .duration(ms(500))
                .fade_in()
                .duration(ms(100))
                .delay(ms(50))
                .scale(0.9),
            &[(0, 500), (50, 150)],
            500,
        ),
        (
            "stagger delays every entry",
            Effects::new()
                .duration(ms(100))
                .fade_in()
                .then()
                .scale(0.9)
                .stagger(stagger, 2, 5),
            &[(60, 160), (160, 260)],
            260,
        ),
    ];
    for (name, effects, spans, total) in rows {
        let expected: Vec<_> = spans.iter().map(|&(a, b)| ms(a)..ms(b)).collect();
        assert_eq!(
            effects.spans().collect::<Vec<_>>(),
            expected,
            "{name}: spans"
        );
        assert_eq!(effects.total(), ms(total), "{name}: total");
    }
}

/// `a + b` keeps each slot from the right operand when it fills it, from the
/// left otherwise.
pub(crate) fn transition_merge_keeps_the_right_operand_per_slot() {
    let a = Transition::fade() + Transition::slide_in(TranslationFraction::new(0.0, 0.5));
    let b = Transition::effect(Effect::Fade { from: 0.5, to: 1.0 });
    // At rest hidden, a transition shows its entry's starting pose.
    let rows = [
        (
            "a + b: b's fade, a's slide",
            a.clone() + b.clone(),
            0.5,
            20.0,
        ),
        ("b + a: a's fade, a's slide", b + a.clone(), 0.0, 20.0),
        (
            "a + scale: a's fade and slide survive",
            a + Transition::scale_in(1.0),
            0.0,
            20.0,
        ),
    ];
    for (name, transition, alpha, dy) in rows {
        let registry = Vsync::new();
        let laid = mount(
            &registry,
            Animate::transition(transition, child()).trigger(Trigger::Target(0.0)),
        );
        close(opacity(&laid), alpha, 1e-9, name);
        close(map(&laid, (0.0, 0.0)).1, dy, 1e-9, name);
    }
}

/// A scale about a corner keeps that corner fixed; the slide composes after it.
pub(crate) fn a_scale_effect_keeps_its_alignment_fixed() {
    let registry = Vsync::new();
    let scale = Effect::Scale {
        from: 0.5,
        alignment: Alignment::BOTTOM_RIGHT,
    };
    let laid = mount(
        &registry,
        Animate::new(Effects::new().effect(scale).slide_x(0.25), child())
            .trigger(Trigger::Target(0.0)),
    );
    let corner = map(&laid, (SIDE, SIDE));
    close(
        corner.0,
        SIDE + SIDE / 4.0,
        1e-9,
        "the corner, then the slide",
    );
    close(corner.1, SIDE, 1e-9, "the corner stays put vertically");
    let origin = map(&laid, (0.0, 0.0));
    close(
        origin.0,
        SIDE / 2.0 + SIDE / 4.0,
        1e-9,
        "the far corner moves in",
    );
}

/// Mounting plays the timeline once: the first entry, then the one after
/// `then`, each from its start look to its rest.
pub(crate) fn on_mount_plays_the_timeline_in_order() {
    let registry = Vsync::new();
    let mut laid = mount(
        &registry,
        child().animate(linear(100).fade_in().then().slide_y(1.0)),
    );
    close(opacity(&laid), 0.0, 1e-9, "mounted at the start");
    close(map(&laid, (0.0, 0.0)).1, SIDE, 1e-9, "the slide waits");

    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    close(opacity(&laid), 0.5, 0.05, "halfway through the fade");
    close(
        map(&laid, (0.0, 0.0)).1,
        SIDE,
        1e-9,
        "the slide still waits",
    );

    laid.pump_for(ms(100));
    close(opacity(&laid), 1.0, 1e-9, "the fade is done");
    close(
        map(&laid, (0.0, 0.0)).1,
        SIDE / 2.0,
        2.0,
        "halfway through the slide",
    );

    laid.pump_for(ms(100));
    close(map(&laid, (0.0, 0.0)).1, 0.0, 1e-9, "the slide is done");
}

/// `OnChange` replays from the start for a new value, and only then.
pub(crate) fn on_change_replays_only_for_a_new_value() {
    let registry = Vsync::new();
    let tree = |value| {
        VsyncScope::new(
            registry.clone(),
            Animate::new(linear(100).fade_in(), child()).trigger(Trigger::OnChange(value)),
        )
    };
    let mut laid = lay_out_animated(tree(1), loose(400.0), registry.clone());
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(200));
    close(opacity(&laid), 1.0, 1e-9, "the mount run finished");

    laid.pump_widget(tree(1));
    laid.pump_for(ms(20));
    close(opacity(&laid), 1.0, 1e-9, "the same value does not replay");

    laid.pump_widget(tree(2));
    close(
        opacity(&laid),
        0.0,
        1e-9,
        "a new value restarts at the start",
    );
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    close(opacity(&laid), 0.5, 0.05, "and plays again");
}

/// A target reached mid-run reverses from the displayed pose along the same
/// side of an asymmetric transition; from rest, each direction plays its side.
pub(crate) fn target_reverses_mid_flight_without_a_jump() {
    let registry = Vsync::new();
    let transition = Transition::asymmetric(Transition::fade(), Transition::scale_in(0.5))
        .duration(ms(100))
        .curve(curves::LINEAR);
    let tree = |position| {
        VsyncScope::new(
            registry.clone(),
            Animate::transition(transition.clone(), child()).trigger(Trigger::Target(position)),
        )
    };
    let scale = |laid: &LaidOut| {
        let (a, b) = (map(laid, (0.0, 0.0)), map(laid, (SIDE, 0.0)));
        (b.0 - a.0) / SIDE
    };
    let mut laid = lay_out_animated(tree(0.0), loose(400.0), registry.clone());
    close(opacity(&laid), 0.0, 1e-9, "hidden at rest");

    laid.pump_widget(tree(1.0));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    let seam = opacity(&laid);
    assert!(seam > 0.2 && seam < 0.8, "entering: {seam}");
    close(scale(&laid), 1.0, 1e-9, "the entry does not scale");

    laid.pump_widget(tree(0.0));
    laid.pump_for(Duration::ZERO);
    close(
        opacity(&laid),
        seam,
        1e-9,
        "the reversal starts at the seam",
    );
    close(
        scale(&laid),
        1.0,
        1e-9,
        "the reversal keeps the entry's effects",
    );
    laid.pump_for(ms(5));
    let next = opacity(&laid);
    assert!(
        next <= seam && seam - next < 0.1,
        "moving back continuously: {seam} -> {next}"
    );
    laid.pump_for(ms(200));
    close(opacity(&laid), 0.0, 1e-9, "back to hidden");

    laid.pump_widget(tree(1.0));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(200));
    close(opacity(&laid), 1.0, 1e-9, "shown");
    laid.pump_widget(tree(0.0));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    close(opacity(&laid), 1.0, 1e-9, "the exit does not fade");
    let exiting = scale(&laid);
    assert!(
        exiting > 0.55 && exiting < 0.95,
        "the exit scales: {exiting}"
    );
}

/// The same effects staggered by index start one step apart.
pub(crate) fn staggered_effects_start_one_step_apart() {
    let registry = Vsync::new();
    let stagger = Stagger::new(ms(100), StaggerOrigin::First);
    let children: Vec<BoxedView> = (0..3)
        .map(|index| {
            child()
                .animate(linear(100).fade_in().stagger(stagger, index, 3))
                .boxed()
        })
        .collect();
    let mut laid = mount(&registry, Column::new(children));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(150));
    let column = laid.current_root();
    let alphas: Vec<f64> = laid
        .children(column)
        .into_iter()
        .map(|fade| laid.opacity(fade))
        .collect();
    close(alphas[0], 1.0, 1e-9, "the first finished");
    close(alphas[1], 0.5, 0.05, "the second is halfway");
    close(alphas[2], 0.0, 1e-9, "the third has not started");
}

/// A caller's controller is the progress itself.
pub(crate) fn a_controller_trigger_follows_its_animation() {
    let registry = Vsync::new();
    let controller = AnimationController::builder(ms(100)).build();
    let progress: Rc<dyn Animation<f64>> = Rc::new(controller.clone());
    let mut laid = mount(
        &registry,
        child()
            .animate(linear(100).fade_in())
            .trigger(Trigger::Controller(progress)),
    );
    close(opacity(&laid), 0.0, 1e-9, "at the controller's start");
    controller.set_value(0.25);
    laid.pump_for(Duration::ZERO);
    close(opacity(&laid), 0.25, 1e-9, "at the controller's value");
}

/// Under reduced motion a mounting effect paints its end on the first frame;
/// at full motion the same mount paints its start.
pub(crate) fn reduced_motion_paints_the_end_on_the_first_frame() {
    for (policy, alpha, dy) in [
        (MotionOverride::Full, 0.0, SIDE),
        (MotionOverride::Reduce, 1.0, 0.0),
    ] {
        let registry = Vsync::new();
        let tree = |shown: bool| -> BoxedView {
            let content = if shown {
                child()
                    .animate(
                        Effects::new()
                            .duration(Duration::from_secs(10))
                            .fade_in()
                            .slide_y(1.0),
                    )
                    .boxed()
            } else {
                child().boxed()
            };
            VsyncScope::new(registry.clone(), content).boxed()
        };
        let mut laid = lay_out_animated(tree(false), loose(400.0), registry.clone());
        laid.set_motion_override(policy);
        laid.pump_widget(tree(true));
        laid.pump_for(Duration::ZERO);
        close(opacity(&laid), alpha, 1e-6, &format!("{policy:?}: opacity"));
        close(
            map(&laid, (0.0, 0.0)).1,
            dy,
            1e-3,
            &format!("{policy:?}: slide"),
        );
    }
}

/// Scales compose without intermediate overflow: a zero scale collapses the
/// child whatever huge scales precede it.
pub(crate) fn extreme_scales_compose_without_overflow() {
    let scale = |from| Effect::Scale {
        from,
        alignment: Alignment::CENTER,
    };
    let rows = [
        ("zero after two huge scales", [f64::MAX, f64::MAX, 0.0], 0.0),
        (
            "zero before two huge scales",
            [0.0, f64::MAX, f64::MAX],
            0.0,
        ),
        ("ordinary scales multiply", [0.5, 0.5, 2.0], 0.5),
    ];
    for (name, scales, expected) in rows {
        let registry = Vsync::new();
        let effects = scales
            .into_iter()
            .fold(Effects::new(), |effects, from| effects.effect(scale(from)));
        let laid = mount(
            &registry,
            Animate::new(effects, child()).trigger(Trigger::Target(0.0)),
        );
        let (left, right) = (map(&laid, (0.0, 0.0)), map(&laid, (SIDE, 0.0)));
        close((right.0 - left.0) / SIDE, expected, 1e-9, name);
    }
}

/// Handing progress to a caller's controller stops the owned run, so the
/// presentation stops requesting frames for it.
pub(crate) fn adopting_a_controller_stops_the_owned_run() {
    let registry = Vsync::new();
    let idle: Rc<dyn Animation<f64>> = Rc::new(AnimationController::builder(ms(100)).build());
    let tree = |trigger| {
        VsyncScope::new(
            registry.clone(),
            Animate::new(
                Effects::new().duration(Duration::from_secs(10)).fade_in(),
                child(),
            )
            .trigger(trigger),
        )
    };
    let mut laid = lay_out_animated(tree(Trigger::OnMount), loose(400.0), registry.clone());
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(20));
    assert!(registry.has_running(), "the mount run is playing");
    laid.pump_widget(tree(Trigger::Controller(Rc::clone(&idle))));
    laid.pump_for(ms(20));
    assert!(
        !registry.has_running(),
        "no seat keeps running once the controller drives progress"
    );
}

/// A controller whose status-subscription removal panics.
#[derive(Debug)]
struct RemovalPanics {
    inner: AnimationController,
    anchor: Rc<()>,
}

impl flui_foundation::Listenable for RemovalPanics {
    fn add_observer(
        &self,
        observer: flui_foundation::ListenerObserver,
    ) -> flui_foundation::ListenerId {
        self.inner.add_observer(observer)
    }

    fn remove_listener(&self, id: flui_foundation::ListenerId) {
        self.inner.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.inner.remove_all_listeners();
    }
}

impl Animation<f64> for RemovalPanics {
    fn value(&self) -> f64 {
        self.inner.value()
    }

    fn status(&self) -> flui_animation::AnimationStatus {
        self.inner.status()
    }

    fn is_animating(&self) -> bool {
        self.inner.is_animating()
    }

    fn observe_status(
        &self,
        _observer: flui_animation::StatusObserver,
    ) -> flui_animation::StatusSubscription {
        fn remove(_: &(), (): (), _: &mut flui_foundation::panic::RecoveryScope<'_>) {
            panic!("status subscription removal panics");
        }
        flui_animation::StatusSubscription::new(&self.anchor, (), remove)
    }
}

/// A panicking subscription removal while progress returns from a caller's
/// controller still completes the handoff: the owned run takes over from the
/// controller's value.
pub(crate) fn a_panicking_controller_release_still_hands_progress_back() {
    let registry = Vsync::new();
    let inner = AnimationController::builder(ms(100)).build();
    inner.set_value(0.5);
    let controller: Rc<dyn Animation<f64>> = Rc::new(RemovalPanics {
        inner,
        anchor: Rc::new(()),
    });
    let tree = |trigger| {
        VsyncScope::new(
            registry.clone(),
            Animate::new(linear(100).fade_in(), child()).trigger(trigger),
        )
    };
    let mut laid = lay_out_animated(
        tree(Trigger::Controller(controller)),
        loose(400.0),
        registry.clone(),
    );
    close(opacity(&laid), 0.5, 1e-9, "at the controller's value");
    // The lifecycle boundary contains the removal panic; the identical
    // rebuild after it changes nothing.
    laid.pump_widget(tree(Trigger::Target(1.0)));
    laid.pump_widget(tree(Trigger::Target(1.0)));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(200));
    close(opacity(&laid), 1.0, 1e-9, "the owned run took over");
}

// ── Shimmer and Skeleton ──

/// A painted rounded rectangle's colour and gradient `from`/`to` (`None`
/// when flat).
type Fill = (Color, Option<(Offset, Offset)>);

/// Every rounded rectangle painted in the last frame, in paint order.
fn skeleton_fills(laid: &LaidOut) -> Vec<Fill> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::RRect { paint, .. } => Some((
                paint.color,
                match &paint.shader {
                    Some(Shader::LinearGradient { from, to, .. }) => Some((*from, *to)),
                    _ => None,
                },
            )),
            _ => None,
        })
        .collect()
}

/// Two 50 px skeletons with a 30 px gap: the second sits 80 px right of the
/// first.
fn two_skeletons() -> Row {
    Row::new(vec![
        Skeleton::rect(Size::new(50.0, 20.0)).boxed(),
        SizedBox::new(30.0, 20.0).boxed(),
        Skeleton::rect(Size::new(50.0, 20.0)).boxed(),
    ])
}

/// Every skeleton under one shimmer cuts its window out of one band: their
/// gradients differ by exactly their offset from each other, frame after
/// frame, and the band moves with the phase. Quarter periods keep every
/// coordinate exact in `f64`.
pub(crate) fn skeletons_share_one_band() {
    let registry = Vsync::new();
    let mut laid = mount(&registry, Shimmer::new(two_skeletons()));
    laid.pump_for(Duration::ZERO);
    let mut previous = None;
    for _ in 0..3 {
        laid.pump_for(ms(375));
        let fills = skeleton_fills(&laid);
        assert_eq!(fills.len(), 2, "{fills:?}");
        let first = fills[0].1.expect("the first skeleton shimmers");
        let second = fills[1].1.expect("the second skeleton shimmers");
        assert_eq!(first.0.dx - second.0.dx, 80.0, "{first:?} vs {second:?}");
        assert_eq!(first.1.dx - second.1.dx, 80.0, "{first:?} vs {second:?}");
        assert_ne!(previous, Some(first), "the band moves with the phase");
        previous = Some(first);
    }
}

/// One sweep takes 1.5 s by default: half way through, the band (two fifths
/// of the 400 px shimmer) leads at 0.5 × 560 − 160 = 120 px, and a whole
/// period later it is back where it started.
pub(crate) fn a_shimmer_sweeps_once_per_default_period() {
    let registry = Vsync::new();
    let mut laid = mount(&registry, Shimmer::new(two_skeletons()));
    laid.pump_for(Duration::ZERO);
    let lead = |laid: &LaidOut| skeleton_fills(laid)[0].1.expect("shimmering").0.dx;
    close(
        lead(&laid),
        -160.0,
        1e-9,
        "the band starts before the leading edge",
    );
    laid.pump_for(ms(750));
    close(lead(&laid), 120.0, 1e-9, "half a period");
    laid.pump_for(ms(750));
    close(lead(&laid), -160.0, 1e-9, "a whole period");
}

/// Pumps two frames and checks every skeleton painted flat both times.
fn assert_flat(laid: &mut LaidOut, what: &str) {
    for _ in 0..2 {
        laid.pump_for(ms(300));
        let fills = skeleton_fills(laid);
        assert_eq!(fills.len(), 2, "{what}: {fills:?}");
        assert!(
            fills.iter().all(|(_, gradient)| gradient.is_none()),
            "{what}: a still shimmer paints flat: {fills:?}"
        );
    }
}

fn flat_under_reduced_motion() {
    let registry = Vsync::new();
    let mut laid = mount(&registry, SizedBox::shrink());
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_widget(VsyncScope::new(registry, Shimmer::new(two_skeletons())));
    laid.pump_for(Duration::ZERO);
    assert_flat(&mut laid, "reduced motion");
}

fn flat_once_motion_is_reduced_mid_sweep() {
    let registry = Vsync::new();
    let mut laid = mount(&registry, Shimmer::new(two_skeletons()));
    laid.set_motion_override(MotionOverride::Full);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(300));
    assert!(skeleton_fills(&laid)[0].1.is_some(), "full motion sweeps");
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::ZERO);
    assert_flat(&mut laid, "reduced while running");
}

fn flat_when_disabled() {
    let registry = Vsync::new();
    let mut laid = mount(&registry, Shimmer::new(two_skeletons()).enabled(false));
    laid.pump_for(Duration::ZERO);
    assert_flat(&mut laid, "enabled(false)");
    assert!(
        !registry.has_running(),
        "a disabled shimmer asks for no frames"
    );
}

fn flat_under_a_disabled_ticker_mode() {
    let registry = Vsync::new();
    let mut laid = mount(
        &registry,
        TickerMode::new(Shimmer::new(two_skeletons())).enabled(false),
    );
    laid.pump_for(Duration::ZERO);
    assert_flat(&mut laid, "disabled ticker mode");
}

fn flat_without_a_clock() {
    let registry = Vsync::new();
    let mut laid = mount(
        &registry,
        VsyncScope::detached(Shimmer::new(two_skeletons())),
    );
    laid.pump_for(Duration::ZERO);
    assert_flat(&mut laid, "no clock");
}

/// A shimmer that holds still paints every skeleton in its flat base
/// colour: reduced motion (from the start and mid-sweep), `enabled(false)`,
/// a disabled ticker mode and a missing clock.
pub(crate) fn a_still_shimmer_paints_skeletons_flat() {
    crate::common::cases::run_cases(
        "still shimmer",
        &[
            ("reduced motion", flat_under_reduced_motion),
            ("reduced mid-sweep", flat_once_motion_is_reduced_mid_sweep),
            ("enabled(false)", flat_when_disabled),
            ("disabled ticker mode", flat_under_a_disabled_ticker_mode),
            ("no clock", flat_without_a_clock),
        ],
    );
}

/// Assistive technology hears one loading indicator for the whole
/// placeholder region and none of the placeholders inside it.
pub(crate) fn a_shimmer_announces_one_loading_region() {
    let registry = Vsync::new();
    let mut laid = mount(
        &registry,
        Shimmer::new(Column::new((Text::new("Inbox"), Skeleton::text(2)))).label("Loading mail"),
    );
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let region = tree
        .find_by_label("Loading mail")
        .expect("the shimmer announces the loading region");
    assert_eq!(region.role(), Role::ProgressIndicator);
    assert!(
        tree.find_by_label("Inbox").is_err(),
        "placeholder content is not announced"
    );
}

/// The shimmer effect sweeps a shader mask over arbitrary content once:
/// masked mid-sweep, unmasked before and after.
pub(crate) fn a_shimmer_effect_masks_the_child_mid_sweep() {
    let registry = Vsync::new();
    let mut laid = mount(
        &registry,
        Animate::new(linear(100).shimmer(ShimmerBand::new(Color::WHITE)), child()),
    );
    let masked = |laid: &LaidOut| laid.layer_kinds().contains(&"ShaderMask");
    laid.pump_for(Duration::ZERO);
    assert!(!masked(&laid), "the band starts off the child");
    laid.pump_for(ms(50));
    assert!(masked(&laid), "{:?}", laid.layer_kinds());
    laid.pump_for(ms(60));
    assert!(!masked(&laid), "the band rests past the child");
}
