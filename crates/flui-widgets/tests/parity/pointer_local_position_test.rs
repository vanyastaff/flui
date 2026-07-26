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
