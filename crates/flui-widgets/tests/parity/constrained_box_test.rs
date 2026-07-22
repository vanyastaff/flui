//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/constrained_box_test.dart`
//! (tag `3.44.0`, 9 cases — every case in the file is a `ConstrainedBox`
//! intrinsics query, none touch layout size, hit-testing, or paint).
//!
//! Ported cases (8 of 9 — every `ConstrainedBox intrinsics - *` case):
//! - `'ConstrainedBox intrinsics - minHeight'` — [`constrained_box_intrinsics_min_height`].
//! - `'ConstrainedBox intrinsics - minWidth'` — [`constrained_box_intrinsics_min_width`].
//! - `'ConstrainedBox intrinsics - maxHeight'` — [`constrained_box_intrinsics_max_height`].
//! - `'ConstrainedBox intrinsics - maxWidth'` — [`constrained_box_intrinsics_max_width`].
//! - `'ConstrainedBox intrinsics - tight'` — [`constrained_box_intrinsics_tight`].
//! - `'ConstrainedBox intrinsics - minHeight - with infinite width'` —
//!   [`constrained_box_intrinsics_min_height_with_infinite_width`].
//! - `'ConstrainedBox intrinsics - minWidth - with infinite height'` —
//!   [`constrained_box_intrinsics_min_width_with_infinite_height`].
//! - `'ConstrainedBox intrinsics - infinite'` — [`constrained_box_intrinsics_infinite`].
//!
//! Out of scope (1 case): `'Placeholder intrinsics'` — asserts a bare
//! `Placeholder`'s own zero-intrinsic baseline (`getMin/MaxIntrinsicWidth/
//! Height(double.infinity)` all `0.0`). FLUI has no `Placeholder` widget, and
//! the invariant it establishes — a childless leaf reports `0.0` on every
//! intrinsic dimension — is exercised by every ported case below through
//! `RenderConstrainedBox`'s own childless branch (each oracle case wraps a
//! `Placeholder` child whose intrinsics are all `0.0`; a bare, childless
//! `ConstrainedBox` hits the identical `ctx.child_count() == 0` branch in
//! `RenderConstrainedBox::compute_{min,max}_intrinsic_{width,height}`), so no
//! separate port target remains.
//!
//! Every oracle case asserts all four of `getMinIntrinsicWidth`,
//! `getMaxIntrinsicWidth`, `getMinIntrinsicHeight`, and
//! `getMaxIntrinsicHeight` at `double.infinity` extent — each ported test
//! asserts the same four via the new
//! [`common::LaidOut::intrinsic_dimension`] harness primitive
//! (`PipelineOwner::box_intrinsic_dimension`, the production entry point
//! Flutter's `getMinIntrinsicWidth`-family maps to). No assertion is dropped
//! from any ported case.
//!
//! Widget → render-object mapping: `ConstrainedBox` → `RenderConstrainedBox`
//! (`crates/flui-objects/src/layout/constrained_box.rs`).
//!
//! Delta, not in the oracle: every case above ports a `Placeholder` child as
//! a bare, childless `ConstrainedBox`, so all 8 only exercise
//! `RenderConstrainedBox`'s childless (`ctx.child_count() == 0`) intrinsics
//! branch. [`constrained_box_intrinsics_with_child_delegates_then_clamps`]
//! closes that gap with a real, non-zero-intrinsic child, proving the
//! `ctx.child_count() > 0` delegation path and the outer bound's clamp both
//! run correctly end to end through the widget tree.

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::storage::IntrinsicDimension;
use flui_types::geometry::px;
use flui_widgets::{ConstrainedBox, SizedBox};

use crate::harness;

/// Mounts a childless `ConstrainedBox(constraints)` and asserts its four
/// intrinsic dimensions at `double.infinity` extent — Flutter's
/// `getMinIntrinsicWidth(double.infinity)` / `getMaxIntrinsicWidth` /
/// `getMinIntrinsicHeight` / `getMaxIntrinsicHeight` quartet.
fn assert_childless_intrinsics(
    constraints: BoxConstraints,
    expected_min_width: f32,
    expected_max_width: f32,
    expected_min_height: f32,
    expected_max_height: f32,
) {
    let laid = harness::pump_widget(ConstrainedBox::new(constraints), harness::screen());
    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");

    let min_width = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MinWidth,
        f32::INFINITY,
    );
    let max_width = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MaxWidth,
        f32::INFINITY,
    );
    let min_height = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MinHeight,
        f32::INFINITY,
    );
    let max_height = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MaxHeight,
        f32::INFINITY,
    );

    assert_eq!(min_width, expected_min_width, "min intrinsic width");
    assert_eq!(max_width, expected_max_width, "max intrinsic width");
    assert_eq!(min_height, expected_min_height, "min intrinsic height");
    assert_eq!(max_height, expected_max_height, "max intrinsic height");
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - minHeight'` (3.44.0) — `minHeight: 20.0`
/// forces both height intrinsics to `20.0`; width is untouched (`0.0`).
#[test]
fn constrained_box_intrinsics_min_height() {
    assert_childless_intrinsics(
        BoxConstraints::new(px(0.0), px(f32::INFINITY), px(20.0), px(f32::INFINITY)),
        0.0,
        0.0,
        20.0,
        20.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - minWidth'` (3.44.0) — `minWidth: 20.0`
/// forces both width intrinsics to `20.0`; height is untouched (`0.0`).
#[test]
fn constrained_box_intrinsics_min_width() {
    assert_childless_intrinsics(
        BoxConstraints::new(px(20.0), px(f32::INFINITY), px(0.0), px(f32::INFINITY)),
        20.0,
        20.0,
        0.0,
        0.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - maxHeight'` (3.44.0) — a bare upper bound
/// with no minimum leaves every intrinsic at `0.0` (the childless answer,
/// clamped against a lower bound of `0.0`).
#[test]
fn constrained_box_intrinsics_max_height() {
    assert_childless_intrinsics(
        BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(20.0)),
        0.0,
        0.0,
        0.0,
        0.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - maxWidth'` (3.44.0) — symmetric to the
/// `maxHeight` case above.
#[test]
fn constrained_box_intrinsics_max_width() {
    assert_childless_intrinsics(
        BoxConstraints::new(px(0.0), px(20.0), px(0.0), px(f32::INFINITY)),
        0.0,
        0.0,
        0.0,
        0.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - tight'` (3.44.0) — a fully tight
/// `10.0 × 30.0` box answers its own tight value on every intrinsic query,
/// without consulting the (nonexistent) child at all.
#[test]
fn constrained_box_intrinsics_tight() {
    assert_childless_intrinsics(
        BoxConstraints::new(px(10.0), px(10.0), px(30.0), px(30.0)),
        10.0,
        10.0,
        30.0,
        30.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - minHeight - with infinite width'` (3.44.0) —
/// an infinite `minWidth` (Flutter/FLUI's `hasInfiniteWidth`, keyed on the
/// MIN bound, not the max) short-circuits the width answer to the raw
/// childless `0.0` with no clamping; height behaves as the plain `minHeight`
/// case.
#[test]
fn constrained_box_intrinsics_min_height_with_infinite_width() {
    assert_childless_intrinsics(
        BoxConstraints::new(
            px(f32::INFINITY),
            px(f32::INFINITY),
            px(20.0),
            px(f32::INFINITY),
        ),
        0.0,
        0.0,
        20.0,
        20.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - minWidth - with infinite height'` (3.44.0) —
/// mirror of the case above on the other axis.
#[test]
fn constrained_box_intrinsics_min_width_with_infinite_height() {
    assert_childless_intrinsics(
        BoxConstraints::new(
            px(20.0),
            px(f32::INFINITY),
            px(f32::INFINITY),
            px(f32::INFINITY),
        ),
        20.0,
        20.0,
        0.0,
        0.0,
    );
}

/// Flutter parity: `constrained_box_test.dart`
/// `'ConstrainedBox intrinsics - infinite'` (3.44.0) — fully infinite tight
/// constraints on both axes: `hasInfiniteWidth`/`hasInfiniteHeight` both fire
/// (their MIN bound is infinite), short-circuiting every dimension to the raw
/// childless `0.0`.
#[test]
fn constrained_box_intrinsics_infinite() {
    assert_childless_intrinsics(
        BoxConstraints::new(
            px(f32::INFINITY),
            px(f32::INFINITY),
            px(f32::INFINITY),
            px(f32::INFINITY),
        ),
        0.0,
        0.0,
        0.0,
        0.0,
    );
}

/// Not in the oracle (every `constrained_box_test.dart` case wraps a
/// zero-intrinsic `Placeholder`, so all 8 ported cases above only exercise
/// `RenderConstrainedBox`'s CHILDLESS intrinsics branch): a `ConstrainedBox`
/// with a real, non-zero-intrinsic child closes that gap, proving
/// `ctx.child_count() > 0`'s delegation to `ctx.child_{min,max}_intrinsic_
/// {width,height}` actually runs, not just the `0.0` fallback.
///
/// `ConstrainedBox(minHeight: 20)` wrapping a tight `30×5` `SizedBox`:
/// width is unbounded on the outer box, so it delegates straight to the
/// child's own tight intrinsic width (`30`, unaffected by the `height`
/// extent argument — a `SizedBox`'s tight fast path ignores it). Height
/// delegates to the child's tight intrinsic height (`5`), but the outer
/// `minHeight: 20` then clamps that UP to `20` — the child's smaller
/// intrinsic loses to the box's own bound, exactly as it must when a
/// `ConstrainedBox` widens a small child rather than merely reporting it.
#[test]
fn constrained_box_intrinsics_with_child_delegates_then_clamps() {
    let laid = harness::pump_widget(
        ConstrainedBox::new(BoxConstraints::new(
            px(0.0),
            px(f32::INFINITY),
            px(20.0),
            px(f32::INFINITY),
        ))
        .child(SizedBox::new(30.0, 5.0)),
        harness::screen(),
    );
    // The outer `ConstrainedBox` is the tree root; its `SizedBox` child is
    // ALSO a `RenderConstrainedBox` (see `SizedBox`'s widget→render mapping),
    // so `find_by_render_type` alone would be ambiguous here.
    let constrained_box_id = laid.current_root();

    let min_width = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MinWidth,
        f32::INFINITY,
    );
    let max_width = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MaxWidth,
        f32::INFINITY,
    );
    let min_height = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MinHeight,
        f32::INFINITY,
    );
    let max_height = laid.intrinsic_dimension(
        constrained_box_id,
        IntrinsicDimension::MaxHeight,
        f32::INFINITY,
    );

    assert_eq!(
        min_width, 30.0,
        "delegates straight to the child's own tight width"
    );
    assert_eq!(
        max_width, 30.0,
        "delegates straight to the child's own tight width"
    );
    assert_eq!(
        min_height, 20.0,
        "the child's 5 loses to the box's own minHeight: 20"
    );
    assert_eq!(
        max_height, 20.0,
        "the child's 5 loses to the box's own minHeight: 20"
    );
}
