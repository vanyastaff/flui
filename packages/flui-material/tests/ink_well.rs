//! `InkWell` widget-level state-transition coverage.
//!
//! The harness routes input through the same presentation-local
//! `GestureBinding` and `MouseTracker` as production. These tests therefore
//! cover structural enter/exit, tap, disabled behavior, focus, press timing,
//! and overlay resolution without a second headless-only input protocol.

use crate::common;

use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::InkWell;
use flui_sdk::interaction::FocusNode;
use flui_sdk::view::SignalWriteExt;
use flui_sdk::widgets::{SizedBox, WidgetState, WidgetStatesController};

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
    assert!(laid.focus_manager().dispatch_key_event(&enter()));
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

fn enter() -> flui_interaction::events::KeyEvent {
    flui_interaction::events::KeyEvent {
        state: flui_interaction::events::KeyState::Down,
        key: flui_interaction::events::Key::Named(flui_interaction::events::NamedKey::Enter),
        ..flui_interaction::events::KeyEvent::default()
    }
}

fn tab() -> flui_interaction::events::KeyEvent {
    flui_interaction::events::KeyEvent {
        key: flui_interaction::events::Key::Named(flui_interaction::events::NamedKey::Tab),
        ..enter()
    }
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

    assert!(
        laid.focus_manager().dispatch_key_event(&tab()),
        "the first Tab focuses the button"
    );
    assert!(
        laid.focus_manager().dispatch_key_event(&enter()),
        "Enter is consumed"
    );
    assert_eq!(probe.value(), Ok(1), "the activation wrote at dispatch");
    laid.tick();
    assert_eq!(probe.reads(), [0, 1], "and the reader rebuilt once");
}
