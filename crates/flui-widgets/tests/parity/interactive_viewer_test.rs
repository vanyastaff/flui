//! Flutter parity tests — `InteractiveViewer`.
//!
//! Flutter source: `packages/flutter/test/widgets/interactive_viewer_test.dart`
//! (tag `3.44.0`, **40** `testWidgets` cases —
//! `grep -cE '^\s*testWidgets\(' interactive_viewer_test.dart`).
//!
//! ## Scope of this port
//!
//! `crates/flui-widgets/src/interaction/interactive_viewer.rs`'s module docs
//! record the exact V1 boundary; the short version, so the denominator below
//! is honest:
//!
//! - **Pan** is a genuine single-pointer drag, dispatched through the real
//!   gesture arena (`LaidOutScoped::dispatch_pointer_*`), slop and all.
//! - **Scale** is wired through mouse-wheel scroll only
//!   (`Listener::on_pointer_signal`, dispatched with a real
//!   `PointerEvent::Scroll` via `LaidOut::route_event`) — the wheel branch of
//!   the oracle's `_receivedPointerSignal`.
//! - **Pinch-to-zoom / two-finger rotation are out of scope**: FLUI's
//!   `GestureDetector` has no combined scale/rotate recognizer yet (a
//!   framework gap, not a harness one — see the widget module's docs). Every
//!   oracle case built on `tester.createGesture()` × 2 (pinch, rotate,
//!   two-finger "child bigger than viewport" checks) has no analogue here.
//! - **`constrained: false`** is deferred (no `OverflowBox`-equivalent
//!   wiring yet); every oracle case that depends on it (`'child bigger than
//!   viewport'`, `'child has no dimensions'`, `'Scaling amount is equal
//!   forth and back...'`'s literal `constrained: false`, `'builder can
//!   change widgets that are off-screen'`, the `.builder` constructor
//!   entirely) is out of scope. This port only exercises the `child:`
//!   constructor.
//! - Inertia/fling after a pan release, `alignment`, `scaleFactor` beyond the
//!   default, `trackpadScrollCausesScale`, and discrete
//!   `PointerScaleEvent`/trackpad-gesture scaling are not ported.
//!
//! **16 tests ported below**, covering the portable core: pan translates the
//! child (`pan_translates_child_by_the_drag_delta`), boundary clamp
//! (`pan_clamps_at_the_boundary_edge`), an infinite margin removing the
//! boundary (`infinite_boundary_margin_pans_without_clamping`), `pan_axis`
//! locking (`pan_axis_horizontal_locks_out_vertical_movement`,
//! `pan_axis_vertical_locks_out_horizontal_movement`,
//! `pan_axis_aligned_locks_to_the_first_updates_dominant_axis_for_the_whole_gesture`),
//! `pan_enabled: false` still firing callbacks without moving the transform
//! (`pan_disabled_ignores_the_drag_but_still_fires_callbacks`), an external
//! controller composing with a later gesture and notifying its own listener
//! (`controller_driven_initial_value_composes_with_a_later_pan`), the wheel
//! path's `min_scale`/`max_scale` clamp
//! (`wheel_scale_is_clamped_to_a_fixed_min_and_max_scale`),
//! `scale_enabled: false` leaving the transform untouched while still firing
//! callbacks (`wheel_scale_disabled_still_fires_callbacks_but_does_not_scale`),
//! a scale-then-inverse-scale round trip returning to identity within
//! floating-point tolerance (`wheel_scale_round_trips_back_to_identity`), the
//! wheel path's off-center focal-point correction
//! (`wheel_scale_keeps_the_scene_point_under_an_off_center_cursor_fixed`),
//! `on_interaction_*` firing in start/update/end order for both the pan and
//! wheel paths (`on_interaction_callbacks_fire_in_order_for_a_pan`,
//! `on_interaction_callbacks_fire_in_order_for_a_wheel_scale`), the
//! `boundary_margin` mixed-finite/infinite precondition
//! (`boundary_margin_mixing_finite_and_infinite_edges_is_rejected`), and
//! unmounting unsubscribing from an external controller
//! (`unmounting_the_widget_unsubscribes_from_an_external_controller`).
//!
//! None of the 16 corresponds 1:1 to a single named oracle `testWidgets` case
//! (the oracle drives everything through `tester.pumpWidget` +
//! `tester.startGesture`/`scrollAt`, which exercise pan and wheel together
//! with the full `constrained: true` default this port also uses) — they are
//! written directly against this port's real gesture/wheel paths and the
//! same boundary-clamp/scale-clamp contract the oracle's
//! `_matrixTranslate`/`_matrixScale` encode, cross-checked against the
//! oracle's `'boundary slightly bigger than child'` (clamp), `'no boundary'`
//! (infinite margin), `'Can scale with mouse'` / `'Cannot scale with mouse
//! when scale is disabled'` / `'Scale with mouse returns onInteraction
//! properties'` / `'Scaling amount is equal forth and back with a mouse
//! scroll'` (the four wheel-scale cases), and the `PanAxis.*` group.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_geometry::Matrix4;
use flui_interaction::events::make_scroll_event;
use flui_types::{EdgeInsets, Offset, geometry::px};
use flui_widgets::prelude::*;
use flui_widgets::{
    InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails, InteractiveViewer,
    PanAxis, SizedBox, TransformationController,
};

use crate::common::{lay_out, lay_out_with_arena, loose};

/// A 200x200 child under loose (up-to-500px) constraints: the child settles
/// at its own preferred 200x200 size, and — because `InteractiveViewer`'s
/// proxy chain is layout-transparent under `constrained: true` (V1's only
/// mode, see the widget's module docs) — that is also the viewport size.
fn child() -> SizedBox {
    SizedBox::new(200.0, 200.0)
}

/// The uniform scale factor of a matrix built solely from translation +
/// scale (no rotation, matching every matrix `InteractiveViewer` produces):
/// `m[0]` is exactly the x-scale.
fn scale_of(matrix: Matrix4) -> f32 {
    matrix.to_col_major_array()[0]
}

// ============================================================================
// Pan
// ============================================================================

/// Down, a slop-crossing move (recognized as the drag start — no delta
/// reported for it, matching `DragGestureRecognizer`'s `dragStartBehavior:
/// Start` default, see `draggable_test.rs`'s
/// `drag_update_reports_delta_after_start`), then a second move whose raw
/// delta is what `on_pan_update` reports and what this widget applies.
#[test]
fn pan_translates_child_by_the_drag_delta() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(100.0, 50.0); // +50px: crosses slop, starts
    scoped.dispatch_pointer_move(140.0, 50.0); // +40px: the reported update
    scoped.dispatch_pointer_up(140.0, 50.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert_eq!(
        tx, 40.0,
        "the second move's raw +40px delta must apply exactly"
    );
    assert_eq!(ty, 0.0);
}

/// Child 200x200, `boundary_margin: 10` on every edge, so the boundary is
/// 220x220 — 10px of slack past the (loose, so viewport == child) 200x200
/// viewport on each side. A drag attempting -60px must clamp to exactly
/// -10px, the same boundary-clamp arithmetic as the oracle's `'boundary
/// slightly bigger than child'` (`translation.x == -boundaryMargin`).
#[test]
fn pan_clamps_at_the_boundary_edge() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .boundary_margin(EdgeInsets::all(px(10.0)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(150.0, 100.0);
    scoped.dispatch_pointer_move(90.0, 100.0); // -60px: crosses slop, starts
    scoped.dispatch_pointer_move(30.0, 100.0); // -60px: the reported update
    scoped.dispatch_pointer_up(30.0, 100.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert_eq!(tx, -10.0, "must clamp to exactly -boundary_margin (-10.0)");
    assert_eq!(ty, 0.0);
}

/// `EdgeInsets::all(f32::INFINITY)` removes the boundary entirely. The exact
/// same -60px drag [`pan_clamps_at_the_boundary_edge`] clamps to -10px with a
/// 10px margin must apply in full here — a direct, same-magnitude contrast
/// against that test. Oracle: `'no boundary'`.
#[test]
fn infinite_boundary_margin_pans_without_clamping() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(150.0, 100.0);
    scoped.dispatch_pointer_move(90.0, 100.0); // -60px: crosses slop, starts
    scoped.dispatch_pointer_move(30.0, 100.0); // -60px: the reported update
    scoped.dispatch_pointer_up(30.0, 100.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert_eq!(
        tx, -60.0,
        "an infinite margin must never clamp — contrast with pan_clamps_at_the_boundary_edge's -10.0"
    );
    assert_eq!(ty, 0.0);
}

/// `PanAxis::Horizontal` zeroes the vertical component of every update,
/// regardless of the drag's own direction. Oracle group: `PanAxis.*`.
#[test]
fn pan_axis_horizontal_locks_out_vertical_movement() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .pan_axis(PanAxis::Horizontal)
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(100.0, 100.0);
    scoped.dispatch_pointer_move(150.0, 140.0); // +50,+40: crosses slop, starts
    scoped.dispatch_pointer_move(190.0, 180.0); // +40,+40: the reported update
    scoped.dispatch_pointer_up(190.0, 180.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert_eq!(tx, 40.0, "horizontal component must still apply");
    assert_eq!(ty, 0.0, "vertical component must be locked out entirely");
}

/// `PanAxis::Vertical` is `Horizontal`'s mirror image.
#[test]
fn pan_axis_vertical_locks_out_horizontal_movement() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .pan_axis(PanAxis::Vertical)
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(100.0, 100.0);
    scoped.dispatch_pointer_move(150.0, 140.0); // +50,+40: crosses slop, starts
    scoped.dispatch_pointer_move(190.0, 180.0); // +40,+40: the reported update
    scoped.dispatch_pointer_up(190.0, 180.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert_eq!(tx, 0.0, "horizontal component must be locked out entirely");
    assert_eq!(ty, 40.0, "vertical component must still apply");
}

/// `pan_enabled: false` must never move the transform, but
/// `on_interaction_*` still fires — Flutter's documented "will be called
/// even if the interaction is disabled" contract (`InteractiveViewer`'s
/// `onInteractionEnd` doc, `{@template
/// flutter.widgets.InteractiveViewer.onInteractionEnd}`).
#[test]
fn pan_disabled_ignores_the_drag_but_still_fires_callbacks() {
    let controller = TransformationController::new();
    let updates = Arc::new(AtomicUsize::new(0));
    let updates_cb = Arc::clone(&updates);
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .pan_enabled(false)
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .on_interaction_update(move |_details| {
            updates_cb.fetch_add(1, Ordering::SeqCst);
        })
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(150.0, 100.0);
    scoped.dispatch_pointer_move(90.0, 100.0); // crosses slop, starts
    scoped.dispatch_pointer_move(30.0, 100.0); // the reported update
    scoped.dispatch_pointer_up(30.0, 100.0);

    assert_eq!(
        controller.value().m,
        Matrix4::identity().m,
        "pan_enabled: false must leave the transform untouched"
    );
    assert_eq!(
        updates.load(Ordering::SeqCst),
        1,
        "on_interaction_update must still fire exactly once for the real update"
    );
}

// ============================================================================
// Controller
// ============================================================================

/// An externally supplied `TransformationController` seeded with a
/// translation before mount composes with a *later* gesture-driven pan
/// (proving the widget reads and writes through the caller's controller,
/// not an internally created one — if it used its own, the gesture would
/// start from identity and this would read `40.0`, not `60.0`), and the
/// gesture's `set_value` call notifies the caller's own listener on that
/// same controller.
#[test]
fn controller_driven_initial_value_composes_with_a_later_pan() {
    let controller = TransformationController::new();
    controller.set_value(Matrix4::translation(20.0, 0.0, 0.0));

    let notified = Arc::new(AtomicUsize::new(0));
    let notified_cb = Arc::clone(&notified);
    controller.as_listenable().add_listener(Arc::new(move || {
        notified_cb.fetch_add(1, Ordering::SeqCst);
    }));

    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(100.0, 50.0); // crosses slop, starts
    scoped.dispatch_pointer_move(140.0, 50.0); // +40px update
    scoped.dispatch_pointer_up(140.0, 50.0);

    let (tx, _, _) = controller.value().translation_component();
    assert_eq!(
        tx, 60.0,
        "the gesture's +40px must compose on top of the externally seeded +20px"
    );
    assert!(
        notified.load(Ordering::SeqCst) >= 1,
        "the gesture-driven set_value must notify the externally supplied controller's own listener"
    );
}

// ============================================================================
// Wheel scale
// ============================================================================

/// `min_scale == max_scale == 1.0` pins the scale exactly — a mouse-wheel
/// scroll (which would otherwise scale up) must be clamped back to 1.0,
/// leaving the transform at the identity. Oracle: the boundary-floor /
/// min-max clamp `_matrixScale` applies, exercised through `'Can scale with
/// mouse'`'s wheel path.
#[test]
fn wheel_scale_is_clamped_to_a_fixed_min_and_max_scale() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .min_scale(1.0)
        .max_scale(1.0)
        .child(child());
    let laid = lay_out(widget, loose(500.0));

    let position = Offset::new(px(100.0), px(100.0));
    let event = make_scroll_event(position, Offset::new(px(0.0), px(-20.0)));
    laid.route_event(&event, 100.0, 100.0);

    assert_eq!(
        controller.value().m,
        Matrix4::identity().m,
        "min_scale == max_scale == 1.0 must clamp every wheel scroll back to identity"
    );
}

/// `scale_enabled: false` must never change the transform, but
/// `on_interaction_*` still fires (same Flutter contract as
/// `pan_disabled_ignores_the_drag_but_still_fires_callbacks`). Oracle:
/// `'Cannot scale with mouse when scale is disabled'`.
#[test]
fn wheel_scale_disabled_still_fires_callbacks_but_does_not_scale() {
    let controller = TransformationController::new();
    let starts = Arc::new(AtomicUsize::new(0));
    let starts_cb = Arc::clone(&starts);
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .scale_enabled(false)
        .on_interaction_start(move |_details| {
            starts_cb.fetch_add(1, Ordering::SeqCst);
        })
        .child(child());
    let laid = lay_out(widget, loose(500.0));

    let position = Offset::new(px(100.0), px(100.0));
    let event = make_scroll_event(position, Offset::new(px(0.0), px(-20.0)));
    laid.route_event(&event, 100.0, 100.0);

    assert_eq!(controller.value().m, Matrix4::identity().m);
    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "on_interaction_start must still fire once even though scale_enabled is false"
    );
}

/// Scaling up then back down by the same wheel-scroll magnitude, with an
/// infinite `boundary_margin` (removing the `min_scale`-below-1.0 floor a
/// zero margin otherwise imposes — see the widget's `min_scale` doc), must
/// return the scale to `1.0` within floating-point tolerance. Oracle:
/// `'Scaling amount is equal forth and back with a mouse scroll'` — ported
/// with an epsilon here (not exact) because `exp`/its floating-point inverse
/// do not round-trip bit-for-bit, exactly as the oracle's own assertion
/// needs `closeTo` for the same reason.
#[test]
fn wheel_scale_round_trips_back_to_identity() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .min_scale(0.01)
        .max_scale(100_000.0)
        .child(child());
    let laid = lay_out(widget, loose(500.0));

    let position = Offset::new(px(100.0), px(100.0));
    let zoom_in = make_scroll_event(position, Offset::new(px(0.0), px(-200.0)));
    let zoom_out = make_scroll_event(position, Offset::new(px(0.0), px(200.0)));

    laid.route_event(&zoom_in, 100.0, 100.0);
    let after_one_zoom_in = scale_of(controller.value());
    assert!(
        (after_one_zoom_in - std::f32::consts::E).abs() < 1e-3,
        "expected scale ~= e^1, got {after_one_zoom_in}"
    );

    laid.route_event(&zoom_out, 100.0, 100.0);
    let after_round_trip = scale_of(controller.value());
    assert!(
        (after_round_trip - 1.0).abs() < 1e-4,
        "scale must round-trip back to ~1.0, got {after_round_trip}"
    );
}

// ============================================================================
// Callback ordering
// ============================================================================

/// `on_interaction_start`/`_update`/`_end` must fire in exactly that order
/// for a pan gesture — one start (at slop-crossing), one update (the second
/// move), one end (the release).
#[test]
fn on_interaction_callbacks_fire_in_order_for_a_pan() {
    let order: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
    let order_start = Arc::clone(&order);
    let order_update = Arc::clone(&order);
    let order_end = Arc::clone(&order);

    let widget = InteractiveViewer::new()
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .on_interaction_start(move |_: InteractionStartDetails| {
            order_start.lock().expect("not poisoned").push("start");
        })
        .on_interaction_update(move |_: InteractionUpdateDetails| {
            order_update.lock().expect("not poisoned").push("update");
        })
        .on_interaction_end(move |_: InteractionEndDetails| {
            order_end.lock().expect("not poisoned").push("end");
        })
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(100.0, 50.0); // crosses slop: on_interaction_start
    scoped.dispatch_pointer_move(140.0, 50.0); // the update: on_interaction_update
    scoped.dispatch_pointer_up(140.0, 50.0); // on_interaction_end

    assert_eq!(
        *order.lock().expect("not poisoned"),
        vec!["start", "update", "end"],
        "callbacks must fire in exactly start, update, end order"
    );
}

/// The same order contract for the discrete wheel path — a single scroll
/// event fires start, then update, then end (Flutter parity:
/// `_receivedPointerSignal`'s mouse-wheel branch treats one wheel tick as a
/// complete, instantaneous interaction).
#[test]
fn on_interaction_callbacks_fire_in_order_for_a_wheel_scale() {
    let order: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
    let order_start = Arc::clone(&order);
    let order_update = Arc::clone(&order);
    let order_end = Arc::clone(&order);

    let widget = InteractiveViewer::new()
        .on_interaction_start(move |_: InteractionStartDetails| {
            order_start.lock().expect("not poisoned").push("start");
        })
        .on_interaction_update(move |_: InteractionUpdateDetails| {
            order_update.lock().expect("not poisoned").push("update");
        })
        .on_interaction_end(move |_: InteractionEndDetails| {
            order_end.lock().expect("not poisoned").push("end");
        })
        .child(child());
    let laid = lay_out(widget, loose(500.0));

    let position = Offset::new(px(100.0), px(100.0));
    let event = make_scroll_event(position, Offset::new(px(0.0), px(-20.0)));
    laid.route_event(&event, 100.0, 100.0);

    assert_eq!(
        *order.lock().expect("not poisoned"),
        vec!["start", "update", "end"],
        "the wheel path must fire callbacks in exactly start, update, end order"
    );
}

// ============================================================================
// Precondition
// ============================================================================

/// `boundary_margin` must be either fully finite or fully infinite on all
/// four edges — Flutter's own constructor `assert`, which (like all Dart
/// `assert`s) is debug-only; this port's `debug_assert!` mirrors that
/// exactly. Oracle: the constructor assert backing every finite-margin test
/// (e.g. `'boundary slightly bigger than child'`) and every infinite-margin
/// test (`'no boundary'`).
#[test]
#[should_panic(expected = "boundary_margin must be either fully finite or fully infinite")]
fn boundary_margin_mixing_finite_and_infinite_edges_is_rejected() {
    let mixed = EdgeInsets {
        top: px(10.0),
        right: px(f32::INFINITY),
        bottom: px(10.0),
        left: px(10.0),
    };
    let _ = InteractiveViewer::new().boundary_margin(mixed);
}

// ============================================================================
// Wheel focal-point correction
// ============================================================================

/// The wheel-scale path must keep the *same scene point* under the cursor
/// before and after the zoom — Flutter parity: the `_receivedPointerSignal`
/// mouse-wheel branch scales, then translates by exactly the shift the scale
/// introduced at the focal point, cross-checked against the oracle's `'Can
/// scale with mouse'` / `'onInteraction can be used to get scene point'`
/// intent. A focal point at the widget's own center makes this correction a
/// zero vector by symmetry, which is why an off-center focal point (`(50,
/// 50)`, not the 200x200 widget's `(100, 100)` center) is required to
/// exercise it at all: mutating the correction to `Offset::ZERO` passes
/// every other test in this file but fails both assertions here.
#[test]
fn wheel_scale_keeps_the_scene_point_under_an_off_center_cursor_fixed() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let laid = lay_out(widget, loose(500.0));

    let focal = Offset::new(px(50.0), px(50.0));
    let scene_before = controller.to_scene(focal);

    let event = make_scroll_event(focal, Offset::new(px(0.0), px(-20.0)));
    laid.route_event(&event, 50.0, 50.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert!(
        tx != 0.0 || ty != 0.0,
        "an off-center wheel-zoom must produce a compensating translation, not a pure \
         scale-about-the-origin — a zeroed-out correction would leave the transform at \
         (0.0, 0.0)"
    );

    let scene_after = controller.to_scene(focal);
    assert!(
        (scene_after.dx.get() - scene_before.dx.get()).abs() < 0.01,
        "the scene point under the cursor must not drift on the x axis: before={scene_before:?} after={scene_after:?}"
    );
    assert!(
        (scene_after.dy.get() - scene_before.dy.get()).abs() < 0.01,
        "the scene point under the cursor must not drift on the y axis: before={scene_before:?} after={scene_after:?}"
    );
}

// ============================================================================
// PanAxis::Aligned
// ============================================================================

/// `PanAxis::Aligned` locks to whichever axis dominates the *first* update's
/// cumulative movement from the drag's start position, and that lock holds
/// for the rest of the gesture — even once a later update's own per-event
/// delta leans the other way. Oracle group: `PanAxis.*`
/// (`'PanAxis.aligned allows panning in one direction only...'`).
#[test]
fn pan_axis_aligned_locks_to_the_first_updates_dominant_axis_for_the_whole_gesture() {
    let controller = TransformationController::new();
    let widget = InteractiveViewer::new()
        .controller(controller.clone())
        .pan_axis(PanAxis::Aligned)
        .boundary_margin(EdgeInsets::all(px(f32::INFINITY)))
        .child(child());
    let scoped = lay_out_with_arena(widget, loose(500.0));

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_move(110.0, 50.0); // +60,0: crosses slop, starts (start_local = (110, 50))
    // Update 1: cumulative movement from start = (50, 10) -> x dominates -> locks Horizontal.
    // This update's own per-event delta is (50, 10) -> aligned to (50, 0).
    scoped.dispatch_pointer_move(160.0, 60.0);
    // Update 2: per-event delta (-40, 80) is mostly VERTICAL, but the axis lock from
    // update 1 must still force it to (-40, 0) — that's the behavior this test pins.
    scoped.dispatch_pointer_move(120.0, 140.0);
    scoped.dispatch_pointer_up(120.0, 140.0);

    let (tx, ty, _) = controller.value().translation_component();
    assert_eq!(
        tx, 10.0,
        "locked to horizontal: +50 (update 1) + -40 (update 2) = 10"
    );
    assert_eq!(
        ty, 0.0,
        "vertical must stay locked out for the whole gesture, even though update 2's own \
         raw delta was mostly vertical"
    );
}

// ============================================================================
// Controller disposal
// ============================================================================

/// A stable root TYPE that toggles its shape internally — `pump_widget`
/// dispatches by `TypeId`, so swapping between two *different* concrete root
/// widget types is not the supported way to unmount a subtree (see
/// `LaidOut::pump_widget`'s doc, and `focus_test.rs`'s
/// `nodes_are_removed_when_all_focuses_are_removed` for the same pattern);
/// `show` is.
#[derive(Clone, StatelessView)]
struct InteractiveViewerHost {
    controller: TransformationController,
    show: bool,
}

impl StatelessView for InteractiveViewerHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.show {
            return SizedBox::new(1.0, 1.0).into_view().boxed();
        }
        InteractiveViewer::new()
            .controller(self.controller.clone())
            .child(child())
            .into_view()
            .boxed()
    }
}

/// Unmounting `InteractiveViewer` must remove its `AnimatedBuilder`
/// subscription from an externally supplied `TransformationController` — a
/// dangling listener would keep firing into a torn-down subtree (and would
/// keep the controller from ever reporting itself listener-free, which
/// matters to a caller that wants to know it is safe to drop).
///
/// Uses plain `lay_out`/`LaidOut::pump_widget`, not
/// `lay_out_with_arena`/`LaidOutScoped`: the latter mounts the root wrapped
/// in a `GestureArenaScope`, so `LaidOutScoped::pump_widget(new_root)` would
/// itself swap the *actual* mounted root from `GestureArenaScope<Host>` to a
/// bare `Host` — the exact unsupported different-root-type swap
/// `InteractiveViewerHost`'s own doc warns about, one level further out. This
/// test dispatches no pointer events, so it does not need the arena.
#[test]
fn unmounting_the_widget_unsubscribes_from_an_external_controller() {
    let controller = TransformationController::new();
    let mut laid = lay_out(
        InteractiveViewerHost {
            controller: controller.clone(),
            show: true,
        },
        loose(500.0),
    );

    assert!(
        controller.has_listeners(),
        "mounting must subscribe to the externally supplied controller"
    );

    laid.pump_widget(InteractiveViewerHost {
        controller: controller.clone(),
        show: false,
    });

    assert!(
        !controller.has_listeners(),
        "unmounting must remove the subscription — a leaked listener would keep this true \
         forever, even though nothing is mounted against the controller anymore"
    );
}
