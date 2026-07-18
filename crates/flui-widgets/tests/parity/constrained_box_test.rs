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

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::storage::IntrinsicDimension;
use flui_types::geometry::px;
use flui_widgets::ConstrainedBox;

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
