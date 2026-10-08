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
use flui_view::ViewExt;
use flui_widgets::{
    ActivityIndicator, Center, RefreshController, RefreshIndicator, Row, ScrollController,
    SizedBox, TickerMode, VsyncScope,
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
    next.tick_all(50.0);
    laid.pump_for(Duration::ZERO);
    assert_eq!(only_arc(&laid), before);
    next.tick_all(50.3);
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
            ("ten_hours", ten_hours),
            ("semantics", announced_as_a_loading_spinner),
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
    let scroll = ScrollController::new();
    scroll.update_dimensions(300.0, 0.0, 4700.0);
    let refresh = RefreshController::new();
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            RefreshIndicator::new()
                .controller(refresh.clone())
                .scroll_controller(scroll)
                .child(SizedBox::new(300.0, 5000.0)),
        ),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    let idle = vsync.len();
    assert!(arcs(&laid).is_empty(), "no spinner while idle");

    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    laid.dispatch_pointer_up(150.0, 140.0);
    laid.pump();
    assert!(refresh.is_refreshing());
    assert_eq!(
        vsync.len(),
        idle + 1,
        "the spinner registered its controller"
    );
    laid.pump_for(Duration::ZERO);
    let first = only_arc(&laid);
    laid.pump_for(ms(300));
    let later = only_arc(&laid);
    assert_ne!(first, later, "the spinner moves");

    refresh.finish();
    laid.pump();
    assert!(arcs(&laid).is_empty(), "no spinner after finish");
    assert_eq!(vsync.len(), idle, "the spinner's controller is released");
}
