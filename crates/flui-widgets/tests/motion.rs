//! `Effects`, `Transition`, `Animate` and `AnimatedSwitcher`: the timeline
//! arithmetic as a pure table, and every trigger and switch through a mounted
//! tree, read back from the laid-out opacity and the composed transform of the
//! child.

use std::rc::Rc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, loose};
use flui_animation::{
    Animation, AnimationController, MotionOverride, Stagger, StaggerOrigin, Vsync, curves,
};
use flui_foundation::RenderId;
use flui_objects::TranslationFraction;
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::motion::{Animate, AnimateExt, Effect, Effects, Transition, Trigger};
use flui_widgets::{AnimatedSwitcher, ColoredBox, Column, SizedBox, VsyncScope};

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

/// The side of the switcher's second child, a coloured box of a different
/// type than the first, so swapping them starts a transition.
const SMALL: f64 = SIDE / 2.0;

fn first() -> BoxedView {
    child().boxed()
}

fn second() -> BoxedView {
    ColoredBox::new(Color::rgb(10, 20, 30))
        .child(SizedBox::new(SMALL, SMALL))
        .boxed()
}

/// A switcher whose own duration is far longer than any transition here, so
/// a run that settles in time follows the transition's duration.
fn switcher(registry: &Vsync, transition: &Transition, shown: BoxedView) -> VsyncScope {
    VsyncScope::new(
        registry.clone(),
        AnimatedSwitcher::new(Duration::from_secs(10))
            .transition(transition.clone())
            .child(shown),
    )
}

/// The fade stage of each switcher entry showing a child of `side`.
fn entries_of(laid: &LaidOut, side: f64) -> Vec<RenderId> {
    laid.find_all_by_render_type("RenderAnimatedOpacity")
        .into_iter()
        .filter(|&fade| {
            let mut leaf = fade;
            while !laid.children(leaf).is_empty() {
                leaf = laid.only_child(leaf);
            }
            (laid.size(leaf).width - side).abs() < 1e-9
        })
        .collect()
}

/// The single entry showing a child of `side`: its opacity and scale.
fn entry_pose(laid: &LaidOut, side: f64) -> (f64, f64) {
    let fades = entries_of(laid, side);
    assert_eq!(fades.len(), 1, "one entry shows the {side} child");
    let fade = fades[0];
    let (a, b) = (
        map_from(laid, fade, (0.0, 0.0)),
        map_from(laid, fade, (side, 0.0)),
    );
    (laid.opacity(fade), (b.0 - a.0) / side)
}

/// Enter fades, exit scales: the asymmetric transition shows which side plays.
fn fade_in_scale_out() -> Transition {
    Transition::asymmetric(Transition::fade(), Transition::scale_in(0.5))
        .duration(ms(100))
        .curve(curves::LINEAR)
}

/// A switcher given a transition plays its entry on the incoming child and its
/// exit on the outgoing one, over the transition's duration rather than the
/// switcher's, and drops the outgoing child once the exit settles.
pub(crate) fn a_switcher_plays_its_transition_on_both_sides() {
    let registry = Vsync::new();
    let transition = fade_in_scale_out();
    let mut laid = lay_out_animated(
        switcher(&registry, &transition, first()),
        loose(400.0),
        registry.clone(),
    );
    close(
        entry_pose(&laid, SIDE).0,
        1.0,
        1e-9,
        "the first child rests shown",
    );

    laid.pump_widget(switcher(&registry, &transition, second()));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    let (alpha, scale) = entry_pose(&laid, SMALL);
    close(alpha, 0.5, 0.1, "the incoming child fades in");
    close(scale, 1.0, 1e-9, "the entry does not scale");
    let (alpha, scale) = entry_pose(&laid, SIDE);
    close(alpha, 1.0, 1e-9, "the exit does not fade");
    close(scale, 0.75, 0.05, "the outgoing child scales out");

    laid.pump_for(ms(100));
    laid.pump_for(Duration::ZERO);
    assert!(entries_of(&laid, SIDE).is_empty(), "the exit has settled");
    close(
        entry_pose(&laid, SMALL).0,
        1.0,
        1e-9,
        "the entry has settled",
    );
}

/// Switching back to a child mid-exit takes that entry back: its exit reverses
/// from the pose it reached, along the exit's effects, instead of a fresh
/// entry fading in beside it.
pub(crate) fn switching_back_mid_exit_reverses_from_current_progress() {
    let registry = Vsync::new();
    let transition = fade_in_scale_out();
    let mut laid = lay_out_animated(
        switcher(&registry, &transition, first()),
        loose(400.0),
        registry.clone(),
    );
    laid.pump_widget(switcher(&registry, &transition, second()));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    let (_, seam) = entry_pose(&laid, SIDE);
    assert!(seam > 0.6 && seam < 0.9, "mid-exit: {seam}");

    laid.pump_widget(switcher(&registry, &transition, first()));
    laid.pump_for(Duration::ZERO);
    let (alpha, scale) = entry_pose(&laid, SIDE);
    close(alpha, 1.0, 1e-9, "the returning child does not fade in");
    close(scale, seam, 1e-9, "the reversal starts at the seam");
    laid.pump_for(ms(5));
    let (_, next) = entry_pose(&laid, SIDE);
    assert!(
        next >= seam && next - seam < 0.05,
        "growing back continuously: {seam} -> {next}"
    );

    laid.pump_for(ms(200));
    laid.pump_for(Duration::ZERO);
    let (alpha, scale) = entry_pose(&laid, SIDE);
    close(alpha, 1.0, 1e-9, "shown");
    close(scale, 1.0, 1e-9, "back at full size");
    assert!(
        entries_of(&laid, SMALL).is_empty(),
        "the interrupted entry has left"
    );
}

/// Under reduced motion a switch shows the new child in place on its first
/// frame and drops the old one; at full motion the same switch has only begun.
pub(crate) fn reduced_motion_switches_at_once() {
    let transition = Transition::fade() + Transition::slide_in(TranslationFraction::new(0.0, 1.0));
    for (policy, alpha, dy, remaining) in [
        (MotionOverride::Full, 0.0, SMALL, 1),
        (MotionOverride::Reduce, 1.0, 0.0, 0),
    ] {
        let registry = Vsync::new();
        let mut laid = lay_out_animated(
            switcher(&registry, &transition, first()),
            loose(400.0),
            registry.clone(),
        );
        laid.set_motion_override(policy);
        laid.pump_widget(switcher(&registry, &transition, second()));
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::ZERO);
        close(
            entry_pose(&laid, SMALL).0,
            alpha,
            1e-6,
            &format!("{policy:?}: the incoming child"),
        );
        let incoming = entries_of(&laid, SMALL)[0];
        close(
            map_from(&laid, incoming, (0.0, 0.0)).1,
            dy,
            1e-6,
            &format!("{policy:?}: the incoming slide"),
        );
        assert_eq!(
            entries_of(&laid, SIDE).len(),
            remaining,
            "{policy:?}: the outgoing child"
        );
    }
}
