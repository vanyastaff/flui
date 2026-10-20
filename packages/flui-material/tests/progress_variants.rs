//! The progress indicators beyond the linear bars' timing: the gap and stop, the circular
//! and wavy variants, the shared value policy, reduced motion, and the Material refresh
//! indicator, read from recorded frames and the semantics tree.

use std::f64::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use flui_material::{
    CircularProgressIndicator, CircularWavyProgressIndicator, LinearProgressIndicator,
    LinearWavyProgressIndicator, MaterialApp, Theme, ThemeData,
};
use flui_painting::paint::PathCommand;
use flui_sdk::animation::{MotionOverride, Vsync};
use flui_sdk::geometry::Point;
use flui_sdk::painting::{Color, DrawOp, TextDirection};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    Alternative, ControlSpec, Directionality, GestureAlternatives, RefreshController,
    RefreshIndicator, RefreshIntent, ScrollController, Scrollable, SizedBox,
};
use flui_testing::a11y::Role;

use crate::common::{LaidOut, lay_out_animated, tight};

const WIDTH: f64 = 240.0;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: {actual}, expected {expected}"
    );
}

/// The light theme's active and track colors.
fn colors() -> (Color, Color) {
    let scheme = ThemeData::light().color_scheme;
    (scheme.primary, scheme.secondary_container)
}

fn themed(view: impl View, direction: TextDirection) -> impl View {
    Directionality::new(direction, Theme::new(ThemeData::light(), view))
}

/// `view` laid out alone at `width` × `height` under a ticking scope, its first frame
/// anchored at virtual time zero.
fn mount(view: impl View, width: f64, height: f64, direction: TextDirection) -> (LaidOut, Vsync) {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), themed(view, direction)),
        tight(width, height),
        vsync.clone(),
    );
    laid.pump_for(Duration::ZERO);
    (laid, vsync)
}

fn rects(laid: &LaidOut, color: Color) -> Vec<(f64, f64)> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { rect, paint } if paint.color == color => {
                Some((rect.left(), rect.right()))
            }
            _ => None,
        })
        .collect()
}

fn circles(laid: &LaidOut, color: Color) -> Vec<(f64, f64, f64)> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Circle {
                center,
                radius,
                paint,
            } if paint.color == color => Some((center.x, center.y, radius)),
            _ => None,
        })
        .collect()
}

fn arcs(laid: &LaidOut, color: Color) -> Vec<(f64, f64)> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Arc {
                start_angle,
                sweep_angle,
                paint,
                ..
            } if paint.color == color => Some((start_angle, sweep_angle)),
            _ => None,
        })
        .collect()
}

/// The points of every polyline path painted in `color`.
fn paths(laid: &LaidOut, color: Color) -> Vec<Vec<Point<f64>>> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Path { path, paint } if paint.color == color => Some(
                path.commands()
                    .filter_map(|command| match command {
                        PathCommand::MoveTo(p) | PathCommand::LineTo(p) => Some(p),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// The value assistive technology reads for the indicator labelled "Loading", which must be
/// a progress bar.
fn announced(laid: &mut LaidOut) -> Option<String> {
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    let node = tree.find_by_label("Loading").expect("the default label");
    assert_eq!(node.role(), Role::ProgressIndicator);
    node.value().map(str::to_owned)
}

/// How far a 240 × 10 linear wave strays from its centre line.
fn linear_swing(laid: &LaidOut, primary: Color) -> f64 {
    paths(laid, primary)
        .iter()
        .flatten()
        .map(|p| (p.y - 5.0).abs())
        .fold(0.0, f64::max)
}

/// How far a 48 dp circular wave strays from its 20.4 dp circle.
fn circular_swing(laid: &LaidOut, primary: Color) -> f64 {
    paths(laid, primary)
        .iter()
        .flatten()
        .map(|p| ((p.x - 24.0).hypot(p.y - 24.0) - 20.4).abs())
        .fold(0.0, f64::max)
}

/// Horizontal spans, `(left, right)` in pixels.
type Spans = &'static [(f64, f64)];
/// Stop dots, `(x, y, radius)` in pixels.
type Dots = &'static [(f64, f64, f64)];

/// A determinate linear indicator: the active bar, a 4 dp gap, the track, and the 4 dp stop
/// dot at the track's end, mirrored in RTL; no stop once the bar reaches it; the still
/// indeterminate segment keeps its gaps and has no stop.
pub fn linear_gap_and_stop() {
    let (primary, track) = colors();
    let rows: [(f64, TextDirection, Spans, Spans, Dots); 5] = [
        (
            0.25,
            TextDirection::Ltr,
            &[(0.0, 60.0)],
            &[(64.0, 240.0)],
            &[(238.0, 2.0, 2.0)],
        ),
        (
            0.25,
            TextDirection::Rtl,
            &[(180.0, 240.0)],
            &[(0.0, 176.0)],
            &[(2.0, 2.0, 2.0)],
        ),
        (
            0.0,
            TextDirection::Ltr,
            &[],
            &[(0.0, 240.0)],
            &[(238.0, 2.0, 2.0)],
        ),
        (1.0, TextDirection::Ltr, &[(0.0, 240.0)], &[], &[]),
        (0.99, TextDirection::Ltr, &[(0.0, 237.6)], &[], &[]),
    ];
    let same = |painted: &[(f64, f64)], expected: Spans, what: &str| {
        assert_eq!(painted.len(), expected.len(), "{what}: {painted:?}");
        for (piece, want) in painted.iter().zip(expected) {
            close(piece.0, want.0, what);
            close(piece.1, want.1, what);
        }
    };
    for (value, direction, active, pieces, stop) in rows {
        let (laid, _) = mount(
            LinearProgressIndicator::new().value(Some(value)),
            WIDTH,
            4.0,
            direction,
        );
        let what = format!("{value} {direction:?}");
        same(&rects(&laid, primary), active, &format!("{what} active"));
        same(&rects(&laid, track), pieces, &format!("{what} track"));
        assert_eq!(circles(&laid, primary), stop, "{what} stop");
    }

    let (mut laid, _) = mount(
        LinearProgressIndicator::new(),
        WIDTH,
        4.0,
        TextDirection::Ltr,
    );
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::ZERO);
    assert_eq!(rects(&laid, primary), [(90.0, 150.0)]);
    assert_eq!(rects(&laid, track), [(0.0, 86.0), (154.0, 240.0)]);
    assert!(
        circles(&laid, primary).is_empty(),
        "unknown work has no end to mark"
    );
}

/// A determinate circular indicator: the active arc clockwise from 12 o'clock and the track
/// round the rest, a gap of 4 dp plus a stroke (for the round caps) at each end, on the 18 dp
/// circle through the stroke's centre; under reduced motion a new value shows at once.
pub fn circular_determinate_arcs() {
    let (primary, track) = colors();
    let gap = 8.0 / 18.0;
    let (mut laid, vsync) = mount(
        CircularProgressIndicator::new().value(Some(0.25)),
        40.0,
        40.0,
        TextDirection::Ltr,
    );
    let active = arcs(&laid, primary);
    assert_eq!(active.len(), 1, "{active:?}");
    close(active[0].0, -FRAC_PI_2, "active start");
    close(active[0].1, FRAC_PI_2, "active sweep");
    let rest = arcs(&laid, track);
    assert_eq!(rest.len(), 1, "{rest:?}");
    close(rest[0].0, gap, "track start");
    close(rest[0].1, TAU - FRAC_PI_2 - 2.0 * gap, "track sweep");
    assert_eq!(announced(&mut laid).as_deref(), Some("25%"));
    let tree = laid.a11y_tree().expect("semantics enabled");
    let node = tree.find_by_label("Loading").expect("the default label");
    assert_eq!(node.raw().numeric_value(), Some(25.0), "a 0–100 range");
    assert_eq!(node.raw().max_numeric_value(), Some(100.0));

    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_widget(VsyncScope::new(
        vsync,
        themed(
            CircularProgressIndicator::new().value(Some(0.6)),
            TextDirection::Ltr,
        ),
    ));
    close(arcs(&laid, primary)[0].1, 0.6 * TAU, "the value jumps");
}

/// Every determinate variant draws a finite arc or bar and announces a finite percentage for
/// any value: clamped into [0, 1], a non-finite value keeping the last finite one (0 before
/// the first).
pub fn determinate_values_stay_finite() {
    let (primary, _) = colors();
    let sweep_of = |laid: &LaidOut| arcs(laid, primary).iter().map(|arc| arc.1).sum::<f64>();
    let wave_end = |laid: &LaidOut| {
        paths(laid, primary)
            .iter()
            .flatten()
            .map(|p| p.x)
            .fold(0.0, f64::max)
    };

    // A first value: NaN and ∞ show 0, 1.5 shows all of it.
    for (value, share, percent) in [
        (f64::NAN, 0.0, "0%"),
        (f64::INFINITY, 0.0, "0%"),
        (1.5, 1.0, "100%"),
    ] {
        let (mut laid, _) = mount(
            CircularProgressIndicator::new().value(Some(value)),
            40.0,
            40.0,
            TextDirection::Ltr,
        );
        close(sweep_of(&laid), share * TAU, &format!("circular {value}"));
        assert_eq!(
            announced(&mut laid).as_deref(),
            Some(percent),
            "circular {value}"
        );

        let (mut laid, _) = mount(
            LinearWavyProgressIndicator::new().value(Some(value)),
            WIDTH,
            10.0,
            TextDirection::Ltr,
        );
        close(
            wave_end(&laid),
            share * WIDTH,
            &format!("linear wavy {value}"),
        );
        assert_eq!(
            announced(&mut laid).as_deref(),
            Some(percent),
            "linear wavy {value}"
        );

        let (mut laid, _) = mount(
            CircularWavyProgressIndicator::new().value(Some(value)),
            48.0,
            48.0,
            TextDirection::Ltr,
        );
        assert_eq!(
            paths(&laid, primary).is_empty(),
            share == 0.0,
            "circular wavy {value}"
        );
        assert_eq!(
            announced(&mut laid).as_deref(),
            Some(percent),
            "circular wavy {value}"
        );
    }

    // A later non-finite value keeps the last finite fraction.
    let (mut laid, vsync) = mount(
        CircularProgressIndicator::new().value(Some(0.4)),
        40.0,
        40.0,
        TextDirection::Ltr,
    );
    for value in [f64::INFINITY, f64::NAN, f64::NEG_INFINITY] {
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            themed(
                CircularProgressIndicator::new().value(Some(value)),
                TextDirection::Ltr,
            ),
        ));
        close(sweep_of(&laid), 0.4 * TAU, &format!("after 0.4, {value}"));
        assert_eq!(
            announced(&mut laid).as_deref(),
            Some("40%"),
            "after 0.4, {value}"
        );
    }
}

/// An indeterminate circular indicator is the activity arc in the theme's primary color: a
/// progress bar without a value, moving while motion is permitted.
pub fn circular_indeterminate_is_the_activity_arc() {
    let (primary, _) = colors();
    let (mut laid, vsync) = mount(
        CircularProgressIndicator::new(),
        40.0,
        40.0,
        TextDirection::Ltr,
    );
    let first = arcs(&laid, primary);
    assert_eq!(first.len(), 1, "{first:?}");
    close(first[0].0, -FRAC_PI_2, "resting start");
    close(first[0].1, 0.1 * TAU, "resting sweep");
    laid.pump_for(ms(300));
    assert_ne!(arcs(&laid, primary), first, "the arc moves");
    assert_eq!(announced(&mut laid), None);
    assert_eq!(vsync.len(), 1);
}

/// The wave: 3 dp (linear) and 1.6 dp (circular) at 50 %, flat at 5 % and at 97 %, flat and
/// still under reduced motion, and travelling one wavelength a second.
pub fn waves_rise_lie_flat_and_travel() {
    let (primary, _) = colors();
    for (value, linear, circular) in [(0.5, 3.0, 1.6), (0.05, 0.0, 0.0), (0.97, 0.0, 0.0)] {
        let (laid, _) = mount(
            LinearWavyProgressIndicator::new().value(Some(value)),
            WIDTH,
            10.0,
            TextDirection::Ltr,
        );
        let swing = linear_swing(&laid, primary);
        assert!(
            swing <= linear + 1e-9 && swing >= linear * 0.95,
            "linear {value}: {swing}"
        );
        let (laid, _) = mount(
            CircularWavyProgressIndicator::new().value(Some(value)),
            48.0,
            48.0,
            TextDirection::Ltr,
        );
        let swing = circular_swing(&laid, primary);
        assert!(
            swing <= circular + 1e-9 && swing >= circular * 0.95,
            "circular {value}: {swing}"
        );
    }

    let (mut laid, _) = mount(
        LinearWavyProgressIndicator::new().value(Some(0.5)),
        WIDTH,
        10.0,
        TextDirection::Ltr,
    );
    let before = paths(&laid, primary);
    laid.pump_for(ms(250));
    let after = paths(&laid, primary);
    assert_ne!(before, after, "the wave travels");
    // A quarter period moves the wave a quarter of its 40 dp wavelength.
    let shifted = |x: f64| 5.0 + 3.0 * (TAU * (x / 40.0 - 0.25)).sin();
    for p in after.iter().flatten() {
        assert!((p.y - shifted(p.x)).abs() < 1e-6, "{p:?}");
    }

    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::ZERO);
    assert_eq!(
        linear_swing(&laid, primary),
        0.0,
        "reduced motion lies flat"
    );
    let still = paths(&laid, primary);
    laid.pump_for(ms(300));
    assert_eq!(paths(&laid, primary), still, "and still");

    let (mut laid, vsync) = mount(
        CircularWavyProgressIndicator::new(),
        48.0,
        48.0,
        TextDirection::Ltr,
    );
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::ZERO);
    let still = paths(&laid, primary);
    assert_eq!(still.len(), 1, "an indeterminate wavy arc stays visible");
    assert!(circular_swing(&laid, primary) < 1e-9, "flat");
    laid.pump_for(ms(300));
    assert_eq!(
        paths(&laid, primary),
        still,
        "a still frame under reduced motion"
    );
    assert!(
        !vsync.has_running(),
        "a parked indicator needs no continuation"
    );
    assert_eq!(announced(&mut laid), None);
}

/// Leaving the flat band eases the amplitude in over 500 ms.
pub fn wave_amplitude_eases_in() {
    let (primary, _) = colors();
    let (mut laid, vsync) = mount(
        LinearWavyProgressIndicator::new().value(Some(0.05)),
        WIDTH,
        10.0,
        TextDirection::Ltr,
    );
    assert_eq!(linear_swing(&laid, primary), 0.0);
    laid.pump_widget(VsyncScope::new(
        vsync,
        themed(
            LinearWavyProgressIndicator::new().value(Some(0.5)),
            TextDirection::Ltr,
        ),
    ));
    // The run's first tick anchors it at this frame.
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(250));
    let halfway = linear_swing(&laid, primary);
    assert!(halfway > 0.3 && halfway < 2.7, "halfway: {halfway}");
    laid.pump_for(ms(300));
    let full = linear_swing(&laid, primary);
    assert!(full > 2.9 && full <= 3.0 + 1e-9, "after 550 ms: {full}");
}

/// Under `MaterialApp` a `RefreshIndicator` shows a determinate circular indicator filling
/// with the pull, then an indeterminate one in the theme's primary color while refreshing.
pub fn material_app_refresh_shows_the_circular_indicator() {
    let (primary, track) = colors();
    let refresh = RefreshController::new();
    let vsync = Vsync::new();
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        RefreshIntent::command("Refresh"),
        SizedBox::new(24.0, 24.0),
    )))
    .expect("one labelled command");
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            MaterialApp::new(
                RefreshIndicator::new(
                    Scrollable::new()
                        .controller(ScrollController::new())
                        .child(SizedBox::new(300.0, 5000.0)),
                    alternatives,
                )
                .controller(refresh.clone())
                .on_refresh(|_cx| std::future::pending::<()>()),
            )
            .theme(ThemeData::light()),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    laid.pump_for(Duration::ZERO);
    assert!(arcs(&laid, primary).is_empty(), "nothing while idle");

    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 100.0);
    laid.pump();
    let pull = refresh.pull_distance_px();
    assert!(pull > 0.0 && pull < 80.0, "a partial pull: {pull}");
    let active = arcs(&laid, primary);
    assert_eq!(active.len(), 1, "the pull fills an arc: {active:?}");
    close(
        active[0].1,
        pull / 80.0 * TAU,
        "the arc is the pull's share",
    );
    assert_eq!(arcs(&laid, track).len(), 1, "with its track");

    laid.dispatch_pointer_move(150.0, 200.0);
    laid.dispatch_pointer_up(150.0, 200.0);
    laid.pump();
    assert!(refresh.is_refreshing());
    laid.pump_for(Duration::ZERO);
    let spinning = arcs(&laid, primary);
    assert_eq!(spinning.len(), 1, "{spinning:?}");
    close(spinning[0].1, 0.1 * TAU, "the indeterminate arc at rest");
    assert!(
        arcs(&laid, track).is_empty(),
        "an indeterminate arc has no track"
    );
    for _ in 0..20 {
        laid.pump_for(ms(16));
    }
    assert_ne!(arcs(&laid, primary), spinning, "it spins");
}

/// A determinate wave that is flat needs no frames: at rest in a flat band nothing runs, a
/// visible wave runs, and easing back into a flat band stops once the amplitude settles.
pub fn flat_waves_need_no_frames() {
    let (mut laid, vsync) = mount(
        LinearWavyProgressIndicator::new().value(Some(0.0)),
        WIDTH,
        10.0,
        TextDirection::Ltr,
    );
    laid.pump_for(ms(100));
    assert!(
        !vsync.has_running(),
        "a flat wave at rest requests no frames"
    );
    let rebuild = |laid: &mut LaidOut, value: f64| {
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            themed(
                LinearWavyProgressIndicator::new().value(Some(value)),
                TextDirection::Ltr,
            ),
        ));
        laid.pump_for(Duration::ZERO);
    };
    rebuild(&mut laid, 0.5);
    laid.pump_for(ms(600));
    assert!(vsync.has_running(), "a visible wave travels");
    rebuild(&mut laid, 0.05);
    laid.pump_for(ms(250));
    assert!(
        vsync.has_running(),
        "the wave keeps travelling while it eases out"
    );
    laid.pump_for(ms(300));
    laid.pump_for(ms(16));
    assert!(!vsync.has_running(), "a settled flat wave stops");
    assert_eq!(linear_swing(&laid, colors().0), 0.0);
}

/// Stretched across a huge layout, a wave's path stays finite and bounded in length.
pub fn huge_layouts_stay_bounded() {
    let (primary, _) = colors();
    let check = |laid: &LaidOut, what: &str| {
        let paths = paths(laid, primary);
        assert!(!paths.is_empty(), "{what}: a wave is drawn");
        for path in &paths {
            assert!(path.len() <= 4097, "{what}: {} points", path.len());
            assert!(
                path.iter().all(|p| p.x.is_finite() && p.y.is_finite()),
                "{what}: a non-finite point"
            );
        }
    };
    for side in [1e300, 1e308] {
        let (laid, _) = mount(
            CircularWavyProgressIndicator::new().value(Some(0.5)),
            side,
            side,
            TextDirection::Ltr,
        );
        check(&laid, &format!("circular {side}"));
        let (laid, _) = mount(
            CircularWavyProgressIndicator::new(),
            side,
            side,
            TextDirection::Ltr,
        );
        check(&laid, &format!("circular indeterminate {side}"));
        let (laid, _) = mount(
            LinearWavyProgressIndicator::new().value(Some(0.5)),
            side,
            10.0,
            TextDirection::Ltr,
        );
        check(&laid, &format!("linear {side}"));
    }
}
