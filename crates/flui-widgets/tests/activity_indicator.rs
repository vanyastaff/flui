//! `ActivityIndicator` and the `RefreshIndicator` spinner, on virtual time:
//! the painted arc against the Material 3 indeterminate-indicator constants
//! (6 s cycle, sweep 0.1 → 0.87 → 0.1 of the circle, a linear 1080° turn plus
//! four 90° steps of 300 ms every 1.5 s), the controller's registration over
//! the indicator's life, and the static frame without ticks.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use flui_animation::Vsync;
use flui_painting::display_list::DrawOp;
use flui_rendering::layer::Layer;
use flui_testing::a11y::Role;
use flui_view::{SignalWriteExt, ViewExt};
use flui_widgets::{
    ActivityIndicator, Alternative, Center, ControlSpec, GestureAlternatives, RefreshController,
    RefreshIndicator, RefreshIntent, Row, ScrollController, Scrollable, SizedBox, TickerMode,
    VsyncScope,
};

use crate::common::{LaidOut, lay_out, lay_out_animated, loose, tight};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Every arc painted in the last frame, as `(start, sweep)` in radians.
fn arcs(laid: &LaidOut) -> Vec<(f64, f64)> {
    let mut arcs = Vec::new();
    for (_, node) in laid.layer_tree().expect("a frame painted").iter() {
        if let Layer::Picture(picture) = node.layer() {
            for command in picture.picture() {
                if let DrawOp::Arc {
                    start_angle,
                    sweep_angle,
                    ..
                } = &command.op
                {
                    arcs.push((*start_angle, *sweep_angle));
                }
            }
        }
    }
    arcs
}

fn only_arc(laid: &LaidOut) -> (f64, f64) {
    let arcs = arcs(laid);
    assert_eq!(arcs.len(), 1, "one indicator paints one arc: {arcs:?}");
    arcs[0]
}

/// The arc's rotation in degrees at `t` into a cycle, from the published
/// constants: 1080° per 6 s, plus 90° for each step already taken. Only
/// times on a step's plateau or end are used.
fn expected_rotation(t: Duration) -> f64 {
    let millis = t.as_millis() as f64;
    let linear = 1080.0 * millis / 6000.0;
    let steps = match t.as_millis() {
        0 => 0.0,
        300..=1500 => 90.0,
        1800..=3000 => 180.0,
        3300..=4500 => 270.0,
        4800..=6000 => 360.0,
        other => panic!("{other} ms is inside a step"),
    };
    linear + steps
}

fn assert_angle(actual: f64, expected_degrees: f64, what: &str) {
    let expected = expected_degrees.to_radians() - FRAC_PI_2;
    let difference = (actual - expected).rem_euclid(TAU);
    let difference = difference.min(TAU - difference);
    assert!(
        difference < 1e-6,
        "{what}: start {actual} rad, expected {expected_degrees}° ({expected} rad)"
    );
}

/// Mounts one indicator and drives its first tick, which anchors the
/// repeating run at virtual time zero.
fn indicator(vsync: &Vsync) -> LaidOut {
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), Center::new().child(ActivityIndicator::new())),
        loose(200.0),
        vsync.clone(),
    );
    laid.pump_for(Duration::ZERO);
    laid
}

fn arc_at_published_times() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    let mut now = 0;
    for at in [0, 300, 1500, 1800, 3000, 4800, 6000] {
        laid.pump_for(ms(at - now));
        now = at;
        let (start, sweep) = only_arc(&laid);
        // 6 s is the next cycle's first frame.
        let expected = if at == 6000 {
            0.0
        } else {
            expected_rotation(ms(at))
        };
        assert_angle(start, expected, &format!("{at} ms"));
        // The sweep's keyframes: 10 % of the circle at a cycle's start, 87 %
        // half way.
        match at {
            0 | 6000 => assert!((sweep - 0.1 * TAU).abs() < 1e-9, "{at} ms: {sweep}"),
            3000 => assert!((sweep - 0.87 * TAU).abs() < 1e-9, "{at} ms: {sweep}"),
            _ => {}
        }
    }
}

fn sweep_grows_then_shrinks() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    let mut sweeps = vec![only_arc(&laid).1];
    for _ in 0..12 {
        laid.pump_for(ms(500));
        sweeps.push(only_arc(&laid).1);
    }
    assert!((sweeps[0] - 0.1 * TAU).abs() < 1e-9, "starts at 10 %");
    assert!(sweeps[..=6].windows(2).all(|w| w[0] < w[1]), "{sweeps:?}");
    assert!(sweeps[6..].windows(2).all(|w| w[0] > w[1]), "{sweeps:?}");
}

fn registry_migration_preserves_the_painted_phase() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    laid.pump_for(ms(1500));
    let before = only_arc(&laid);
    let next = Vsync::new();
    laid.pump_widget(VsyncScope::new(
        next.clone(),
        Center::new().child(ActivityIndicator::new()),
    ));
    assert!(vsync.is_empty());
    assert_eq!(only_arc(&laid), before);
    next.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.0)),
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(only_arc(&laid), before);
    next.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.3)),
    );
    laid.pump_for(Duration::ZERO);
    assert_angle(
        only_arc(&laid).0,
        expected_rotation(ms(1800)),
        "after migration",
    );
}

fn zero_dt() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    laid.pump_for(ms(700));
    let before = only_arc(&laid);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::ZERO);
    assert_eq!(only_arc(&laid), before, "no time, no motion");
}

fn indicator_ticks_preserve_surrounding_paint_content() {
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, row};
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Row::new(row![
                SizedBox::new(160.0, 200.0).child(ColoredBox::new(Color::RED)),
                ActivityIndicator::new(),
            ]),
        ),
        tight(200.0, 200.0),
        vsync,
    );
    laid.pump_for(Duration::ZERO);
    let surrounding_content = |laid: &LaidOut| {
        laid.layer_tree()
            .expect("painted scene")
            .iter()
            .find_map(|(_, node)| {
                node.boundary()
                    .map(|stamp| (stamp.render_id(), stamp.content().clone()))
            })
            .expect("the presentation's root certifies its own pixels")
    };
    let original = surrounding_content(&laid);
    let mut previous_arc = only_arc(&laid);
    for elapsed in [ms(300), ms(900), ms(1500)] {
        laid.pump_for(elapsed);
        let current_arc = only_arc(&laid);
        assert_ne!(current_arc, previous_arc, "the indicator actually moves");
        assert_eq!(
            surrounding_content(&laid),
            original,
            "an indicator tick must preserve the surrounding boundary's content certificate"
        );
        previous_arc = current_arc;
    }
}

fn ten_hours() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    laid.pump_for(ms(1800));
    let before = only_arc(&laid);
    // Ten hours is 6000 whole cycles: the same frame, nothing accumulated.
    laid.pump_for(Duration::from_hours(10));
    let (start, sweep) = only_arc(&laid);
    assert_angle(start, expected_rotation(ms(1800)), "after ten hours");
    assert!((sweep - before.1).abs() < 1e-6, "{sweep} vs {}", before.1);
}

fn announced_as_a_loading_spinner() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let node = tree
        .find_by_label("Loading")
        .expect("the indicator's label");
    assert_eq!(node.role(), Role::ProgressIndicator);
}

fn reduced_motion_keeps_visible_loading_without_continuation() {
    use flui_animation::MotionOverride;
    fn live() {
        policy_case(false);
    }
    fn initial() {
        policy_case(true);
    }
    fn policy_case(initially_reduced: bool) {
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(vsync.clone(), SizedBox::shrink()),
            loose(200.0),
            vsync.clone(),
        );
        laid.set_motion_override(if initially_reduced {
            MotionOverride::Reduce
        } else {
            MotionOverride::Full
        });
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            Center::new().child(ActivityIndicator::new()),
        ));
        laid.pump_for(Duration::ZERO);
        if !initially_reduced {
            laid.pump_for(ms(300));
            laid.set_motion_override(MotionOverride::Reduce);
            laid.pump_for(Duration::ZERO);
        }
        let stopped = only_arc(&laid);
        assert!(stopped.1 > 0.0, "static loading stays visible");
        laid.pump_for(ms(350));
        assert_eq!(only_arc(&laid), stopped, "Reduce stops visible motion");
        assert!(
            !vsync.has_running(),
            "a parked indicator needs no continuation"
        );
        laid.enable_semantics();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("Loading")
            .expect("loading label retained");
        assert_eq!(node.role(), Role::ProgressIndicator);
        assert_eq!(node.value(), None, "unknown work has no percentage");
        laid.set_motion_override(MotionOverride::Full);
        laid.pump_for(Duration::ZERO);
        let resumed = only_arc(&laid);
        laid.pump_for(ms(300));
        assert_ne!(only_arc(&laid), resumed, "Full resumes visible motion");
        laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::shrink()));
        assert!(
            vsync.is_empty(),
            "unmount releases the parked or resumed owner"
        );
    }
    crate::common::cases::run_cases(
        "reduced indicator",
        &[("live preference", live), ("initial preference", initial)],
    );
}

#[derive(Clone)]
struct KeyedIndicator {
    isolated: bool,
    key: flui_view::GlobalKey<KeyedIndicatorState>,
    registry: std::rc::Rc<std::cell::RefCell<Option<Vsync>>>,
    inits: std::rc::Rc<std::cell::Cell<usize>>,
}

struct KeyedIndicatorState {
    registry: std::rc::Rc<std::cell::RefCell<Option<Vsync>>>,
    inits: std::rc::Rc<std::cell::Cell<usize>>,
}

impl flui_view::View for KeyedIndicator {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

impl flui_view::StatefulView for KeyedIndicator {
    type State = KeyedIndicatorState;
    fn create_state(&self) -> Self::State {
        KeyedIndicatorState {
            registry: self.registry.clone(),
            inits: self.inits.clone(),
        }
    }
}

impl flui_view::ViewState<KeyedIndicator> for KeyedIndicatorState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.inits.set(self.inits.get() + 1);
        self.did_change_dependencies(ctx);
    }
    fn did_change_dependencies(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        *self.registry.borrow_mut() = VsyncScope::maybe_of(ctx);
    }
    fn build(
        &self,
        view: &KeyedIndicator,
        _ctx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        if view.isolated {
            flui_widgets::Semantics::new()
                .child(flui_widgets::RepaintBoundary::new().child(ActivityIndicator::new()))
                .boxed()
        } else {
            ActivityIndicator::new().boxed()
        }
    }
}

fn keyed_indicator_moves_between_configured_permissions_without_restarting() {
    keyed_indicator_permission_case(false);
}

fn isolated_keyed_indicator_moves_between_configured_permissions_without_restarting() {
    keyed_indicator_permission_case(true);
}

fn keyed_indicator_permission_case(isolated: bool) {
    use flui_animation::MotionOverride;
    let root = Vsync::new();
    let registry = std::rc::Rc::new(std::cell::RefCell::new(None::<Vsync>));
    let inits = std::rc::Rc::new(std::cell::Cell::new(0));
    let key = flui_view::GlobalKey::new();
    let tree = |moved, enabled| {
        let child = KeyedIndicator {
            isolated,
            key: key.clone(),
            registry: registry.clone(),
            inits: inits.clone(),
        };
        let empty = SizedBox::shrink().boxed();
        let (left, right) = if moved {
            (empty, child.boxed())
        } else {
            (child.boxed(), empty)
        };
        VsyncScope::new(
            root.clone(),
            Row::new((
                TickerMode::new(left),
                TickerMode::new(right).enabled(enabled),
            )),
        )
    };
    let mut laid = lay_out_animated(tree(false, false), loose(200.0), root.clone());
    laid.set_motion_override(MotionOverride::Full);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(300));
    let shown = only_arc(&laid);
    let old = registry
        .borrow()
        .clone()
        .expect("old registry captured through lifecycle");
    laid.pump_widget(tree(true, false));
    let next = registry
        .borrow()
        .clone()
        .expect("new registry captured through lifecycle");
    assert!(!old.is_same(&next));
    assert!(old.is_empty());
    assert_eq!(next.len(), 1);
    assert_eq!(
        inits.get(),
        1,
        "GlobalKey retains the actual indicator subtree"
    );
    let stopped = only_arc(&laid);
    assert!(stopped.1 > 0.0);
    laid.pump_for(ms(500));
    assert_eq!(only_arc(&laid), stopped);
    laid.pump_widget(tree(true, true));
    laid.pump_for(Duration::ZERO);
    assert_eq!(
        only_arc(&laid),
        shown,
        "migration preserves the old run's phase"
    );
    laid.pump_for(ms(300));
    assert_ne!(
        only_arc(&laid),
        shown,
        "the retained run advances after resumption"
    );
    laid.pump_widget(VsyncScope::new(root, SizedBox::shrink()));
    assert!(next.is_empty());
}

#[test]
fn activity_indicator_arc_follows_keyframes() {
    crate::common::cases::run_cases(
        "activity_indicator_arc_follows_keyframes",
        &[
            ("published times", arc_at_published_times),
            (
                "registry migration",
                registry_migration_preserves_the_painted_phase,
            ),
            ("sweep", sweep_grows_then_shrinks),
            ("zero_dt", zero_dt),
            (
                "localized paint content",
                indicator_ticks_preserve_surrounding_paint_content,
            ),
            ("ten_hours", ten_hours),
            ("semantics", announced_as_a_loading_spinner),
            (
                "reduced motion",
                reduced_motion_keeps_visible_loading_without_continuation,
            ),
            (
                "keyed permission migration",
                keyed_indicator_moves_between_configured_permissions_without_restarting,
            ),
            (
                "isolated keyed permission migration",
                isolated_keyed_indicator_moves_between_configured_permissions_without_restarting,
            ),
        ],
    );
}

#[test]
fn indicator_paused_paints_static_frame() {
    let first_frame = |laid: &LaidOut| {
        let (start, sweep) = only_arc(laid);
        assert_angle(start, 0.0, "first frame");
        assert!((sweep - 0.1 * TAU).abs() < 1e-9);
    };
    // No VsyncScope: nothing ticks the controller.
    let mut laid = lay_out(Center::new().child(ActivityIndicator::new()), loose(200.0));
    laid.pump_for(ms(900));
    first_frame(&laid);
    // A disabled TickerMode mutes the registry the indicator joined.
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            TickerMode::new(Center::new().child(ActivityIndicator::new())).enabled(false),
        ),
        loose(200.0),
        vsync,
    );
    laid.pump_for(ms(900));
    first_frame(&laid);
}

fn two_indicators(count: usize) -> Row {
    Row::new(
        (0..count)
            .map(|_| ActivityIndicator::new().boxed())
            .collect(),
    )
}

#[test]
fn two_indicators_share_one_vsync() {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), two_indicators(2)),
        loose(200.0),
        vsync.clone(),
    );
    assert_eq!(vsync.len(), 2, "one registration per indicator");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(300));
    let both = arcs(&laid);
    assert_eq!(both.len(), 2);
    for &(start, _) in &both {
        assert_angle(start, expected_rotation(ms(300)), "each indicator ticks");
    }
}

#[test]
fn indicator_unmount_mid_frame_releases_controller() {
    fn first_source_requests_removal() {
        indicator_callback_requests_removal(0);
    }
    fn second_source_requests_removal() {
        indicator_callback_requests_removal(1);
    }
    crate::common::cases::run_cases(
        "indicator retirement",
        &[
            ("first repaint source", first_source_requests_removal),
            ("second repaint source", second_source_requests_removal),
            (
                "inherited registry replacement",
                indicator_rebind_after_unmount,
            ),
        ],
    );
}

#[derive(Clone)]
struct VisibleIndicators(flui_view::Signal<usize>);

impl flui_view::View for VisibleIndicators {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl flui_view::StatelessView for VisibleIndicators {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        two_indicators(self.0.get(ctx))
    }
}

fn indicator_callback_requests_removal(source: usize) {
    use std::cell::Cell;
    use std::rc::Rc;

    let vsync = Vsync::new();
    let mut laid = lay_out_animated(SizedBox::shrink(), loose(200.0), vsync.clone());
    let reactive = laid.with_build_owner_mut(|owner| owner.signal_graph().clone());
    let count = reactive.signal(2usize);
    laid.pump_widget(VsyncScope::new(vsync.clone(), VisibleIndicators(count)));
    laid.pump_for(Duration::ZERO);
    // The mounted painter's public repaint source is the indicator's real
    // animation. Copy the handles before invoking any callbacks or pumping.
    let animations = laid.pipeline_owner().with(|owner| {
        owner
            .render_tree()
            .iter()
            .filter_map(|(id, node)| {
                node.downcast_render_object::<flui_objects::RenderCustomPaint>()
                    .and_then(|paint| paint.painter())
                    .and_then(|painter| painter.repaint())
                    .map(|animation| (id, animation))
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(animations.len(), 2);
    let deliveries = Rc::new([Cell::new(0usize), Cell::new(0usize)]);
    let monitors = animations
        .iter()
        .enumerate()
        .map(|(index, (_, animation))| {
            animation.add_listener(Rc::new({
                let deliveries = deliveries.clone();
                move || deliveries[index].set(deliveries[index].get() + 1)
            }))
        })
        .collect::<Vec<_>>();
    let requested = Rc::new(Cell::new(false));
    let writer = laid.with_build_owner_mut(|owner| owner.writer_source());
    let callback = animations[source].1.add_listener(Rc::new({
        let requested = requested.clone();
        move || {
            if !requested.replace(true) {
                writer
                    .write(|cx| count.set(cx, 1))
                    .expect("callback may update visibility");
            }
        }
    }));
    laid.pump_for(ms(300));
    assert!(
        requested.get(),
        "removal originates in the indicator callback"
    );
    assert_eq!(
        vsync.len(),
        1,
        "same-frame reconciliation retires the owner"
    );
    let retained = laid.pipeline_owner().with(|owner| {
        owner
            .render_tree()
            .iter()
            .filter_map(|(id, node)| {
                node.downcast_render_object::<flui_objects::RenderCustomPaint>()
                    .map(|_| id)
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(retained.len(), 1);
    let survivor = animations
        .iter()
        .position(|(id, _)| *id == retained[0])
        .expect("reconciliation retains one original indicator");
    let departed = 1 - survivor;
    assert_eq!(deliveries[survivor].get(), 1, "survivor receives this tick");
    assert_eq!(deliveries[departed].get(), 1, "departure follows this tick");
    let (start, _) = only_arc(&laid);
    assert_angle(
        start,
        expected_rotation(ms(300)),
        "survivor paints this tick",
    );
    laid.pump_for(ms(300));
    assert_eq!(
        deliveries[survivor].get(),
        2,
        "survivor receives the next tick"
    );
    assert_eq!(
        deliveries[departed].get(),
        1,
        "disposed source delivers nothing more"
    );
    let (start, _) = only_arc(&laid);
    assert_angle(
        start,
        expected_rotation(ms(600)),
        "survivor advances next frame",
    );
    laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::shrink()));
    assert!(
        vsync.is_empty(),
        "retained repaint handles do not retain registrations"
    );
    let after_teardown = deliveries.each_ref().map(Cell::get);
    laid.pump_for(ms(300));
    assert_eq!(deliveries.each_ref().map(Cell::get), after_teardown);
    assert!(
        arcs(&laid).is_empty(),
        "disposed indicators paint no further arcs"
    );
    animations[source].1.remove_listener(callback);
    for ((_, animation), monitor) in animations.iter().zip(monitors) {
        animation.remove_listener(monitor);
    }
}

fn indicator_rebind_after_unmount() {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), two_indicators(2)),
        loose(200.0),
        vsync.clone(),
    );
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(300));
    laid.pump_widget(VsyncScope::new(vsync.clone(), two_indicators(1)));
    assert_eq!(vsync.len(), 1, "the unmounted indicator unregistered");
    let replacement = Vsync::new();
    laid.pump_widget(VsyncScope::new(replacement.clone(), two_indicators(1)));
    assert!(
        vsync.is_empty(),
        "the old inherited registry no longer owns the survivor"
    );
    assert_eq!(
        replacement.len(),
        1,
        "a retained state joins its new inherited registry"
    );
    laid.pump_widget(VsyncScope::new(vsync, two_indicators(1)));
    assert!(replacement.is_empty());
    laid.pump_for(ms(1500));
    let (start, _) = only_arc(&laid);
    assert_angle(
        start,
        expected_rotation(ms(1800)),
        "the survivor keeps ticking",
    );
}

#[test]
fn indicator_ui_runtime_stop_mid_repeat() {
    let vsync = Vsync::new();
    let mut laid = indicator(&vsync);
    laid.pump_for(ms(450));
    assert_eq!(vsync.len(), 1);
    laid.end_session();
    assert_eq!(vsync.len(), 0, "teardown releases the endless run");
}

#[test]
fn refresh_indicator_spins_while_refreshing() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::task::{Poll, Waker};

    use flui_animation::MotionOverride;

    fn full() {
        refreshing_case(MotionOverride::Full);
    }
    fn reduced() {
        refreshing_case(MotionOverride::Reduce);
    }
    fn refreshing_case(preference: MotionOverride) {
        let refresh = RefreshController::new();
        let done = Rc::new(Cell::new(false));
        let waker: Rc<RefCell<Option<Waker>>> = Rc::default();
        let (gate, parked) = (Rc::clone(&done), Rc::clone(&waker));
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                RefreshIndicator::new(
                    Scrollable::new()
                        .controller(ScrollController::new())
                        .child(SizedBox::new(250.0, 5000.0)),
                    GestureAlternatives::try_new(Alternative::VisibleControl(ControlSpec::new(
                        RefreshIntent::command("Refresh"),
                        SizedBox::new(24.0, 24.0),
                    )))
                    .expect("one labelled command"),
                )
                .controller(refresh.clone())
                .on_refresh(move |_cx| {
                    let (gate, parked) = (Rc::clone(&gate), Rc::clone(&parked));
                    std::future::poll_fn(move |cx| {
                        if gate.get() {
                            return Poll::Ready(());
                        }
                        *parked.borrow_mut() = Some(cx.waker().clone());
                        Poll::Pending
                    })
                }),
            ),
            tight(300.0, 300.0),
            vsync.clone(),
        );
        let idle = vsync.len();
        laid.set_motion_override(preference);
        laid.pump_for(Duration::ZERO);
        assert!(arcs(&laid).is_empty(), "no spinner while idle");

        laid.dispatch_pointer_down(100.0, 40.0);
        for y in [60.0, 100.0, 140.0, 180.0, 220.0] {
            laid.dispatch_pointer_move_after(100.0, y, ms(16));
        }
        laid.pump_for(ms(16));
        assert!(
            refresh.pull_distance_px() > 0.0,
            "the finger is still pulling"
        );
        assert_eq!(
            arcs(&laid).len(),
            1,
            "without a style the band holds the spinner while pulling (ADR-0235)"
        );
        laid.dispatch_pointer_up(100.0, 220.0);
        for _ in 0..30 {
            laid.pump_for(ms(16));
        }
        assert!(refresh.is_refreshing());
        assert_eq!(refresh.pull_distance_px(), 0.0, "the pull settled");
        assert_eq!(
            vsync.len(),
            idle + 1,
            "the spinner registered its controller"
        );
        let first = only_arc(&laid);
        laid.pump_for(ms(300));
        let later = only_arc(&laid);
        if preference == MotionOverride::Full {
            assert_ne!(first, later, "the spinner moves");
        } else {
            assert_eq!(
                first, later,
                "refresh stays visible without cosmetic motion"
            );
            assert!(first.1 > 0.0);
        }

        done.set(true);
        if let Some(waker) = waker.take() {
            waker.wake();
        }
        laid.pump_for(ms(16));
        laid.pump_for(ms(16));
        assert!(!refresh.is_refreshing());
        assert!(arcs(&laid).is_empty(), "no spinner after the refresh");
        assert_eq!(vsync.len(), idle, "the spinner's controller is released");
    }
    crate::common::cases::run_cases("refresh loading", &[("Full", full), ("Reduce", reduced)]);
}

/// A `RefreshIndicatorStyle` builds the indicator from the phase: the pull's share of the
/// threshold while pulling (capped at one), and its own refreshing indicator in place of the
/// default spinner. A threshold that is not finite and positive is refused where it is set, so
/// the default stands.
#[test]
fn refresh_style_builds_the_indicator_from_the_phase() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_painting::styling::Color;
    use flui_widgets::{RefreshIndicatorPhase, RefreshIndicatorStyle};

    const STYLED: Color = Color::rgb(1, 2, 3);

    fn threshold_80() {
        style_case(80.0, |pull| pull / 80.0, true);
    }
    fn threshold_zero() {
        style_case(0.0, |pull| pull / 80.0, true);
    }
    fn threshold_nan() {
        style_case(f64::NAN, |pull| pull / 80.0, true);
    }
    fn style_case(threshold: f64, progress: fn(f64) -> f64, refreshes: bool) {
        let refresh = RefreshController::new();
        let phases = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&phases);
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(
                vsync.clone(),
                RefreshIndicatorStyle::new(
                    move |phase| {
                        seen.borrow_mut().push(phase);
                        match phase {
                            RefreshIndicatorPhase::Refreshing => {
                                Some(ActivityIndicator::new().color(STYLED).boxed())
                            }
                            _ => None,
                        }
                    },
                    RefreshIndicator::new(
                        Scrollable::new()
                            .controller(ScrollController::new())
                            .child(SizedBox::new(300.0, 5000.0)),
                        GestureAlternatives::try_new(Alternative::VisibleControl(
                            ControlSpec::new(
                                RefreshIntent::command("Refresh"),
                                SizedBox::new(24.0, 24.0),
                            ),
                        ))
                        .expect("one labelled command"),
                    )
                    .controller(refresh.clone())
                    .threshold_px(threshold)
                    .on_refresh(|_cx| std::future::pending::<()>()),
                ),
            ),
            tight(300.0, 300.0),
            vsync,
        );
        laid.pump_for(Duration::ZERO);
        assert!(phases.borrow().is_empty(), "no phase while idle");

        laid.dispatch_pointer_down(150.0, 40.0);
        let mut pulls = Vec::new();
        for y in [80.0, 110.0] {
            laid.dispatch_pointer_move(150.0, y);
            laid.pump();
            pulls.push(refresh.pull_distance_px());
        }
        let pulled: Vec<RefreshIndicatorPhase> = pulls
            .iter()
            .filter(|&&pull| pull > 0.0)
            .map(|&pull| RefreshIndicatorPhase::Pulling {
                progress: progress(pull).min(1.0),
            })
            .collect();
        assert!(!pulled.is_empty(), "the drag pulled: {pulls:?}");
        assert_eq!(
            *phases.borrow(),
            pulled,
            "the builder sees each pull's share"
        );

        laid.dispatch_pointer_move(150.0, 200.0);
        laid.dispatch_pointer_up(150.0, 200.0);
        laid.pump();
        assert_eq!(refresh.is_refreshing(), refreshes);
        laid.pump_for(Duration::ZERO);
        if refreshes {
            assert_eq!(
                phases.borrow().last(),
                Some(&RefreshIndicatorPhase::Refreshing)
            );
            assert_eq!(arcs(&laid).len(), 1, "the style's spinner, not the default");
            let styled = laid.draw_ops().into_iter().any(
                |command| matches!(command.op, DrawOp::Arc { paint, .. } if paint.color == STYLED),
            );
            assert!(styled, "the style's spinner is drawn");
        } else {
            assert_eq!(arcs(&laid), Vec::<(f64, f64)>::new(), "nothing refreshes");
        }
    }
    crate::common::cases::run_cases(
        "refresh style",
        &[
            ("threshold 80", threshold_80),
            ("zero threshold", threshold_zero),
            ("NaN threshold", threshold_nan),
        ],
    );
}
