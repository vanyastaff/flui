//! Scroll-path tests:
//!
//! 1. `SingleChildScrollView` viewport geometry (cross-protocol Box→Sliver path).
//! 2. `ScrollController` thumb geometry helpers.
//! 3. `Scrollable` interactive drag integration (gesture → offset change).
//! 4. `ClampingScrollPhysics` hard-boundary enforcement.
//! 5. `ScrollController::animate_to` (ADR-0037) — curve-driven animation,
//!    grab-to-cancel, and jump_to-cancels-in-flight.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out, tight};
use flui_animation::{Curves, Vsync};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::view::ScrollDirection;
use flui_view::{IntoView, ViewExt};
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, ScrollController, Scrollable,
    SharedScrollPhysics, SizedBox, VsyncScope,
};

// ============================================================================
// Viewport — Position/Fixed mode switching
// ============================================================================

// ============================================================================
// ListView / GridView — `.position()` passthrough
// ============================================================================

// ============================================================================
// ScrollController — thumb geometry helpers
// ============================================================================

// ============================================================================
// ScrollPhysics — clamping boundary enforcement
// ============================================================================

// ============================================================================
// Scrollable — drag gesture integration
// ============================================================================

/// A drag upward (finger moves toward smaller y-values, delta.dy < 0) must
/// increase the scroll offset because upward drag reveals content below the
/// current viewport position. This test FAILS if the pan callback is not
/// wired: `controller.pixels()` stays 0.0 when no gesture fires.
pub(crate) fn scrollable_drag_up_increases_scroll_offset() {
    let controller = ScrollController::new();
    // 300px viewport, 800px content → 500px scroll extent.
    controller.update_dimensions(300.0, 0.0, 500.0);

    let physics: SharedScrollPhysics = Arc::new(ClampingScrollPhysics::default());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Starting position: top of content.
    assert_eq!(controller.pixels(), 0.0, "initial scroll offset must be 0");

    // With no competing recognizer, the arena awards the drag after Down.
    // The first 50px upward move is therefore delivered in full.
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);

    assert_eq!(
        controller.pixels(),
        50.0,
        "an upward 50px finger move must increase the scroll offset by exactly 50px"
    );
}

// ============================================================================
// Scrollable — fling ballistic simulation integration
// ============================================================================

/// Wrap `widget` in a [`VsyncScope`] so its `ScrollableState::init_state` can
/// register the fling controller, then lay it out under `constraints` with a
/// gesture arena. Adopts the same vsync in the tree binding so
/// [`LaidOut::pump_for`] ticks the fling animation deterministically.
fn fling_scoped(widget: Scrollable, vsync: Vsync, constraints: BoxConstraints) -> LaidOut {
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, constraints);
    scoped.adopt_vsync(vsync);
    scoped
}

/// After a pan gesture ends with sufficient velocity the scroll offset must
/// continue to advance beyond the release position when the binding pumps
/// animation frames — confirming that the fling animation controller is wired
/// to the scroll controller and the vsync is driving it.
pub(crate) fn scrollable_fling_advances_offset_past_release() {
    let controller = ScrollController::new();
    // Large extent prevents the fling from hitting the boundary on the first
    // frame — we want to observe forward motion, not clamping.
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Upward drag well past the 18 px slop to establish a recognizable fling
    // velocity. The first move crosses slop (on_pan_start). The second fires
    // on_pan_update, advancing the offset. The pointer_up triggers on_pan_end
    // which calls animate_with on the fling controller.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 150.0); // 30 px more: on_pan_update
    scoped.dispatch_pointer_up(150.0, 150.0);

    let pixels_at_release = controller.pixels();
    assert!(
        pixels_at_release > 0.0,
        "pan drag must advance the offset before release; got {pixels_at_release}"
    );

    // First pump: vsync detects the new run generation from `animate_with` and
    // anchors the run start at t=0. The controller ticks at elapsed=0, which
    // gives x(0) = start (the release position). No net advance yet.
    scoped.pump_for(Duration::from_millis(16));
    // Second pump: advances to t=16 ms. The ballistic simulation gives
    // x(0.016) > start (friction deceleration carries the position forward).
    scoped.pump_for(Duration::from_millis(16));

    assert!(
        controller.pixels() > pixels_at_release,
        "scroll offset must continue past the release position after two fling frames; \
         release={pixels_at_release:.1}, now={:.1}",
        controller.pixels()
    );
}

/// Bouncing physics allows the drag to carry the scroll position past
/// `max_scroll_extent` with spring damping. On release, a
/// `ScrollSpringSimulation` springs the position back to the boundary. After
/// enough frames the position must be within 1 px of `max_scroll_extent`.
pub(crate) fn bouncing_physics_fling_springs_back_after_overscroll() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f64;
    controller.update_dimensions(300.0, 0.0, max_extent);

    let physics: SharedScrollPhysics = Arc::new(BouncingScrollPhysics::new());
    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Pre-position the scroll just below max_extent so a moderate in-bounds
    // upward drag pushes it past the boundary under bouncing physics.
    controller.set_pixels(480.0);

    // Upward drag past slop, then a further in-bounds move that applies
    // `apply_boundary_conditions` and lets pixels exceed max_extent (damped
    // by the overscroll spring coefficient 0.52):
    //   proposed = 480 − (−60) = 540 → clamped = 500 + 40×0.52 = 520.8
    // on_pan_end sees pixels = 520.8 > max_extent and returns a
    // ScrollSpringSimulation that springs the position back to max_extent.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 120.0); // 60 px more upward: on_pan_update
    scoped.dispatch_pointer_up(150.0, 120.0);

    // Pump 100 frames (1.6 s) — sufficient for the critically-damped spring
    // (SpringDescription with damping_ratio ≥ 0.75) to settle.
    for _ in 0..100 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let final_pixels = controller.pixels();
    assert!(
        final_pixels <= max_extent + 1.0,
        "bouncing spring-back must return scroll to within 1 px of max_extent ({max_extent}); \
         got {final_pixels:.3}"
    );
}

// ============================================================================
// Scrollable — animate_to (ADR-0037)
// ============================================================================

/// `jump_to` called while an `animate_to` is in flight must cancel it
/// SYNCHRONOUSLY — a subsequent frame must not resume driving toward the
/// original target, and must not even transiently show a stale fling-tick
/// value before the cancellation "catches up" (see `ScrollController`'s
/// `stop_hook` field docs for the one-frame race a merely QUEUED
/// cancellation would otherwise leave open, since `flui-testing::pump_frame`
/// ticks registered controllers before draining the rebuild queue that
/// services a queued command).
///
/// `jump_to` first cancels whatever activity currently owns the position
/// (goes idle) before touching the pixel offset.
pub(crate) fn scrollable_jump_to_during_animate_to_cancels_it_synchronously() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    controller.animate_to(1000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    // Three pumps of warm-up, one more than a direct `animate_with` fling:
    // `animate_to` queues a command, and pump 1's rebuild services it after
    // pump 1's own controller tick; pump 2 anchors the new run at `t = 0`;
    // pump 3 is the first tick that advances the value.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        controller.pixels() > 0.0 && controller.pixels() < 1000.0,
        "sanity: the animation must be in flight before jump_to"
    );

    controller.jump_to(42.0);
    assert_eq!(
        controller.pixels(),
        42.0,
        "jump_to must move to its own target immediately"
    );

    // The very next frame tick must NOT resume the old animation — if the
    // cancellation were only queued (not synchronous), this frame's tick step
    // (which runs BEFORE the rebuild that would service the queued Cancel)
    // would advance the still-running fling controller once more, stomping
    // the 42.0 this assertion checks.
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(
        controller.pixels(),
        42.0,
        "the frame immediately after jump_to must not have resumed the \
         canceled animate_to even transiently; got {:.2}",
        controller.pixels()
    );

    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.pixels(),
        42.0,
        "a jump_to mid-animate_to must cancel the run for good — later frames \
         must not resume driving toward the original 1000.0 target; got {:.2}",
        controller.pixels()
    );
}

// ============================================================================
// Scrollbar — thumb drag
// ============================================================================

// ============================================================================
// RefreshIndicator — pull-to-refresh
// ============================================================================

// ============================================================================
// Hit-test transform-stack composition through the sliver hit-test walk
// ============================================================================
//
// `PipelineOwner::hit_test_sliver_subtree` computes each child's position and
// recurses into it, but (before the fix this section pins down) never pushed
// the paint offset it consumed onto the `HitTestResult` transform stack --
// unlike the box-side walk, which does for `RenderBox` ancestors
// (`crates/flui-rendering/src/pipeline/owner/accessors.rs`). A `Listener`
// living anywhere below a sliver therefore received the raw, un-localized
// GLOBAL dispatch position instead of one local to its own box -- wrong for
// any sliver whose child paints at a nonzero offset, the common case for a
// scrolled list.
//
// The invariant is that a tap's `localPosition` stays scroll-correct through
// nested slivers. FLUI has no `SliverMainAxisGroup` yet, so these regression
// tests reproduce the class of defect through the sliver widgets FLUI
// does have: `ListView` (the sliver->box leg, under a real scroll offset,
// vertical and horizontal), `SliverPadding` wrapping a `SliverToBoxAdapter`
// (the sliver->sliver leg), and a reversed `AxisDirection` (the sliver->box
// leg's sign, not just its axis).

// ============================================================================
// Scrollable — scroll-activity signal
// ============================================================================

/// The scroll-activity signal across a full gesture: idle before, live
/// from the grab with the user's direction recorded, live through the
/// ballistic run past the release, and idle again — direction reset — once
/// the run settles. The signal a floating header's snap trigger keys on.
pub(crate) fn scroll_activity_tracks_the_whole_gesture_lifecycle() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let position = controller.position();

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller)
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    assert!(!position.is_scrolling(), "idle before any gesture");
    assert_eq!(position.user_scroll_direction(), ScrollDirection::Idle);

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: on_pan_start
    assert!(
        position.is_scrolling(),
        "the grab begins the scroll activity"
    );
    scoped.dispatch_pointer_move(150.0, 150.0); // on_pan_update
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Reverse,
        "an upward finger drag increases the offset — Reverse"
    );
    scoped.dispatch_pointer_up(150.0, 150.0);

    // Whether the release produced a ballistic run (fast release) or not,
    // the signal must return to idle once everything settles; the bounded
    // pump keeps a stuck-true regression a loud failure, not a hang.
    let mut frames = 0;
    while position.is_scrolling() && frames < 2_000 {
        scoped.pump_for(Duration::from_millis(16));
        frames += 1;
    }
    assert!(
        !position.is_scrolling(),
        "the activity must end after the release settles (still scrolling \
         after {frames} frames)"
    );
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Idle,
        "ending the scroll resets the direction"
    );
}

// ============================================================================
// Scrollable — arbitrated pointer-signal (wheel) routing
// ============================================================================

/// Two vertically nested scrollables under one wheel tick: the outer's
/// 300×300 viewport holds a 300×200 inner scrollable at the top of its
/// content, and the tick lands over the inner.
///
/// Layout used by the three arbitration tests below:
/// outer content = Column[SizedBox(300×200){inner Scrollable}, 300×4800
/// filler], inner content = 300×1000, so the inner can travel 0..=800 and
/// the outer 0..=4700.
fn nested_scrollables(outer: &ScrollController, inner: &ScrollController, vsync: Vsync) -> LaidOut {
    outer.update_dimensions(300.0, 0.0, 4700.0);
    inner.update_dimensions(200.0, 0.0, 800.0);

    let inner_scrollable = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(300.0, 1000.0));
    let outer_scrollable =
        Scrollable::new()
            .controller(outer.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(300.0, 200.0)
                    .child(inner_scrollable)
                    .into_view()
                    .boxed(),
                SizedBox::new(300.0, 4800.0).into_view().boxed(),
            ]));

    let wrapped = VsyncScope::new(vsync.clone(), outer_scrollable);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);
    scoped
}

/// One wheel tick over nested scrollables moves ONLY the innermost one that
/// can move — the pointer-signal arbitration contract: every scrollable
/// on the hit path registers interest, the first (leaf-most) registrant wins,
/// and the rest never act. Without arbitration
/// the same tick advances BOTH controllers (issue #717's double-scroll).
pub(crate) fn a_wheel_tick_over_nested_scrollables_moves_only_the_inner() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());

    // Over the inner scrollable (its region is 0..200 in root coordinates).
    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);

    assert_eq!(
        inner.pixels(),
        53.0,
        "the inner scrollable claims the tick and scrolls"
    );
    assert_eq!(
        outer.pixels(),
        0.0,
        "the outer scrollable must NOT also scroll — the inner claimed the tick"
    );
}
