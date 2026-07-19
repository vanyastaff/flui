//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/rotated_box_test.dart` (tag
//! `3.44.0`) — 1 `testWidgets` case, 0 bare `test(` cases (confirmed via
//! `git -C /mnt/data/dev/flutter show
//! 3.44.0:packages/flutter/test/widgets/rotated_box_test.dart | grep -cE
//! "^\s*testWidgets\("` → `1`; no `test/rendering/rotated_box_test.dart`
//! exists — `RenderRotatedBox` has no render-level oracle file, only this
//! widget-level one). Upstream's second case, `'RotatedBox does not crash at
//! zero area'`, postdates `3.44.0` (PR #186201, commit `c2d451e1237`, not an
//! ancestor of the `3.44.0` tag) — it is NOT part of the 3.44.0 oracle corpus
//! and is not cited as an oracle case below (see the FLUI-added edge case
//! instead).
//!
//! The one 3.44.0 oracle case, split into one Rust test per independent
//! assertion (this file's established granularity, see e.g. `transform_test.rs`):
//! - `'Rotated box control test'` — the box-size half (odd-turn width/height
//!   swap reported through the full widget → render-object pipeline, not the
//!   render object alone) plus the hit-test half (two side-by-side children
//!   inside a rotated `Row`; a tap must reach the correct ONE of them, which
//!   proves the rotation's *direction*, not merely that some rotation
//!   happened — a pointer at the parent's own center would hit either child
//!   under a translation-only bug) —
//!   [`rotated_box_control_test_swaps_box_size_for_an_odd_turn`],
//!   [`rotated_box_control_test_hit_test_reaches_the_left_child`],
//!   [`rotated_box_control_test_hit_test_reaches_the_right_child`].
//!
//! FLUI-added edge case (NOT part of the 3.44.0 oracle corpus — see above;
//! ported as an independent regression guard, not an oracle citation, since
//! the upstream case it mirrors in shape postdates the tag this port is
//! against):
//! - a childless, odd-turn `RotatedBox` under a zero-area surface must report
//!   `Size::ZERO` and must not panic during layout —
//!   [`rotated_box_does_not_crash_at_zero_area`].
//!
//! Widget → render-object mapping: `RotatedBox`
//! (`crates/flui-widgets/src/layout/rotated_box.rs`) → `RenderRotatedBox`
//! (`crates/flui-objects/src/layout/rotated_box.rs`).
//!
//! Dropped from `'Rotated box control test'`: the upstream `Row` wraps each
//! child in a plain `Container` with a background color purely for visual
//! identification in a real widget test; FLUI's headless harness has no
//! paint/golden capture, so the color is dropped (the `GestureDetector` +
//! `SizedBox` pair is what upstream's own `Container` — itself a
//! `ColoredBox` + `ConstrainedBox` composite — reduces to for hit-testing
//! purposes; nothing observable is lost).
//!
//! Coordinate divergence in the LEFT-child leg of `'Rotated box control
//! test'` (empirically confirmed, not assumed): upstream taps
//! `Offset(420.0, 280.0)`. The laid-out geometry here matches Flutter's own
//! math exactly (confirmed by direct inspection: `RenderRotatedBox` reports
//! 65×175, the `Row` sits at the box's origin sized 175×65, and
//! `CrossAxisAlignment::Center` offsets the 40-tall left child by
//! `(65 - 40) / 2 = 12.5` inside it — the same arithmetic Flutter's own
//! `RenderFlex` uses). Working through `RenderRotatedBox`'s inverse paint
//! transform by hand, `(420.0, 280.0)` lands at row-local `(67.5, 12.5)` —
//! exactly ON the left child's top edge (`offset.dy == 12.5`), not inside it.
//! Flutter's `f64` trig (`cos`/`sin` of `pi/2`) rounds close enough to land
//! that boundary inclusive; FLUI's geometry types are `f32`
//! (`std::f32::consts::FRAC_PI_2`), whose `cos`/`sin` carry a coarser
//! ~4e-8 rounding error — enough to push the computed row-local `y` a hair
//! below `12.5`, outside the child's half-open `[12.5, 52.5)` span, so the
//! literal pixel misses BOTH children here (confirmed empirically: nudging
//! the tap 0.5px in either direction along the affected axis flips it
//! cleanly to a hit, and the flip point is stable). This is a knife-edge
//! floating-point representation difference between two independent `sin`/
//! `cos` implementations at two different precisions, not a rotation-
//! direction defect — confirmed by mutating `RenderRotatedBox`'s rotation
//! angle to the wrong sign and observing every nearby interior point flip
//! from hitting the left child to hitting the right one instead (proving the
//! coordinate genuinely discriminates rotation direction, it just cannot sit
//! ON the exact oracle pixel). The LEFT-child test below uses `(410.0,
//! 280.0)` instead — 10px inside the same child, same axis, same intent,
//! comfortably clear of the boundary — while the RIGHT-child leg keeps the
//! literal oracle pixel `(380.0, 320.0)` unchanged (it lands solidly
//! interior, no adjustment needed).
//!
//! Delta port (not a named upstream case): no upstream test — nor any
//! existing `harness_rotated_box_*` case before this port — discriminates
//! "the child was laid out under the FLIPPED constraints" from "the child's
//! final size happens to match after the parent's own w/h swap"; every prior
//! case uses either a leaf that ignores incoming constraints in the swapped
//! direction, or symmetric `loose(200.0)` bounds where flipping the
//! constraints doesn't change the clamped result. See
//! `crates/flui-objects/tests/render_object_harness.rs`'s
//! `harness_rotated_box_odd_turn_lays_out_child_under_flipped_constraints` for
//! the render-object-level proof (asymmetric bounds distinguish the two
//! outcomes), added alongside this file.

use crate::harness::{pump_widget, screen, screen_of};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::ViewExt;
use flui_widgets::{Center, GestureDetector, MainAxisSize, RotatedBox, Row, SizedBox};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Odd-turn box-size half of `'Rotated box control test'`: a `Row` of two
/// fixed-size children (100×40, 75×65) under `MainAxisSize::Min` reports a
/// natural (unrotated) size of 175×65 (width = sum of children, height = max
/// cross-axis extent); `RotatedBox::new(1)` swaps that to width 65, height
/// 175 for the box's own reported size.
///
/// Flutter parity: `rotated_box_test.dart` `'Rotated box control test'`
/// (3.44.0) — `expect(box.size.width, equals(65.0))` and
/// `expect(box.size.height, equals(175.0))`.
#[test]
fn rotated_box_control_test_swaps_box_size_for_an_odd_turn() {
    let laid = pump_widget(
        Center::new().child(
            RotatedBox::new(1).child(
                Row::new(vec![
                    SizedBox::new(100.0, 40.0).boxed(),
                    SizedBox::new(75.0, 65.0).boxed(),
                ])
                .main_axis_size(MainAxisSize::Min),
            ),
        ),
        screen(),
    );

    let id = laid.find_by_render_type("RenderRotatedBox");
    let size = laid.size(id);

    assert_eq!(
        size.width.get(),
        65.0,
        "an odd-turn RotatedBox must report the child's height (65) as its own width"
    );
    assert_eq!(
        size.height.get(),
        175.0,
        "an odd-turn RotatedBox must report the child's width (175) as its own height"
    );
}

/// Hit-test half of `'Rotated box control test'`: a tap must reach the FIRST
/// (left, 100×40) child only, proving the rotation maps screen coordinates
/// back to the correct child rather than just "some" child under the rotated
/// `Row`. Uses `(410.0, 280.0)` rather than the literal oracle pixel — see
/// the module doc's "Coordinate divergence" note for the empirically
/// confirmed floating-point reason.
///
/// Flutter parity: `rotated_box_test.dart` `'Rotated box control test'`
/// (3.44.0) — the `tapAt(const Offset(420.0, 280.0))` /
/// `expect(log, equals(<String>['left']))` leg (coordinate adjusted, same
/// child/assertion).
#[test]
fn rotated_box_control_test_hit_test_reaches_the_left_child() {
    let did_tap_left = Arc::new(AtomicBool::new(false));
    let did_tap_right = Arc::new(AtomicBool::new(false));
    let (left_cb, right_cb) = (Arc::clone(&did_tap_left), Arc::clone(&did_tap_right));

    let laid = pump_widget(
        Center::new().child(
            RotatedBox::new(1).child(
                Row::new(vec![
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || left_cb.store(true, Ordering::SeqCst))
                        .child(SizedBox::new(100.0, 40.0))
                        .boxed(),
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || right_cb.store(true, Ordering::SeqCst))
                        .child(SizedBox::new(75.0, 65.0))
                        .boxed(),
                ])
                .main_axis_size(MainAxisSize::Min),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(410.0, 280.0);
    laid.dispatch_pointer_up(410.0, 280.0);

    assert!(
        did_tap_left.load(Ordering::SeqCst),
        "a tap at (410, 280) must reach the left child through the rotation"
    );
    assert!(
        !did_tap_right.load(Ordering::SeqCst),
        "a tap at (410, 280) must not reach the right child"
    );
}

/// The other half of the same hit test: `(380.0, 320.0)` must reach the
/// SECOND (right, 75×65) child only. This leg lands solidly interior to the
/// right child's mapped region, so the literal oracle pixel carries over
/// unchanged (unlike the left leg — see the module doc's "Coordinate
/// divergence" note).
///
/// Flutter parity: `rotated_box_test.dart` `'Rotated box control test'`
/// (3.44.0) — `tapAt(const Offset(380.0, 320.0))` /
/// `expect(log, equals(<String>['right']))`.
#[test]
fn rotated_box_control_test_hit_test_reaches_the_right_child() {
    let did_tap_left = Arc::new(AtomicBool::new(false));
    let did_tap_right = Arc::new(AtomicBool::new(false));
    let (left_cb, right_cb) = (Arc::clone(&did_tap_left), Arc::clone(&did_tap_right));

    let laid = pump_widget(
        Center::new().child(
            RotatedBox::new(1).child(
                Row::new(vec![
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || left_cb.store(true, Ordering::SeqCst))
                        .child(SizedBox::new(100.0, 40.0))
                        .boxed(),
                    GestureDetector::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_tap(move || right_cb.store(true, Ordering::SeqCst))
                        .child(SizedBox::new(75.0, 65.0))
                        .boxed(),
                ])
                .main_axis_size(MainAxisSize::Min),
            ),
        ),
        screen(),
    );

    laid.dispatch_pointer_down(380.0, 320.0);
    laid.dispatch_pointer_up(380.0, 320.0);

    assert!(
        did_tap_right.load(Ordering::SeqCst),
        "a tap at (380, 320) must reach the right child through the rotation"
    );
    assert!(
        !did_tap_left.load(Ordering::SeqCst),
        "a tap at (380, 320) must not reach the left child"
    );
}

/// A childless `RotatedBox(quarterTurns: 1)` under a zero-size test surface
/// must report `Size.zero` and must not panic during layout.
///
/// FLUI-added edge case, NOT a 3.44.0 oracle citation: upstream's
/// `'RotatedBox does not crash at zero area'` in `rotated_box_test.dart`
/// checks this same shape, but that case was added to upstream by PR
/// #186201 (commit `c2d451e1237`), which postdates the `3.44.0` tag this
/// port is scoped to — it is not an ancestor of `3.44.0` and so is not part
/// of the oracle corpus being ported here. This test stands on its own as a
/// regression guard for the odd-turn no-child branch, not as a ported
/// upstream case.
#[test]
fn rotated_box_does_not_crash_at_zero_area() {
    let laid = pump_widget(Center::new().child(RotatedBox::new(1)), screen_of(0.0, 0.0));

    let id = laid.find_by_render_type("RenderRotatedBox");
    let size = laid.size(id);

    assert_eq!(
        size.width.get(),
        0.0,
        "zero-area surface must yield zero width"
    );
    assert_eq!(
        size.height.get(),
        0.0,
        "zero-area surface must yield zero height"
    );
}
