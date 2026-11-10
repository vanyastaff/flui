//! The motion gate (ADR-0321) through its public surface: each rule fires on the motion it names
//! and stays quiet on the animated version of the same change, findings are charged to the
//! outermost node that shows them, and the allowlist refuses rows that match nothing.

use std::time::Duration;

use flui_animation::{SpringDescription, Vsync};
use flui_foundation::geometry::{EdgeInsets, Rect};
use flui_interaction::events::PointerEvent;
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_testing::motion::{
    Allow, MotionConfig, MotionRun, Recording, Rule, assert_smooth, charge_to_outermost, check,
    check_all, gate,
};
use flui_testing::widgets::{LaidOut, lay_out_animated, loose};
use flui_view::environment::VsyncScope;
use flui_view::prelude::*;
use flui_view::{IntoView, View, ViewExt};
use flui_widgets::navigator::{Navigator, NavigatorHandle, PopupRoute, SimpleRoute};
use flui_widgets::{
    Align, AnimatedLerp, AnimatedOpacity, AnimatedPadding, ClipRect, ColoredBox, HitTestBehavior,
    Listener, MotionBuilder, Opacity, Padding, SizedBox, UnconstrainedBox,
};

const FRAME: Duration = Duration::from_millis(16);
const RUN: Duration = Duration::from_millis(200);

fn square() -> impl IntoView {
    ColoredBox::new(Color::rgb(200, 40, 40)).child(SizedBox::new(40.0, 40.0))
}

/// Mounts `tree(from)`, changes it to `tree(to)` and plays a second, recording every frame.
fn record<V: View>(vsync: &Vsync, tree: impl Fn(f64) -> V, from: f64, to: f64) -> Recording {
    let mut laid = lay_out_animated(tree(from), loose(200.0), vsync.clone());
    run_change(&mut laid, || tree(to), Duration::from_secs(1))
}

fn run_change<V: View>(laid: &mut LaidOut, next: impl FnOnce() -> V, play: Duration) -> Recording {
    let mut run = MotionRun::new(laid);
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(next()));
    run.play(play, FRAME);
    run.finish()
}

fn rules(recording: &Recording) -> Vec<Rule> {
    check(recording, &MotionConfig::default())
        .into_iter()
        .map(|finding| finding.rule)
        .collect()
}

fn padding(vsync: &Vsync, animated: bool) -> Recording {
    record(
        vsync,
        |inset| {
            let content = if animated {
                AnimatedPadding::new(EdgeInsets::all(inset), square())
                    .duration(RUN)
                    .boxed()
            } else {
                Padding::all(inset).child(square()).boxed()
            };
            VsyncScope::new(vsync.clone(), content)
        },
        0.0,
        60.0,
    )
}

pub(crate) fn a_padding_change_jumps_unless_it_animates() {
    let vsync = Vsync::new();
    let instant = rules(&padding(&vsync, false));
    assert!(
        instant.contains(&Rule::Jump),
        "an instant move jumps: {instant:?}"
    );

    let vsync = Vsync::new();
    let animated = rules(&padding(&vsync, true));
    assert!(
        animated.is_empty(),
        "an animated move is smooth: {animated:?}"
    );
}

pub(crate) fn an_opacity_change_snaps_unless_it_fades() {
    let opacity = |vsync: &Vsync, animated: bool| {
        record(
            vsync,
            |value| {
                let content = if animated {
                    AnimatedOpacity::new(value, square()).duration(RUN).boxed()
                } else {
                    Opacity::new(value).child(square()).boxed()
                };
                VsyncScope::new(vsync.clone(), content)
            },
            1.0,
            0.2,
        )
    };
    let vsync = Vsync::new();
    let instant = rules(&opacity(&vsync, false));
    assert!(
        instant.contains(&Rule::FadeSnap),
        "an instant fade snaps: {instant:?}"
    );

    let vsync = Vsync::new();
    let animated = rules(&opacity(&vsync, true));
    assert!(
        animated.is_empty(),
        "an animated fade is smooth: {animated:?}"
    );
}

/// A square shown (`opacity` = its entry opacity) or not, at the root of the tree.
fn maybe_square(vsync: &Vsync, shown: bool, opacity: f64) -> impl View {
    let content = if shown {
        AnimatedOpacity::new(opacity, square())
            .duration(RUN)
            .boxed()
    } else {
        SizedBox::new(0.0, 0.0).boxed()
    };
    VsyncScope::new(vsync.clone(), content)
}

pub(crate) fn a_node_that_appears_at_once_pops_in_and_one_that_enters_does_not() {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        maybe_square(&vsync, false, 1.0),
        loose(200.0),
        vsync.clone(),
    );
    let popped = rules(&run_change(
        &mut laid,
        || maybe_square(&vsync, true, 1.0),
        Duration::from_secs(1),
    ));
    assert!(
        popped.contains(&Rule::PopIn),
        "mounting opaque pops in: {popped:?}"
    );

    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        maybe_square(&vsync, false, 1.0),
        loose(200.0),
        vsync.clone(),
    );
    let mut run = MotionRun::new(&mut laid);
    run.act(|laid| laid.pump_widget(maybe_square(&vsync, true, 0.0)));
    run.act(|laid| laid.pump_widget(maybe_square(&vsync, true, 1.0)));
    run.play(Duration::from_secs(1), FRAME);
    let entered = rules(&run.finish());
    assert!(
        entered.is_empty(),
        "mounting transparent and fading in enters: {entered:?}"
    );
}

pub(crate) fn a_node_that_vanishes_pops_out() {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(maybe_square(&vsync, true, 1.0), loose(200.0), vsync.clone());
    let vanished = rules(&run_change(
        &mut laid,
        || maybe_square(&vsync, false, 1.0),
        Duration::from_secs(1),
    ));
    assert!(
        vanished.contains(&Rule::PopOut),
        "unmounting opaque pops out: {vanished:?}"
    );
}

pub(crate) fn an_opacity_dip_that_comes_back_flickers() {
    let tree = |value: f64| Opacity::new(value).child(square());
    let mut laid = lay_out_animated(tree(1.0), loose(200.0), Vsync::new());
    let mut run = MotionRun::new(&mut laid);
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(0.6)));
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(1.0)));
    run.play(RUN, FRAME);
    let found = rules(&run.finish());
    assert!(
        found.contains(&Rule::Flicker),
        "a dip that comes back flickers: {found:?}"
    );
    assert!(
        !found.contains(&Rule::FadeSnap),
        "a 0.4 step is under the fade-snap threshold: {found:?}"
    );

    // The same dip over several small steps: no step snaps, the dip still flickers.
    let vsync = Vsync::new();
    let tree = |value: f64| {
        VsyncScope::new(
            vsync.clone(),
            AnimatedOpacity::new(value, square()).duration(Duration::from_millis(100)),
        )
    };
    let mut laid = lay_out_animated(tree(1.0), loose(200.0), vsync.clone());
    let mut run = MotionRun::new(&mut laid);
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(0.6)));
    run.play(Duration::from_millis(112), FRAME);
    run.act(|laid| laid.pump_widget(tree(1.0)));
    run.play(Duration::from_secs(1), FRAME);
    let found = rules(&run.finish());
    assert!(
        found.contains(&Rule::Flicker),
        "a gradual dip that comes back flickers: {found:?}"
    );
    assert!(
        !found.contains(&Rule::FadeSnap),
        "no step of a gradual dip snaps: {found:?}"
    );
}

pub(crate) fn a_blink_through_opacity_zero_flickers() {
    // Opacity 0 skips painting its subtree: the square is left out of one frame, not drawn at
    // opacity 0, and the gap still reads as a dip.
    let tree = |value: f64| Opacity::new(value).child(square());
    let mut laid = lay_out_animated(tree(1.0), loose(200.0), Vsync::new());
    let mut run = MotionRun::new(&mut laid);
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(0.0)));
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(1.0)));
    run.play(RUN, FRAME);
    let recording = run.finish();
    let found = check_all(&recording, &MotionConfig::default());
    assert!(
        found.iter().any(|finding| finding.rule == Rule::Flicker
            && finding.locator.ends_with("RenderDecoratedBox")),
        "the square blinks out for a frame: {found:#?}"
    );

    // A clip that shuts for a frame: the square stays in the tree at opacity 1, wholly clipped.
    let tree = |shape: Rect| ClipRect::new().clipper(shape).child(square());
    let open = Rect::from_ltrb(0.0, 0.0, 40.0, 40.0);
    let mut laid = lay_out_animated(tree(open), loose(200.0), Vsync::new());
    let mut run = MotionRun::new(&mut laid);
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(Rect::ZERO)));
    run.frame(FRAME);
    run.act(|laid| laid.pump_widget(tree(open)));
    run.play(RUN, FRAME);
    let found = check_all(&run.finish(), &MotionConfig::default());
    assert!(
        found.iter().any(|finding| finding.rule == Rule::Flicker
            && finding.locator.ends_with("RenderDecoratedBox")),
        "the square is clipped out for a frame: {found:#?}"
    );
}

pub(crate) fn a_bouncy_spring_overshoots_and_a_critical_one_does_not() {
    let spring = |vsync: &Vsync, bounce: f64| {
        let description =
            SpringDescription::with_duration_and_bounce(Duration::from_millis(400), bounce)
                .expect("a valid spring");
        record(
            vsync,
            |inset| {
                VsyncScope::new(
                    vsync.clone(),
                    AnimatedPadding::new(EdgeInsets::all(inset), square()).spring(description),
                )
            },
            0.0,
            60.0,
        )
    };
    let vsync = Vsync::new();
    let bouncy = rules(&spring(&vsync, 0.5));
    assert!(
        bouncy.contains(&Rule::Overshoot),
        "bounce 0.5 overshoots: {bouncy:?}"
    );

    let mut wide_jump = MotionConfig::default();
    wide_jump.jump = 1e6;
    let vsync = Vsync::new();
    let bouncy = check(&spring(&vsync, 0.5), &wide_jump);
    assert!(
        bouncy.iter().any(|finding| finding.rule == Rule::Overshoot),
        "an overshoot does not depend on the jump threshold: {bouncy:#?}"
    );

    let vsync = Vsync::new();
    let critical = rules(&spring(&vsync, 0.0));
    assert!(
        critical.is_empty(),
        "bounce 0 lands without passing: {critical:?}"
    );
}

pub(crate) fn motion_still_running_at_the_end_never_settles() {
    let vsync = Vsync::new();
    let tree = |value: f64| {
        VsyncScope::new(
            vsync.clone(),
            AnimatedOpacity::new(value, square()).duration(Duration::from_secs(5)),
        )
    };
    let mut laid = lay_out_animated(tree(1.0), loose(200.0), vsync.clone());
    let found = rules(&run_change(&mut laid, || tree(0.0), Duration::from_secs(1)));
    assert!(
        found.contains(&Rule::NeverSettles),
        "a 5 s fade recorded for 1 s: {found:?}"
    );

    // A reveal that only moves a clip: no box moves or fades, what the window shows still grows.
    let vsync = Vsync::new();
    let tree = |width: f64| {
        VsyncScope::new(
            vsync.clone(),
            AnimatedLerp::new(Rect::from_ltrb(0.0, 0.0, width, 40.0), |shape: &Rect| {
                ClipRect::new().clipper(*shape).child(square())
            })
            .duration(Duration::from_secs(5)),
        )
    };
    let mut laid = lay_out_animated(tree(0.0), loose(200.0), vsync.clone());
    let found = rules(&run_change(
        &mut laid,
        || tree(40.0),
        Duration::from_secs(1),
    ));
    assert!(
        found.contains(&Rule::NeverSettles),
        "a 5 s reveal recorded for 1 s: {found:?}"
    );
}

pub(crate) fn frames_asked_for_after_the_motion_ends_wake_after_end() {
    // The inner fade runs under a parent drawn at opacity 0, which paints nothing below it: the
    // window keeps asking for frames that change nothing it shows.
    let vsync = Vsync::new();
    let tree = |value: f64| {
        VsyncScope::new(
            vsync.clone(),
            Opacity::new(0.0)
                .child(AnimatedOpacity::new(value, square()).duration(Duration::from_secs(10))),
        )
    };
    let mut laid = lay_out_animated(tree(1.0), loose(200.0), vsync.clone());
    let found = rules(&run_change(&mut laid, || tree(0.0), Duration::from_secs(2)));
    assert!(
        found.contains(&Rule::WakesAfterEnd),
        "hidden motion keeps waking: {found:?}"
    );
}

pub(crate) fn wakes_after_end_reads_what_the_window_shows() {
    // The padding animates a square that lies wholly outside its parent's clip: its box moves
    // every frame, the window shows none of it, and the frames it asks for change nothing.
    let vsync = Vsync::new();
    let tree = |inset: f64| {
        VsyncScope::new(
            vsync.clone(),
            ClipRect::new().child(
                SizedBox::new(40.0, 40.0).child(
                    UnconstrainedBox::new()
                        .alignment(Alignment::TOP_LEFT)
                        .child(
                            Padding::only(60.0, 0.0, 0.0, 0.0).child(
                                AnimatedPadding::new(
                                    EdgeInsets::only_left(inset),
                                    SizedBox::new(20.0, 20.0),
                                )
                                .duration(Duration::from_secs(10)),
                            ),
                        ),
                ),
            ),
        )
    };
    let mut laid = lay_out_animated(tree(0.0), loose(200.0), vsync.clone());
    let found = rules(&run_change(
        &mut laid,
        || tree(100.0),
        Duration::from_secs(2),
    ));
    assert!(
        found.contains(&Rule::WakesAfterEnd),
        "clipped motion keeps waking: {found:?}"
    );

    // A drift slower than `still` per frame, in view, is motion that adds up: the frames it asks
    // for change what the window shows.
    let vsync = Vsync::new();
    let tree = |inset: f64| {
        VsyncScope::new(
            vsync.clone(),
            AnimatedPadding::new(EdgeInsets::only_left(inset), square())
                .duration(Duration::from_secs(10)),
        )
    };
    let mut laid = lay_out_animated(tree(0.0), loose(200.0), vsync.clone());
    let found = rules(&run_change(
        &mut laid,
        || tree(20.0),
        Duration::from_secs(4),
    ));
    assert!(
        !found.contains(&Rule::WakesAfterEnd),
        "a slow visible drift is a change: {found:?}"
    );
}

pub(crate) fn a_finding_is_charged_to_the_outermost_node_that_shows_it() {
    let vsync = Vsync::new();
    let recording = padding(&vsync, false);
    let config = MotionConfig::default();
    let all: Vec<_> = check_all(&recording, &config)
        .into_iter()
        .filter(|finding| finding.rule == Rule::Jump)
        .collect();
    assert!(
        all.len() >= 2,
        "the box and the square inside it both move: {all:#?}"
    );
    let charged = charge_to_outermost(all.clone(), &recording);
    assert_eq!(
        charged.len(),
        1,
        "one jump, on the outermost moving node: {charged:#?}"
    );
    let outer = &charged[0];
    for inner in all.iter().filter(|finding| finding.id != outer.id) {
        assert!(
            inner.locator.starts_with(&outer.locator),
            "{} lies below {}",
            inner.locator,
            outer.locator
        );
    }
    assert_eq!(
        check(&recording, &config),
        charged,
        "check is check_all charged"
    );
}

pub(crate) fn the_allowlist_covers_with_a_reason_and_refuses_rows_that_match_nothing() {
    let vsync = Vsync::new();
    let recording = padding(&vsync, false);
    let config = MotionConfig::default();
    let jump = check(&recording, &config)
        .into_iter()
        .find(|finding| finding.rule == Rule::Jump)
        .expect("the instant padding jumps");
    let leaked = jump.locator.leak();
    let covered = Allow::new(Rule::Jump, leaked, "the inset changes at once by design");
    gate(&recording, &config, &[covered]).expect("a covered jump passes");
    assert_smooth(&recording, &[covered]);

    let stale = Allow::new(Rule::PopIn, "RenderPadding", "an old entrance");
    let report = gate(&recording, &config, &[covered, stale]).expect_err("a stale row fails");
    assert_eq!(report.unused, vec![stale]);

    let unexplained = Allow::new(Rule::Jump, leaked, " ");
    let report = gate(&recording, &config, &[unexplained]).expect_err("a row needs a reason");
    assert_eq!(report.unexplained, vec![unexplained]);

    let report = gate(&recording, &config, &[]).expect_err("an uncovered jump fails");
    assert!(
        report
            .unallowed
            .iter()
            .any(|finding| finding.rule == Rule::Jump)
    );
}

pub(crate) fn presented_nodes_compose_opacity_and_clip() {
    let tree = Opacity::new(0.5).child(
        Opacity::new(0.5).child(
            ClipRect::new().child(
                SizedBox::new(40.0, 40.0)
                    .child(Padding::only(60.0, 0.0, 0.0, 0.0).child(SizedBox::new(20.0, 20.0))),
            ),
        ),
    );
    let mut laid = lay_out_animated(tree, loose(200.0), Vsync::new());
    let recording = MotionRun::new(&mut laid).finish();
    let samples = &recording.frames()[0].samples;
    let inner = samples
        .iter()
        .rfind(|sample| sample.locator.contains("RenderOpacity/RenderOpacity"))
        .expect("the innermost node under both opacities");
    assert!(
        (inner.opacity - 0.25).abs() < 1e-9,
        "two 0.5 group opacities draw at 0.25: {inner:#?}"
    );
    let outside = samples.last().expect("the padded leaf");
    assert!(
        outside.rect.left() >= 40.0,
        "the leaf lies right of the clip: {outside:#?}"
    );
    assert!(outside.visible.is_none(), "the clip hides it: {outside:#?}");
}

pub(crate) fn a_page_under_a_popup_stays_presented() {
    // A popup is modal and not opaque: the page below leaves the semantics tree but is still
    // painted, so it stays in the recording and does not pop out.
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| square().into_view().boxed()));
    let mut laid = lay_out_animated(
        Navigator::new(navigator.clone()),
        loose(200.0),
        Vsync::new(),
    );
    let mut run = MotionRun::new(&mut laid);
    run.frame(FRAME);
    run.act(|_| {
        drop(navigator.push(PopupRoute::<()>::new(|_ctx, _a, _s| {
            SizedBox::new(10.0, 10.0).into_view().boxed()
        })));
    });
    run.play(Duration::from_secs(1), FRAME);
    let recording = run.finish();
    let page = recording.frames()[0]
        .samples
        .iter()
        .find(|sample| sample.locator.ends_with("RenderDecoratedBox"))
        .expect("the page's square")
        .id;
    let last = recording.frames().last().expect("a recorded frame");
    assert!(
        last.samples.iter().any(|sample| sample.id == page),
        "the page under the popup is still drawn: {:#?}",
        last.samples
    );
    let found = check_all(&recording, &MotionConfig::default());
    assert!(
        !found
            .iter()
            .any(|finding| finding.id == page && finding.rule == Rule::PopOut),
        "covering a page with a popup does not pop it out: {found:#?}"
    );
}

/// A square that follows the pointer's x while it moves over the window.
#[derive(Clone, StatelessView)]
struct Follower {
    x: Signal<f64>,
}

impl StatelessView for Follower {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let x = self.x.get(ctx);
        let signal = self.x;
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(move |cx, event| {
                if let PointerEvent::Move(event) = event.global {
                    let _ = signal.set(cx, event.current().position.get().x);
                }
            })
            .child(
                SizedBox::new(200.0, 200.0).child(
                    Align::new(Alignment::TOP_LEFT)
                        .child(Padding::only(x, 0.0, 0.0, 0.0).child(square())),
                ),
            )
    }
}

pub(crate) fn a_drag_step_moves_on_its_direct_frame() {
    let mut laid = lay_out_animated(SizedBox::new(0.0, 0.0), loose(200.0), Vsync::new());
    let x = laid
        .with_build_owner_mut(|owner| owner.signal_graph().clone())
        .signal(0.0);
    laid.pump_widget(Follower { x });
    let mut run = MotionRun::new(&mut laid);
    run.frame(FRAME);
    run.act(|laid| laid.dispatch_pointer_down(10.0, 50.0));
    run.frame(FRAME);
    run.direct(|laid| laid.dispatch_pointer_move(60.0, 50.0));
    run.frame(FRAME);
    run.direct(|laid| laid.dispatch_pointer_move(110.0, 50.0));
    run.frame(FRAME);
    run.act(|laid| laid.dispatch_pointer_up(110.0, 50.0));
    run.play(RUN, FRAME);
    let recording = run.finish();
    let moved = recording.frames().iter().position(|frame| {
        frame.samples.iter().any(|sample| {
            sample.locator.ends_with("RenderDecoratedBox") && sample.rect.left() > 30.0
        })
    });
    assert!(
        moved.is_some_and(|frame| recording.frames()[frame].direct),
        "the square moves on the drag step's own frame: {moved:?}"
    );
    let found = rules(&recording);
    assert!(
        !found.contains(&Rule::Jump),
        "a step that follows the hand is not a jump: {found:?}"
    );
}
