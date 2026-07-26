//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/listener_test.dart` (tag
//! `3.44.0`), group `'transformed events'`.
//!
//! Ported cases:
//! - `'simple offset for touch/signal'` —
//!   [`a_delivered_event_carries_the_receivers_local_position`].
//! - `'scaled and offset for touch/signal'` —
//!   [`a_scaled_and_offset_ancestor_localises_the_delivered_position`].
//! - `'rotated for touch/signal'` —
//!   [`a_rotated_ancestor_localises_the_delivered_position`].
//!
//! Also (not a `listener_test.dart` port; a second, independent defect this
//! file's fix also closes): [`fractional_translation_true_localises_to_the_shifted_child`]
//! and [`fractional_translation_false_localises_to_the_unshifted_child`] pin
//! down `RenderFractionalTranslation`'s ctx-level hit-test transform push,
//! which used to be discarded before reaching `HitTestEntry.transform` —
//! see `crates/flui-interaction/docs/HIT_TESTING.md`'s former "Known gaps"
//! section.
//!
//! A third, independent defect (also not a `listener_test.dart` port):
//! [`fractionally_sized_box_localises_the_delivered_position_to_the_aligned_child`]
//! pins down `HitTestContext::hit_test_child_at_offset`
//! (`crates/flui-rendering/src/context/hit_test.rs`) itself — every caller of
//! that method (`RenderFractionallySizedBox::hit_test` among them) moves the
//! position into the child's frame but, before the fix, never recorded that
//! move on the ctx-level transform stack, so descent hit-tested correctly
//! while the delivered position stayed un-localized.
//!
//! A fourth, independent defect surfaced by auditing every caller of the raw
//! `ctx.hit_test_child` for the same "moves the position but never records
//! it" shape: [`fitted_box_localises_the_delivered_position_through_the_scale`]
//! pins down `RenderFittedBox::hit_test`
//! (`crates/flui-objects/src/layout/fitted_box.rs`), which computed a child
//! position through the inverse of its own scale/align matrix but never
//! pushed that matrix anywhere — neither the driver-level `hit_test_transform`
//! hook (`RenderTransform`/`RenderRotatedBox`'s mechanism) nor the ctx-level
//! `push_transform`/`with_transform` stack (`Flow`'s mechanism, and now this
//! one's).
//!
//! Not ported:
//! - `'scaled for touch/signal'` — `Align(topLeft)` contributes a zero offset
//!   (`TOP_LEFT`'s alignment factor is `(0, 0)` regardless of the size
//!   difference), so only ONE non-identity transform (the scale) ever lands
//!   on the hit-test transform stack. A single-level chain cannot expose a
//!   composition-*order* bug — ordering only matters once two or more
//!   non-commuting parts are folded together — so this case adds no coverage
//!   beyond `'simple offset for touch/signal'` once the offset+scale case
//!   (below) is ported.
//! - The `.position`/`.delta`/`.localPosition`/`.localDelta`/`.transform`
//!   assertions on the `move`/`up` events and the trailing `scrollAt` leg of
//!   each upstream case: FLUI's `transform_pointer_event` only overwrites
//!   `position` in place and leaves `delta`/`coalesced`/`predicted`
//!   untransformed (`crates/flui-interaction/src/routing/hit_test.rs:608-676`),
//!   and there is no `.transform` field on FLUI's `PointerEvent` at all — a
//!   separate, undocumented gap, out of scope for this file. Each case below
//!   asserts only the `down` event's localized `position`, the minimum
//!   surface that proves (or disproves) the transform-stack composition this
//!   file exists to pin down.
//!
//! Widget → render-object mapping:
//! - `Listener` → `RenderListener`
//!   (`crates/flui-objects/src/interaction/listener.rs`)
//! - `Center` → `RenderCenter`, `Transform` → `RenderTransform`
//!
//! Divergence: FLUI's `Transform::alignment` defaults to `Alignment::CENTER`
//! where Flutter's bare `Transform(transform:, origin:)` constructor
//! contributes no pivot at all when both `alignment` and `origin` are left
//! unset (documented at `crates/flui-widgets/src/layout/transform.rs:14-24`).
//! Every case below that uses `Transform` sets `.alignment(Alignment::TOP_LEFT)`
//! explicitly — `TOP_LEFT`'s `alongSize` contributes `(0, 0)`, matching
//! Flutter's null-origin default exactly — so the pivot lands at the box's
//! own local origin, as upstream.
//!
//! # Why this file exists
//!
//! `crates/flui-widgets/tests/parity/transform_test.rs` covers hit/miss
//! (whether a `Transform`-nested target is hit at all) extensively but never
//! asserts the *position* a `Listener` callback receives once it fires — a
//! render/layout defect in the hit-test transform-stack composition (see
//! `crates/flui-interaction/src/routing/hit_test.rs`'s `HitTestResult`
//! transform stack, fixed alongside this file) left the RECORDED position
//! wrong for any ancestor chain that mixes a paint offset with a
//! non-commuting matrix transform (scale, rotation), while leaving hit/miss
//! itself unaffected (descent computes its own coordinates independently of
//! the recorded bookkeeping transform). The first case here is
//! translation-only and already passed before that fix — kept as the control
//! case proving the defect needs a non-commuting ancestor to surface, not as
//! a regression risk in isolation.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::events::PointerEventExt as _;
use flui_types::Color;
use flui_widgets::prelude::*;

use crate::common::{loose, tight};
use crate::harness;

const TOLERANCE: f32 = 1e-3;

fn assert_close(actual: (f32, f32), expected: (f32, f32), what: &str) {
    assert!(
        (actual.0 - expected.0).abs() < TOLERANCE && (actual.1 - expected.1).abs() < TOLERANCE,
        "{what}: expected ({:.4}, {:.4}), got ({:.4}, {:.4})",
        expected.0,
        expected.1,
        actual.0,
        actual.1,
    );
}

/// A hit-testable 100×100 leaf — `ColoredBox` wraps a childless `SizedBox` so
/// the box actually hit-tests true (a bare `SizedBox` with no child does not;
/// see `crates/flui-objects/src/layout/constrained_box.rs:130-139`).
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(255, 0, 0)).child(SizedBox::new(100.0, 100.0))
}

/// The `dx`/`dy` of a locally-transformed position, recorded by a callback
/// and readable back by the test.
type RecordedPosition = Rc<Cell<Option<(f32, f32)>>>;

/// A `Listener` recording the `dx`/`dy` of the position its `on_pointer_down`
/// receives, plus a handle to read it back.
fn recording_listener() -> (RecordedPosition, Listener) {
    let recorded = Rc::new(Cell::new(None));
    let probe = Rc::clone(&recorded);
    let listener = Listener::new().on_pointer_down(move |event: &PointerEvent| {
        let position = event.position();
        probe.set(Some((position.dx.get(), position.dy.get())));
    });
    (recorded, listener)
}

/// A delivered pointer-down event carries the position local to the
/// `Listener` that receives it, not the raw global dispatch position —
/// translation-only ancestry.
///
/// Flutter parity: `'simple offset for touch/signal'`. `Center` places the
/// 100×100 target at `((800-100)/2, (600-100)/2) = (350, 250)` on the
/// `harness::screen()` 800×600 surface; its center is global `(400, 300)`.
/// The `Listener`'s own local center is `(50, 50)`.
///
/// This case is translation-only (`with_paint_offset` is the only transform
/// pushed): the composition-order defect this file targets needs a second,
/// non-commuting part on the stack to surface (see the module doc), so this
/// passes both before and after the fix. It stays as the control case: if
/// THIS one goes red, the regression is somewhere else entirely.
#[test]
fn a_delivered_event_carries_the_receivers_local_position() {
    let (recorded, listener) = recording_listener();

    let laid = harness::pump_widget(
        Center::new().child(listener.child(target())),
        harness::screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(local, (50.0, 50.0), "translation-only local position");
    assert_ne!(
        local,
        (400.0, 300.0),
        "the recorded position must be LOCAL to the Listener, not the raw dispatch position"
    );
}

/// A scaled AND offset ancestor chain must still localise the delivered
/// position to the `Listener`'s own box.
///
/// Flutter parity: `'scaled and offset for touch/signal'`
/// (`listener_test.dart`). `Center(Transform(scale(2))(Listener(Container(100×100))))`:
/// `Transform`'s own laid-out size equals its child's untransformed size
/// (100×100 — the transform affects paint/hit-test, not layout), so `Center`
/// places it at `(350, 250)`, same as case 1 — the fixed point the scale
/// pivots around (`Alignment::TOP_LEFT`'s `alongSize` is `(0, 0)`, matching
/// Flutter's null-origin default). The painted container's own local center
/// `(50, 50)` therefore lands at the global point
/// `(350, 250) + 2 * (50, 50) = (450, 350)`.
///
/// RED before the fix: the pre-fix `HitTestResult` transform stack composes
/// this two-level (offset, scale) chain in the wrong order — left-multiplying
/// FORWARD parts and inverting once yields `inv(scale)·inv(offset)` where the
/// correct global→local mapping needs `inv(offset)·inv(scale)`. The two
/// coincide only when every part commutes (pure translations, case 1); they
/// do not here, so the recorded local position is wrong.
#[test]
fn a_scaled_and_offset_ancestor_localises_the_delivered_position() {
    let (recorded, listener) = recording_listener();

    let laid = harness::pump_widget(
        Center::new().child(
            Transform::scale(2.0, 2.0)
                .alignment(Alignment::TOP_LEFT)
                .child(listener.child(target())),
        ),
        harness::screen(),
    );

    laid.dispatch_pointer_down(450.0, 350.0);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(
        local,
        (50.0, 50.0),
        "a scaled+offset ancestor must still localise to the Listener's own 100x100 box",
    );
}

/// A rotated AND offset ancestor chain must still localise the delivered
/// position to the `Listener`'s own box.
///
/// Flutter parity: `'rotated for touch/signal'` (`listener_test.dart`).
/// Upstream dispatches at `downPosition = center + (10, 5)` and states
/// directly that `down.localPosition == Offset(50, 50) + Offset(5, -10) ==
/// (55, 40)` — Flutter's own quoted answer, not re-derived here. Rather than
/// hand-compute the rotated GLOBAL dispatch point (risking an independent
/// sign/handedness error unrelated to the bug under test), this forward-maps
/// that quoted local answer through the exact matrix the pipeline pushes
/// (`RenderTransform::hit_test_transform`, proven identical to
/// `paint_transform` by its own `paint_and_hit_test_share_one_transform` unit
/// test) to get the global dispatch point, then asserts the round trip
/// recovers Flutter's local answer.
///
/// RED before the fix: same composition-order defect as the scaled case —
/// rotation does not commute with the outer `Center` offset either.
///
/// # What this does NOT prove
///
/// Because `global_down` is derived by forward-mapping Flutter's own quoted
/// local answer through this same rotation matrix, the round trip is
/// tautological for ANY invertible rotation `R`: forward-map by `R`, then
/// localise by `R`'s inverse, always recovers the starting point, regardless
/// of whether `R`'s sign/handedness actually matches Flutter's rotation
/// convention. This case genuinely detects composition-ORDER bugs (offset
/// and rotation folded in the wrong sequence) — it went red before the fix
/// for exactly that reason — but it cannot by itself catch a wrong rotation
/// sign or handedness relative to Flutter, because the dispatch point it
/// tests against was computed from FLUI's own matrix, not from an
/// independently-derived global point the way upstream's hardcoded
/// `downPosition = center + (10, 5)` is.
#[test]
fn a_rotated_ancestor_localises_the_delivered_position() {
    let (recorded, listener) = recording_listener();

    let laid = harness::pump_widget(
        Center::new().child(
            Transform::new(Matrix4::rotation_z(std::f32::consts::FRAC_PI_2))
                .alignment(Alignment::TOP_LEFT)
                .child(listener.child(target())),
        ),
        harness::screen(),
    );

    // Same pivot as case 2: Transform's untransformed 100x100 box is centered
    // by `Center` on the 800x600 screen at (350, 250).
    let pivot = (350.0_f32, 250.0_f32);
    let local_down = (55.0_f32, 40.0_f32); // Flutter's own quoted local answer.
    let forward = Matrix4::rotation_z(std::f32::consts::FRAC_PI_2);
    let (dx, dy) = forward.transform_point(px(local_down.0), px(local_down.1));
    let global_down = (pivot.0 + dx.get(), pivot.1 + dy.get());

    laid.dispatch_pointer_down(global_down.0, global_down.1);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(
        local,
        local_down,
        "a rotated+offset ancestor must localise back to Flutter's own quoted local position",
    );
}

/// `RenderFractionalTranslation::hit_test` (`transform_hit_tests(true)`,
/// the default) pushes the pixel offset it computed onto the ctx-level
/// `BoxHitTestContext` transform stack (`ctx.push_offset(offset)`,
/// `crates/flui-objects/src/layout/fractional_translation.rs:251`) before
/// delegating to the shifted child. That ctx-level stack lived in a private
/// `BoxHitTestCtx` that `RenderBox::hit_test_raw`
/// (`crates/flui-rendering/src/traits/render_box.rs`) discarded when it
/// went out of scope, so `HitTestEntry.transform` never recorded the
/// offset even though `hit_test` itself moved `position` by that same
/// offset to decide which child pixel was hit.
///
/// Flutter parity: `RenderFractionalTranslation.hitTestChildren`
/// (`rendering/proxy_box.dart:3121-3132`) calls
/// `result.addWithPaintOffset(offset: Offset(translation.dx * size.width,
/// translation.dy * size.height), position: position, hitTest: ...)` —
/// the SAME `BoxHitTestResult` the whole walk shares records the offset,
/// there is no separate context to lose it in.
///
/// RED before the fix: a tap on the visually-shifted child delivered the
/// raw global position instead of the position local to the child's own
/// box.
#[test]
fn fractional_translation_true_localises_to_the_shifted_child() {
    let (recorded, listener) = recording_listener();

    // 100x100 target, translated by half its own size on both axes ->
    // painted (and, with `transform_hit_tests(true)`, hit-tested) at
    // global (50, 50)..(150, 150).
    let laid = harness::pump_widget(
        FractionalTranslation::new(0.5, 0.5).child(listener.child(target())),
        loose(200.0),
    );

    // Dead center of the shifted child's occupied region.
    laid.dispatch_pointer_down(100.0, 100.0);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(
        local,
        (50.0, 50.0),
        "a tap on the shifted child must localise to the child's own 100x100 box",
    );
    assert_ne!(
        local,
        (100.0, 100.0),
        "the recorded position must be LOCAL to the child, not the raw dispatch position"
    );
}

/// `transform_hit_tests(false)` is the control: `RenderFractionalTranslation`
/// takes `ctx.hit_test_child_at_layout_offset(0)`
/// (`crates/flui-objects/src/layout/fractional_translation.rs:259`), which
/// the pipeline driver already resolves and pushes correctly — this path
/// never touched the discarded ctx-level stack, so it must deliver the
/// correct position both before and after the fix.
///
/// Flutter parity: same `hitTestChildren`, `transformHitTests == false`
/// branch — `addWithPaintOffset(offset: null, ...)` performs no
/// transform at all, matching a layout-offset-only child test.
#[test]
fn fractional_translation_false_localises_to_the_unshifted_child() {
    let (recorded, listener) = recording_listener();

    let laid = harness::pump_widget(
        FractionalTranslation::new(0.5, 0.5)
            .transform_hit_tests(false)
            .child(listener.child(target())),
        loose(200.0),
    );

    // Within the child's ORIGINAL (unshifted) 100x100 bounds.
    laid.dispatch_pointer_down(30.0, 40.0);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(
        local,
        (30.0, 40.0),
        "transform_hit_tests(false) must localise against the child's original, unshifted \
         layout position",
    );
}

/// `RenderFractionallySizedBox::hit_test`
/// (`crates/flui-objects/src/layout/fractionally_sized_box.rs:319-328`) takes
/// `ctx.hit_test_child_at_offset(0, self.child_offset)` — the same
/// alignment-resolved offset `perform_layout` handed to `ctx.position_child`.
/// `hit_test_child_at_offset` (`crates/flui-rendering/src/context/hit_test.rs`)
/// subtracts that offset to compute the child-local position it hands to the
/// child's own `hit_test`, so hit/miss descent was always correct — but,
/// before the fix, never recorded the offset on the ctx-level transform
/// stack the way `hit_test_child_at_layout_offset` (and, since the sibling
/// fix in this same change, the ctx-level `push_offset`/`push_transform`
/// stack in general) does, so the RECORDED `HitTestEntry.transform` stayed
/// identity: delivery localized against the raw global dispatch point
/// instead of the child's own box.
///
/// Non-parity regression test — this is a FLUI-internal transform-stack
/// bookkeeping bug in a single ctx method, not Flutter behavior being ported.
///
/// A tight 200×200 `FractionallySizedBox(width_factor: 0.5, height_factor:
/// 0.5, alignment: CENTER)` constrains its 400×400 child down to a tight
/// 100×100 (`factor × incoming max` on each axis — see `child_constraints`)
/// and centers it inside its own 200×200 box, so `child_offset = (50, 50)`.
/// A tap at the global point `(60, 60)` lands `(10, 10)` inside the child's
/// own 100×100 box.
///
/// RED before the fix: the delivered local position was `(60, 60)` — the
/// raw global dispatch point, un-localized.
#[test]
fn fractionally_sized_box_localises_the_delivered_position_to_the_aligned_child() {
    let (recorded, listener) = recording_listener();

    let laid =
        harness::pump_widget(
            FractionallySizedBox::new()
                .width_factor(0.5)
                .height_factor(0.5)
                .alignment(Alignment::CENTER)
                .child(listener.child(
                    ColoredBox::new(Color::rgb(255, 0, 0)).child(SizedBox::new(400.0, 400.0)),
                )),
            tight(200.0, 200.0),
        );

    laid.dispatch_pointer_down(60.0, 60.0);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(
        local,
        (10.0, 10.0),
        "a tap on the centered, factor-shrunk child must localise to the child's own 100x100 box",
    );
    assert_ne!(
        local,
        (60.0, 60.0),
        "the recorded position must be LOCAL to the child, not the raw dispatch position"
    );
}

/// `RenderFittedBox::hit_test` (`crates/flui-objects/src/layout/fitted_box.rs`)
/// computes the child-local position by inverting `effective_transform()` —
/// the same scale/align matrix `paint_transform` hands the pipeline — but,
/// before the fix, called the raw `ctx.hit_test_child` directly with that
/// computed position, recording nothing: neither a driver-level
/// `hit_test_transform` override (the mechanism `RenderTransform` and
/// `RenderRotatedBox` use) nor a ctx-level `push_transform`/`with_transform`
/// scope (the mechanism `RenderFlow` uses for its own per-child delegate
/// matrix). Hit/miss descent was still correct — the computed local position
/// IS where the child sits — but the RECORDED `HitTestEntry.transform` stayed
/// identity, so delivery localized against the raw global dispatch point.
///
/// Non-parity regression test — a FLUI-internal transform-stack bookkeeping
/// bug, not Flutter behavior being ported.
///
/// `FittedBox::fit(BoxFit::Fill)` under tight 100×100 constraints stretches
/// its unconstrained-intrinsic 50×50 child to exactly fill the box on both
/// axes (`Fill` never crops or letterboxes): `scale_x = scale_y = 2.0`,
/// `align_offset = source_offset = Offset::ZERO`, so `effective_transform()`
/// is a pure `scale(2, 2)`. A tap at the global point `(30, 40)` lands at the
/// child-local point `(15, 20)` (divide by the scale).
///
/// RED before the fix: the delivered local position was `(30, 40)` — the raw
/// global dispatch point, un-localized.
#[test]
fn fitted_box_localises_the_delivered_position_through_the_scale() {
    let (recorded, listener) = recording_listener();

    let laid = harness::pump_widget(
        FittedBox::new().fit(BoxFit::Fill).child(
            listener.child(ColoredBox::new(Color::rgb(255, 0, 0)).child(SizedBox::new(50.0, 50.0))),
        ),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(30.0, 40.0);

    let local = recorded.get().expect("on_pointer_down must have fired");
    assert_close(
        local,
        (15.0, 20.0),
        "a tap through a 2x FittedBox scale must localise to the child's own unscaled box",
    );
    assert_ne!(
        local,
        (30.0, 40.0),
        "the recorded position must be LOCAL to the child, not the raw dispatch position"
    );
}
