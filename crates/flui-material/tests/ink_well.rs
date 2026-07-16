//! `InkWell` widget-level state-transition coverage.
//!
//! # Spike outcome
//!
//! `tests/common/mod.rs` here is `flui-widgets`' own `tests/common/mod.rs`
//! (ported, since `flui-material`'s copy had been trimmed to `Theme`'s
//! needs: no pointer dispatch, no vsync ticking) — the same infrastructure
//! `flui-widgets/tests/mouse_region.rs` and
//! `flui-widgets/tests/gesture_detector.rs` already use to prove
//! `dispatch_pointer_move`/`dispatch_pointer_down`/`dispatch_pointer_up`
//! route through a real hit-test + dispatch pass. Investigating the spike
//! surfaced one real finding, not a bug in `InkWell`:
//!
//! - **Drivable**: `MouseRegion::on_hover` (fires on every move while the
//!   pointer is inside the region — the mechanism `mouse_region.rs`'s own
//!   existing test suite already exercises), `GestureDetector::on_tap` (a
//!   standalone down+up), and the vsync-driven press-deactivation timer
//!   (`lay_out_animated`/`pump_for`, proven by `flui-widgets`' own
//!   implicitly-animated widget tests). `InkWell` is wired on `on_hover`,
//!   not `on_enter`, specifically because of the next point.
//! - **NOT drivable through this headless harness**: `MouseRegion::on_enter`/
//!   `on_exit`. Both require `MouseTracker::update_with_event` — an
//!   annotation-diffing pass (this hit-test's region set vs. the previous
//!   one) that only a full `AppBinding` frame pump runs today; the raw
//!   `HitTestResult::dispatch` this harness's `dispatch_pointer_move` calls
//!   never reaches it. Confirmed directly: a bare `MouseRegion` with only
//!   `.on_enter(..)` never fires it under this harness, while the same
//!   region with `.on_hover(..)` does, for an identical single
//!   `dispatch_pointer_move` call. `InkWell` still uses `on_exit` in its own
//!   composition (real, `AppBinding`-correct behavior — see `ink_well.rs`),
//!   but that half of hover tracking is **not** covered by a headless test
//!   here; a real click-to-exit / real-window regression test is future
//!   work once a headless `MouseTracker` pump exists.
//! - **NOT drivable through this headless harness, at all**: FOCUS, by any
//!   API path. Neither `FocusNode::request_focus()` nor
//!   `FocusManager::global().request_focus(id)` fires `on_focus_change` for
//!   a mounted `Focus` widget under this harness — confirmed directly with
//!   a minimal `Focus::new(child).focus_node(external).on_focus_change(..)`
//!   repro (no `InkWell` involved) for both APIs; `changed` stayed `false`
//!   either way. This is a gap in the headless harness/focus-manager wiring
//!   (nothing in `HeadlessBinding`'s pump appears to drive whatever
//!   `FocusManager`'s change-propagation needs), not a defect in `InkWell`
//!   or `Focus` — `ink_well.rs`'s `Focus::new(..).can_request_focus(enabled)
//!   .on_focus_change(..)` wiring is structurally identical to every other
//!   `on_focus_change` consumer in this codebase (e.g. `text_field.rs`).
//!   Also unrelated: keyboard/click-to-focus through a real interaction
//!   sequence has no dispatch helper in this harness either (no "Tab key"
//!   dispatch, and mouse clicks do not request focus in FLUI's current
//!   `NavigationMode` handling — matching the oracle, which also does not
//!   focus-on-click by default).
//!
//! Given the above, the matrix below covers: hover-on, tap, disabled (both),
//! press-state timing (both the vsync-driven delay and the standalone
//! immediate-clear fallback), and overlay-color resolution end to end.
//! **Not covered, both named gaps, not silently dropped**: hover-off
//! (`on_exit`) and ANY focus transition. `InkWell::focus_node` and the
//! `Focus` wiring inside `build()` remain part of the shipped surface —
//! only the test coverage is deferred, pending a headless `MouseTracker`/
//! focus-propagation pump.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, lay_out_animated, tight};
use flui_animation::Vsync;
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

// `focused_state_updates_when_an_external_focus_node_is_granted_focus` was
// removed after the spike showed focus is not drivable through this
// headless harness at all: even a BARE `Focus` widget (no InkWell involved)
// with an external `FocusNode` does not fire `on_focus_change` when
// `FocusNode::request_focus()` is called from outside the widget tree in
// this harness (confirmed directly: `changed` stayed `false` for a minimal
// `Focus::new(child).focus_node(external).on_focus_change(..)` repro). This
// is a gap in the headless test harness/focus-manager wiring, not a defect
// in `InkWell` or `Focus` — both compose correctly (see `ink_well.rs`'s
// `Focus::new(..).can_request_focus(enabled).on_focus_change(..)` wiring,
// which is structurally identical to every other `on_focus_change` consumer
// in this codebase, e.g. `text_field.rs`). Focus coverage for `InkWell` is
// therefore a named gap here, not silently dropped — see the module doc
// above.
