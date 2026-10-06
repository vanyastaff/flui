//! `LinearProgressIndicator` on virtual time: the indeterminate bars against
//! Material 3's published timing (1750 ms cycle; bar ends delayed 0, 250, 650
//! and 900 ms, running 1000, 1000, 850 and 850 ms with emphasized-accelerate
//! easing), and the switch to a determinate fraction.

use std::time::Duration;

use flui_sdk::animation::{Cubic, Curve, Vsync};
use flui_sdk::painting::{Color, DrawOp};
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
    let theme = ThemeData::light();
    let primary = theme.color_scheme.primary;
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), Theme::new(theme, indicator)),
        tight(WIDTH, HEIGHT),
        vsync.clone(),
    );
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
    for t in [200, 250, 1000, 1100, 1250, 1600, 1750, 1950] {
        mounted.laid.pump_for(ms(t - now));
        now = t;
        assert_bars(&mounted, t % 1750);
    }
    assert_eq!(mounted.vsync.len(), 1);
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
