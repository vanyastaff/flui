//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/scrollable_test.dart`
//! (tag `3.44.0`).
//!
//! The large majority of this ~1800-line file is out of scope for this
//! headless, geometry-only harness:
//! - Platform-specific momentum-carry heuristics (Android "no momentum
//!   build", iOS/macOS drag-threshold attenuation and momentum carry/kill) —
//!   FLUI's `Scrollable` has one fixed 18px drag-slop model, no
//!   platform-conditional physics variant selection.
//! - Mouse pointer-signal scrolling (`PointerScrollEvent`), keyboard
//!   scrolling, trackpad axis handling — no pointer-signal/keyboard input
//!   path exists on `Scrollable` yet.
//! - Semantics (`hasImplicitScrolling`, two-pane semantics nodes),
//!   `PageView.ensureVisible`, deferred-loading heuristics — paint/semantics
//!   are Phase 3 (deferred), per this crate's `parity/main.rs` module doc.
//! - `ScrollBehavior.dragDevices` — no `ScrollBehavior` type exists.
//!
//! What **is** already covered, in `tests/scroll.rs`: `hitTestBehavior`
//! (`scrollable_drag_up_increases_scroll_offset` and siblings use
//! `HitTestBehavior::Opaque` throughout), drag-slop
//! (`scrollable_sub_slop_drag_does_not_move_scroll_offset`), clamping AND
//! bouncing physics driven by a real gesture + `Vsync` fling at the MAX
//! (bottom) boundary (`clamping_physics_fling_stays_within_max_extent`,
//! `bouncing_physics_fling_springs_back_after_overscroll`), grabbing during
//! an active fling (`pan_start_during_fling_halts_momentum`), and
//! `viewportBuilder` composition (`scrollable_viewport_builder_composes_a_custom_viewport_with_working_drag_and_feedback`,
//! ported from `'Swapping viewports in a scrollable does not crash'`'s
//! non-semantics half).
//!
//! This file closes the one clear geometry-level gap left in that coverage:
//! every existing clamp/bounce integration test above drags toward the
//! bottom and asserts at `max_scroll_extent`; none exercises the symmetric
//! MIN (top) boundary through a real gesture + vsync. No single upstream test
//! isolates the top-boundary case either (Flutter's own boundary-condition
//! coverage lives in unit-level `scroll_physics_test.dart`, not here) — the
//! oracle is `ClampingScrollPhysics`/`BouncingScrollPhysics::apply_boundary_conditions`
//! (`crates/flui-widgets/src/scroll/scroll_physics.rs`), already unit-pinned
//! at the function level by `bouncing_apply_boundary_allows_overscroll_past_min_with_resistance`
//! and `bouncing_ballistic_springs_back_when_overscrolled_past_min` in that
//! same file — these two cases are the missing gesture-driven integration
//! half, symmetric to `tests/scroll.rs`'s existing MAX-boundary pair.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_rendering::constraints::BoxConstraints;
use flui_types::typography::TextDirection;
use flui_widgets::prelude::Axis;
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, Directionality, ScrollController, Scrollable,
    SharedScrollPhysics, SizedBox, VsyncScope,
};

use crate::common::{LaidOut, lay_out, tight};

/// Wrap `widget` in a [`VsyncScope`] so its `ScrollableState::init_state` can
/// register the fling controller, then lay it out under `constraints` with a
/// gesture arena — the same helper `tests/scroll.rs` uses, duplicated here
/// per that file's own precedent of one small helper per test binary rather
/// than a shared cross-binary dependency for a handful of call sites.
fn fling_scoped(widget: Scrollable, vsync: Vsync, constraints: BoxConstraints) -> LaidOut {
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, constraints);
    scoped.adopt_vsync(vsync);
    scoped
}

/// A drag at the top edge (offset = min_scroll_extent = 0) must not scroll
/// past it: clamping physics holds the position at the minimum. Symmetric to
/// `tests/scroll.rs`'s `scrollable_drag_up_at_max_extent_is_clamped_by_physics`.
#[test]
fn scrollable_drag_down_at_min_extent_is_clamped_by_physics() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);
    // Pre-scroll away from the top so a passing run must OBSERVE the gesture
    // moving pixels before the clamp engages — an expected value equal to the
    // initial state could not distinguish "clamped" from "gesture never ran".
    controller.set_pixels(20.0);

    let physics: SharedScrollPhysics = Arc::new(ClampingScrollPhysics::default());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Downward drag: first move crosses slop (fires on_pan_start), second
    // fires on_pan_update — proposes 20 − 60 = −40 (past the 0 minimum) ->
    // clamping physics holds at exactly 0, having demonstrably moved from 20.
    scoped.dispatch_pointer_down(150.0, 100.0);
    scoped.dispatch_pointer_move(150.0, 160.0); // 60 px downward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 220.0); // 60 px more: fires on_update
    scoped.dispatch_pointer_up(150.0, 220.0);

    assert_eq!(
        controller.pixels(),
        0.0,
        "clamping physics must hold the offset at the minimum (0) when a downward \
         drag from 20 proposes a negative offset; got {:.1}",
        controller.pixels()
    );
}

/// Bouncing physics allows a downward drag at the top to carry the scroll
/// position below `min_scroll_extent` with spring damping. On release, a
/// `ScrollSpringSimulation` springs the position back to the minimum.
/// Symmetric to `tests/scroll.rs`'s `bouncing_physics_fling_springs_back_after_overscroll`.
#[test]
fn bouncing_physics_top_overscroll_springs_back_to_min_extent() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f32;
    controller.update_dimensions(300.0, 0.0, max_extent);
    // Pre-position just below the top so a moderate downward drag pushes the
    // proposed offset past the minimum.
    controller.set_pixels(20.0);

    let physics: SharedScrollPhysics = Arc::new(BouncingScrollPhysics::new());
    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Downward drag past slop, then a further in-bounds move that applies
    // `apply_boundary_conditions` and lets pixels go negative (damped by the
    // overscroll spring coefficient 0.52):
    //   proposed = 20 − 60 = −40 → clamped = 0 + (−40) × 0.52 = −20.8
    // on_pan_end sees pixels = −20.8 < min_extent and returns a
    // ScrollSpringSimulation that springs the position back to 0.
    scoped.dispatch_pointer_down(150.0, 100.0);
    scoped.dispatch_pointer_move(150.0, 170.0); // 70 px downward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 230.0); // 60 px more: fires on_update
    scoped.dispatch_pointer_up(150.0, 230.0);

    // The overscroll must be OBSERVED before the spring settles — otherwise a
    // dead gesture path (pixels stuck at the 20.0 seed) would pass the settle
    // assertion below.
    let overscrolled = controller.pixels();
    assert!(
        overscrolled < 0.0,
        "the damped drag must carry pixels below the minimum before release \
         (expected ≈ −20.8); got {overscrolled:.3}"
    );

    // Pump 100 frames (1.6 s) — sufficient for the critically-damped spring
    // (SpringDescription with damping_ratio ≥ 0.75) to settle.
    for _ in 0..100 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let final_pixels = controller.pixels();
    assert!(
        (-1.0..=1.0).contains(&final_pixels),
        "bouncing spring-back must return scroll to within 1 px of the minimum (0); \
         got {final_pixels:.3}"
    );
}

// ============================================================================
// Gesture orientation under RTL Directionality
// ============================================================================
//
// A horizontal scroll view's *layout* now resolves its `AxisDirection` from
// ambient `Directionality` (RTL -> `RightToLeft`), but `Scrollable`'s gesture
// code oriented drags/flings by the bare `Axis` alone, unconditionally
// negating as if the axis were always non-reversed (`LeftToRight`/
// `TopToBottom`). Under RTL that leaves content moving AGAINST the finger.
//
// Oracle: `axisDirectionIsReversed` (`painting/basic_types.dart`) — `up` and
// `left` are reversed, `down` and `right` are not — feeding
// `ScrollDragController.update`'s `if (_reversed) { offset = -offset; }` and
// `.end`'s `double velocity = -details.primaryVelocity!; if (_reversed) {
// velocity = -velocity; }` (`widgets/scroll_activity.dart`), both ahead of
// `ScrollPosition.applyUserOffset`'s `pixels - delta`. FLUI's
// `AxisDirection::is_reversed` (`crates/flui-types/src/layout/axis.rs`)
// already implements the identical `RightToLeft | BottomToTop => true`
// rule, so `Scrollable` only needed to consult it.

/// The drag-delta half of the defect (`on_pan_update`). Builds a horizontal
/// `Scrollable` with (LTR, the default) and without (RTL) an ambient
/// `Directionality`, drags the SAME physical distance in both, and asserts
/// the offset moves in OPPOSITE directions.
///
/// LTR resolves `LeftToRight` (not reversed): `pixels -= raw_delta`, so a
/// 50px LEFTWARD drag (`raw_delta = -50`) increases the offset by 50 — the
/// same "drag toward the axis start increases the offset" convention
/// `scrollable_drag_up_increases_scroll_offset` (`tests/scroll.rs`) pins for
/// the vertical (`TopToBottom`, also not reversed) case.
///
/// RTL resolves `RightToLeft` (reversed): `pixels += raw_delta`, so the
/// SAME 50px leftward drag DECREASES the offset by 50 instead — the mirror
/// image, not a smaller/larger version of the LTR case.
#[test]
fn horizontal_drag_under_rtl_directionality_moves_the_offset_opposite_of_ltr() {
    fn dragged_pixels(directionality: Option<TextDirection>) -> f32 {
        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 500.0);
        // Midpoint start: a ±50px move from here never clips against either
        // boundary, so a difference between the LTR and RTL runs can only
        // come from the sign the gesture code applies, not from clamping.
        controller.set_pixels(250.0);

        let scrollable = Scrollable::new()
            .controller(controller.clone())
            .scroll_direction(Axis::Horizontal)
            .child(SizedBox::new(800.0, 300.0));

        let scoped = match directionality {
            Some(direction) => lay_out(
                Directionality::new(direction, scrollable),
                tight(300.0, 300.0),
            ),
            None => lay_out(scrollable, tight(300.0, 300.0)),
        };

        // A single 50px leftward drag: with no competing recognizer the
        // arena awards the drag after Down, so this first move is delivered
        // in full — same pattern as `scrollable_drag_up_increases_scroll_offset`
        // (`tests/scroll.rs`).
        scoped.dispatch_pointer_down(200.0, 150.0);
        scoped.dispatch_pointer_move(150.0, 150.0);
        scoped.dispatch_pointer_up(150.0, 150.0);

        controller.pixels()
    }

    let ltr_pixels = dragged_pixels(None);
    assert_eq!(
        ltr_pixels, 300.0,
        "LTR control: a 50px leftward drag on a non-reversed (LeftToRight) horizontal \
         Scrollable must increase the offset by 50 (250 -> 300); got {ltr_pixels:.1}"
    );

    let rtl_pixels = dragged_pixels(Some(TextDirection::Rtl));
    assert_eq!(
        rtl_pixels, 200.0,
        "under RTL Directionality the SAME physical leftward drag must move the offset \
         the OPPOSITE way (250 -> 200), not the LTR-control direction; got {rtl_pixels:.1}"
    );
}

/// The independent fling/velocity half of the same defect (`on_pan_end`) —
/// the sign bug in `on_pan_update` and the sign bug in `on_pan_end` are two
/// separate `match scroll_direction` arms in `Scrollable::build`
/// (`crates/flui-widgets/src/scroll/scrollable.rs`), so a fix that only
/// covers the drag-delta test above would leave this half uncovered.
///
/// A fast horizontal drag released mid-gesture must keep advancing the
/// offset in the SAME direction the drag itself established: LTR continues
/// increasing past the release point (mirroring
/// `scrollable_fling_advances_offset_past_release` in `tests/scroll.rs`);
/// RTL continues DECREASING past its (already-decreasing) release point —
/// the mirror image, not a fling that reverses direction on release.
#[test]
fn horizontal_fling_under_rtl_directionality_continues_the_opposite_way_of_ltr() {
    fn fling_pixels(directionality: Option<TextDirection>) -> (f32, f32, f32) {
        let controller = ScrollController::new();
        // Large extent keeps the fling well clear of either boundary so
        // clamping physics never masks a wrong-signed velocity.
        controller.update_dimensions(300.0, 0.0, 4700.0);
        let start_pixels = 2000.0;
        controller.set_pixels(start_pixels);

        let scrollable = Scrollable::new()
            .controller(controller.clone())
            .scroll_direction(Axis::Horizontal)
            .child(SizedBox::new(5000.0, 300.0));

        let vsync = Vsync::new();
        let wrapped = VsyncScope::new(vsync.clone(), scrollable);
        let mut scoped = match directionality {
            Some(direction) => {
                lay_out(Directionality::new(direction, wrapped), tight(300.0, 300.0))
            }
            None => lay_out(wrapped, tight(300.0, 300.0)),
        };
        scoped.adopt_vsync(vsync);

        // Leftward drag well past the 18px slop, mirroring
        // `scrollable_fling_advances_offset_past_release`'s magnitude: first
        // move crosses slop (on_pan_start), second fires on_pan_update.
        scoped.dispatch_pointer_down(250.0, 150.0);
        scoped.dispatch_pointer_move(150.0, 150.0); // 100px leftward: slop-crossing
        scoped.dispatch_pointer_move(100.0, 150.0); // 50px more: on_pan_update
        scoped.dispatch_pointer_up(100.0, 150.0);

        let pixels_at_release = controller.pixels();

        // Same two-pump settle window as `scrollable_fling_advances_offset_past_release`.
        scoped.pump_for(Duration::from_millis(16));
        scoped.pump_for(Duration::from_millis(16));

        (start_pixels, pixels_at_release, controller.pixels())
    }

    let (ltr_start, ltr_at_release, ltr_after_pump) = fling_pixels(None);
    assert!(
        ltr_at_release > ltr_start,
        "LTR control: a leftward drag on a non-reversed horizontal Scrollable must \
         increase the offset before release ({ltr_start} -> {ltr_at_release})"
    );
    assert!(
        ltr_after_pump > ltr_at_release,
        "LTR control: the fling must keep advancing (increasing) past the release point \
         after two animation frames; release={ltr_at_release:.1}, after_pump={ltr_after_pump:.1}"
    );

    let (rtl_start, rtl_at_release, rtl_after_pump) = fling_pixels(Some(TextDirection::Rtl));
    assert!(
        rtl_at_release < rtl_start,
        "under RTL Directionality the SAME leftward drag must DECREASE the offset before \
         release ({rtl_start} -> {rtl_at_release}), the opposite of the LTR control"
    );
    assert!(
        rtl_after_pump < rtl_at_release,
        "under RTL Directionality the fling must keep advancing (decreasing) past the \
         release point after two animation frames — the opposite of the LTR control's \
         continued increase; release={rtl_at_release:.1}, after_pump={rtl_after_pump:.1}"
    );
}
