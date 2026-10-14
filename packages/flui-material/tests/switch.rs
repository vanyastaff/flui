//! `Switch` widget-level mount/interaction coverage.
//!
//! Complements `switch.rs`'s own unit tests (M3 default token-table probes,
//! thumb-radius selection, painter `should_repaint`) with end-to-end
//! mount/dispatch proof: a real down+up through the render tree reaches
//! [`Switch::on_changed`] with `!value`, and a handler-removal/-addition
//! across the lifecycle resyncs interactivity — the same classes
//! `tests/checkbox.rs` proves for `Checkbox`, since `Switch` shares the same
//! `InkWell`-composition shape.
//!
//! The toggle motion is read from the recorded display list at virtual
//! times: the thumb circle's centre and radius and the track fill.
//!
//! **Not covered here:** the M3 default token-table branch
//! order/combined-state values, the widget -> theme -> default tier
//! precedence, `active_thumb_color`'s `!Disabled && Selected` gate and the
//! `colorScheme.surface` alpha blend (computed by
//! `resolve_switch_thumb_color`, outside `build`). No test asserts them.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out, lay_out_animated, tight};
use flui_material::{Switch, Theme, ThemeData};
use flui_painting::DrawOp;
use flui_sdk::animation::{MotionOverride, Vsync};
use flui_sdk::painting::{Color, TextDirection};
use flui_sdk::view::View;
use flui_sdk::widgets::{Directionality, VsyncScope};
use flui_testing::a11y::{Role, Toggled};

/// The switch's full tap target (track + M3 default horizontal padding).
const TAP_TARGET_WIDTH: f64 = 60.0;
const TAP_TARGET_HEIGHT: f64 = 48.0;

fn constraints() -> flui_sdk::rendering::BoxConstraints {
    tight(TAP_TARGET_WIDTH, TAP_TARGET_HEIGHT)
}

/// Every `Switch` needs a [`Theme`] ancestor (`Theme::of` panics without
/// one) — mirrors `tests/checkbox.rs`'s own `themed` helper.
fn themed(switch: Switch) -> Theme {
    Theme::new(ThemeData::light(), switch)
}

pub fn tap_fires_on_changed_with_the_flipped_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let laid = lay_out(
        themed(Switch::new(false).on_changed(move |_cx, next| {
            *recorder.borrow_mut() = Some(next);
        })),
        constraints(),
    );

    laid.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);

    assert_eq!(
        *observed.borrow(),
        Some(true),
        "a tap on an off, enabled switch must fire on_changed(true)",
    );
}

pub fn disabled_switch_swallows_a_tap_then_resyncs_once_a_handler_is_added() {
    // Same "handler-removal resync" class `tests/checkbox.rs` proves for
    // `Checkbox`: `Switch` shares its `WidgetStatesController` with the
    // `InkWell` it builds, so adding `on_changed` across a rebuild must
    // flip `Disabled` and make the NEXT tap interactive.
    let taps = Rc::new(RefCell::new(0_u32));

    let laid_disabled = lay_out(themed(Switch::new(false)), constraints());
    laid_disabled.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid_disabled.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    // No on_changed at all: nothing to observe going wrong beyond "does not
    // panic" — the InkWell-level swallow behavior itself is already proven
    // by `tests/ink_well.rs`'s disabled-state coverage.

    let mut laid_enabled = laid_disabled;
    let counter = Rc::clone(&taps);
    laid_enabled.pump_widget(themed(Switch::new(false).on_changed(move |_cx, _| {
        *counter.borrow_mut() += 1;
    })));
    laid_enabled.dispatch_pointer_down(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);
    laid_enabled.dispatch_pointer_up(TAP_TARGET_WIDTH / 2.0, TAP_TARGET_HEIGHT / 2.0);

    assert_eq!(
        *taps.borrow(),
        1,
        "adding on_changed on rebuild must make the very next tap interactive",
    );
}

pub fn mounted_switch_exports_its_role_and_toggle_state() {
    for (value, expected) in [(false, Toggled::False), (true, Toggled::True)] {
        let mut laid = lay_out(
            themed(Switch::new(value).on_changed(|_cx, _| {})),
            constraints(),
        );
        laid.enable_semantics();
        laid.pump();
        let tree = laid
            .a11y_tree()
            .expect("semantics enabled before the frame");
        let switch = tree
            .find(Role::Switch)
            .unwrap_or_else(|error| panic!("expected one switch node: {error}"));
        assert_eq!(switch.toggled(), Some(expected), "a switch set to {value}");
    }
}

/// The thumb centre's x in the tap target with the track's inner ends at
/// 20 (start) and 40 (end): `(60 - 52) / 2 + 32 / 2` and that plus 20.
const THUMB_START_X: f64 = 20.0;
const THUMB_END_X: f64 = 40.0;
/// Half of the default 200 ms toggle.
const HALF_TOGGLE: Duration = Duration::from_millis(100);
/// Long enough for any default toggle to settle.
const SETTLE: Duration = Duration::from_secs(1);

/// What one frame of the switch painted.
#[derive(Debug, Clone, Copy)]
struct SwitchFrame {
    thumb_x: f64,
    thumb_radius: f64,
    track: Color,
}

fn painted_switch(laid: &LaidOut) -> SwitchFrame {
    let ops = laid.draw_ops();
    let (thumb_x, thumb_radius) = ops
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Circle { center, radius, .. } => Some((
                command.transform.transform_point(center.x, center.y).0,
                *radius,
            )),
            _ => None,
        })
        .expect("the switch paints its thumb as a circle");
    let track = ops
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::RRect { rrect, paint } if (rrect.width() - 52.0).abs() < 1e-9 => {
                Some(paint.color)
            }
            _ => None,
        })
        .expect("the switch paints its 52-wide track");
    SwitchFrame {
        thumb_x,
        thumb_radius,
        track,
    }
}

fn animated_switch(vsync: &Vsync, value: bool, direction: TextDirection) -> impl View {
    VsyncScope::new(
        vsync.clone(),
        Directionality::new(
            direction,
            themed(Switch::new(value).on_changed(|_cx, _| {})),
        ),
    )
}

/// Mounts an off switch, turns it on and pumps the frame that starts the
/// toggle; returns the harness and the settled off frame.
fn toggled_on(direction: TextDirection, policy: MotionOverride) -> (LaidOut, SwitchFrame) {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        animated_switch(&vsync, false, direction),
        constraints(),
        vsync.clone(),
    );
    laid.set_motion_override(policy);
    laid.pump_for(Duration::ZERO);
    let off = painted_switch(&laid);
    laid.pump_widget(animated_switch(&vsync, true, direction));
    laid.pump_for(Duration::ZERO);
    (laid, off)
}

fn strictly_between(value: f64, a: f64, b: f64) -> bool {
    value > a.min(b) + 1e-6 && value < a.max(b) - 1e-6
}

/// Half way through a toggle the thumb centre, the thumb radius and the
/// track colour are each strictly between their off and on values; right
/// to left mirrors the thumb's travel about the track's centre.
pub fn a_toggle_is_strictly_between_its_ends_at_half_time_and_mirrored_in_rtl() {
    let mut halves = Vec::new();
    for (direction, off_x, on_x) in [
        (TextDirection::Ltr, THUMB_START_X, THUMB_END_X),
        (TextDirection::Rtl, THUMB_END_X, THUMB_START_X),
    ] {
        let (mut laid, off) = toggled_on(direction, MotionOverride::Full);
        laid.pump_for(HALF_TOGGLE);
        let half = painted_switch(&laid);
        laid.pump_for(SETTLE);
        let on = painted_switch(&laid);

        assert!(
            (off.thumb_x - off_x).abs() < 1e-6 && (on.thumb_x - on_x).abs() < 1e-6,
            "{direction:?}: the thumb settles at {off_x} off and {on_x} on, got {off:?} and {on:?}"
        );
        assert!(
            strictly_between(half.thumb_x, off_x, on_x),
            "{direction:?}: half way the thumb is between {off_x} and {on_x}, got {}",
            half.thumb_x
        );
        assert!(
            strictly_between(half.thumb_radius, 8.0, 12.0),
            "{direction:?}: half way the thumb radius is between 8 and 12, got {}",
            half.thumb_radius
        );
        assert!(
            half.track != off.track && half.track != on.track,
            "{direction:?}: half way the track is neither its off colour {:?} nor its on colour \
             {:?}, got {:?}",
            off.track,
            on.track,
            half.track
        );
        halves.push(half.thumb_x);
    }
    let centre = f64::midpoint(THUMB_START_X, THUMB_END_X);
    assert!(
        ((halves[0] - centre) + (halves[1] - centre)).abs() < 1e-6,
        "right to left mirrors the half-way thumb about {centre}: {halves:?}"
    );
}

/// Turning a switch back off part way through turning it on continues from
/// the shown thumb position instead of jumping to an end.
pub fn reversing_a_toggle_mid_flight_continues_from_the_shown_position() {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        animated_switch(&vsync, false, TextDirection::Ltr),
        constraints(),
        vsync.clone(),
    );
    laid.pump_widget(animated_switch(&vsync, true, TextDirection::Ltr));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(HALF_TOGGLE);
    let shown = painted_switch(&laid).thumb_x;
    assert!(
        strictly_between(shown, THUMB_START_X, THUMB_END_X),
        "the toggle is in flight, got {shown}"
    );

    laid.pump_widget(animated_switch(&vsync, false, TextDirection::Ltr));
    laid.pump_for(Duration::ZERO);
    let reversed = painted_switch(&laid).thumb_x;
    assert!(
        (reversed - shown).abs() < 0.5,
        "reversing keeps the shown thumb at {shown}, got {reversed}"
    );
    // The reversal inherits the thumb's velocity, so the next frame stays
    // close by rather than jumping, and the thumb then heads back.
    laid.pump_for(Duration::from_millis(16));
    let next = painted_switch(&laid).thumb_x;
    assert!(
        (next - reversed).abs() < 4.0,
        "the frame after reversing stays near {reversed}, got {next}"
    );
    // A reversal is shortened by what was already travelled (about half).
    laid.pump_for(Duration::from_millis(40));
    let returning = painted_switch(&laid).thumb_x;
    assert!(
        returning < reversed && returning > THUMB_START_X,
        "the reversed thumb heads back toward {THUMB_START_X} from {reversed}, got {returning}"
    );
    laid.pump_for(SETTLE);
    assert!(
        (painted_switch(&laid).thumb_x - THUMB_START_X).abs() < 1e-6,
        "the reversed toggle settles off"
    );
}

/// Under reduced motion the first frame after a toggle shows the new value,
/// where full motion is still at the start of its run.
pub fn reduced_motion_shows_a_toggle_on_its_first_frame() {
    let (full, _) = toggled_on(TextDirection::Ltr, MotionOverride::Full);
    let full = painted_switch(&full);
    assert!(
        full.thumb_x < THUMB_END_X - 1e-6,
        "full motion has not arrived on the first frame, got {full:?}"
    );

    let (reduced, _) = toggled_on(TextDirection::Ltr, MotionOverride::Reduce);
    let reduced = painted_switch(&reduced);
    assert!(
        (reduced.thumb_x - THUMB_END_X).abs() < 1e-6 && (reduced.thumb_radius - 12.0).abs() < 1e-6,
        "reduced motion shows the on thumb at once, got {reduced:?}"
    );
}
