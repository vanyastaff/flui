//! Scroll-path tests:
//!
//! 1. `SingleChildScrollView` viewport geometry (cross-protocol Box→Sliver path).
//! 2. `ScrollController` thumb geometry helpers.
//! 3. `Scrollable` interactive drag integration (gesture → offset change).
//! 4. `ClampingScrollPhysics` hard-boundary enforcement.
//! 5. `ScrollController::animate_to` (ADR-0037) — curve-driven animation,
//!    grab-to-cancel, and jump_to-cancels-in-flight.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out, tight};
use flui_animation::{Curves, Vsync};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::view::ScrollDirection;
use flui_view::{IntoView, ViewExt};
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, RefreshController, RefreshIndicator,
    ScrollController, Scrollable, Scrollbar, SharedScrollPhysics, SizedBox, VsyncScope,
};

#[derive(Clone, flui_view::prelude::StatelessView)]
pub(crate) struct FlingProfile {
    pub(crate) provider: flui_interaction::settings::GestureSettingsProvider,
    pub(crate) child: flui_view::BoxedView,
}

impl flui_view::StatelessView for FlingProfile {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        flui_widgets::GestureArenaScope::new(
            flui_widgets::GestureArenaScope::of(ctx),
            self.child.clone(),
        )
        .settings(self.provider.clone())
    }
}

pub(crate) fn terminal_scroll_motion_uses_the_admitted_fling_profile() {
    terminal_motion_uses_the_admitted_fling_profile(false);
}

pub(crate) fn terminal_refresh_motion_uses_the_admitted_fling_profile() {
    terminal_motion_uses_the_admitted_fling_profile(true);
}

fn terminal_motion_uses_the_admitted_fling_profile(refresh: bool) {
    let profile = |min, max| {
        flui_interaction::GestureSettings::default()
            .try_with_fling_velocity(min, max)
            .expect("valid fling range")
    };
    let source = flui_interaction::settings::GestureSettingsSource::new(profile(50.0, 300.0));
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let child = if refresh {
        refresh_content(&controller, &RefreshController::new()).boxed()
    } else {
        Scrollable::new()
            .controller(controller.clone())
            .child(SizedBox::new(300.0, 5000.0))
            .boxed()
    };
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            FlingProfile {
                provider: source.provider(),
                child,
            },
        ),
        tight(300.0, 300.0),
        vsync,
    );
    controller.set_pixels(500.0);
    for attempt in 0..4 {
        laid.dispatch_pointer_down(150.0, 250.0);
        let step = if attempt == 3 { 200.0 } else { 20.0 };
        let end = 250.0 - 5.0 * step;
        for sample in 1..=5 {
            let y = 250.0 - f64::from(sample) * step;
            laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
        }
        if attempt == 0 {
            source.replace(profile(5000.0, 5000.0));
        }
        laid.dispatch_pointer_up(150.0, end);
        let released = controller.pixels();
        // Anchor the controller's first tick, then observe actual pixels.
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        let coast = controller.pixels() - released;
        if attempt == 1 {
            assert_eq!(
                coast, 0.0,
                "{refresh}: next contact below its admitted minimum does not coast"
            );
            source.replace(profile(50.0, 600.0));
        } else if attempt == 3 {
            assert!(
                coast > 200.0 && coast < 250.0,
                "{refresh}: admitted max15000 determines the coasting distance, coast {coast}"
            );
        } else {
            let bound = if attempt == 0 { 10.0 } else { 20.0 };
            assert!(
                coast > 0.0 && coast < bound,
                "{refresh}: attempt {attempt} uses its captured maximum, coast {coast}"
            );
            if attempt == 2 {
                source.replace(profile(50.0, 15000.0));
            }
        }
    }
}

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

pub(crate) fn a_remaining_touch_continues_scroll_without_an_intermediate_fling() {
    use flui_platform_api::{
        EventTime,
        pointer::{
            PointerButton, PointerButtons, PointerEvent, PointerInfo, PointerKind, PointerMove,
            PointerPosition, PointerPress, PointerRelease, PointerSample,
        },
    };
    use flui_testing::PointerPhase;

    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, Vsync::new(), tight(300.0, 300.0));
    let event = |id: u64, millis: u64, y: f64, phase| {
        let info = PointerInfo::new(
            flui_interaction::PointerId::try_from(id).expect("nonzero touch identity"),
            PointerKind::Touch,
        );
        let sample = PointerSample::new(
            EventTime::from_nanos(millis * 1_000_000),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(150.0, y))
                .expect("finite touch position"),
        );
        match phase {
            PointerPhase::Down => PointerEvent::Down(PointerPress::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE.with(PointerButton::PRIMARY),
                sample,
            )),
            PointerPhase::Move => PointerEvent::Move(PointerMove::new(
                info,
                PointerButtons::NONE.with(PointerButton::PRIMARY),
                sample,
            )),
            PointerPhase::Up => PointerEvent::Up(PointerRelease::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
            PointerPhase::Cancel => unreachable!("this row scripts touch release"),
        }
    };
    for (id, millis, y, phase) in [
        (2, 0, 250.0, PointerPhase::Down),
        (2, 10, 200.0, PointerPhase::Move),
        (2, 20, 150.0, PointerPhase::Move),
        (3, 25, 60.0, PointerPhase::Down),
        (3, 30, 70.0, PointerPhase::Move),
        (3, 40, 80.0, PointerPhase::Move),
    ] {
        scoped.dispatch_pointer_event(&event(id, millis, y, phase));
    }
    assert_eq!(
        controller.pixels(),
        100.0,
        "passive touch motion does not move the active drag"
    );
    scoped.dispatch_pointer_event(&event(2, 45, 150.0, PointerPhase::Up));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(
        controller.pixels(),
        100.0,
        "first touch release must not start a ballistic run while another touch remains"
    );

    scoped.dispatch_pointer_event(&event(3, 55, 90.0, PointerPhase::Move));
    assert_eq!(
        controller.pixels(),
        90.0,
        "handoff rebases to the successor: only its next 10px delta scrolls"
    );
    scoped.dispatch_pointer_event(&event(3, 65, 100.0, PointerPhase::Move));
    assert_eq!(controller.pixels(), 80.0);
    scoped.dispatch_pointer_event(&event(3, 70, 100.0, PointerPhase::Up));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        controller.pixels() < 80.0,
        "final fling uses the successor's downward measured history, not the first touch's upward velocity: {}",
        controller.pixels()
    );

    controller.jump_to(200.0);
    scoped.dispatch_pointer_event(&event(2, 200, 200.0, PointerPhase::Down));
    scoped.dispatch_pointer_event(&event(2, 210, 190.0, PointerPhase::Move));
    assert_eq!(
        controller.pixels(),
        210.0,
        "reused touch identity starts a fresh drag after completion"
    );
    scoped.dispatch_pointer_event(&event(2, 220, 190.0, PointerPhase::Up));
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
/// spring springs the position back to the boundary. After
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
    // spring simulation that springs the position back to max_extent.
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

pub(crate) fn scrollbar_thumb_stays_inside_short_tracks_and_drag_remains_bounded() {
    fn short_track() {
        assert_scrollbar_track(10.0);
    }
    fn minimum_track() {
        assert_scrollbar_track(18.0);
    }
    fn ordinary_track() {
        assert_scrollbar_track(100.0);
    }
    crate::common::cases::run_cases(
        "scrollbar track containment",
        &[
            ("short track", short_track as fn()),
            ("minimum track", minimum_track as fn()),
            ("ordinary track", ordinary_track as fn()),
        ],
    );
}

fn assert_scrollbar_track(height: f64) {
    let controller = ScrollController::new();
    controller.update_dimensions(height, 0.0, 900.0);
    let mut laid = lay_out(
        Scrollbar::new()
            .controller(controller.clone())
            .child(SizedBox::new(100.0, height)),
        tight(100.0, height),
    );
    // Scrollbar's ColoredBox paints through RenderDecoratedBox; the SizedBox
    // content has no decoration, so this identifies exactly the real thumb.
    let thumbs = laid.find_all_by_render_type("RenderDecoratedBox");
    assert_eq!(thumbs.len(), 1);
    let thumb = thumbs[0];
    assert!(
        laid.size(thumb).height <= height,
        "thumb exceeds {height}px track"
    );
    controller.jump_to(900.0);
    laid.tick();
    let bottom = laid.absolute_offset(thumb).dy + laid.size(thumb).height;
    assert!(
        (bottom - height).abs() < 1e-8,
        "thumb must end at track boundary"
    );

    controller.jump_to(0.0);
    laid.tick();
    laid.dispatch_pointer_down(97.0, 5.0);
    laid.dispatch_pointer_move(97.0, 25.0);
    laid.dispatch_pointer_move(97.0, 205.0);
    laid.dispatch_pointer_up(97.0, 205.0);
    if height <= 18.0 {
        assert_eq!(controller.pixels(), 0.0, "a full-track thumb has no travel");
    } else {
        assert_eq!(
            controller.pixels(),
            900.0,
            "thumb drag clamps to content end"
        );
    }
    controller.jump_to(0.0);
    laid.tick();
    assert_eq!(
        controller.pixels(),
        0.0,
        "next jump still works after dragging"
    );
}

pub(crate) fn dragging_a_scrollbar_thumb_interrupts_animation_before_the_next_tick() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Scrollbar::new().controller(scroll.clone()).child(
                Scrollable::new()
                    .controller(scroll.clone())
                    .child(SizedBox::new(300.0, 5000.0)),
            ),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    scroll.animate_to(1000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    advance_scroll_run(&mut laid);
    assert!(scroll.pixels() > 0.0 && scroll.pixels() < 1000.0);
    assert!(scroll.position().is_scrolling());
    let thumbs = laid.find_all_by_render_type("RenderDecoratedBox");
    assert_eq!(thumbs.len(), 1);
    let top = laid.absolute_offset(thumbs[0]).dy;
    let before = scroll.pixels();
    laid.dispatch_pointer_down(297.0, top + 5.0);
    laid.dispatch_pointer_move(297.0, top + 25.0);
    laid.dispatch_pointer_move(297.0, top + 45.0);
    let dragged = scroll.pixels();
    assert!(
        dragged > before,
        "pointer drag must advance the actual position"
    );
    laid.dispatch_pointer_up(297.0, top + 45.0);
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(
        scroll.pixels(),
        dragged,
        "old animation must not overwrite the drag"
    );
    for _ in 0..24 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        scroll.pixels(),
        dragged,
        "retired animation must stay stopped"
    );
    assert!(!scroll.position().is_scrolling());
    scroll.animate_to(4000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    advance_scroll_run(&mut laid);
    assert!(
        scroll.pixels() > dragged,
        "a later animation still makes progress"
    );
}

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
pub(crate) fn refresh_motion_notifies_activity_through_release_and_recovery() {
    let mut failures = Vec::new();
    for cancelled in [false, true] {
        let scroll = ScrollController::new();
        let refresh = RefreshController::new();
        let vsync = Vsync::new();
        let position = scroll.position();
        let observations = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&observations);
        let observed_position = position.clone();
        let listener = position.add_activity_listener(Arc::new(move || {
            observed.lock().expect("activity observer").push((
                observed_position.is_scrolling(),
                observed_position.user_scroll_direction(),
            ));
        }));
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(vsync.clone(), refresh_content(&scroll, &refresh)),
            tight(300.0, 300.0),
            vsync,
        );
        laid.dispatch_pointer_down(150.0, 250.0);
        for y in [230.0, 210.0, 190.0, 170.0, 150.0] {
            laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
        }
        let released = scroll.pixels();
        assert!(released > 0.0, "refresh wrapper actually scrolls");
        if !observations.lock().expect("activity observer")
            .contains(&(true, ScrollDirection::Reverse))
        {
            failures.push(format!("cancelled={cancelled}: no active-direction delivery during drag"));
        }
        if cancelled {
            laid.dispatch_pointer_cancel();
        } else {
            laid.dispatch_pointer_up(150.0, 150.0);
        }
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        if cancelled {
            assert_eq!(scroll.pixels(), released, "cancelled in-range drag does not coast");
        } else {
            assert!(scroll.pixels() > released, "completed refresh drag actually coasts");
            if !position.is_scrolling() {
                failures.push("ballistic pixels move while activity is idle".into());
            }
        }
        laid.pump_for(Duration::from_secs(10));
        assert!(!position.is_scrolling(), "terminal motion settles");
        assert_eq!(position.user_scroll_direction(), ScrollDirection::Idle);
        observations.lock().expect("activity observer").clear();
        let before = scroll.pixels();
        laid.dispatch_pointer_down(150.0, 250.0);
        laid.dispatch_pointer_move_after(150.0, 210.0, Duration::from_millis(10));
        laid.dispatch_pointer_move_after(150.0, 190.0, Duration::from_millis(10));
        assert!(scroll.pixels() > before, "next healthy contact drives content");
        if !observations.lock().expect("activity observer")
            .contains(&(true, ScrollDirection::Reverse))
        {
            failures.push(format!("cancelled={cancelled}: next contact has no activity delivery"));
        }
        laid.dispatch_pointer_cancel();
        position.remove_activity_listener(listener);
    }
    assert!(failures.is_empty(), "refresh activity delivery failed: {failures:?}");
}

pub(crate) fn a_failed_refresh_notification_releases_activity_and_recovers() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut failures = Vec::new();
    for (phase_fault, activity_fault) in [(true, false), (false, true), (true, true)] {
    let scroll = ScrollController::new();
    let refresh = RefreshController::new();
    let fail = Arc::new(AtomicBool::new(phase_fault));
    let failed = Arc::new(AtomicBool::new(false));
    let watched = refresh.clone();
    let fail_callback = fail.clone();
    let failed_callback = failed.clone();
    let listenable = refresh.as_listenable();
    let listener = listenable.add_listener(Arc::new(move || {
        if watched.is_refreshing() && fail_callback.swap(false, Ordering::SeqCst) {
            failed_callback.store(true, Ordering::SeqCst);
            panic!("refresh phase subscriber failed");
        }
    }));
    let activity_fail = Arc::new(AtomicBool::new(activity_fault));
    let activity_failed = Arc::new(AtomicBool::new(false));
    let activity_flag = activity_fail.clone();
    let activity_observed = activity_failed.clone();
    let phase = refresh.clone();
    let position = scroll.position();
    let watched_position = position.clone();
    let activity_listener = position.add_activity_listener(Arc::new(move || {
        if phase.is_refreshing() && !watched_position.is_scrolling()
            && activity_flag.swap(false, Ordering::SeqCst)
        {
            activity_observed.store(true, Ordering::SeqCst);
            panic!("refresh activity subscriber failed");
        }
    }));
    let calls = Rc::new(Cell::new(0));
    let calls_callback = calls.clone();
    let content = refresh_content(&scroll, &refresh).on_refresh(move |_| {
        calls_callback.set(calls_callback.get() + 1);
    });
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), content),
        tight(300.0, 300.0),
        vsync,
    );
    let pull = |laid: &LaidOut| {
        laid.dispatch_pointer_down(150.0, 100.0);
        for y in [130.0, 160.0, 190.0, 220.0, 250.0] {
            laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
        }
    };
    pull(&laid);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.dispatch_pointer_up(150.0, 250.0);
    }));
    if let Err(payload) = failure {
        assert_eq!(flui_foundation::panic::payload_text(payload.as_ref()), Some(if phase_fault {
            "refresh phase subscriber failed"
        } else {
            "refresh activity subscriber failed"
        }));
    }
    assert_eq!(failed.load(Ordering::SeqCst), phase_fault);
    let attempted_cleanup = activity_failed.load(Ordering::SeqCst);
    activity_fail.store(false, Ordering::SeqCst);
    assert!(refresh.is_refreshing(), "accepted refresh phase survives its observer");
    let stranded = scroll.position().is_scrolling();
    refresh.finish();
    laid.pump_for(Duration::from_millis(16));
    pull(&laid);
    laid.dispatch_pointer_up(150.0, 250.0);
    assert_eq!(calls.get(), 1, "next healthy refresh callback remains deliverable");
    refresh.finish();
    assert!(!scroll.position().is_scrolling());
    listenable.remove_listener(listener);
    position.remove_activity_listener(activity_listener);
    if attempted_cleanup != activity_fault {
        failures.push(format!("phase={phase_fault}, activity={activity_fault}: mandatory cleanup not attempted"));
    }
    if stranded {
        failures.push(format!("phase={phase_fault}, activity={activity_fault}: terminal activity stranded"));
    }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

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

fn refresh_content(scroll: &ScrollController, refresh: &RefreshController) -> RefreshIndicator {
    RefreshIndicator::new()
        .controller(refresh.clone())
        .scroll_controller(scroll.clone())
        .child(SizedBox::new(300.0, 5000.0))
}

pub(crate) fn incremental_pulls_refresh_once_and_finish_allows_the_next_gesture() {
    let scroll = ScrollController::new();
    scroll.update_dimensions(300.0, 0.0, 4700.0);
    let refresh = RefreshController::new();
    let calls = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&calls);
    let mut laid = lay_out(
        refresh_content(&scroll, &refresh).on_refresh(move |_| recorded.set(recorded.get() + 1)),
        tight(300.0, 300.0),
    );
    for expected_calls in [1, 2] {
        laid.dispatch_pointer_down(150.0, 40.0);
        for y in [60.0, 80.0, 100.0, 120.0, 140.0] {
            laid.dispatch_pointer_move(150.0, y);
            laid.pump();
        }
        assert_eq!(refresh.pull_distance_px(), 100.0);
        assert_eq!(scroll.pixels(), 0.0);
        laid.dispatch_pointer_up(150.0, 140.0);
        assert_eq!(calls.get(), expected_calls);
        assert!(refresh.is_refreshing());
        assert_eq!(refresh.pull_distance_px(), 0.0);
        laid.dispatch_pointer_down(150.0, 40.0);
        laid.dispatch_pointer_move(150.0, 140.0);
        laid.dispatch_pointer_up(150.0, 140.0);
        assert_eq!(
            calls.get(),
            expected_calls,
            "refreshing suppresses another operation"
        );
        assert_eq!(scroll.pixels(), 0.0);
        refresh.finish();
        laid.pump();
        assert!(!refresh.is_refreshing());
    }
}

pub(crate) fn reversing_a_pull_consumes_it_before_scrolling_content() {
    let scroll = ScrollController::new();
    scroll.update_dimensions(300.0, 0.0, 4700.0);
    let refresh = RefreshController::new();
    let laid = lay_out(refresh_content(&scroll, &refresh), tight(300.0, 300.0));
    laid.dispatch_pointer_down(150.0, 100.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    laid.dispatch_pointer_move(150.0, 160.0);
    assert_eq!(refresh.pull_distance_px(), 60.0);
    laid.dispatch_pointer_move(150.0, 130.0);
    assert_eq!(refresh.pull_distance_px(), 30.0);
    assert_eq!(scroll.pixels(), 0.0);
    laid.dispatch_pointer_move(150.0, 90.0);
    assert_eq!(refresh.pull_distance_px(), 0.0);
    assert_eq!(
        scroll.pixels(),
        10.0,
        "movement crosses the remaining pull boundary"
    );
    laid.dispatch_pointer_up(150.0, 90.0);
    assert!(!refresh.is_refreshing());
}

fn start_refresh_content_fling(laid: &LaidOut) {
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
}

pub(crate) fn a_fast_gesture_while_refreshing_does_not_start_a_fling() {
    let scroll = ScrollController::new();
    scroll.update_dimensions(300.0, 0.0, 4700.0);
    let refresh = RefreshController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), refresh_content(&scroll, &refresh)),
        tight(300.0, 300.0),
        vsync,
    );
    // One sufficient pull isolates this row from incremental accumulation.
    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    laid.dispatch_pointer_up(150.0, 140.0);
    laid.pump();
    assert!(refresh.is_refreshing());

    start_refresh_content_fling(&laid);
    assert_eq!(
        scroll.pixels(),
        0.0,
        "refreshing ignores direct drag updates"
    );
    // The first frame anchors a newly started simulation. Later frames would
    // reveal the erroneous run even though its initial value was still zero.
    for _ in 0..4 {
        laid.pump_for(Duration::from_millis(16));
        assert_eq!(
            scroll.pixels(),
            0.0,
            "refreshing must not coast after release"
        );
    }
    assert!(refresh.is_refreshing());
    refresh.finish();
    laid.pump();
    start_refresh_content_fling(&laid);
    let released = scroll.pixels();
    assert!(released > 0.0, "the next gesture scrolls after finish");
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        scroll.pixels() > released,
        "post-finish fling makes progress"
    );
}

pub(crate) fn a_refresh_controller_swap_retires_the_old_fling_and_drives_the_new_position() {
    let a = ScrollController::new();
    let b = ScrollController::new();
    for scroll in [&a, &b] {
        scroll.update_dimensions(300.0, 0.0, 4700.0);
    }
    let refresh = RefreshController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), refresh_content(&a, &refresh)),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    start_refresh_content_fling(&laid);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        a.pixels() > 100.0,
        "old controller has a live ballistic run"
    );
    let retired = a.pixels();
    laid.pump_widget(VsyncScope::new(vsync, refresh_content(&b, &refresh)));
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        a.pixels(),
        retired,
        "retired position receives no more ticks"
    );
    assert_eq!(
        b.pixels(),
        0.0,
        "old run cannot jump the replacement position"
    );
    start_refresh_content_fling(&laid);
    let released = b.pixels();
    assert!(released > 0.0);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert!(b.pixels() > released, "replacement receives its new fling");
    assert_eq!(a.pixels(), retired);
}

pub(crate) fn rebuilding_refresh_content_with_the_same_position_preserves_its_fling() {
    let scroll = ScrollController::new();
    scroll.update_dimensions(300.0, 0.0, 4700.0);
    let refresh = RefreshController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), refresh_content(&scroll, &refresh)),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    start_refresh_content_fling(&laid);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    let before = scroll.pixels();
    assert!(
        before > 100.0,
        "control fling advances before reconfiguration"
    );
    laid.pump_widget(VsyncScope::new(vsync, refresh_content(&scroll, &refresh)));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        scroll.pixels() > before,
        "same-position configuration keeps the run"
    );
}

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

fn phased_scroll(
    y: f64,
    dy: f64,
    phase: Option<flui_interaction::events::pointer::ScrollPhase>,
    device: u64,
) -> flui_interaction::PointerEvent {
    use flui_interaction::events::pointer::*;
    let pointer = PointerInfo::new(
        PointerId::try_from(1_u64).expect("pointer"),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(device).expect("actual source"));
    let mut event = ScrollEvent::new(
        pointer,
        flui_platform_api::EventTime::from_nanos(0),
        PointerPosition::try_new(flui_foundation::geometry::Point::new(150.0, y))
            .expect("finite focal point"),
        ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, dy).expect("finite scroll"),
    );
    event.phase = phase;
    PointerEvent::Scroll(event)
}

/// The same native gesture cannot jump into an ancestor at the child's edge.
pub(crate) fn nested_scroll_sequence_keeps_its_first_consumptive_target() {
    use flui_interaction::events::pointer::ScrollPhase;
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());
    inner.jump_to(inner.max_scroll_extent() - 20.0);
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 20.0, Some(ScrollPhase::Began), 1));
    assert_eq!(inner.pixels(), inner.max_scroll_extent());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        outer.pixels(),
        0.0,
        "reaching the edge does not chain the same gesture into the ancestor"
    );
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 0.0, Some(ScrollPhase::Ended), 1));
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 30.0, Some(ScrollPhase::Began), 1));
    assert_eq!(
        outer.pixels(),
        30.0,
        "the next gesture selects the first target that can actually move"
    );
    inner.jump_to(0.0);
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        inner.pixels(),
        0.0,
        "a newly eligible descendant cannot steal an already claimed stream"
    );
    assert_eq!(outer.pixels(), 60.0);
}

pub(crate) fn scroll_latch_survives_focal_motion_and_releases_on_cancel() {
    use flui_interaction::events::pointer::ScrollPhase;
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 53.0, Some(ScrollPhase::Began), 1));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        inner.pixels(),
        83.0,
        "the exact first claimant receives later focal motion outside its viewport"
    );
    assert_eq!(outer.pixels(), 0.0);
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 0.0, Some(ScrollPhase::Cancelled), 1));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Began), 1));
    assert_eq!(
        outer.pixels(),
        30.0,
        "cancellation releases the next fresh claim"
    );
}

pub(crate) fn phase_less_scroll_latch_expires_on_owner_clock_inactivity() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let mut scoped = nested_scrollables(&outer, &inner, Vsync::new());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 53.0, None, 1));
    scoped.pump_for(Duration::from_millis(499));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, None, 1));
    assert_eq!(
        inner.pixels(),
        83.0,
        "a wheel burst remains latched just before the inactivity boundary"
    );
    assert_eq!(outer.pixels(), 0.0);
    scoped.pump_for(Duration::from_millis(500));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, None, 1));
    assert_eq!(
        outer.pixels(),
        30.0,
        "500ms of owner-clock inactivity releases the burst without sleeping"
    );
}

pub(crate) fn scroll_latches_are_source_local_and_device_removal_releases() {
    use flui_interaction::events::pointer::*;
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());
    scoped.dispatch_pointer_event(&phased_scroll(100.0, 53.0, Some(ScrollPhase::Began), 1));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Began), 2));
    assert_eq!(
        outer.pixels(),
        30.0,
        "another source owns its independent claim"
    );
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(inner.pixels(), 83.0);
    assert_eq!(outer.pixels(), 30.0);
    scoped.dispatch_pointer_event(&PointerEvent::DeviceRemoved(PointerDeviceChange::new(
        DeviceId::try_from(1_u64).expect("device"),
        PointerKind::Mouse,
        flui_platform_api::EventTime::from_nanos(0),
    )));
    scoped.dispatch_pointer_event(&phased_scroll(250.0, 30.0, Some(ScrollPhase::Changed), 1));
    assert_eq!(
        outer.pixels(),
        60.0,
        "a removed source cannot retain its stale claim"
    );
}

/// Shift turns a plain vertical wheel tick into horizontal scrolling: a
/// horizontal list takes it, a vertical list does not, and a device that
/// already reports horizontal motion is passed through unchanged.
pub(crate) fn shift_wheel_scrolls_the_horizontal_axis() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::events::Modifiers;

    let laid_out = |axis: Axis, scroll: &ScrollController| {
        scroll.update_dimensions(300.0, 0.0, 700.0);
        lay_out(
            Scrollable::new()
                .scroll_direction(axis)
                .controller(scroll.clone())
                .child(SizedBox::new(1000.0, 1000.0)),
            tight(300.0, 300.0),
        )
    };

    let horizontal = ScrollController::new();
    let h = laid_out(Axis::Horizontal, &horizontal);
    h.dispatch_scroll(150.0, 150.0, 0.0, 53.0);
    assert_eq!(horizontal.pixels(), 0.0, "a plain wheel tick is vertical");
    h.dispatch_scroll_with_modifiers(150.0, 150.0, 0.0, 53.0, Modifiers::SHIFT);
    assert_eq!(
        horizontal.pixels(),
        53.0,
        "shift+wheel scrolls a horizontal list"
    );
    h.dispatch_scroll_with_modifiers(150.0, 150.0, 20.0, 53.0, Modifiers::SHIFT);
    assert_eq!(
        horizontal.pixels(),
        73.0,
        "reported horizontal motion is kept"
    );

    let vertical = ScrollController::new();
    let v = laid_out(Axis::Vertical, &vertical);
    v.dispatch_scroll_with_modifiers(150.0, 150.0, 0.0, 53.0, Modifiers::SHIFT);
    assert_eq!(
        vertical.pixels(),
        0.0,
        "shift+wheel does not scroll a vertical list"
    );
}

fn assert_bounce(current: f64, proposed: f64, expected: f64) {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};
    let actual = BouncingScrollPhysics::new()
        .apply_boundary_conditions(&ScrollMetrics::new(current, 0.0, 100.0, 300.0), proposed);
    assert!(
        (actual - expected).abs() < 1e-10,
        "current={current}, proposed={proposed}: expected={expected}, actual={actual}"
    );
}

pub(crate) fn bouncing_lower_edge_preserves_outward_direction() {
    assert_bounce(0.0, -100.0, -52.0);
    assert_bounce(-52.0, -62.0, -57.2);
}

pub(crate) fn bouncing_upper_edge_preserves_outward_direction() {
    assert_bounce(100.0, 200.0, 152.0);
    assert_bounce(152.0, 162.0, 157.2);
}

pub(crate) fn bouncing_stationary_input_preserves_overscroll() {
    assert_bounce(-52.0, -52.0, -52.0);
    assert_bounce(152.0, 152.0, 152.0);
}

pub(crate) fn bouncing_inward_motion_and_crossing_respect_the_new_edge() {
    assert_bounce(-52.0, -42.0, -42.0);
    assert_bounce(152.0, 142.0, 142.0);
    assert_bounce(-52.0, 120.0, 110.4);
    assert_bounce(152.0, -20.0, -10.4);
}

fn animated_scroll_content(scroll: &ScrollController, vsync: &Vsync) -> impl flui_view::View {
    VsyncScope::new(
        vsync.clone(),
        Scrollable::new()
            .controller(scroll.clone())
            .child(SizedBox::new(300.0, 5000.0)),
    )
}

fn advance_scroll_run(laid: &mut LaidOut) {
    for _ in 0..3 {
        laid.pump_for(Duration::from_millis(16));
    }
}

pub(crate) fn replacing_vsync_retires_old_motion_and_drives_fresh_contacts() {
    use flui_foundation::geometry::EdgeInsets;
    use flui_widgets::{InteractiveViewer, TransformationController};

    let mut failures = Vec::new();
    for family in ["scrollable", "refresh", "viewer"] {
        let scroll = ScrollController::new();
        let transform = TransformationController::new();
        let child = match family {
            "scrollable" => Scrollable::new()
                .controller(scroll.clone())
                .child(SizedBox::new(300.0, 5000.0))
                .boxed(),
            "refresh" => refresh_content(&scroll, &RefreshController::new()).boxed(),
            _ => InteractiveViewer::new()
                .controller(transform.clone())
                .boundary_margin(EdgeInsets::all(1000.0))
                .scale_enabled(false)
                .child(SizedBox::new(300.0, 300.0))
                .boxed(),
        };
        let pixels = || {
            if family == "viewer" {
                -transform.value().to_col_major_array()[13]
            } else {
                scroll.pixels()
            }
        };
        let first = Vsync::new();
        let second = Vsync::new();
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(first.clone(), child.clone()),
            tight(300.0, 300.0),
            first.clone(),
        );
        let fling = |laid: &LaidOut| {
            laid.dispatch_pointer_down(150.0, 250.0);
            for y in [230.0, 210.0, 190.0, 170.0, 150.0] {
                laid.dispatch_pointer_move_after(150.0, y, Duration::from_millis(10));
            }
            laid.dispatch_pointer_up(150.0, 150.0);
        };
        fling(&laid);
        let released = pixels();
        advance_scroll_run(&mut laid);
        assert!(pixels() > released, "{family}: initial owner drives real inertia");
        let before_same = pixels();
        laid.pump_widget(VsyncScope::new(first.clone(), child.clone()));
        laid.pump_for(Duration::from_millis(16));
        assert!(pixels() > before_same, "{family}: same owner preserves the run");

        laid.pump_widget(VsyncScope::new(second.clone(), child));
        let replaced = pixels();
        laid.pump_for(Duration::from_millis(16));
        if pixels() != replaced {
            failures.push(format!("{family}: retired Vsync still advances content"));
        }
        fling(&laid);
        let fresh_release = pixels();
        second.tick_all(1.0);
        second.tick_all(1.032);
        if pixels() <= fresh_release {
            failures.push(format!("{family}: replacement Vsync cannot drive fresh inertia"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

pub(crate) fn replacing_a_scroll_position_cancels_its_contact_and_recovers() {
    use std::sync::Mutex;

    let mut failures = Vec::new();
    for (family, cancelled) in [
        ("scrollable", false),
        ("scrollable", true),
        ("refresh", false),
        ("refresh", true),
    ] {
        let old = ScrollController::new();
        let new = ScrollController::new();
        let vsync = Vsync::new();
        let incoming = new.position();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = observed.clone();
        let watched = incoming.clone();
        let subscription = incoming.add_activity_listener(Arc::new(move || {
            sink.lock().expect("activity observer lock").push((
                watched.is_scrolling(),
                watched.user_scroll_direction(),
            ));
        }));
        let refresh = RefreshController::new();
        let content = |controller: &ScrollController| {
            let child = if family == "refresh" {
                refresh_content(controller, &refresh).boxed()
            } else {
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(300.0, 5000.0))
                    .boxed()
            };
            VsyncScope::new(vsync.clone(), child)
        };
        let mut laid = crate::common::lay_out_animated(
            content(&old),
            tight(300.0, 300.0),
            vsync.clone(),
        );
        laid.dispatch_pointer_down(150.0, 250.0);
        laid.dispatch_pointer_move_after(150.0, 200.0, Duration::from_millis(10));
        laid.dispatch_pointer_move_after(150.0, 180.0, Duration::from_millis(10));
        assert!(old.pixels() > 0.0);
        if family == "scrollable" {
            assert!(old.position().is_scrolling());
        }
        let same = old.pixels();
        laid.pump_widget(content(&old));
        laid.dispatch_pointer_move_after(150.0, 160.0, Duration::from_millis(10));
        assert!(old.pixels() > same, "same position preserves the admitted contact");

        laid.pump_widget(content(&new));
        let retired = old.pixels();
        let before = new.pixels();
        laid.dispatch_pointer_move_after(150.0, 140.0, Duration::from_millis(10));
        if new.pixels() != before {
            failures.push(format!("{family}: replacement position consumed the retired contact"));
        }
        assert_eq!(old.pixels(), retired);
        assert!(!old.position().is_scrolling());
        if cancelled {
            laid.dispatch_pointer_cancel();
        } else {
            laid.dispatch_pointer_up(150.0, 140.0);
        }
        advance_scroll_run(&mut laid);
        if new.pixels() != before || incoming.is_scrolling() {
            failures.push(format!("{family}: retired terminal started replacement motion or activity"));
        }
        observed.lock().expect("activity observer lock").clear();
        let fresh = new.pixels();
        laid.dispatch_pointer_down(150.0, 250.0);
        laid.dispatch_pointer_move_after(150.0, 200.0, Duration::from_millis(10));
        laid.dispatch_pointer_move_after(150.0, 180.0, Duration::from_millis(10));
        assert!(new.pixels() > fresh, "fresh contact drives replacement content");
        assert!(
            family == "refresh"
                || observed.lock().expect("activity observer lock")
                    .contains(&(true, ScrollDirection::Reverse)),
            "fresh movement reaches the real activity consumer"
        );
        laid.dispatch_pointer_cancel();
        assert!(!incoming.is_scrolling());
        incoming.remove_activity_listener(subscription);
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

fn dispatch_typed_wheel(
    laid: &LaidOut,
    precision: flui_platform_api::pointer::ScrollPrecision,
    dy: f64,
) {
    use flui_foundation::geometry::Point;
    use flui_platform_api::EventTime;
    use flui_platform_api::pointer::{
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta,
        ScrollEvent, ScrollUnit,
    };
    let event = ScrollEvent::new(
        PointerInfo::new(
            PointerId::try_from(1_u64).expect("nonzero mouse"),
            PointerKind::Mouse,
        ),
        EventTime::from_nanos(0),
        PointerPosition::try_new(Point::new(150.0, 100.0)).expect("finite wheel position"),
        ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, dy).expect("finite wheel distance"),
    )
    .with_precision(precision);
    laid.dispatch_pointer_event(&PointerEvent::Scroll(event));
}

#[derive(Clone, flui_view::prelude::StatelessView)]
struct AuthoredWheelContent {
    axis: flui_foundation::geometry::Axis,
    controller: ScrollController,
    provider: flui_interaction::WheelPreferencesProvider,
    distances: flui_widgets::WheelScrollDistances,
    extent: f64,
}

impl flui_view::StatelessView for AuthoredWheelContent {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        let arena = flui_widgets::GestureArenaScope::of(ctx);
        let scroll = Scrollable::new()
            .scroll_direction(self.axis)
            .controller(self.controller.clone())
            .wheel_distances(self.distances)
            .child(SizedBox::new(self.extent, self.extent));
        let inner = flui_widgets::GestureArenaScope::new(arena.clone(), scroll)
            .settings(flui_interaction::GestureSettings::default());
        flui_widgets::GestureArenaScope::new(arena, inner).wheel_preferences(self.provider.clone())
    }
}

fn dispatch_wheel_distance(
    laid: &LaidOut,
    unit: flui_platform_api::pointer::ScrollUnit,
    x: f64,
    y: f64,
    modifiers: flui_platform_api::keyboard::Modifiers,
) {
    use flui_platform_api::pointer::{
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta,
        ScrollEvent,
    };
    laid.dispatch_pointer_event(&PointerEvent::Scroll(
        ScrollEvent::new(
            PointerInfo::new(
                PointerId::try_from(1_u64).expect("pointer"),
                PointerKind::Mouse,
            ),
            flui_platform_api::EventTime::from_nanos(1),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(150.0, 100.0))
                .expect("point"),
            ScrollDelta::try_new(unit, x, y).expect("finite wheel distance"),
        )
        .with_modifiers(modifiers),
    ));
}

pub(crate) fn wheel_policy_resolves_authored_axes_and_provider_replacement() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::WheelPreferencesSource;
    use flui_platform_api::{
        WheelPreferences, WheelStep, keyboard::Modifiers, pointer::ScrollUnit,
    };
    use flui_widgets::WheelScrollDistances;

    for axis in [Axis::Vertical, Axis::Horizontal] {
        let source = WheelPreferencesSource::new(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(3))
                .with_horizontal_characters(4),
        );
        let scroll = ScrollController::new();
        let content = AuthoredWheelContent {
            axis,
            controller: scroll.clone(),
            provider: source.provider(),
            distances: WheelScrollDistances::try_new(20.0, 7.0).expect("authored distances"),
            extent: 5000.0,
        };
        let mut laid = lay_out(content.clone(), tight(300.0, 300.0));
        for (label, unit, x, y, modifiers, vertical, horizontal) in [
            (
                "raw axis counts",
                ScrollUnit::Detents,
                1.0,
                1.0,
                Modifiers::NONE,
                60.0,
                28.0,
            ),
            (
                "fractional raw axis counts",
                ScrollUnit::Detents,
                0.25,
                0.25,
                Modifiers::NONE,
                15.0,
                7.0,
            ),
            (
                "shift chooses receiving axis",
                ScrollUnit::Detents,
                0.0,
                1.0,
                Modifiers::SHIFT,
                0.0,
                28.0,
            ),
            (
                "existing horizontal axis is not shifted again",
                ScrollUnit::Detents,
                1.0,
                1.0,
                Modifiers::SHIFT,
                60.0,
                28.0,
            ),
            (
                "translated lines bypass counts",
                ScrollUnit::Lines,
                2.0,
                2.0,
                Modifiers::NONE,
                40.0,
                40.0,
            ),
            (
                "translated pages use viewport",
                ScrollUnit::Pages,
                0.5,
                0.5,
                Modifiers::NONE,
                150.0,
                150.0,
            ),
        ] {
            scroll.jump_to(0.0);
            dispatch_wheel_distance(&laid, unit, x, y, modifiers);
            assert_eq!(
                scroll.pixels(),
                if axis == Axis::Vertical {
                    vertical
                } else {
                    horizontal
                },
                "{label}: {axis:?}"
            );
        }
        source.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(0))
                .with_horizontal_characters(0),
        );
        scroll.jump_to(0.0);
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(scroll.pixels(), 0.0, "zero disables only raw detents");
        dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 19.0, 17.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 17.0 } else { 19.0 },
            "pixel control"
        );

        // Different live sources begin with identical values. A replacement
        // must retain the new authority even when the old writer changes later.
        source.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(2))
                .with_horizontal_characters(2),
        );
        scroll.jump_to(0.0);
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 40.0 } else { 14.0 }
        );
        let replacement = WheelPreferencesSource::new(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(2))
                .with_horizontal_characters(2),
        );
        let mut replaced = content;
        replaced.provider = replacement.provider();
        laid.pump_widget(replaced);
        source.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(0))
                .with_horizontal_characters(0),
        );
        replacement.replace(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(5))
                .with_horizontal_characters(5),
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 140.0 } else { 49.0 },
            "replacement authority stays live in a retained scrollable"
        );
    }

    let source = WheelPreferencesSource::new(
        WheelPreferences::default().with_horizontal_characters(u32::MAX),
    );
    let scroll = ScrollController::new();
    let laid = lay_out(
        AuthoredWheelContent {
            axis: Axis::Horizontal,
            controller: scroll.clone(),
            provider: source.provider(),
            distances: WheelScrollDistances::try_new(20.0, 1e-8).expect("positive distances"),
            extent: 5000.0,
        },
        tight(300.0, 300.0),
    );
    dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 0.0, Modifiers::NONE);
    assert!(
        (scroll.pixels() - 42.949_672_95).abs() < 1e-10,
        "horizontal UINT_MAX remains a character count, not a page sentinel"
    );
}

pub(crate) fn wheel_distances_refuse_overflow_and_the_next_packet_recovers() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::WheelPreferencesSource;
    use flui_platform_api::{
        WheelPreferences, WheelStep, keyboard::Modifiers, pointer::ScrollUnit,
    };
    use flui_widgets::{InvalidWheelScrollDistance, WheelScrollDistances};

    for invalid in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            WheelScrollDistances::try_new(invalid, 7.0),
            Err(InvalidWheelScrollDistance::Line)
        );
        assert_eq!(
            WheelScrollDistances::try_new(20.0, invalid),
            Err(InvalidWheelScrollDistance::Character)
        );
    }
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let source = WheelPreferencesSource::new(
            WheelPreferences::default()
                .with_vertical(WheelStep::Lines(2))
                .with_horizontal_characters(2),
        );
        let scroll = ScrollController::new();
        let laid = lay_out(
            AuthoredWheelContent {
                axis,
                controller: scroll.clone(),
                provider: source.provider(),
                distances: WheelScrollDistances::try_new(f64::MAX, f64::MAX)
                    .expect("finite authored distances"),
                extent: 5000.0,
            },
            tight(300.0, 300.0),
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Detents, 1.0, 1.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            0.0,
            "overflowing native count product is refused before page-like cap"
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Lines, 2.0, 2.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            0.0,
            "overflowing translated line product is refused"
        );
        dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 9.0, 7.0, Modifiers::NONE);
        assert_eq!(
            scroll.pixels(),
            if axis == Axis::Vertical { 7.0 } else { 9.0 },
            "pixel recovery bypasses authored distance multipliers"
        );
    }

    // A real large content extent admits the first finite packet. A second
    // finite packet whose destination overflows must not turn into an end jump.
    let scroll = ScrollController::new();
    let laid = lay_out(
        AuthoredWheelContent {
            axis: Axis::Vertical,
            controller: scroll.clone(),
            provider: WheelPreferences::default().into(),
            distances: WheelScrollDistances::default(),
            extent: f64::MAX,
        },
        tight(300.0, 300.0),
    );
    let distance = f64::MAX * 0.75;
    dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 0.0, distance, Modifiers::NONE);
    assert_eq!(
        scroll.pixels(),
        distance,
        "finite large content admits meaningful movement"
    );
    dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 0.0, distance, Modifiers::NONE);
    assert_eq!(
        scroll.pixels(),
        distance,
        "overflowing destination is refused rather than clamped"
    );
    dispatch_wheel_distance(&laid, ScrollUnit::Pixels, 0.0, -distance, Modifiers::NONE);
    assert_eq!(
        scroll.pixels(),
        0.0,
        "healthy input recovers after destination refusal"
    );
}

pub(crate) fn notched_wheel_accumulates_distance_and_eases_out_in_150ms() {
    use flui_platform_api::pointer::ScrollPrecision::Notched;
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync,
    );
    dispatch_typed_wheel(&laid, Notched, 50.0);
    dispatch_typed_wheel(&laid, Notched, 50.0);
    assert_eq!(
        scroll.pixels(),
        0.0,
        "notches start a trajectory without jumping"
    );
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(75));
    assert!(
        scroll.pixels() > 50.0 && scroll.pixels() < 100.0,
        "ease-out covers more than half the accepted distance halfway through: {}",
        scroll.pixels()
    );
    dispatch_typed_wheel(&laid, Notched, 50.0);
    dispatch_typed_wheel(&laid, Notched, -25.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scroll.pixels(),
        125.0,
        "all accepted ticks accumulate, including reversal"
    );
    assert!(
        !scroll.position().is_scrolling(),
        "the settled wheel activity ends"
    );
    dispatch_typed_wheel(&laid, Notched, -25.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scroll.pixels(),
        100.0,
        "a healthy next wheel trajectory completes"
    );
}

pub(crate) fn precise_and_unknown_wheels_interrupt_synthetic_motion_once() {
    use flui_platform_api::pointer::ScrollPrecision::{Notched, Precise, Unknown};
    for precision in [Precise, Unknown] {
        let scroll = ScrollController::new();
        let vsync = Vsync::new();
        let mut laid = crate::common::lay_out_animated(
            animated_scroll_content(&scroll, &vsync),
            tight(300.0, 300.0),
            vsync,
        );
        dispatch_typed_wheel(&laid, precision, 20.0);
        assert_eq!(scroll.pixels(), 20.0, "{precision:?}: immediate control");
        laid.pump_for(Duration::from_millis(200));
        assert_eq!(scroll.pixels(), 20.0, "{precision:?}: no duplicated motion");
        dispatch_typed_wheel(&laid, Notched, 100.0);
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(60));
        let before = scroll.pixels();
        assert!(
            before > 20.0 && before < 120.0,
            "notched motion is still live"
        );
        dispatch_typed_wheel(&laid, precision, 10.0);
        assert_eq!(
            scroll.pixels(),
            before + 10.0,
            "immediate input starts at current pixels"
        );
        laid.pump_for(Duration::from_millis(200));
        assert_eq!(
            scroll.pixels(),
            before + 10.0,
            "interrupted target cannot overwrite input"
        );
        dispatch_typed_wheel(&laid, Notched, 30.0);
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(150));
        assert_eq!(
            scroll.pixels(),
            before + 40.0,
            "old accepted target was retired"
        );
    }
}

pub(crate) fn replacing_or_unmounting_a_scrollable_retires_its_notched_motion() {
    use flui_platform_api::pointer::ScrollPrecision::Notched;
    let old = ScrollController::new();
    let new = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&old, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    let retired = old.pixels();
    assert!(
        retired > 0.0 && retired < 100.0,
        "the old owner's motion is live"
    );
    laid.pump_widget(animated_scroll_content(&new, &vsync));
    dispatch_typed_wheel(&laid, Notched, 50.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        old.pixels(),
        retired,
        "replacement does not keep driving the old owner"
    );
    assert_eq!(
        new.pixels(),
        50.0,
        "replacement starts with its own accepted target"
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    let unmounted = new.pixels();
    laid.pump_widget(SizedBox::new(300.0, 300.0));
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        new.pixels(),
        unmounted,
        "unmount retires motion before future frames"
    );
}

pub(crate) fn dragging_interrupts_notched_motion_and_windows_progress_independently() {
    use flui_platform_api::pointer::ScrollPrecision::Notched;
    let scroll = ScrollController::new();
    let other = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync,
    );
    let other_vsync = Vsync::new();
    let mut independent = crate::common::lay_out_animated(
        animated_scroll_content(&other, &other_vsync),
        tight(300.0, 300.0),
        other_vsync,
    );
    dispatch_typed_wheel(&laid, Notched, 100.0);
    dispatch_typed_wheel(&independent, Notched, 200.0);
    laid.pump_for(Duration::ZERO);
    independent.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(60));
    assert_eq!(
        other.pixels(),
        0.0,
        "another owner's clock has not advanced"
    );
    let grabbed = scroll.pixels();
    assert!(
        grabbed > 0.0 && grabbed < 100.0,
        "the grabbed motion is live"
    );
    laid.dispatch_pointer_down(150.0, 200.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    let dragged = scroll.pixels();
    assert!(dragged > grabbed, "drag starts from the displayed position");
    laid.dispatch_pointer_cancel();
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        scroll.pixels(),
        dragged,
        "the cancelled drag leaves no old wheel motion"
    );
    independent.pump_for(Duration::from_millis(150));
    assert_eq!(
        other.pixels(),
        200.0,
        "the independent owner's trajectory survives"
    );
    dispatch_typed_wheel(&laid, Notched, 25.0);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        scroll.pixels(),
        dragged + 25.0,
        "next wheel uses the new drag position"
    );
}

pub(crate) fn a_scrollable_swap_stops_old_motion_and_retires_its_jump_hook() {
    let old = ScrollController::new();
    let new = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&old, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    old.animate_to(1000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    advance_scroll_run(&mut laid);
    assert!(old.pixels() > 0.0);
    let retired = old.pixels();
    laid.pump_widget(animated_scroll_content(&new, &vsync));
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(
        new.pixels(),
        0.0,
        "old trajectory must not drive the new position"
    );
    assert_eq!(old.pixels(), retired);
    new.animate_to(900.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    advance_scroll_run(&mut laid);
    let before = new.pixels();
    assert!(before > 0.0);
    old.jump_to(42.0);
    laid.pump_for(Duration::from_millis(16));
    assert!(
        new.pixels() > before,
        "retired controller must not stop the new run"
    );
    assert_eq!(old.pixels(), 42.0);
}

pub(crate) fn a_same_position_scrollable_rebuild_preserves_motion() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    scroll.animate_to(1000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    advance_scroll_run(&mut laid);
    let before = scroll.pixels();
    assert!(before > 0.0);
    laid.pump_widget(animated_scroll_content(&scroll, &vsync));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        scroll.pixels() > before,
        "same position keeps its active trajectory"
    );
}

pub(crate) fn retiring_one_scrollable_preserves_a_later_owners_jump_hook() {
    let scroll = ScrollController::new();
    let first_vsync = Vsync::new();
    let second_vsync = Vsync::new();
    let mut first = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &first_vsync),
        tight(300.0, 300.0),
        first_vsync.clone(),
    );
    let mut second = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &second_vsync),
        tight(300.0, 300.0),
        second_vsync.clone(),
    );
    second.dispatch_pointer_down(150.0, 250.0);
    second.dispatch_pointer_move(150.0, 180.0);
    second.dispatch_pointer_move(150.0, 150.0);
    second.dispatch_pointer_up(150.0, 150.0);
    let released = scroll.pixels();
    advance_scroll_run(&mut second);
    assert!(
        scroll.pixels() > released,
        "later attachment has its own live fling"
    );
    let before = scroll.pixels();
    assert!(before > 0.0);
    first.pump_widget(SizedBox::new(300.0, 300.0));
    // Equal-value jumps emit no position notification. Only the surviving
    // owner's synchronous hook can stop its run before the next tick writes.
    scroll.jump_to(before);
    second.pump_for(Duration::from_millis(16));
    assert_eq!(
        scroll.pixels(),
        before,
        "detaching a different owner must preserve cancellation"
    );
    scroll.animate_to(900.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    advance_scroll_run(&mut second);
    assert!(scroll.pixels() > before, "next command still progresses");
}

pub(crate) fn cancelling_an_in_range_scroll_ends_activity_without_coasting() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        animated_scroll_content(&scroll, &vsync),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    let cancelled_at = scroll.pixels();
    assert!(cancelled_at > 0.0);
    assert!(scroll.position().is_scrolling());
    laid.dispatch_pointer_cancel();
    for _ in 0..8 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        scroll.pixels(),
        cancelled_at,
        "cancel supplies no fling impulse"
    );
    assert!(!scroll.position().is_scrolling());
    assert_eq!(
        scroll.position().user_scroll_direction(),
        ScrollDirection::Idle
    );
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_move(150.0, 150.0);
    laid.dispatch_pointer_up(150.0, 150.0);
    let released_at = scroll.pixels();
    advance_scroll_run(&mut laid);
    assert!(
        scroll.pixels() > released_at,
        "next completed gesture still flings"
    );
}

pub(crate) fn cancelling_bouncing_overscroll_settles_without_release_velocity() {
    let scroll = ScrollController::new();
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Scrollable::new()
                .controller(scroll.clone())
                .physics(Arc::new(BouncingScrollPhysics::new()))
                .child(SizedBox::new(300.0, 800.0)),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 100.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    let overscroll = scroll.pixels();
    assert!(overscroll < 0.0);
    laid.dispatch_pointer_cancel();
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        scroll.pixels() > overscroll,
        "zero-impulse spring immediately recovers toward edge"
    );
    for _ in 0..160 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(scroll.pixels().abs() < 1.0);
    assert!(!scroll.position().is_scrolling());
    laid.dispatch_pointer_down(150.0, 250.0);
    laid.dispatch_pointer_move(150.0, 180.0);
    laid.dispatch_pointer_up(150.0, 180.0);
    assert!(scroll.pixels() > 0.0, "next gesture advances content");
}

pub(crate) fn cancelling_a_threshold_refresh_pull_does_not_refresh() {
    let scroll = ScrollController::new();
    let refresh = RefreshController::new();
    let calls = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&calls);
    let vsync = Vsync::new();
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            refresh_content(&scroll, &refresh)
                .on_refresh(move |_| recorded.set(recorded.get() + 1)),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    assert!(refresh.pull_distance_px() >= 100.0);
    laid.dispatch_pointer_cancel();
    for _ in 0..8 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(calls.get(), 0);
    assert!(!refresh.is_refreshing());
    assert_eq!(refresh.pull_distance_px(), 0.0);
    assert_eq!(scroll.pixels(), 0.0);
    laid.dispatch_pointer_down(150.0, 40.0);
    laid.dispatch_pointer_move(150.0, 140.0);
    laid.dispatch_pointer_up(150.0, 140.0);
    assert_eq!(calls.get(), 1, "normal release still starts refresh");
}

// ============================================================================
// Ballistic rest — device pixel ratio, extents, overscroll
// ============================================================================

/// Clamping physics that records the metrics each release hands it.
#[derive(Debug, Default)]
struct RecordingPhysics {
    ratios: std::sync::Mutex<Vec<f64>>,
}

impl flui_widgets::ScrollPhysics for RecordingPhysics {
    fn apply_boundary_conditions(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        proposed_pixels: f64,
    ) -> f64 {
        ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed_pixels)
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn flui_animation::Simulation>> {
        self.ratios
            .lock()
            .expect("recording lock")
            .push(metrics.device_pixel_ratio);
        ClampingScrollPhysics::new().create_ballistic_simulation(metrics, velocity_px_per_sec)
    }
}

/// A release hands the physics the presentation's device pixel ratio, and a
/// fling rests once half a device pixel of glide remains: 8000 px/s at drag
/// 0.135 rests at 4.4874 s at a ratio of 1 and at 4.8336 s at a ratio of 2
/// (`8000·0.135ᵗ / |ln 0.135| = 0.5 / ratio`).
pub(crate) fn scroll_fling_rest_scales_with_device_pixel_ratio() {
    use flui_widgets::{ScrollMetrics, ScrollPhysics};

    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let recording = Arc::new(RecordingPhysics::default());
    let physics: SharedScrollPhysics = recording.clone();
    let widget = Scrollable::new()
        .controller(controller)
        .physics(physics)
        .child(SizedBox::new(300.0, 5000.0));
    let scoped = fling_scoped(widget, Vsync::new(), tight(300.0, 300.0));
    scoped
        .pipeline_owner()
        .with_mut(|owner| owner.set_device_pixel_ratio(2.0));
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);
    assert_eq!(
        *recording.ratios.lock().expect("recording lock"),
        [2.0],
        "the release reads the presentation's device pixel ratio"
    );

    let fling = |ratio: f64| {
        ClampingScrollPhysics::new()
            .create_ballistic_simulation(
                &ScrollMetrics::new(0.0, 0.0, 1e9, 300.0).with_device_pixel_ratio(ratio),
                8000.0,
            )
            .expect("a fling")
    };
    let (single, double) = (fling(1.0), fling(2.0));
    assert!(!single.is_done(4.4874) && single.is_done(4.4875));
    assert!(!double.is_done(4.8335) && double.is_done(4.8336));
    let bouncing = BouncingScrollPhysics::new().create_ballistic_simulation(
        &ScrollMetrics::new(0.0, 0.0, 1e9, 300.0).with_device_pixel_ratio(2.0),
        8000.0,
    );
    let bouncing = bouncing.expect("a fling");
    assert!(!bouncing.is_done(4.8335) && bouncing.is_done(4.8336));
}

/// Unordered or non-finite extents start no ballistic run, under every
/// physics, instead of panicking.
pub(crate) fn inverted_extents_do_not_fling() {
    use flui_widgets::{PageScrollPhysics, ScrollMetrics, ScrollPhysics};

    let physics: [&dyn ScrollPhysics; 3] = [
        &ClampingScrollPhysics::new(),
        &BouncingScrollPhysics::new(),
        &PageScrollPhysics::new(1.0),
    ];
    for metrics in [
        ScrollMetrics::new(50.0, 100.0, 0.0, 300.0),
        ScrollMetrics::new(f64::NAN, 0.0, 100.0, 300.0),
        ScrollMetrics::new(50.0, 0.0, 100.0, 300.0).with_device_pixel_ratio(f64::NAN),
    ] {
        for physics in physics {
            assert!(
                physics
                    .create_ballistic_simulation(&metrics, 4000.0)
                    .is_none(),
                "{physics:?} under {metrics:?}"
            );
        }
    }
}

/// A fling inside the range that friction carries past the edge overscrolls,
/// then springs back and rests exactly on the edge.
pub(crate) fn bouncing_fling_into_the_edge_overscrolls_and_returns() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f64;
    controller.update_dimensions(300.0, 0.0, max_extent);
    let physics: SharedScrollPhysics = Arc::new(BouncingScrollPhysics::new());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));
    let mut scoped = fling_scoped(widget, Vsync::new(), tight(300.0, 300.0));
    controller.set_pixels(300.0);

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);
    assert!(
        controller.pixels() < max_extent,
        "released inside the range"
    );

    let mut furthest = controller.pixels();
    for _ in 0..240 {
        scoped.pump_for(Duration::from_millis(16));
        furthest = furthest.max(controller.pixels());
    }
    assert!(
        furthest > max_extent + 1.0,
        "the fling overscrolls past the edge; furthest {furthest:.3}"
    );
    assert_eq!(controller.pixels(), max_extent, "rests exactly on the edge");
}

// ============================================================================
// RefreshIndicator — scrolling and pulling without rebuilding
// ============================================================================

/// A `RefreshIndicator` on the headless binding, whose per-frame report runs
/// the build phase every frame, driven by pointer events between frames.
struct RefreshHarness {
    binding: flui_testing::HeadlessBinding,
    scroll: ScrollController,
    refresh: RefreshController,
}

const REFRESH_FRAME: Duration = Duration::from_nanos(16_666_667);

impl RefreshHarness {
    fn mount() -> Self {
        let scroll = ScrollController::new();
        scroll.update_dimensions(300.0, 0.0, 4700.0);
        let refresh = RefreshController::new();
        let mut binding = flui_testing::HeadlessBinding::new();
        let root = flui_widgets::GestureArenaScope::new(
            binding.arena().clone(),
            flui_widgets::FocusRoot::new(VsyncScope::new(
                binding.vsync().clone(),
                refresh_content(&scroll, &refresh),
            )),
        );
        let _ = binding.mount_root(
            &root,
            flui_testing::MountOwners::fresh(),
            flui_testing::MountOptions::tight(300.0, 300.0),
        );
        binding.pump_frame(REFRESH_FRAME);
        Self {
            binding,
            scroll,
            refresh,
        }
    }

    fn pointer(&self, phase: flui_testing::PointerPhase, y: f64) {
        let event = flui_testing::ScriptedPointer::new(
            Duration::ZERO,
            flui_interaction::PointerId::try_from(1).expect("nonzero fixture contact"),
            phase,
            flui_foundation::geometry::Offset::new(150.0, y),
        )
        .to_event();
        let binding = &self.binding;
        binding.dispatch_pointer(&event, |position| binding.hit_test(position));
    }

    /// Pumps one frame and returns how many elements it rebuilt.
    fn frame(&mut self) -> usize {
        self.binding.pump_frame(REFRESH_FRAME);
        self.binding.last_frame_report().build.elements_built
    }

    /// Where the content's top edge is painted, in root coordinates.
    fn content_top(&self) -> f64 {
        self.binding
            .pipeline_owner()
            .expect("tree-bound")
            .with(|owner| {
                let root = owner.root_id().expect("rooted");
                let content = owner
                    .render_tree()
                    .iter()
                    .map(|(id, _)| id)
                    .find(|&id| {
                        owner.box_size(id)
                            == Some(flui_foundation::geometry::Size::new(300.0, 5000.0))
                    })
                    .expect("the 300x5000 content is mounted");
                owner
                    .transform_to(content, root)
                    .expect("content laid out")
                    .transform_point(0.0, 0.0)
                    .1
            })
    }
}

/// Dragging the content scrolls the viewport on every frame without
/// rebuilding any element.
pub(crate) fn refresh_indicator_drag_scrolls_without_rebuilding() {
    use flui_testing::PointerPhase::{Down, Move, Up};
    let mut harness = RefreshHarness::mount();
    harness.pointer(Down, 250.0);
    assert_eq!(harness.frame(), 0, "touch down rebuilds nothing");
    for y in [230.0, 200.0, 170.0, 140.0] {
        harness.pointer(Move, y);
        assert_eq!(harness.frame(), 0, "a drag frame to y={y} must not rebuild");
        assert!(
            (harness.content_top() + harness.scroll.pixels()).abs() < 1e-9,
            "the viewport follows the position on the same frame: top {} at pixels {}",
            harness.content_top(),
            harness.scroll.pixels()
        );
    }
    assert!(harness.scroll.pixels() > 0.0, "the drag scrolled");
    harness.pointer(Up, 140.0);
    for _ in 0..4 {
        let before = harness.scroll.pixels();
        assert_eq!(harness.frame(), 0, "a fling frame must not rebuild");
        assert!(
            harness.scroll.pixels() >= before,
            "the fling coasts forward"
        );
        assert!((harness.content_top() + harness.scroll.pixels()).abs() < 1e-9);
    }
}

/// A pull changes only the pull distance: no rebuild, while an external
/// listener still hears every change. Entering and leaving the refreshing
/// phase rebuilds, once each.
pub(crate) fn refresh_indicator_rebuilds_only_on_a_phase_change() {
    use flui_testing::PointerPhase::{Down, Move, Up};
    let mut harness = RefreshHarness::mount();
    let heard = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&heard);
    let _subscription = harness
        .refresh
        .as_listenable()
        .add_listener(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));

    harness.pointer(Down, 40.0);
    harness.frame();
    let mut moves = 0;
    for y in [60.0, 80.0, 100.0, 120.0, 140.0] {
        harness.pointer(Move, y);
        moves += 1;
        assert_eq!(harness.frame(), 0, "a pull frame to y={y} must not rebuild");
    }
    assert!(harness.refresh.pull_distance_px() > 80.0);
    assert!(
        heard.load(std::sync::atomic::Ordering::SeqCst) >= moves,
        "external listeners hear every pull change"
    );

    harness.pointer(Up, 140.0);
    assert!(harness.refresh.is_refreshing());
    assert!(
        harness.frame() > 0,
        "entering the refreshing phase rebuilds"
    );
    assert_eq!(harness.frame(), 0, "and then settles");

    harness.refresh.finish();
    assert!(harness.frame() > 0, "leaving the refreshing phase rebuilds");
    assert_eq!(harness.frame(), 0, "and then settles");
}
fn nested_fling_content(
    outer: &ScrollController,
    inner: &ScrollController,
    vsync: &Vsync,
    outer_axis: flui_foundation::geometry::Axis,
    inner_axis: flui_foundation::geometry::Axis,
    inner_reversed: bool,
    bouncing: bool,
) -> impl flui_view::View {
    use flui_foundation::geometry::Axis;
    let mut child = Scrollable::new()
        .controller(inner.clone())
        .scroll_direction(inner_axis)
        .child(match inner_axis {
            Axis::Vertical => SizedBox::new(300.0, 1000.0),
            Axis::Horizontal => SizedBox::new(1000.0, 300.0),
        });
    if inner_reversed {
        child = child
            .axis_direction(match inner_axis {
                Axis::Vertical => flui_rendering::constraints::AxisDirection::BottomToTop,
                Axis::Horizontal => flui_rendering::constraints::AxisDirection::RightToLeft,
            })
            .viewport_builder(Rc::new(move |position| {
                flui_widgets::SingleChildScrollView::new()
                    .scroll_direction(inner_axis)
                    .reverse(true)
                    .position(position)
                    .child(match inner_axis {
                        Axis::Vertical => SizedBox::new(300.0, 1000.0),
                        Axis::Horizontal => SizedBox::new(1000.0, 300.0),
                    })
                    .boxed()
            }));
    }
    if bouncing {
        child = child.physics(Arc::new(BouncingScrollPhysics::new()));
    }
    let child = flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT)
        .child(SizedBox::new(200.0, 200.0).child(child));
    let content: flui_view::BoxedView = match outer_axis {
        Axis::Vertical => flui_widgets::Column::new(vec![
            SizedBox::new(300.0, 600.0).boxed(),
            SizedBox::new(300.0, 200.0).child(child).boxed(),
            SizedBox::new(300.0, 4800.0).boxed(),
        ])
        .boxed(),
        Axis::Horizontal => flui_widgets::Row::new(vec![
            SizedBox::new(600.0, 300.0).boxed(),
            SizedBox::new(200.0, 300.0).child(child).boxed(),
            SizedBox::new(4800.0, 300.0).boxed(),
        ])
        .boxed(),
    };
    VsyncScope::new(
        vsync.clone(),
        Scrollable::new()
            .controller(outer.clone())
            .scroll_direction(outer_axis)
            .child(content),
    )
}

fn release_inner_fling(laid: &LaidOut, axis: flui_foundation::geometry::Axis, reversed: bool) {
    use flui_foundation::geometry::Axis;
    let samples = if reversed {
        [50.0, 80.0, 100.0]
    } else {
        [150.0, 120.0, 100.0]
    };
    let at = |main| match axis {
        Axis::Vertical => (100.0, main),
        Axis::Horizontal => (main, 100.0),
    };
    let (x, y) = at(samples[0]);
    laid.dispatch_pointer_down(x, y);
    for main in samples.into_iter().skip(1) {
        let (x, y) = at(main);
        laid.dispatch_pointer_move(x, y);
    }
    let (x, y) = at(samples[2]);
    laid.dispatch_pointer_up(x, y);
}

pub(crate) fn nested_fling_hands_remaining_velocity_to_matching_parent_axes() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for axis in [Vertical, Horizontal] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(&outer, &inner, &vsync, axis, axis, false, false),
            tight(300.0, 300.0),
            vsync,
        );
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        release_inner_fling(&laid, axis, false);
        assert_eq!(
            inner.pixels(),
            670.0,
            "{axis:?}: actual inner drag receives the post-threshold move"
        );
        assert_eq!(
            outer.pixels(),
            600.0,
            "parent does not also consume the drag"
        );
        for _ in 0..15 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert_eq!(inner.pixels(), 800.0, "inner hits its real hard extent");
        assert!(
            outer.pixels() > 600.0,
            "{axis:?}: residual fling reaches parent, got {}",
            outer.pixels()
        );
        for _ in 0..500 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            !outer.position().is_scrolling(),
            "parent eventually settles"
        );
        assert!(
            !inner.position().is_scrolling(),
            "inner retires its transferred motion"
        );
    }
}

pub(crate) fn nested_fling_parent_boundary_policy_receives_presentation_pixel_ratio() {
    use flui_animation::Simulation;
    use flui_platform_api::{
        EventTime, PlatformInput,
        pointer::{
            PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
            PointerMove, PointerPosition, PointerPress, PointerRelease, PointerSample,
        },
    };
    use flui_testing::{HeadlessHost, HeadlessWindow};
    use flui_widgets::{ScrollMetrics, ScrollPhysics};

    #[derive(Debug)]
    struct HighDensityPhysics;
    impl ScrollPhysics for HighDensityPhysics {
        fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed: f64) -> f64 {
            if metrics.device_pixel_ratio != 2.0 {
                return metrics.pixels;
            }
            ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed)
        }
        fn create_ballistic_simulation(
            &self,
            metrics: &ScrollMetrics,
            velocity: f64,
        ) -> Option<Box<dyn Simulation>> {
            ClampingScrollPhysics::new().create_ballistic_simulation(metrics, velocity)
        }
        fn boundary_velocity(&self, metrics: &ScrollMetrics, velocity: f64) -> Option<f64> {
            ClampingScrollPhysics::new().boundary_velocity(metrics, velocity)
        }
    }

    for density_sensitive in [false, true] {
        let (outer, inner) = (ScrollController::new(), ScrollController::new());
        let physics: SharedScrollPhysics = if density_sensitive {
            Arc::new(HighDensityPhysics)
        } else {
            Arc::new(ClampingScrollPhysics::new())
        };
        let parent = Scrollable::new()
            .controller(outer.clone())
            .physics(physics)
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(300.0, 600.0).boxed(),
                SizedBox::new(300.0, 200.0)
                    .child(
                        flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT).child(
                            SizedBox::new(200.0, 200.0).child(
                                Scrollable::new()
                                    .controller(inner.clone())
                                    .child(SizedBox::new(300.0, 1000.0)),
                            ),
                        ),
                    )
                    .boxed(),
                SizedBox::new(300.0, 4800.0).boxed(),
            ]));
        let mut host = HeadlessHost::new(HeadlessWindow::new(300, 300));
        host.set_scale_factor(host.primary_window(), 2.0);
        host.attach(&parent)
            .expect("mount dense nested scroll owner");
        let _ = host.pump(Duration::ZERO);
        assert_eq!(inner.position().max_scroll_extent(), 800.0);
        for id in 1_u64..=2 {
            outer.jump_to(600.0);
            inner.jump_to(650.0);
            let _ = host.pump(Duration::ZERO);
            let pointer = PointerInfo::new(
                PointerId::try_from(id).expect("nonzero contact"),
                PointerKind::Mouse,
            );
            let sample = |y: f64| {
                PointerSample::new(
                    EventTime::from_nanos(
                        u64::try_from(host.clock().elapsed().as_nanos())
                            .expect("fixture clock fits event time"),
                    ),
                    PointerPosition::try_new(flui_foundation::geometry::Point::new(100.0, y))
                        .expect("finite authored mouse position"),
                )
            };
            host.dispatch(PlatformInput::Pointer(PointerEvent::Down(
                PointerPress::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::only(PointerButton::PRIMARY),
                    sample(150.0),
                ),
            )));
            for y in [120.0, 100.0] {
                host.clock().advance(Duration::from_millis(8));
                host.dispatch(PlatformInput::Pointer(PointerEvent::Move(
                    PointerMove::new(
                        pointer,
                        PointerButtons::only(PointerButton::PRIMARY),
                        sample(y),
                    ),
                )));
            }
            host.dispatch(PlatformInput::Pointer(PointerEvent::Up(
                PointerRelease::new(
                    pointer,
                    PointerButton::PRIMARY,
                    PointerButtons::NONE,
                    sample(100.0),
                ),
            )));
            assert_eq!(inner.pixels(), 670.0, "actual child release premise");
            assert_eq!(outer.pixels(), 600.0, "parent has not consumed the drag");
            for _ in 0..15 {
                let _ = host.pump(Duration::from_millis(16));
            }
            assert_eq!(inner.pixels(), 800.0, "child reaches its hard edge");
            assert!(
                outer.pixels() > 600.0,
                "density_sensitive={density_sensitive}: the real DPR2 parent's policy accepts the impulse, got {}",
                outer.pixels()
            );
        }
    }
}

pub(crate) fn nested_fling_same_controller_rebuild_preserves_accepted_handoff() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for axis in [Vertical, Horizontal] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(&outer, &inner, &vsync, axis, axis, false, false),
            tight(300.0, 300.0),
            vsync.clone(),
        );
        for rebuild in [false, true, false] {
            outer.jump_to(600.0);
            inner.jump_to(650.0);
            laid.tick();
            release_inner_fling(&laid, axis, false);
            assert_eq!(inner.pixels(), 670.0, "actual post-threshold drag");
            assert_eq!(outer.pixels(), 600.0, "parent has not consumed the drag");
            if rebuild {
                laid.pump_widget(nested_fling_content(
                    &outer, &inner, &vsync, axis, axis, false, false,
                ));
            }
            for _ in 0..15 {
                laid.pump_for(Duration::from_millis(16));
            }
            assert_eq!(inner.pixels(), 800.0, "child reaches its actual extent");
            assert!(
                outer.pixels() > 600.0,
                "{axis:?}, rebuild={rebuild}: same controllers retain accepted handoff, got {}",
                outer.pixels()
            );
        }
    }
}

pub(crate) fn nested_fling_projects_reversed_child_and_preserves_orthogonal_and_bounce_policy() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for (outer_axis, reversed, bouncing, transfers) in [
        (Vertical, true, false, true),
        (Horizontal, false, false, false),
        (Vertical, false, true, false),
    ] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(
                &outer, &inner, &vsync, outer_axis, Vertical, reversed, bouncing,
            ),
            tight(300.0, 300.0),
            vsync,
        );
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        release_inner_fling(&laid, Vertical, reversed);
        assert_eq!(
            inner.pixels(),
            670.0,
            "premise: real child drag moves toward its end after recognition"
        );
        for _ in 0..500 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert_eq!(inner.pixels(), 800.0, "child settles at its actual extent");
        if transfers {
            assert!(
                outer.pixels() < 600.0,
                "reversed child's physical impulse decreases normal parent offset"
            );
        } else {
            assert_eq!(
                outer.pixels(),
                600.0,
                "orthogonal parent or child bounce never receives a fling"
            );
        }
        assert!(!outer.position().is_scrolling());
        assert!(!inner.position().is_scrolling());
    }
}

pub(crate) fn replacing_parent_invalidates_old_fling_handoff_and_next_gesture_recovers() {
    use flui_foundation::geometry::Axis::Vertical;
    let (old, new, inner, vsync) = (
        ScrollController::new(),
        ScrollController::with_initial_scroll_offset(600.0),
        ScrollController::new(),
        Vsync::new(),
    );
    let mut laid = crate::common::lay_out_animated(
        nested_fling_content(&old, &inner, &vsync, Vertical, Vertical, false, false),
        tight(300.0, 300.0),
        vsync.clone(),
    );
    old.jump_to(600.0);
    inner.jump_to(650.0);
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    assert_eq!(inner.pixels(), 670.0);
    laid.pump_widget(nested_fling_content(
        &new, &inner, &vsync, Vertical, Vertical, false, false,
    ));
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        old.pixels(),
        600.0,
        "retired parent cannot receive a later edge impulse"
    );
    assert_eq!(
        new.pixels(),
        600.0,
        "new owner cannot inherit an old accepted handoff"
    );
    inner.jump_to(650.0);
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        new.pixels() > 600.0,
        "new gesture resolves the current parent owner"
    );
    let retired = new.pixels();
    laid.pump_widget(SizedBox::new(300.0, 300.0));
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        new.pixels(),
        retired,
        "unmount retires the transferred trajectory"
    );
}

pub(crate) fn nested_fling_failure_keeps_first_panic_and_a_new_gesture_makes_progress() {
    use flui_foundation::geometry::Axis::Vertical;
    use std::sync::atomic::{AtomicBool, Ordering};
    for competing in [false, true] {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = crate::common::lay_out_animated(
            nested_fling_content(&outer, &inner, &vsync, Vertical, Vertical, false, false),
            tight(300.0, 300.0),
            vsync,
        );
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        let listenable = outer.as_listenable();
        let first = Arc::new(AtomicBool::new(true));
        let fail = Arc::clone(&first);
        let first_id = listenable.add_listener(Arc::new(move || {
            assert!(
                !fail.swap(false, Ordering::SeqCst),
                "first parent handoff notification"
            );
        }));
        let second = Arc::new(AtomicBool::new(competing));
        let fail = Arc::clone(&second);
        let second_id = listenable.add_listener(Arc::new(move || {
            assert!(
                !fail.swap(false, Ordering::SeqCst),
                "second parent handoff notification"
            );
        }));
        release_inner_fling(&laid, Vertical, false);
        let ((), log) = flui_testing::log_capture::capture(|| {
            for _ in 0..15 {
                laid.pump_for(Duration::from_millis(16));
            }
        });
        let expected = if competing {
            vec![
                "first parent handoff notification",
                "second parent handoff notification",
            ]
        } else {
            vec!["first parent handoff notification"]
        };
        assert_notification_failures(&log, &expected);
        assert!(!first.load(Ordering::SeqCst));
        assert!(
            !second.load(Ordering::SeqCst),
            "competing notification cleanup completed"
        );
        assert!(
            outer.pixels() > 600.0,
            "contained notification failures do not stall accepted motion"
        );
        listenable.remove_listener(first_id);
        listenable.remove_listener(second_id);
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        release_inner_fling(&laid, Vertical, false);
        for _ in 0..15 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            outer.pixels() > 600.0,
            "after containment a fresh gesture still hands off"
        );
    }
}

fn assert_notification_failures(log: &flui_testing::log_capture::CapturedLog, expected: &[&str]) {
    let reported: Vec<_> = log
        .records()
        .iter()
        .filter_map(|record| record.field("panic_payload"))
        .collect();
    assert_eq!(
        reported, expected,
        "the actual notifier reports the first failure before competing failures: {log}"
    );
}

pub(crate) fn nested_fling_equal_edge_jump_cancels_old_handoff_and_next_gesture_recovers() {
    use flui_foundation::geometry::Axis::Vertical;
    use std::sync::atomic::{AtomicBool, Ordering};
    let (outer, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let mut laid = crate::common::lay_out_animated(
        nested_fling_content(&outer, &inner, &vsync, Vertical, Vertical, false, false),
        tight(300.0, 300.0),
        vsync,
    );
    outer.jump_to(600.0);
    inner.jump_to(650.0);
    laid.tick();
    let pending_jump = Arc::new(AtomicBool::new(true));
    let jump = Arc::clone(&pending_jump);
    let controller = inner.clone();
    let listenable = inner.as_listenable();
    let listener = listenable.add_listener(Arc::new(move || {
        if controller.pixels() == 800.0 && jump.swap(false, Ordering::SeqCst) {
            // Explicit programmatic cancellation must win even when it leaves
            // the already-committed edge pixels unchanged.
            controller.jump_to(800.0);
        }
    }));
    release_inner_fling(&laid, Vertical, false);
    assert_eq!(
        inner.pixels(),
        670.0,
        "actual release retains its accepted velocity"
    );
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        !pending_jump.load(Ordering::SeqCst),
        "edge notification actually reentered jump_to"
    );
    assert_eq!(inner.pixels(), 800.0);
    assert_eq!(
        outer.pixels(),
        600.0,
        "the explicit equal-edge jump cancels the older residual impulse"
    );
    listenable.remove_listener(listener);
    inner.jump_to(650.0);
    outer.jump_to(600.0);
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        outer.pixels() > 600.0,
        "a fresh gesture can still hand off after cancellation"
    );
}

pub(crate) fn nested_fling_skips_saturated_parent_and_reentrant_jump_retires_transfer() {
    let (outer, middle, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let leaf = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(200.0, 1000.0));
    let middle_view =
        Scrollable::new()
            .controller(middle.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(200.0, 800.0).boxed(),
                SizedBox::new(200.0, 200.0).child(leaf).boxed(),
            ]));
    let outer_view = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 600.0).boxed(),
            SizedBox::new(200.0, 200.0).child(middle_view).boxed(),
            SizedBox::new(200.0, 4800.0).boxed(),
        ]));
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), outer_view),
        tight(200.0, 200.0),
        vsync,
    );
    outer.jump_to(600.0);
    middle.jump_to(800.0);
    inner.jump_to(650.0);
    laid.tick();
    release_inner_fling(&laid, flui_foundation::geometry::Axis::Vertical, false);
    assert_eq!(
        inner.pixels(),
        670.0,
        "leaf claimed the post-threshold move"
    );
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        middle.pixels(),
        800.0,
        "saturated intermediate ancestor stays clamped"
    );
    assert!(
        outer.pixels() > 600.0,
        "remaining impulse reaches the nearest willing ancestor"
    );

    outer.jump_to(600.0);
    inner.jump_to(650.0);
    laid.tick();
    let once = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let entered = Arc::clone(&once);
    let reentrant = outer.clone();
    let listenable = outer.as_listenable();
    let listener = listenable.add_listener(Arc::new(move || {
        if entered.swap(false, std::sync::atomic::Ordering::SeqCst) {
            reentrant.jump_to(123.0);
        }
    }));
    release_inner_fling(&laid, flui_foundation::geometry::Axis::Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        !once.load(std::sync::atomic::Ordering::SeqCst),
        "real transferred motion invoked the reentrant listener"
    );
    assert_eq!(
        outer.pixels(),
        123.0,
        "reentrant jump cancels the accepted trajectory"
    );
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        outer.pixels(),
        123.0,
        "no older handoff overwrites the new position"
    );
    listenable.remove_listener(listener);
    outer.jump_to(600.0);
    inner.jump_to(650.0);
    laid.tick();
    release_inner_fling(&laid, flui_foundation::geometry::Axis::Vertical, false);
    for _ in 0..15 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        outer.pixels() > 600.0,
        "fresh handoff recovers after reentry"
    );
}

fn reveal_target_content(axis: flui_foundation::geometry::Axis) -> flui_view::BoxedView {
    reveal_target_content_at(axis, 600.0)
}

pub(crate) fn nested_fling_bouncing_parent_at_extent_absorbs_before_grandparent() {
    use flui_foundation::geometry::Axis::Vertical;
    let (outer, middle, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let leaf = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(200.0, 1000.0));
    let middle_view = Scrollable::new()
        .controller(middle.clone())
        .physics(Arc::new(BouncingScrollPhysics::new()))
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 800.0).boxed(),
            SizedBox::new(200.0, 200.0).child(leaf).boxed(),
        ]));
    let outer_view = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 600.0).boxed(),
            SizedBox::new(200.0, 200.0).child(middle_view).boxed(),
            SizedBox::new(200.0, 4800.0).boxed(),
        ]));
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), outer_view),
        tight(200.0, 200.0),
        vsync,
    );
    outer.jump_to(600.0);
    middle.jump_to(800.0);
    inner.jump_to(650.0);
    laid.tick();
    assert_eq!(
        middle.position().max_scroll_extent(),
        800.0,
        "parent starts at its outward edge"
    );
    release_inner_fling(&laid, Vertical, false);
    assert_eq!(
        inner.pixels(),
        670.0,
        "actual leaf claimed the post-threshold move"
    );
    let mut parent_overscrolled = false;
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
        parent_overscrolled |= middle.pixels() > 800.0;
        assert_eq!(
            outer.pixels(),
            600.0,
            "a willing bouncing parent absorbs before grandparent"
        );
    }
    assert!(
        parent_overscrolled,
        "actual parent ballistic physics received the boundary impulse"
    );
    assert_eq!(inner.pixels(), 800.0);
    for _ in 0..500 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        middle.pixels(),
        800.0,
        "absorbed impulse springs back locally"
    );
    assert_eq!(outer.pixels(), 600.0);
    inner.jump_to(650.0);
    middle.jump_to(800.0);
    laid.tick();
    release_inner_fling(&laid, Vertical, false);
    let mut recovered = false;
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
        recovered |= middle.pixels() > 800.0;
    }
    assert!(
        recovered,
        "next real gesture still reaches the parent physics"
    );
    assert_eq!(outer.pixels(), 600.0);
}

fn reveal_target_content_at(
    axis: flui_foundation::geometry::Axis,
    before: f64,
) -> flui_view::BoxedView {
    use flui_foundation::geometry::Axis;
    let target = flui_widgets::Semantics::new()
        .container(true)
        .button(true)
        .label("reveal target")
        .child(SizedBox::new(40.0, 40.0))
        .boxed();
    match axis {
        Axis::Vertical => flui_widgets::Column::new(vec![
            SizedBox::new(200.0, before).boxed(),
            target,
            SizedBox::new(200.0, 600.0).boxed(),
        ])
        .boxed(),
        Axis::Horizontal => flui_widgets::Row::new(vec![
            SizedBox::new(before, 200.0).boxed(),
            target,
            SizedBox::new(600.0, 200.0).boxed(),
        ])
        .boxed(),
    }
}

fn request_reveal_target(laid: &LaidOut) {
    use flui_testing::a11y::{Action, ActionRequest, TreeId};
    let tree = laid.a11y_tree().expect("actual published semantics");
    let target = tree
        .find_by_label("reveal target")
        .expect("offscreen target remains addressable");
    let advertised = target.supports_action(Action::ScrollIntoView);
    assert!(
        !target.raw().is_hidden(),
        "an offered offscreen target must survive native hidden-subtree filtering"
    );
    laid.invoke_semantics_action(ActionRequest {
        action: Action::ScrollIntoView,
        target_tree: TreeId::ROOT,
        target_node: target.id(),
        data: None,
    })
    .expect("actual ScrollIntoView request resolves through ancestor scrollables");
    assert!(
        advertised,
        "automatic reveal must also be offered to the native adapter"
    );
}

fn assert_reveal_label_fixture_is_visible(axis: flui_foundation::geometry::Axis) {
    let mut visible = lay_out(
        Scrollable::new()
            .scroll_direction(axis)
            .child(reveal_target_content_at(axis, 0.0)),
        tight(200.0, 200.0),
    );
    visible.enable_semantics();
    visible.tick();
    assert_reveal_target_visible(&visible);
}

fn assert_reveal_target_visible(laid: &LaidOut) {
    let tree = laid.a11y_tree().expect("republished semantics");
    let rect = tree
        .find_by_label("reveal target")
        .expect("same target")
        .bounds()
        .expect("actual target geometry");
    assert!(
        rect.x0 >= -1e-9 && rect.y0 >= -1e-9 && rect.x1 <= 200.0 + 1e-9 && rect.y1 <= 200.0 + 1e-9,
        "requested node is visibly inside the receiving viewport: {rect:?}"
    );
}

pub(crate) fn show_on_screen_reveals_offscreen_targets_on_both_axes_and_reverse() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for axis in [Vertical, Horizontal] {
        assert_reveal_label_fixture_is_visible(axis);
        for reversed in [false, true] {
            let scroll = ScrollController::new();
            let mut view = Scrollable::new()
                .controller(scroll.clone())
                .scroll_direction(axis)
                .child(reveal_target_content(axis));
            if reversed {
                view = view
                    .axis_direction(match axis {
                        Vertical => flui_rendering::constraints::AxisDirection::BottomToTop,
                        Horizontal => flui_rendering::constraints::AxisDirection::RightToLeft,
                    })
                    .viewport_builder(Rc::new(move |position| {
                        flui_widgets::SingleChildScrollView::new()
                            .scroll_direction(axis)
                            .reverse(true)
                            .position(position)
                            .child(reveal_target_content(axis))
                            .boxed()
                    }));
            }
            let mut laid = lay_out(view, tight(200.0, 200.0));
            laid.enable_semantics();
            laid.tick();
            let before = laid.a11y_tree().expect("tree");
            let rect = before
                .find_by_label("reveal target")
                .expect("offscreen node")
                .bounds()
                .expect("node bounds");
            assert!(
                rect.x0 < 0.0 || rect.y0 < 0.0 || rect.x1 > 200.0 || rect.y1 > 200.0,
                "premise: target is offscreen {axis:?}/{reversed}: {rect:?}"
            );
            request_reveal_target(&laid);
            laid.tick();
            assert!(
                scroll.pixels() > 0.0,
                "real viewport moved toward its offscreen target"
            );
            assert_reveal_target_visible(&laid);
            let revealed = scroll.pixels();
            request_reveal_target(&laid);
            laid.tick();
            assert_eq!(
                scroll.pixels(),
                revealed,
                "already-visible reveal is idempotent"
            );
            scroll.jump_to(0.0);
            laid.tick();
            request_reveal_target(&laid);
            laid.tick();
            assert_reveal_target_visible(&laid);
        }
    }
}

pub(crate) fn show_on_screen_walks_nested_axes_and_replacement_uses_current_geometry() {
    use flui_foundation::geometry::Axis::{Horizontal, Vertical};
    for inner_axis in [Vertical, Horizontal] {
        assert_reveal_label_fixture_is_visible(inner_axis);
        let (outer, inner, replacement) = (
            ScrollController::new(),
            ScrollController::new(),
            ScrollController::new(),
        );
        let content = |controller: &ScrollController| {
            Scrollable::new()
                .controller(outer.clone())
                .child(flui_widgets::Column::new(vec![
                    SizedBox::new(200.0, 600.0).boxed(),
                    SizedBox::new(200.0, 200.0)
                        .child(
                            Scrollable::new()
                                .controller(controller.clone())
                                .scroll_direction(inner_axis)
                                .child(reveal_target_content(inner_axis)),
                        )
                        .boxed(),
                    SizedBox::new(200.0, 800.0).boxed(),
                ]))
        };
        let mut laid = lay_out(content(&inner), tight(200.0, 200.0));
        laid.enable_semantics();
        laid.tick();
        request_reveal_target(&laid);
        laid.tick();
        assert_eq!(
            inner.pixels(),
            440.0,
            "inner reveals its nearest target edge"
        );
        assert_eq!(
            outer.pixels(),
            600.0,
            "outer reveals the resulting inner viewport"
        );
        assert_reveal_target_visible(&laid);
        outer.jump_to(0.0);
        laid.pump_widget(content(&replacement));
        request_reveal_target(&laid);
        laid.tick();
        assert_eq!(
            inner.pixels(),
            440.0,
            "retired controller is not driven by a new request"
        );
        assert_eq!(
            replacement.pixels(),
            440.0,
            "replacement gets its real target geometry"
        );
        assert_eq!(outer.pixels(), 600.0);
        assert_reveal_target_visible(&laid);
    }
}

fn nested_reveal_content(outer: &ScrollController, inner: &ScrollController) -> Scrollable {
    Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(200.0, 600.0).boxed(),
            SizedBox::new(200.0, 200.0)
                .child(
                    Scrollable::new()
                        .controller(inner.clone())
                        .child(reveal_target_content(
                            flui_foundation::geometry::Axis::Vertical,
                        )),
                )
                .boxed(),
            SizedBox::new(200.0, 800.0).boxed(),
        ]))
}

pub(crate) fn show_on_screen_failure_continues_live_ancestors_and_fresh_requests_recover() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for competing in [false, true] {
        let (outer, inner) = (ScrollController::new(), ScrollController::new());
        let mut laid = lay_out(nested_reveal_content(&outer, &inner), tight(200.0, 200.0));
        laid.enable_semantics();
        laid.tick();
        let first = Arc::new(AtomicBool::new(true));
        let failed = Arc::clone(&first);
        let inner_listenable = inner.as_listenable();
        let first_id = inner_listenable.add_listener(Arc::new(move || {
            assert!(
                !failed.swap(false, Ordering::SeqCst),
                "first inner reveal notification"
            );
        }));
        let second = Arc::new(AtomicBool::new(competing));
        let failed = Arc::clone(&second);
        let outer_listenable = outer.as_listenable();
        let second_id = outer_listenable.add_listener(Arc::new(move || {
            assert!(
                !failed.swap(false, Ordering::SeqCst),
                "second outer reveal notification"
            );
        }));
        let ((), log) = flui_testing::log_capture::capture(|| {
            request_reveal_target(&laid);
        });
        let expected = if competing {
            vec![
                "first inner reveal notification",
                "second outer reveal notification",
            ]
        } else {
            vec!["first inner reveal notification"]
        };
        assert_notification_failures(&log, &expected);
        assert!(!first.load(Ordering::SeqCst));
        assert!(
            !second.load(Ordering::SeqCst),
            "live ancestor delivery completed"
        );
        assert_eq!(inner.pixels(), 440.0);
        assert_eq!(
            outer.pixels(),
            600.0,
            "inner failure cannot starve the enclosing viewport"
        );
        inner_listenable.remove_listener(first_id);
        outer_listenable.remove_listener(second_id);
        inner.jump_to(0.0);
        outer.jump_to(0.0);
        laid.tick();
        request_reveal_target(&laid);
        laid.tick();
        assert_eq!(inner.pixels(), 440.0);
        assert_eq!(outer.pixels(), 600.0);
        assert_reveal_target_visible(&laid);
    }
}

pub(crate) fn show_on_screen_same_pipeline_reentry_keeps_one_reveal_and_recovers() {
    use flui_rendering::semantics::{
        AccessibilityNodeId, SemanticsAction, SemanticsActionInvocation, SemanticsActionRequest,
    };
    use std::cell::RefCell;
    thread_local! {
        static PENDING_REVEAL: RefCell<Option<SemanticsActionInvocation>> = const { RefCell::new(None) };
    }
    struct RestoreScope(Option<SemanticsActionInvocation>);
    impl Drop for RestoreScope {
        fn drop(&mut self) {
            let outgoing = PENDING_REVEAL.with(|slot| slot.replace(self.0.take()));
            drop(outgoing);
        }
    }
    let (outer, inner) = (ScrollController::new(), ScrollController::new());
    let mut laid = lay_out(nested_reveal_content(&outer, &inner), tight(200.0, 200.0));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("actual tree");
    let target = tree
        .find_by_label("reveal target")
        .expect("addressable target")
        .id();
    let invocation = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(target.0)
                    .expect("published nonzero identity"),
                action: SemanticsAction::ShowOnScreen,
                arguments: None,
            })
        })
        .expect("actual automatic ancestor reveal");
    let _scope = RestoreScope(PENDING_REVEAL.with(|slot| slot.replace(Some(invocation))));
    let listenable = inner.as_listenable();
    let listener = listenable.add_listener(Arc::new(|| {
        // The notification runs on the same owner thread. Resolve its local
        // pending request without making that request cross-thread ownership.
        let invocation = PENDING_REVEAL.with(|slot| slot.borrow_mut().take());
        if let Some(invocation) = invocation {
            invocation.invoke();
        }
    }));
    request_reveal_target(&laid);
    laid.tick();
    assert!(
        PENDING_REVEAL.with(|slot| slot.borrow().is_none()),
        "same-pipeline request actually reentered"
    );
    assert_eq!(
        inner.pixels(),
        440.0,
        "reentry cannot apply old reveal geometry twice"
    );
    assert_eq!(outer.pixels(), 600.0);
    assert_reveal_target_visible(&laid);
    listenable.remove_listener(listener);
    inner.jump_to(0.0);
    outer.jump_to(0.0);
    laid.tick();
    request_reveal_target(&laid);
    laid.tick();
    assert_eq!(inner.pixels(), 440.0);
    assert_eq!(outer.pixels(), 600.0);
    assert_reveal_target_visible(&laid);
}

pub(crate) fn show_on_screen_sibling_reentry_delivers_last_target_without_stale_motion() {
    use flui_rendering::semantics::{
        AccessibilityNodeId, SemanticsAction, SemanticsActionInvocation, SemanticsActionRequest,
    };
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    thread_local! {
        static PENDING_SIBLING: RefCell<Option<SemanticsActionInvocation>> = const { RefCell::new(None) };
    }
    struct RestoreScope(Option<SemanticsActionInvocation>);
    impl Drop for RestoreScope {
        fn drop(&mut self) {
            let outgoing = PENDING_SIBLING.with(|slot| slot.replace(self.0.take()));
            drop(outgoing);
        }
    }
    let content = |controller: &ScrollController, before, gap| {
        let target = |label| {
            flui_widgets::Semantics::new()
                .container(true)
                .button(true)
                .label(label)
                .child(SizedBox::new(40.0, 40.0))
                .boxed()
        };
        Scrollable::new()
            .controller(controller.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(200.0, before).boxed(),
                target("first sibling"),
                SizedBox::new(200.0, gap).boxed(),
                target("second sibling"),
                SizedBox::new(200.0, 800.0).boxed(),
            ]))
    };
    // Both labelled nodes from the same constructor are visible and native
    // addressable when their actual layout positions fit the viewport.
    let mut visible = lay_out(
        content(&ScrollController::new(), 0.0, 40.0),
        tight(200.0, 200.0),
    );
    visible.enable_semantics();
    visible.tick();
    for label in ["first sibling", "second sibling"] {
        let tree = visible.a11y_tree().expect("visible control tree");
        let target = tree.find_by_label(label).expect("visible labelled sibling");
        assert!(!target.raw().is_hidden());
        assert!(target.supports_action(flui_testing::a11y::Action::ScrollIntoView));
        let bounds = target.bounds().expect("actual visible geometry");
        assert!(bounds.y0 >= 0.0 && bounds.y1 <= 200.0);
    }
    for first_is_nearer in [true, false] {
        let controller = ScrollController::new();
        let mut laid = lay_out(content(&controller, 600.0, 360.0), tight(200.0, 200.0));
        laid.enable_semantics();
        laid.tick();
        let (first_label, last_label, expected) = if first_is_nearer {
            ("first sibling", "second sibling", 840.0)
        } else {
            ("second sibling", "first sibling", 600.0)
        };
        let tree = laid.a11y_tree().expect("actual offscreen tree");
        let first = tree
            .find_by_label(first_label)
            .expect("first addressable sibling")
            .id();
        let last = tree
            .find_by_label(last_label)
            .expect("last addressable sibling")
            .id();
        let invocation = laid
            .pipeline_owner()
            .with(|owner| {
                owner.resolve_semantics_action(SemanticsActionRequest::new(
                    AccessibilityNodeId::from_u64(last.0).expect("published identity"),
                    SemanticsAction::ShowOnScreen,
                ))
            })
            .expect("actual pending sibling reveal");
        let _scope = RestoreScope(PENDING_SIBLING.with(|slot| slot.replace(Some(invocation))));
        let notifications = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&notifications);
        let listenable = controller.as_listenable();
        let listener = listenable.add_listener(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            let invocation = PENDING_SIBLING.with(|slot| slot.borrow_mut().take());
            if let Some(invocation) = invocation {
                invocation.invoke();
            }
        }));
        laid.invoke_semantics_action(flui_testing::a11y::ActionRequest {
            action: flui_testing::a11y::Action::ScrollIntoView,
            target_tree: flui_testing::a11y::TreeId::ROOT,
            target_node: first,
            data: None,
        })
        .expect("first accepted native reveal");
        laid.tick();
        assert!(
            PENDING_SIBLING.with(|slot| slot.borrow().is_none()),
            "sibling request actually reentered"
        );
        assert!(
            notifications.load(Ordering::SeqCst) >= 2,
            "both accepted targets caused actual movement"
        );
        assert_eq!(
            controller.pixels(),
            expected,
            "last sibling wins without reapplying cached geometry: {first_label} then {last_label}"
        );
        let current = laid.a11y_tree().expect("republished geometry");
        let bounds = current
            .find_by_label(last_label)
            .expect("last target remains present")
            .bounds()
            .expect("actual last target geometry");
        assert!(
            bounds.y0 >= -1e-9 && bounds.y1 <= 200.0 + 1e-9,
            "last accepted target is visible: {bounds:?}"
        );
        listenable.remove_listener(listener);
        controller.jump_to(0.0);
        laid.tick();
        let current = laid.a11y_tree().expect("fresh geometry");
        let target = current
            .find_by_label(last_label)
            .expect("fresh target")
            .id();
        laid.invoke_semantics_action(flui_testing::a11y::ActionRequest {
            action: flui_testing::a11y::Action::ScrollIntoView,
            target_tree: flui_testing::a11y::TreeId::ROOT,
            target_node: target,
            data: None,
        })
        .expect("fresh request remains deliverable");
        laid.tick();
        assert_eq!(
            controller.pixels(),
            if first_is_nearer { 840.0 } else { 440.0 }
        );
    }
}

pub(crate) fn nested_fling_custom_physics_failure_and_retirement_preserve_first_and_recover() {
    use flui_animation::Simulation;
    use flui_foundation::geometry::Axis::Vertical;
    use flui_widgets::{ScrollMetrics, ScrollPhysics};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct RetiringSimulation {
        motion: Box<dyn Simulation>,
        fail_drop: Arc<AtomicBool>,
    }
    impl Simulation for RetiringSimulation {
        fn x(&self, time: f64) -> f64 {
            self.motion.x(time)
        }
        fn dx(&self, time: f64) -> f64 {
            self.motion.dx(time)
        }
        fn is_done(&self, time: f64) -> bool {
            self.motion.is_done(time)
        }
    }
    impl Drop for RetiringSimulation {
        fn drop(&mut self) {
            assert!(
                !self.fail_drop.swap(false, Ordering::SeqCst),
                "custom parent simulation retirement failure"
            );
        }
    }
    #[derive(Debug)]
    struct FaultPhysics {
        fail_boundary: Arc<AtomicBool>,
        fail_drop: Arc<AtomicBool>,
    }
    impl ScrollPhysics for FaultPhysics {
        fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed: f64) -> f64 {
            ClampingScrollPhysics::new().apply_boundary_conditions(metrics, proposed)
        }
        fn create_ballistic_simulation(
            &self,
            metrics: &ScrollMetrics,
            velocity: f64,
        ) -> Option<Box<dyn Simulation>> {
            Some(Box::new(RetiringSimulation {
                motion: ClampingScrollPhysics::new()
                    .create_ballistic_simulation(metrics, velocity)?,
                fail_drop: Arc::clone(&self.fail_drop),
            }))
        }
        fn boundary_velocity(&self, metrics: &ScrollMetrics, velocity: f64) -> Option<f64> {
            assert!(
                !self.fail_boundary.swap(false, Ordering::SeqCst),
                "custom parent boundary failure"
            );
            ClampingScrollPhysics::new().boundary_velocity(metrics, velocity)
        }
    }

    for (boundary_fault, retirement_fault) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let fail_boundary = Arc::new(AtomicBool::new(boundary_fault));
        let fail_drop = Arc::new(AtomicBool::new(retirement_fault));
        let parent = Scrollable::new()
            .controller(outer.clone())
            .physics(Arc::new(FaultPhysics {
                fail_boundary: Arc::clone(&fail_boundary),
                fail_drop: Arc::clone(&fail_drop),
            }))
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(200.0, 600.0).boxed(),
                SizedBox::new(200.0, 200.0)
                    .child(
                        Scrollable::new()
                            .controller(inner.clone())
                            .child(SizedBox::new(200.0, 1000.0)),
                    )
                    .boxed(),
                SizedBox::new(200.0, 4800.0).boxed(),
            ]));
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(vsync.clone(), parent),
            tight(200.0, 200.0),
            vsync,
        );
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        release_inner_fling(&laid, Vertical, false);
        assert_eq!(inner.pixels(), 670.0);
        let mut failure = None;
        for _ in 0..500 {
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                laid.pump_for(Duration::from_millis(16));
            })) {
                failure = Some(payload);
                break;
            }
        }
        if boundary_fault || retirement_fault {
            let payload = failure.expect("the actual parent physics/retirement callback must run");
            let text = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .expect("the frame reports its first failure text");
            let expected = if boundary_fault {
                "custom parent boundary failure"
            } else {
                "custom parent simulation retirement failure"
            };
            assert!(
                text.contains(expected),
                "the first failure remains authoritative: {text}"
            );
        } else {
            assert!(failure.is_none(), "healthy custom physics completes");
            assert!(
                outer.pixels() > 600.0,
                "actual custom parent receives the impulse"
            );
        }
        assert!(!fail_boundary.load(Ordering::SeqCst));
        // Faulted ownership may be retained instead of invoking another
        // destructor during unwind. Disable that fault for the fresh operation.
        fail_drop.store(false, Ordering::SeqCst);
        inner.jump_to(650.0);
        outer.jump_to(600.0);
        laid.tick();
        release_inner_fling(&laid, Vertical, false);
        for _ in 0..15 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(
            outer.pixels() > 600.0,
            "fresh transfer makes progress after containment"
        );
    }
}
