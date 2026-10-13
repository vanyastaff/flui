//! `LinearProgressIndicator` on virtual time: the indeterminate bars against
//! Material 3's published timing (1750 ms cycle; bar ends delayed 0, 250, 650
//! and 900 ms, running 1000, 1000, 850 and 850 ms with emphasized-accelerate
//! easing), and the switch to a determinate fraction.

use std::time::Duration;

use flui_sdk::animation::{Cubic, Curve, Vsync};
use flui_sdk::painting::{Color, DrawOp, TextDirection};
use flui_sdk::widgets::Directionality;
use flui_sdk::widgets::animated::VsyncScope;
use flui_testing::a11y::Role;

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_material::{LinearProgressIndicator, Theme, ThemeData};

const WIDTH: f64 = 240.0;
const HEIGHT: f64 = 4.0;

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// Where one bar end is at `t` ms into a cycle: 0 before its delay, then
/// `cubic-bezier(0.3, 0, 0.8, 0.15)` of its own progress, then 1.
fn bar_end(t: f64, delay: f64, duration: f64) -> f64 {
    let local = ((t - delay) / duration).clamp(0.0, 1.0);
    Cubic::new(0.3, 0.0, 0.8, 0.15).transform(local)
}

/// The two bars, as `(tail, head)` fractions of the width, at `t` ms.
fn expected_bars(t: f64) -> [(f64, f64); 2] {
    [
        (bar_end(t, 250.0, 1000.0), bar_end(t, 0.0, 1000.0)),
        (bar_end(t, 900.0, 850.0), bar_end(t, 650.0, 850.0)),
    ]
}

/// The bars painted in the primary color, as `(left, right)` in pixels.
fn painted_bars(laid: &LaidOut, primary: Color) -> Vec<(f64, f64)> {
    laid.draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { rect, paint } if paint.color == primary => {
                Some((rect.left(), rect.right()))
            }
            _ => None,
        })
        .collect()
}

struct Mounted {
    laid: LaidOut,
    vsync: Vsync,
    primary: Color,
}

fn mount(indicator: LinearProgressIndicator) -> Mounted {
    mount_in(TextDirection::Ltr, indicator)
}

fn mount_in(direction: TextDirection, indicator: LinearProgressIndicator) -> Mounted {
    let theme = ThemeData::light();
    let primary = theme.color_scheme.primary;
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Directionality::new(direction, Theme::new(theme.clone(), indicator.clone())),
        ),
        tight(WIDTH, HEIGHT),
        vsync.clone(),
    );
    let registrations = vsync.len();
    let next = Vsync::new();
    laid.pump_widget(VsyncScope::new(
        next.clone(),
        Directionality::new(direction, Theme::new(theme.clone(), indicator.clone())),
    ));
    assert!(
        vsync.is_empty(),
        "retained progress state leaves its old registry"
    );
    assert_eq!(next.len(), registrations);
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        Directionality::new(direction, Theme::new(theme, indicator)),
    ));
    assert!(next.is_empty());
    // The first tick anchors the repeating run at virtual time zero.
    laid.pump_for(Duration::ZERO);
    Mounted {
        laid,
        vsync,
        primary,
    }
}

fn assert_bars(mounted: &Mounted, t: u64) {
    let painted = painted_bars(&mounted.laid, mounted.primary);
    let expected: Vec<(f64, f64)> = expected_bars(t as f64)
        .into_iter()
        .filter(|(tail, head)| head > tail)
        .map(|(tail, head)| (tail * WIDTH, head * WIDTH))
        .collect();
    assert_eq!(painted.len(), expected.len(), "{t} ms: {painted:?}");
    for (bar, want) in painted.iter().zip(&expected) {
        assert!(
            (bar.0 - want.0).abs() < 1e-6 && (bar.1 - want.1).abs() < 1e-6,
            "{t} ms: painted {bar:?}, expected {want:?}"
        );
    }
}

pub fn indeterminate_bars_follow_the_published_timing() {
    let mut mounted = mount(LinearProgressIndicator::new());
    let mut now = 0;
    // A delay's plateau, a run's end, both bars at once, the second bar
    // alone, and the next cycle's start.
    for t in [1, 200, 250, 1000, 1100, 1250, 1600, 1749, 1750, 1751, 1950] {
        mounted.laid.pump_for(ms(t - now));
        now = t;
        assert_bars(&mounted, t % 1750);
    }
    assert_eq!(mounted.vsync.len(), 1);

    let next = Vsync::new();
    mounted.laid.pump_widget(VsyncScope::new(
        next.clone(),
        Directionality::new(
            TextDirection::Ltr,
            Theme::new(ThemeData::light(), LinearProgressIndicator::new()),
        ),
    ));
    assert!(mounted.vsync.is_empty());
    assert_bars(&mounted, 200);
    // A new registry may have a different clock origin. Its first tick
    // preserves the painted phase; subsequent ticks advance from there.
    next.tick_all(
        &flui_sdk::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.0)),
    );
    mounted.laid.pump_for(Duration::ZERO);
    assert_bars(&mounted, 200);
    next.tick_all(
        &flui_sdk::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(50.25)),
    );
    mounted.laid.pump_for(Duration::ZERO);
    assert_bars(&mounted, 450);
}

pub fn switching_to_a_value_stops_the_bars() {
    let mut mounted = mount(LinearProgressIndicator::new());
    mounted.laid.pump_for(ms(1749));
    assert_eq!(mounted.vsync.len(), 1, "indeterminate: registered");
    let theme = ThemeData::light();
    mounted.laid.pump_widget(VsyncScope::new(
        mounted.vsync.clone(),
        Theme::new(theme, LinearProgressIndicator::new().value(Some(0.25))),
    ));
    assert_eq!(
        mounted.vsync.len(),
        0,
        "determinate: the controller is released"
    );
    assert_eq!(painted_bars(&mounted.laid, mounted.primary), [(0.0, 60.0)]);
    mounted.laid.pump_for(ms(500));
    assert_eq!(painted_bars(&mounted.laid, mounted.primary), [(0.0, 60.0)]);
    mounted.laid.pump_widget(VsyncScope::new(
        mounted.vsync.clone(),
        Theme::new(ThemeData::light(), LinearProgressIndicator::new()),
    ));
    mounted.laid.pump_for(Duration::ZERO);
    assert_bars(&mounted, 0);
    mounted.laid.pump_for(ms(200));
    assert_bars(&mounted, 200);
}

pub fn values_outside_the_range_are_clamped() {
    for (value, right) in [(Some(-1.0), 0.0), (Some(f64::NAN), 0.0), (Some(3.0), WIDTH)] {
        let mounted = mount(LinearProgressIndicator::new().value(value));
        let bars = painted_bars(&mounted.laid, mounted.primary);
        let painted_right = bars.first().map_or(0.0, |bar| bar.1);
        assert_eq!(painted_right, right, "{value:?}");
        assert_eq!(mounted.vsync.len(), 0, "a value runs no controller");
    }
}

pub fn right_to_left_progress_starts_at_the_right() {
    let mounted = mount_in(
        TextDirection::Rtl,
        LinearProgressIndicator::new().value(Some(0.25)),
    );
    assert_eq!(
        painted_bars(&mounted.laid, mounted.primary),
        [(180.0, 240.0)]
    );

    let mut mounted = mount_in(TextDirection::Rtl, LinearProgressIndicator::new());
    mounted.laid.pump_for(ms(1100));
    let painted = painted_bars(&mounted.laid, mounted.primary);
    // Both bars are on the track at 1100 ms; each is its LTR span mirrored
    // about the centre.
    let expected: Vec<(f64, f64)> = expected_bars(1100.0)
        .into_iter()
        .map(|(tail, head)| (WIDTH - head * WIDTH, WIDTH - tail * WIDTH))
        .collect();
    assert_eq!(painted.len(), 2, "{painted:?}");
    for (bar, want) in painted.iter().zip(&expected) {
        assert!(
            (bar.0 - want.0).abs() < 1e-6 && (bar.1 - want.1).abs() < 1e-6,
            "painted {bar:?}, expected {want:?}"
        );
    }
}

pub fn negative_zero_announces_zero_percent() {
    let mut mounted = mount(LinearProgressIndicator::new().value(Some(-0.0)));
    mounted.laid.enable_semantics();
    mounted.laid.tick();
    let tree = mounted.laid.a11y_tree().expect("semantics enabled");
    let node = tree.find_by_label("Loading").expect("the default label");
    assert_eq!(node.value(), Some("0%"));
}

pub fn announced_as_progress() {
    let mut mounted = mount(
        LinearProgressIndicator::new()
            .value(Some(0.4))
            .label("Upload"),
    );
    mounted.laid.enable_semantics();
    mounted.laid.tick();
    let tree = mounted.laid.a11y_tree().expect("semantics enabled");
    let node = tree.find_by_label("Upload").expect("the indicator's label");
    assert_eq!(node.role(), Role::ProgressIndicator);
    assert_eq!(node.value(), Some("40%"));

    let mut mounted = mount(LinearProgressIndicator::new());
    mounted.laid.enable_semantics();
    mounted.laid.tick();
    let tree = mounted.laid.a11y_tree().expect("semantics enabled");
    let node = tree.find_by_label("Loading").expect("the default label");
    assert_eq!(node.role(), Role::ProgressIndicator);
}

pub fn reduced_motion_keeps_visible_loading_without_continuation() {
    use flui_sdk::animation::MotionOverride;
    use flui_sdk::widgets::SizedBox;
    fn live_ltr() {
        policy_case(TextDirection::Ltr, false);
    }
    fn live_rtl() {
        policy_case(TextDirection::Rtl, false);
    }
    fn initial_ltr() {
        policy_case(TextDirection::Ltr, true);
    }
    fn initial_rtl() {
        policy_case(TextDirection::Rtl, true);
    }
    fn policy_case(direction: TextDirection, initially_reduced: bool) {
        let theme = ThemeData::light();
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(vsync.clone(), SizedBox::shrink()),
            tight(WIDTH, HEIGHT),
            vsync.clone(),
        );
        laid.set_motion_override(if initially_reduced {
            MotionOverride::Reduce
        } else {
            MotionOverride::Full
        });
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            Directionality::new(
                direction,
                Theme::new(theme.clone(), LinearProgressIndicator::new()),
            ),
        ));
        laid.pump_for(Duration::ZERO);
        if !initially_reduced {
            laid.pump_for(ms(200));
            laid.set_motion_override(MotionOverride::Reduce);
            laid.pump_for(Duration::ZERO);
        }
        let stopped = painted_bars(&laid, theme.color_scheme.primary);
        assert!(
            !stopped.is_empty(),
            "static unknown-total loading stays visible"
        );
        assert_eq!(
            stopped,
            [(90.0, 150.0)],
            "unknown work is centered, not a fraction filled"
        );
        laid.pump_for(ms(350));
        assert_eq!(
            painted_bars(&laid, theme.color_scheme.primary),
            stopped,
            "Reduce stops visible motion"
        );
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
        let resumed = painted_bars(&laid, theme.color_scheme.primary);
        laid.pump_for(ms(200));
        assert_ne!(
            painted_bars(&laid, theme.color_scheme.primary),
            resumed,
            "Full resumes visible motion"
        );
        laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::shrink()));
        assert!(vsync.is_empty());
    }
    crate::common::run_cases(&[
        ("live LTR", live_ltr),
        ("live RTL", live_rtl),
        ("initial LTR", initial_ltr),
        ("initial RTL", initial_rtl),
    ]);
}

pub fn initially_inactive_loading_stays_visible_without_a_percentage() {
    use flui_sdk::animation::MotionOverride;
    use flui_sdk::widgets::TickerMode;
    fn disabled_mode() {
        inactive_case(false);
    }
    fn detached_scope() {
        inactive_case(true);
    }
    fn inactive_case(detached: bool) {
        let vsync = Vsync::new();
        let theme = ThemeData::light();
        let indicator = Theme::new(theme.clone(), LinearProgressIndicator::new());
        let root = if detached {
            VsyncScope::detached(TickerMode::new(indicator))
        } else {
            VsyncScope::new(vsync.clone(), TickerMode::new(indicator).enabled(false))
        };
        let mut laid = lay_out_animated(root, tight(WIDTH, HEIGHT), vsync.clone());
        laid.set_motion_override(MotionOverride::Full);
        laid.pump_for(Duration::ZERO);
        let stopped = painted_bars(&laid, theme.color_scheme.primary);
        assert!(
            !stopped.is_empty(),
            "initially inactive loading needs visible busy feedback"
        );
        assert_eq!(stopped, [(90.0, 150.0)]);
        laid.pump_for(ms(500));
        assert_eq!(painted_bars(&laid, theme.color_scheme.primary), stopped);
        laid.enable_semantics();
        laid.tick();
        let tree = laid.a11y_tree().expect("semantics enabled");
        let node = tree
            .find_by_label("Loading")
            .expect("loading label retained");
        assert_eq!(node.role(), Role::ProgressIndicator);
        assert_eq!(node.value(), None);
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            TickerMode::new(Theme::new(theme.clone(), LinearProgressIndicator::new())),
        ));
        laid.pump_for(Duration::ZERO);
        let resumed = painted_bars(&laid, theme.color_scheme.primary);
        laid.pump_for(ms(200));
        assert_ne!(
            painted_bars(&laid, theme.color_scheme.primary),
            resumed,
            "binding or enabling the retained indicator restores motion"
        );
        laid.pump_widget(VsyncScope::new(
            vsync.clone(),
            flui_sdk::widgets::SizedBox::shrink(),
        ));
        assert!(vsync.is_empty());
    }
    crate::common::run_cases(&[
        ("disabled mode", disabled_mode),
        ("detached scope", detached_scope),
    ]);
}

pub fn nested_pause_and_policy_changes_retain_visible_loading() {
    use flui_sdk::animation::MotionOverride;
    use flui_sdk::widgets::{SizedBox, TickerMode};
    let vsync = Vsync::new();
    let theme = ThemeData::light();
    let tree = |registry: &Vsync, enabled| {
        VsyncScope::new(
            registry.clone(),
            TickerMode::new(TickerMode::new(Theme::new(
                theme.clone(),
                LinearProgressIndicator::new(),
            )))
            .enabled(enabled),
        )
    };
    let mut laid = lay_out_animated(tree(&vsync, false), tight(WIDTH, HEIGHT), vsync.clone());
    laid.set_motion_override(MotionOverride::Full);
    laid.pump_for(Duration::ZERO);
    let stopped = painted_bars(&laid, theme.color_scheme.primary);
    assert!(
        !stopped.is_empty(),
        "enabled child cannot override a disabled ancestor"
    );
    assert_eq!(stopped, [(90.0, 150.0)]);
    laid.pump_for(ms(500));
    assert_eq!(painted_bars(&laid, theme.color_scheme.primary), stopped);
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::ZERO);
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(ms(350));
    assert_eq!(painted_bars(&laid, theme.color_scheme.primary), stopped);
    let next = Vsync::new();
    laid.pump_widget(tree(&next, false));
    laid.adopt_vsync(next.clone())
        .expect("replacement registry remains admissible");
    laid.pump_widget(tree(&next, true));
    laid.pump_for(ms(350));
    assert_eq!(
        painted_bars(&laid, theme.color_scheme.primary),
        stopped,
        "resuming the configured mode does not override Reduce"
    );
    assert!(!next.has_running());
    laid.set_motion_override(MotionOverride::Full);
    laid.pump_for(Duration::ZERO);
    let resumed = painted_bars(&laid, theme.color_scheme.primary);
    laid.pump_for(ms(200));
    assert_ne!(painted_bars(&laid, theme.color_scheme.primary), resumed);
    laid.pump_widget(tree(&next, false));
    assert_eq!(
        painted_bars(&laid, theme.color_scheme.primary),
        stopped,
        "live pause replaces the animated painter immediately"
    );
    laid.pump_widget(VsyncScope::new(next.clone(), SizedBox::shrink()));
    assert!(!next.has_running());
    assert!(!vsync.has_running());
}

pub fn reduced_motion_determinate_switches_keep_their_meaning() {
    use flui_sdk::animation::MotionOverride;
    fn ltr() {
        switch_case(TextDirection::Ltr);
    }
    fn rtl() {
        switch_case(TextDirection::Rtl);
    }
    fn switch_case(direction: TextDirection) {
        let mut mounted = mount_in(direction, LinearProgressIndicator::new());
        mounted.laid.set_motion_override(MotionOverride::Reduce);
        mounted.laid.pump_for(Duration::ZERO);
        let unknown = painted_bars(&mounted.laid, mounted.primary);
        assert_eq!(unknown, [(90.0, 150.0)]);
        mounted.laid.pump_widget(VsyncScope::new(
            mounted.vsync.clone(),
            Directionality::new(
                direction,
                Theme::new(
                    ThemeData::light(),
                    LinearProgressIndicator::new().value(Some(0.25)),
                ),
            ),
        ));
        let expected = if direction == TextDirection::Ltr {
            (0.0, 60.0)
        } else {
            (180.0, 240.0)
        };
        assert_eq!(painted_bars(&mounted.laid, mounted.primary), [expected]);
        assert!(mounted.vsync.is_empty());
        mounted.laid.enable_semantics();
        mounted.laid.tick();
        let tree = mounted.laid.a11y_tree().expect("semantics enabled");
        assert_eq!(
            tree.find_by_label("Loading").expect("label").value(),
            Some("25%")
        );
        mounted.laid.pump_widget(VsyncScope::new(
            mounted.vsync.clone(),
            Directionality::new(
                direction,
                Theme::new(ThemeData::light(), LinearProgressIndicator::new()),
            ),
        ));
        mounted.laid.pump_for(ms(500));
        assert_eq!(painted_bars(&mounted.laid, mounted.primary), unknown);
        assert!(!mounted.vsync.has_running());
        let tree = mounted.laid.a11y_tree().expect("semantics enabled");
        assert_eq!(tree.find_by_label("Loading").expect("label").value(), None);
    }
    crate::common::run_cases(&[("LTR", ltr), ("RTL", rtl)]);
}
