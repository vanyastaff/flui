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
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, ScrollController, Scrollable,
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
