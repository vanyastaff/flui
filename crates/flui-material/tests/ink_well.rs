//! `InkWell` widget-level state-transition coverage.
//!
//! The harness routes input through the same presentation-local
//! `GestureBinding` and `MouseTracker` as production. These tests therefore
//! cover structural enter/exit, tap, disabled behavior, focus, press timing,
//! and overlay resolution without a second headless-only input protocol.

mod common;

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_interaction::FocusNode;
use flui_material::InkWell;
use flui_types::Color;
use flui_widgets::animated::VsyncScope;
use flui_widgets::{SizedBox, WidgetState, WidgetStatesController};

#[test]
fn hover_updates_widget_states_when_the_pointer_moves_over_the_ink_well() {
    let states = WidgetStatesController::default();
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(states.clone()),
        tight(60.0, 40.0),
    );

    assert!(!states.value().contains_state(WidgetState::Hovered));

    laid.dispatch_pointer_move(10.0, 10.0);

    assert!(
        states.value().contains_state(WidgetState::Hovered),
        "a pointer move over the InkWell must set WidgetState::Hovered",
    );
}

#[test]
fn hover_clears_when_the_pointer_exits_the_ink_well() {
    let states = WidgetStatesController::default();
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(states.clone()),
        tight(60.0, 40.0),
    );

    laid.dispatch_pointer_move(10.0, 10.0);
    assert!(states.value().contains_state(WidgetState::Hovered));

    laid.dispatch_pointer_move(80.0, 60.0);
    assert!(
        !states.value().contains_state(WidgetState::Hovered),
        "leaving the InkWell must clear WidgetState::Hovered",
    );
}

#[test]
fn disabled_ink_well_does_not_update_hovered_state() {
    // No `.on_tap(..)`: `is_interactive()` is false, so the oracle's
    // `handleMouseEnter` gate on `enabled` should suppress the Hovered
    // update even though the pointer really did move over the region.
    let states = WidgetStatesController::default();
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0)).states_controller(states.clone()),
        tight(60.0, 40.0),
    );

    assert!(states.value().contains_state(WidgetState::Disabled));

    laid.dispatch_pointer_move(10.0, 10.0);

    assert!(
        !states.value().contains_state(WidgetState::Hovered),
        "a disabled InkWell must not report Hovered on pointer move",
    );
}

#[test]
fn tap_fires_on_tap_for_a_down_up_on_the_ink_well() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0)).on_tap(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
        tight(60.0, 40.0),
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on an enabled InkWell must fire on_tap exactly once",
    );
}

#[test]
fn on_tap_handler_observes_pressed_already_set() {
    use std::sync::atomic::AtomicBool;

    // Oracle order (`ink_well.dart` `activateOnIntent`/`_startNewSplash`,
    // `:864-900`): `WidgetState.pressed` is set true BEFORE `onTap` fires,
    // not after. Run this test's mutation yourself to see it matter: swap
    // the handler-then-pressed order back in `ink_well.rs`'s `on_tap`
    // closure and this assertion flips to `false`.
    let states = WidgetStatesController::default();
    let observed_pressed = Arc::new(AtomicBool::new(false));
    let states_for_handler = states.clone();
    let observed_for_handler = Arc::clone(&observed_pressed);
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .states_controller(states.clone())
            .on_tap(move || {
                observed_for_handler.store(
                    states_for_handler
                        .value()
                        .contains_state(WidgetState::Pressed),
                    Ordering::SeqCst,
                );
            }),
        tight(60.0, 40.0),
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    assert!(
        observed_pressed.load(Ordering::SeqCst),
        "on_tap's handler must observe WidgetState::Pressed already set when it runs"
    );
}

#[test]
fn disabled_ink_well_does_not_fire_a_tap_callback() {
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

#[test]
fn pressed_state_clears_after_the_activation_delay_under_an_ambient_vsync() {
    let states = WidgetStatesController::default();
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            InkWell::new(SizedBox::new(60.0, 40.0))
                .on_tap(|| {})
                .states_controller(states.clone()),
        ),
        tight(60.0, 40.0),
        vsync,
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    assert!(
        states.value().contains_state(WidgetState::Pressed),
        "Pressed must be set the instant on_tap fires",
    );

    // The vsync registry anchors a controller's run at its FIRST tick after
    // `forward_from`, not at the call itself (`flui-binding`'s
    // `Vsync`/`AnimationController` restart-aware registry) — so a tiny
    // first `pump_for` both anchors the run and proves it hasn't completed
    // in essentially zero elapsed time.
    laid.pump_for(Duration::from_millis(1));
    assert!(
        states.value().contains_state(WidgetState::Pressed),
        "Pressed must still be set a moment after the timer starts",
    );

    // Comfortably past the 100ms activation delay (measured from the
    // anchor set above): the press-deactivation controller has completed.
    laid.pump_for(Duration::from_millis(150));
    assert!(
        !states.value().contains_state(WidgetState::Pressed),
        "Pressed must clear once the ~100ms activation delay has elapsed",
    );
}

#[test]
fn pressed_state_clears_immediately_without_an_ambient_vsync() {
    // No VsyncScope above this InkWell: there is no clock to time the delay
    // against, so the documented degradation applies — Pressed is set and
    // immediately cleared, with no observable window. This test proves the
    // END state (not pressed) is reached synchronously, with no dangling
    // "still pressed forever" bug from a timer that never fires without a
    // driving vsync.
    let states = WidgetStatesController::default();
    let laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(states.clone()),
        tight(60.0, 40.0),
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    assert!(
        !states.value().contains_state(WidgetState::Pressed),
        "without an ambient VsyncScope, Pressed must already be cleared \
         synchronously after on_tap fires",
    );
}

#[test]
fn overlay_color_resolution_reflects_the_hovered_state() {
    // End-to-end: WidgetStateProperty::resolve is actually consulted from a
    // real mount+dispatch, not bypassed. Uses the Material fill this
    // InkWell wraps around its child once an overlay resolves to `Some`.
    use flui_widgets::{WidgetStateConstraint, WidgetStateProperty};

    let mut laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .overlay_color(WidgetStateProperty::from_map([(
                WidgetStateConstraint::Is(WidgetState::Hovered),
                Some(Color::rgb(200, 10, 10)),
            )])),
        tight(60.0, 40.0),
    );

    // Before any hover: no `RenderPhysicalShape` (the render object
    // `Material` wraps) should be mounted under the InkWell.
    assert!(
        laid.find_by_render_type("RenderPhysicalShape").is_none(),
        "no overlay layer should be mounted before any state resolves to Some",
    );

    laid.dispatch_pointer_move(10.0, 10.0);
    laid.pump();

    assert!(
        laid.find_by_render_type("RenderPhysicalShape").is_some(),
        "hovering must resolve overlay_color to Some and mount the Material overlay",
    );
}

#[test]
fn rebuilding_with_a_different_states_controller_re_homes_hover_tracking() {
    // Flutter parity: `didUpdateWidget` re-homes the states controller when
    // `widget.statesController != oldWidget.statesController`
    // (`ink_well.dart` `:938-940`). Run the mutation yourself: drop the
    // `did_update_view` override in `ink_well.rs` (or make it a no-op) and
    // `controller_b` never sees the hover below — `create_state` only
    // captures the controller once.
    let controller_a = WidgetStatesController::default();
    let controller_b = WidgetStatesController::default();

    let mut laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(controller_a.clone()),
        tight(60.0, 40.0),
    );

    laid.pump_widget(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(controller_b.clone()),
    );

    laid.dispatch_pointer_move(10.0, 10.0);

    assert!(
        controller_b.value().contains_state(WidgetState::Hovered),
        "after a rebuild swaps in controller_b, hover must drive controller_b"
    );
    assert!(
        !controller_a.value().contains_state(WidgetState::Hovered),
        "controller_a must no longer be driven by this InkWell once it has been swapped out"
    );
}

#[test]
fn rebuilding_with_the_same_cloned_controller_keeps_driving_it() {
    // The complementary case: re-cloning the SAME controller on rebuild
    // (the common case — most callers hold one controller and pass
    // `.clone()` on every build) must NOT be treated as a swap, and must
    // NOT drop the listener that drives rebuilds.
    let controller = WidgetStatesController::default();

    let mut laid = lay_out(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(controller.clone()),
        tight(60.0, 40.0),
    );

    laid.pump_widget(
        InkWell::new(SizedBox::new(60.0, 40.0))
            .on_tap(|| {})
            .states_controller(controller.clone()),
    );

    laid.dispatch_pointer_move(10.0, 10.0);

    assert!(
        controller.value().contains_state(WidgetState::Hovered),
        "re-cloning the same controller across a rebuild must not break hover tracking"
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
// `build`, matching Flutter's own "no setState during build" contract —
// `ink_well.rs`'s `init_state`/`did_update_view`) is applied and is correct
// on the oracle's own terms independent of whether this harness can prove
// the "spurious rebuild" symptom specifically; the existing `Disabled`-state
// assertions (`disabled_ink_well_does_not_update_hovered_state`,
// `disabled_ink_well_does_not_fire_a_tap_callback`) already prove the sync
// itself still happens correctly at the new call sites.

#[test]
fn focused_state_tracks_the_exact_external_focus_node() {
    let states = WidgetStatesController::default();
    let focus_node = FocusNode::with_debug_label("ink-well");
    let mut laid = lay_out(
        InkWell::new(SizedBox::new(80.0, 40.0))
            .on_tap(|| {})
            .states_controller(states.clone())
            .focus_node(Rc::clone(&focus_node)),
        tight(80.0, 40.0),
    );

    focus_node.request_focus();
    laid.tick();
    assert!(states.value().contains_state(WidgetState::Focused));

    focus_node.unfocus();
    laid.tick();
    assert!(!states.value().contains_state(WidgetState::Focused));
}
