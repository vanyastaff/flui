//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/scrollable_test.dart`
//! (tag `3.44.0`).
//!
//! The large majority of this ~1800-line file is out of scope for this
//! headless, geometry-only harness:
//! - Platform-specific momentum-carry heuristics (Android "no momentum
//!   build", iOS/macOS drag-threshold attenuation and momentum carry/kill) —
//!   `Scrollable`'s pan recognizer varies its drag-slop by pointer kind
//!   (`GestureDetector`'s `DragGestureRecognizer`, mouse vs. touch — see
//!   `crates/flui-interaction/src/recognizers/drag.rs`), but has no
//!   platform-conditional (Android/iOS/macOS) physics variant selection.
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
use flui_view::ViewExt;
use flui_widgets::prelude::{Axis, AxisDirection};
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, Directionality, ScrollController, Scrollable,
    SharedScrollPhysics, SizedBox, SliverFixedExtentList, Viewport, VsyncScope,
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

// ============================================================================
// Explicit axis_direction override
// ============================================================================
//
// `Scrollable::axis_direction` lets a `.viewport_builder` composition hand
// `Scrollable` a pre-resolved `AxisDirection` directly, bypassing ambient
// `Directionality` resolution entirely — the one case
// `Scrollable::build`'s own fallback (`view.axis_direction.unwrap_or_else`,
// which only ever consults `Directionality`) structurally cannot serve:
// composed content whose `AxisDirection` doesn't reduce to "ambient
// Directionality + scroll_direction + reverse: false". Both tests below set
// the override to a value CONTRADICTING what ambient resolution would
// produce in that same scene, so a silent fallback to ambient (dropping
// `view.axis_direction`) flips the sign and fails with the OTHER case's
// expected value — verified by that exact falsification on both tests
// before this comment was written.

/// The drag-delta half. `.axis_direction(LeftToRight)` under an RTL
/// `Directionality` ancestor (ambient alone would resolve `RightToLeft`)
/// must still orient by `LeftToRight` (not reversed). `.axis_direction(
/// RightToLeft)` with NO `Directionality` ancestor at all (ambient would
/// default to `LeftToRight`) must still orient by `RightToLeft` (reversed).
/// Composed via `.viewport_builder`, `Scrollable::axis_direction`'s actual
/// reason to exist, over the same `Viewport`-over-`SliverFixedExtentList`
/// fixture `scrollable_viewport_builder_composes_a_custom_viewport_with_working_drag_and_feedback`
/// (`tests/scroll.rs`) uses, oriented horizontally instead of vertically.
///
/// Falsified by replacing `Scrollable::build`'s `view.axis_direction
/// .unwrap_or_else(...)` with the unconditional ambient resolution (i.e.
/// dropping `view.axis_direction`): the RTL-ambient case (override
/// `LeftToRight`) landed on 100.0 — the ambient-derived `RightToLeft`
/// value — instead of 200.0, and the no-ambient case (override
/// `RightToLeft`) landed on 200.0 — the ambient-derived `LeftToRight` value
/// — instead of 100.0. Each run produced exactly the OTHER case's expected
/// value, confirming both assertions exercise the override, not the
/// fallback. Restored byte-identically; both green again.
#[test]
fn scrollable_axis_direction_override_governs_the_drag_delta_not_ambient_directionality() {
    fn dragged_pixels(explicit: AxisDirection, ambient: Option<TextDirection>) -> f32 {
        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 300.0);
        controller.set_pixels(150.0);

        let scrollable = Scrollable::new()
            .controller(controller.clone())
            .scroll_direction(Axis::Horizontal)
            .axis_direction(explicit)
            .viewport_builder(std::rc::Rc::new(move |position| {
                let cols: Vec<_> = (0..12)
                    .map(|_| SizedBox::new(50.0, 300.0).boxed())
                    .collect();
                Viewport::new((SliverFixedExtentList::new(50.0, cols),))
                    .axis_direction(explicit)
                    .position(position)
                    .boxed()
            }));

        let scoped = match ambient {
            Some(direction) => lay_out(
                Directionality::new(direction, scrollable),
                tight(300.0, 300.0),
            ),
            None => lay_out(scrollable, tight(300.0, 300.0)),
        };

        // Single 50px leftward drag: no competing recognizer, delivered in
        // full — same pattern used throughout this file.
        scoped.dispatch_pointer_down(200.0, 150.0);
        scoped.dispatch_pointer_move(150.0, 150.0);
        scoped.dispatch_pointer_up(150.0, 150.0);

        controller.pixels()
    }

    // Override LeftToRight; ambient Directionality is RTL (would resolve
    // RightToLeft if the override were ignored). A 50px leftward drag under
    // (not reversed) LeftToRight increases pixels: 150 -> 200.
    let ltr_override_under_rtl_ambient =
        dragged_pixels(AxisDirection::LeftToRight, Some(TextDirection::Rtl));
    assert_eq!(
        ltr_override_under_rtl_ambient, 200.0,
        "an explicit LeftToRight override under an RTL Directionality ancestor must orient \
         by LeftToRight (not reversed), not the ambient RightToLeft; got \
         {ltr_override_under_rtl_ambient:.1}"
    );

    // Override RightToLeft; no Directionality ancestor at all (ambient
    // would default to LeftToRight if the override were ignored). The SAME
    // 50px leftward drag under (reversed) RightToLeft decreases pixels:
    // 150 -> 100.
    let rtl_override_under_no_ambient = dragged_pixels(AxisDirection::RightToLeft, None);
    assert_eq!(
        rtl_override_under_no_ambient, 100.0,
        "an explicit RightToLeft override with no Directionality ancestor must orient by \
         RightToLeft (reversed), not the ambient default LeftToRight; got \
         {rtl_override_under_no_ambient:.1}"
    );
}

/// The fling-velocity half, same override-vs-ambient-contradiction shape as
/// the drag test above. Composed over a longer strip (100 columns, 5000px
/// content, 4700px `max_scroll_extent`) so a two-frame fling settle window
/// never approaches either boundary.
///
/// Falsified the same way as the drag test: dropping `view.axis_direction`
/// from `Scrollable::build` made the RTL-ambient case (override
/// `LeftToRight`) DECREASE after release instead of increase, and the
/// no-ambient case (override `RightToLeft`) INCREASE instead of decrease —
/// each flipping to the other case's expected direction. Restored
/// byte-identically; both green again.
#[test]
fn scrollable_axis_direction_override_governs_the_fling_velocity_not_ambient_directionality() {
    fn fling_pixels(explicit: AxisDirection, ambient: Option<TextDirection>) -> (f32, f32, f32) {
        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 4700.0);
        controller.set_pixels(2000.0);

        let vsync = Vsync::new();
        let scrollable = Scrollable::new()
            .controller(controller.clone())
            .scroll_direction(Axis::Horizontal)
            .axis_direction(explicit)
            .viewport_builder(std::rc::Rc::new(move |position| {
                let cols: Vec<_> = (0..100)
                    .map(|_| SizedBox::new(50.0, 300.0).boxed())
                    .collect();
                Viewport::new((SliverFixedExtentList::new(50.0, cols),))
                    .axis_direction(explicit)
                    .position(position)
                    .boxed()
            }));
        let wrapped = VsyncScope::new(vsync.clone(), scrollable);

        let mut scoped = match ambient {
            Some(direction) => {
                lay_out(Directionality::new(direction, wrapped), tight(300.0, 300.0))
            }
            None => lay_out(wrapped, tight(300.0, 300.0)),
        };
        scoped.adopt_vsync(vsync);

        let start_pixels = controller.pixels();

        // Leftward drag well past the 18px slop, same magnitude as the
        // ambient-Directionality fling test above.
        scoped.dispatch_pointer_down(250.0, 150.0);
        scoped.dispatch_pointer_move(150.0, 150.0); // 100px leftward: slop-crossing
        scoped.dispatch_pointer_move(100.0, 150.0); // 50px more: on_pan_update
        scoped.dispatch_pointer_up(100.0, 150.0);

        let pixels_at_release = controller.pixels();

        scoped.pump_for(Duration::from_millis(16));
        scoped.pump_for(Duration::from_millis(16));

        (start_pixels, pixels_at_release, controller.pixels())
    }

    // Override LeftToRight under an RTL ambient: must behave as the
    // non-reversed control (increase, then keep increasing).
    let (ltr_start, ltr_at_release, ltr_after_pump) =
        fling_pixels(AxisDirection::LeftToRight, Some(TextDirection::Rtl));
    assert!(
        ltr_at_release > ltr_start,
        "an explicit LeftToRight override under an RTL Directionality ancestor must \
         increase pixels before release, not decrease like the ambient RightToLeft would \
         ({ltr_start} -> {ltr_at_release})"
    );
    assert!(
        ltr_after_pump > ltr_at_release,
        "the LeftToRight override's fling must keep advancing (increasing) past release; \
         release={ltr_at_release:.1}, after_pump={ltr_after_pump:.1}"
    );

    // Override RightToLeft with no Directionality ancestor: must behave as
    // the reversed control (decrease, then keep decreasing), not the
    // ambient default LeftToRight.
    let (rtl_start, rtl_at_release, rtl_after_pump) =
        fling_pixels(AxisDirection::RightToLeft, None);
    assert!(
        rtl_at_release < rtl_start,
        "an explicit RightToLeft override with no Directionality ancestor must decrease \
         pixels before release, not increase like the ambient default LeftToRight would \
         ({rtl_start} -> {rtl_at_release})"
    );
    assert!(
        rtl_after_pump < rtl_at_release,
        "the RightToLeft override's fling must keep advancing (decreasing) past release; \
         release={rtl_at_release:.1}, after_pump={rtl_after_pump:.1}"
    );
}

// ── The viewport does not rebuild on scroll ────────────────────────────────

/// A view that counts its own builds, so a test can tell a re-layout from a
/// rebuild of the subtree.
#[derive(Clone)]
struct BuildCounter {
    builds: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl flui_view::view::StatelessView for BuildCounter {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SizedBox::new(300.0, 2000.0)
    }
}

impl flui_view::View for BuildCounter {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// Scrolling must not rebuild the viewport's subtree.
///
/// `RenderViewport` subscribes to the offset and marks itself needing layout
/// (`crates/flui-objects/src/sliver/viewport.rs`, Flutter's
/// `offset.addListener(markNeedsLayout)`), so a scroll is a layout event, not
/// a build event. `Scrollable` used to *also* wrap the whole viewport in an
/// `AnimatedBuilder` on the controller's notify, so every `set_pixels`
/// re-created every sliver view underneath it — the delegate closures
/// included, which is why a builder delegate's residents were re-consulted on
/// every scroll pixel.
///
/// The oracle is the build count, not the pixel value: a scroll that moved
/// content while rebuilding nothing is the whole point, so the test asserts
/// both.
#[test]
fn scrolling_relayouts_the_viewport_without_rebuilding_its_subtree() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 2000.0);
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let scrollable = Scrollable::new()
        .controller(controller.clone())
        .child(BuildCounter {
            builds: std::sync::Arc::clone(&builds),
        });
    let mut laid = lay_out(scrollable, tight(300.0, 300.0));

    let after_mount = builds.load(std::sync::atomic::Ordering::SeqCst);
    assert!(after_mount >= 1, "the content built at least once on mount");

    // `tick` drives a frame WITHOUT dirtying the root, so it rebuilds only
    // what a listenable actually scheduled into the build inbox. `pump` would
    // mark the root dirty itself and rebuild the subtree either way — it
    // measures the harness, not the scroll.
    for _ in 0..4 {
        laid.tick();
    }
    assert_eq!(
        builds.load(std::sync::atomic::Ordering::SeqCst),
        after_mount,
        "an idle frame rebuilds nothing — the control for the scroll below"
    );

    for pixels in [10.0, 25.0, 60.0, 125.0] {
        controller.set_pixels(pixels);
        laid.tick();
    }

    assert_eq!(
        builds.load(std::sync::atomic::Ordering::SeqCst),
        after_mount,
        "four scroll writes must rebuild the viewport's subtree zero times"
    );
    assert_eq!(
        controller.pixels(),
        125.0,
        "the scroll itself still took effect"
    );
}

/// An `animate_to` queued *before* the `Scrollable` mounts must still run.
///
/// The command is a queue drained on the controller's notify, and a call made
/// while no `Scrollable` is attached fires that notify with nobody listening.
/// Nothing guarantees a second one: if the first layout's dimensions match
/// what the controller already holds, `update_dimensions` notifies nothing at
/// all and the command would wait forever. So the listener drains once when it
/// is installed — the `AnimatedBuilder` this replaced covered the same case
/// implicitly, by servicing the queue on its initial build.
#[test]
fn an_animate_to_queued_before_mount_still_runs() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);

    // Queued with no Scrollable in the tree: this notify reaches no listener.
    controller.animate_to(
        200.0,
        Duration::from_millis(100),
        Arc::new(flui_animation::Linear),
    );
    assert_eq!(
        controller.pixels(),
        0.0,
        "nothing has driven the animation yet"
    );

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 800.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    for _ in 0..12 {
        scoped.pump_for(Duration::from_millis(16));
    }

    assert!(
        controller.pixels() > 0.0,
        "the queued animate_to must have been picked up on mount; pixels = {}",
        controller.pixels()
    );
}
