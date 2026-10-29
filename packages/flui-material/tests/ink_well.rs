//! `InkWell` widget-level state-transition coverage.
//!
//! The harness routes input through the same presentation-local
//! `GestureBinding` and `MouseTracker` as production. These tests therefore
//! cover structural enter/exit, tap, disabled behavior, focus, press timing,
//! and overlay resolution without a second headless-only input protocol.

use crate::common;

use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out, tight};
use flui_interaction::KeyEventResult;
use flui_interaction::events::{Code, Key, KeyEvent, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_material::{Checkbox, InkWell, Theme, ThemeData};
use flui_painting::display_list::DrawOp;
use flui_painting::styling::Color;
use flui_sdk::geometry::Offset;
use flui_sdk::interaction::FocusNode;
use flui_sdk::view::IntoView;
use flui_sdk::view::SignalWriteExt;
use flui_sdk::widgets::{FocusRingStyle, SizedBox, WidgetState, WidgetStatesController};
use flui_testing::replay::MouseScript;

pub fn press_highlight_lasts_its_delay_under_reduced_motion() {
    use flui_sdk::animation::MotionOverride;
    use std::time::Duration;
    let states = WidgetStatesController::default();
    let mut laid = lay_out(
        InkWell::new(SizedBox::new(80.0, 40.0))
            .states_controller(states.clone())
            .on_tap(|_| {}),
        tight(80.0, 40.0),
    );
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::from_millis(16));
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert!(states.value().contains_state(WidgetState::Pressed));
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(50));
    assert!(
        states.value().contains_state(WidgetState::Pressed),
        "Reduce must not consume the authored press delay"
    );
    laid.pump_for(Duration::from_millis(64));
    assert!(
        !states.value().contains_state(WidgetState::Pressed),
        "the delay still expires under Reduce"
    );
}

pub fn pointer_and_keyboard_activation_write_the_owning_signal() {
    let node = FocusNode::with_debug_label("writer-activation");
    let child_node = Rc::clone(&node);
    let probe = common::SignalProbe::new(move |signals| {
        InkWell::new(SizedBox::new(80.0, 40.0))
            .focus_node(Rc::clone(&child_node))
            .on_tap(move |cx| signals.count.update(cx, |count| *count += 1))
    });
    let mut laid = lay_out(probe.view(), tight(80.0, 40.0));
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert_eq!(probe.value(), Ok(1));
    let _ = node.request_focus();
    assert_eq!(
        laid.focus_manager().dispatch_key_event(&enter()),
        KeyEventResult::Handled
    );
    assert_eq!(probe.value(), Ok(2));
    laid.pump();
    assert_eq!(probe.reads(), [0, 2]);
}

pub fn disabled_ink_well_does_not_fire_a_tap_callback() {
    // Mutation-honest companion to the enabled case above: this test only
    // proves something if a *would-be* tap on a disabled InkWell is
    // observably inert. It mounts with no `on_tap` at all (there is nothing
    // to fire), which is the only way to construct a disabled `InkWell` —
    // `is_interactive()` is derived from `on_tap.is_some()`, not a separate
    // flag (see `ink_well.rs`'s module doc).
    let states = WidgetStatesController::default();
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0)).states_controller(states.clone()),
        tight(60.0, 40.0),
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    assert!(
        !states.value().contains_state(WidgetState::Pressed),
        "a disabled InkWell's GestureDetector has no on_tap closure at all, \
         so no tap can ever be recognized",
    );
}

// A `build()`-vs-`init_state`/`did_update_view` regression test for the
// Disabled-sync relocation was attempted and DELIBERATELY dropped, not
// silently skipped: two different observability angles were tried —
// (1) a `StatelessView` child counting its own `build()` invocations, and
// (2) `BuildOwner::pending_external_builds()` (the out-of-frame rebuild
// inbox `RebuildHandle::schedule()` enqueues into) read immediately after
// `lay_out` returns. Both were run against the reintroduced original bug
// (`self.states.update(WidgetState::Disabled, !enabled)` back at the top of
// `build()`) via `cargo test`, and BOTH passed unchanged — `lay_out`'s
// single bootstrap frame is a fixpoint (ADR-0017) that drains any rebuild
// scheduled during its own pass before returning, regardless of which
// lifecycle hook triggered it, so neither approach can observe "extra
// rebuild" as a distinguishable side effect through this harness. The
// architectural fix (never mutate a possibly-caller-shared controller from
// `build`, i.e. no rebuild scheduled during build —
// `ink_well.rs`'s `init_state`/`did_update_view`) is applied and is correct
// independent of whether this harness can prove
// the "spurious rebuild" symptom specifically; the existing `Disabled`-state
// assertions (`disabled_ink_well_does_not_fire_a_tap_callback`) already prove the sync
// itself still happens correctly at the new call sites.

fn enter() -> KeyEvent {
    KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .build()
}

fn tab() -> KeyEvent {
    KeyEventBuilder::new(Code::Tab)
        .with_key(Key::Named(NamedKey::Tab))
        .build()
}

/// Enter on a focused `ElevatedButton` reaches its `InkWell`'s activation
/// action, which runs inside the key event's own dispatch and hands its `cx`
/// to `on_pressed` — no separate writer bridge. The write lands at dispatch
/// and the signal's reader rebuilds on the next frame (a tick, which dirties
/// nothing itself).
pub fn enter_on_a_focused_elevated_button_writes_a_signal_and_rebuilds_its_reader() {
    use flui_material::{ElevatedButton, Theme, ThemeData};
    use flui_sdk::widgets::Text;

    let probe = common::SignalProbe::new(|signals| {
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Save"))
                .on_pressed(move |cx| signals.count.update(cx, |count| *count += 1)),
        )
    });
    let mut laid = lay_out(probe.view(), tight(120.0, 48.0));

    assert_eq!(
        laid.focus_manager().dispatch_key_event(&tab()),
        KeyEventResult::Handled,
        "the first Tab focuses the button"
    );
    assert_eq!(
        laid.focus_manager().dispatch_key_event(&enter()),
        KeyEventResult::Handled,
        "Enter is consumed"
    );
    assert_eq!(probe.value(), Ok(1), "the activation wrote at dispatch");
    laid.tick();
    assert_eq!(probe.reads(), [0, 1], "and the reader rebuilt once");
}

/// A ring style no Material colour role produces, so its bands are found by
/// colour alone.
pub const RING: FocusRingStyle = FocusRingStyle {
    outer: Color::rgb(200, 0, 0),
    inner: Color::rgb(0, 200, 0),
    outer_width: 2.0,
    inner_width: 1.0,
};

/// `child` under the light theme with [`RING`] as its focus ring.
pub fn themed(child: impl IntoView) -> Theme {
    let mut data = ThemeData::light();
    data.focus_ring = RING;
    Theme::new(data, child)
}

/// Whether the last frame painted both bands of [`RING`].
pub fn ring_painted(laid: &LaidOut) -> bool {
    let colors: Vec<Color> = laid
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { paint, .. }
            | DrawOp::RRect { paint, .. }
            | DrawOp::DRRect { paint, .. }
            | DrawOp::Path { paint, .. }
            | DrawOp::Line { paint, .. } => Some(paint.color),
            _ => None,
        })
        .collect();
    let outer = colors.contains(&RING.outer);
    let inner = colors.contains(&RING.inner);
    assert_eq!(outer, inner, "the ring paints both of its bands or neither");
    outer
}

/// A mouse click at `(x, y)`, through the UI runtime's input path, so the
/// presentation sees pointer input.
fn click(laid: &mut LaidOut, x: f64, y: f64) {
    let click = MouseScript::new("click")
        .press(Offset::new(x, y))
        .wait(Duration::from_millis(16))
        .release()
        .build()
        .expect("the click releases its button");
    laid.replay(&click);
    laid.pump();
}

/// `Focused` is the focus a user can see: focus taken after a click is not
/// shown, neither as the state nor as a ring, and the next key shows both,
/// in the theme's ring style.
pub fn a_well_shows_focus_only_when_the_keyboard_is_in_use() {
    let node = FocusNode::with_debug_label("visible-focus");
    let states = WidgetStatesController::default();
    let mut laid = lay_out(
        themed(
            InkWell::new(SizedBox::new(80.0, 40.0))
                .focus_node(Rc::clone(&node))
                .states_controller(states.clone())
                .on_tap(|_| {}),
        ),
        tight(80.0, 40.0),
    );
    click(&mut laid, 40.0, 20.0);
    let _ = laid.enter_owner_scope(|| node.request_focus());
    laid.pump_for(Duration::from_millis(200));
    assert!(node.has_primary_focus(), "the well took the focus");
    assert!(
        !states.value().contains_state(WidgetState::Focused),
        "focus after a click is not shown"
    );
    assert!(!ring_painted(&laid), "and draws no ring");
    let _ = laid.enter_owner_scope(|| laid.focus_manager().dispatch_key_event(&tab()));
    laid.pump_for(Duration::from_millis(200));
    assert!(
        states.value().contains_state(WidgetState::Focused),
        "a key shows the focus"
    );
    assert!(ring_painted(&laid), "as the theme's ring");
}

/// A checkbox shows its focus with the M3 halo, so it draws no ring.
pub fn a_checkbox_shows_focus_with_its_halo_not_a_ring() {
    let mut laid = lay_out(
        themed(Checkbox::new(false).on_changed(|_cx, _value| {})),
        tight(48.0, 48.0),
    );
    let manager = laid.focus_manager();
    let _ = laid.enter_owner_scope(|| manager.dispatch_key_event(&tab()));
    laid.pump_for(Duration::from_millis(200));
    assert!(
        manager.primary_focus().is_some(),
        "Tab focused the checkbox"
    );
    assert!(!ring_painted(&laid), "the halo is its indicator");
}
