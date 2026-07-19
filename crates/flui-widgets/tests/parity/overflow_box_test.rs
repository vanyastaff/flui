//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/overflow_box_test.dart`
//! (tag `3.44.0`, 6 `testWidgets` cases). `SizedOverflowBox` has no separate
//! oracle file — `sized_overflow_box_test.dart` does not exist anywhere in
//! Flutter's history (`git log --all` on the Flutter checkout returns no
//! commits for that path); its two cases live in `overflow_box_test.dart`
//! itself (`'SizedOverflowBox alignment'` and
//! `'SizedOverflowBox alignment (direction-sensitive)'`).
//!
//! Both render objects (`RenderConstrainedOverflowBox`,
//! `RenderSizedOverflowBox` in `crates/flui-objects/src/layout/overflow_box.rs`)
//! implement the real Flutter algorithm — per-axis constraint overrides via
//! `_getInnerConstraints`, `OverflowBoxFit::Max`/`DeferToChild` self-sizing,
//! and alignment-driven (possibly negative, i.e. overflowing) child
//! placement through `AligningShiftedBox` — already unit- and harness-tested
//! (`inner_constraints_*`, `parent_size_*` in the module's own `#[cfg(test)]`
//! block; `harness_constrained_overflow_box_*` /
//! `harness_sized_overflow_box_*` in
//! `crates/flui-objects/tests/render_object_harness.rs`). This file adds the
//! missing layer those don't cover: the two widgets wired through `View` →
//! render object via a real widget tree (`pump_widget`), reproducing the
//! Dart oracle's own tree shapes and constraint chains.
//!
//! Ported cases (5 of 6):
//! - `'OverflowBox control test'` — [`overflow_box_control_test`]. An
//!   `Align(bottomRight)` around a tight `SizedBox(10, 20)` around an
//!   `OverflowBox` (overrides `0..100 × 0..50`) around a fill child —
//!   exercises `OverflowBoxFit::Max` (default) self-sizing to the tight
//!   incoming slot while the child overflows both larger AND with a
//!   **negative** alignment offset (the box ends up smaller than its
//!   center-aligned child on both axes).
//! - `'OverflowBox behavior with long and short content'` (parameterized
//!   `contentSuperLong` ∈ `{false, true}`) —
//!   [`overflow_box_defer_to_child_clamps_to_outer_constraints`]. Both
//!   sub-cases asserted in one test body, mirroring how
//!   `aspect_ratio_test.rs`'s `aspect_ratio_control_test` folds a Dart `for`
//!   loop's iterations into one `#[test]`. `OverflowBoxFit::DeferToChild`
//!   with a `max_height` override large enough (1,000,000) that the
//!   override never binds the child, but the box's own reported size still
//!   clamps to the **outer** (non-overridden) constraints — proving the
//!   override only reaches `_getInnerConstraints`/`inner_constraints`, not
//!   the `constraints.constrain(child_size)` self-sizing call.
//! - `'no child'` — [`overflow_box_defer_to_child_no_child_sizes_to_smallest`].
//!   A childless `OverflowBoxFit::DeferToChild` box sizes to the outer
//!   constraints' smallest (0×0 under a loose surface), not the max-height
//!   override.
//! - `'SizedOverflowBox alignment'` —
//!   [`sized_overflow_box_alignment_places_the_child_by_its_own_slot`]. A
//!   `Center` around a fixed 100×100 `SizedOverflowBox` (`Alignment.topRight`)
//!   around a 50×50 child — the child is laid out under the *incoming*
//!   constraints (the key `SizedOverflowBox` contract: it claims one size,
//!   the child lives in another) and is aligned within the claimed 100×100
//!   slot, not the child's own bounds.
//! - `'SizedOverflowBox alignment (direction-sensitive)'` —
//!   [`sized_overflow_box_alignment_resolves_directional_alignment`]. Same
//!   tree with `AlignmentDirectional.bottomStart` under RTL. FLUI's
//!   `SizedOverflowBox::with_alignment` (like every shifted-box-family widget
//!   in FLUI — `Align`, `Center`, and this whole family — takes an already
//!   resolved `Alignment`, not an `AlignmentGeometry`/`AlignmentDirectional`
//!   plus ambient `Directionality`; there is no code path here that reads a
//!   `TextDirection`). This test resolves `AlignmentDirectional::BOTTOM_START`
//!   with `resolve(false)` (RTL) at the call site — the same resolution the
//!   Dart oracle's build phase performs internally — and asserts the
//!   identical resulting position. It proves `Alignment::resolve`'s RTL
//!   math and the box's alignment placement agree with the oracle; it does
//!   **not** exercise automatic ambient-`Directionality` resolution inside
//!   the widget tree, because that capability does not exist yet for this
//!   widget family (a pre-existing, systemic gap across `Align`/`Center`/
//!   `OverflowBox`/`SizedOverflowBox`, not something introduced or
//!   discovered as OverflowBox-specific — out of scope for a parity-test
//!   port to newly build). Filed as a follow-up gap, not a regression.
//!
//! Out of scope (1 of 6):
//! - `'OverflowBox implements debugFillProperties'` — asserts the exact
//!   ordered `DiagnosticPropertiesBuilder` string list (including
//!   `ifNull: 'use parent ... constraint'` placeholder text and an
//!   `'alignment: Alignment.center'` entry contributed by the
//!   `RenderAligningShiftedBox` base class). This is a diagnostics/
//!   introspection formatting test, not a layout-geometry test — outside
//!   this slice's "deterministic layout widget" scope. It would also fail
//!   today: `RenderConstrainedOverflowBox::debug_fill_properties`
//!   (`crates/flui-objects/src/layout/overflow_box.rs`) never emits an
//!   `alignment` property and has no `ifNull` placeholder text for unset
//!   overrides — a real but separate diagnostics-fidelity gap, not opened
//!   as a Cross.H here since it sits outside layout geometry.
//!
//! Denominator: 6 upstream `testWidgets` cases (all in
//! `overflow_box_test.dart`), 5 ported (one covering 2 parameterized
//! sub-cases), 1 out of scope with reason recorded above.
//!
//! Widget → render-object mapping:
//! `OverflowBox` (`crates/flui-widgets/src/layout/overflow_box.rs`) →
//! `RenderConstrainedOverflowBox`; `SizedOverflowBox`
//! (`crates/flui-widgets/src/layout/sized_overflow_box.rs`) →
//! `RenderSizedOverflowBox` (both in
//! `crates/flui-objects/src/layout/overflow_box.rs`).
//!
//! Scaffolding simplification: the Dart oracle wraps its `deferToChild`
//! cases in `Directionality(ltr) > Stack > Container(key)`. `Stack`'s
//! `StackFit.loose` default hands each non-positioned child
//! `constraints.loosen()` (here `0..800 × 0..600`, from the 800×600 tight
//! test surface) and positions it at the top-left (`AlignmentDirectional
//! .topStart`, the Stack default); `Container` with no size/child of its own
//! passes its child's size straight through. Neither changes the geometry
//! this test asserts, so — following the same reasoning
//! `aspect_ratio_test.rs` uses to drop scaffolding that doesn't affect its
//! assertion — these tests mount `OverflowBox` directly as the pumped root
//! under the equivalent `loose(800, 600)` constraints and read its own
//! committed size, which is exactly what `Container`'s bottom-left would
//! have reported (top-left `(0, 0)` plus the box's own height).

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::layout::AlignmentDirectional;
use flui_types::{Alignment, Size};
use flui_widgets::{
    Align, Center, Column, MainAxisSize, OverflowBox, SizedBox, SizedOverflowBox, column,
};

use crate::common::{offset, size};
use crate::harness;

/// A loose `width × height` box: `min = 0`, `max = (width, height)` —
/// Flutter parity: `BoxConstraints.loose(Size(width, height))`.
fn loose(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(width), px(0.0), px(height))
}

/// `OverflowBox` claims its (tight) incoming slot and lets a center-aligned
/// fill child overflow it on every side — including with a **negative**
/// alignment offset, since the child (sized to the override's 100×50 upper
/// bound) is bigger than the 10×20 box it's centered within.
///
/// Flutter parity: `'OverflowBox control test'` (`overflow_box_test.dart`,
/// tag `3.44.0`) — `Align(bottomRight)` positions a tight `SizedBox(10, 20)`
/// at the test surface's bottom-right corner; its child `OverflowBox`
/// (overrides `minWidth: 0, maxWidth: 100, minHeight: 0, maxHeight: 50`)
/// reports `OverflowBoxFit::Max`'s default self-size (`constraints.biggest()`
/// of its own tight 10×20 incoming slot, i.e. 10×20 — unchanged), while its
/// fill child is laid out under the overridden loose `0..100 × 0..50` and
/// grows to (100, 50), then centers within the 10×20 box at a negative
/// offset. The oracle asserts the CHILD's (not the box's) global offset and
/// size.
#[test]
fn overflow_box_control_test() {
    let laid = harness::pump_widget(
        Align::new(Alignment::BOTTOM_RIGHT).child(
            SizedBox::new(10.0, 20.0).child(
                OverflowBox::new()
                    .with_min_width(px(0.0))
                    .with_max_width(px(100.0))
                    .with_min_height(px(0.0))
                    .with_max_height(px(50.0))
                    .child(SizedBox::expand()),
            ),
        ),
        harness::screen(),
    );

    let root = laid.root();
    let sized_box = laid.only_child(root);
    let overflow_box = laid.only_child(sized_box);
    let fill_child = laid.only_child(overflow_box);

    assert_eq!(
        laid.size(fill_child),
        size(100.0, 50.0),
        "the fill child grows to the override's loose upper bound (100x50), \
         not the 10x20 box it lives in",
    );
    assert_eq!(
        laid.absolute_offset(fill_child),
        offset(745.0, 565.0),
        "the fill child's global offset must account for the box's \
         bottom-right position (790, 580) PLUS a negative center-alignment \
         offset ((10-100)/2, (20-50)/2) = (-45, -15), since the child \
         overflows the box on every side",
    );
}

/// `OverflowBoxFit::DeferToChild`'s self-reported size follows
/// `outer_constraints.constrain(child_size)` — the box's OWN incoming
/// constraints, never the per-axis override (which only reaches the child's
/// `_getInnerConstraints`/`inner_constraints`). A `max_height` override of
/// 1,000,000 lets the child grow arbitrarily large, but the box's own height
/// still clamps to the outer loose bound.
///
/// Flutter parity: `'OverflowBox behavior with long and short content'`
/// (`overflow_box_test.dart`, tag `3.44.0`), parameterized over
/// `contentSuperLong` ∈ `{false, true}` — both sub-cases asserted here in one
/// test, mirroring `aspect_ratio_test.rs`'s handling of a Dart `for` loop.
/// A `Column` (`mainAxisSize: min`) wrapping one `SizedBox(width: 100,
/// height: contentSuperLong ? 10000 : 100)` sits inside the overridden
/// `OverflowBox`; the oracle asserts `getBottomLeft(key).dy` — the
/// scaffolding `Container`'s bottom-left, which (see the module doc's
/// scaffolding-simplification note) equals the box's own top-left (0) plus
/// its own committed height.
#[test]
fn overflow_box_defer_to_child_clamps_to_outer_constraints() {
    for (content_super_long, expected_height) in [(false, 100.0), (true, 600.0)] {
        let child_height = if content_super_long { 10_000.0 } else { 100.0 };
        let laid = harness::pump_widget(
            OverflowBox::new()
                .with_max_height(px(1_000_000.0))
                .with_fit(flui_widgets::OverflowBoxFit::DeferToChild)
                .child(
                    Column::new(column![SizedBox::new(100.0, child_height)])
                        .main_axis_size(MainAxisSize::Min),
                ),
            loose(800.0, 600.0),
        );

        let height = laid.size(laid.root()).height.get();
        assert!(
            (height - expected_height).abs() < 1e-3,
            "contentSuperLong={content_super_long}: DeferToChild must clamp \
             its own height to the outer loose bound (600), not the \
             max_height override (1,000,000); got {height}, expected \
             {expected_height}",
        );
    }
}

/// A childless `OverflowBoxFit::DeferToChild` box sizes to the outer
/// constraints' smallest — the max-height override never applies because
/// there is no child to lay out under it.
///
/// Flutter parity: `'no child'` (`overflow_box_test.dart`, tag `3.44.0`) —
/// asserts `getBottomLeft(key).dy == 0` for a childless `OverflowBox`
/// (`maxHeight: 1000000`, `fit: deferToChild`) under the same
/// `Stack`/`Container` scaffolding as the previous case; per the module
/// doc's scaffolding-simplification note this equals the box's own
/// committed height, which must be 0.
#[test]
fn overflow_box_defer_to_child_no_child_sizes_to_smallest() {
    let laid = harness::pump_widget(
        OverflowBox::new()
            .with_max_height(px(1_000_000.0))
            .with_fit(flui_widgets::OverflowBoxFit::DeferToChild),
        loose(800.0, 600.0),
    );

    assert_eq!(
        laid.size(laid.root()),
        size(0.0, 0.0),
        "a childless DeferToChild box must size to the outer constraints' \
         smallest (0x0 under a loose surface), not the max_height override",
    );
}

/// `SizedOverflowBox` claims a fixed 100×100 slot; its 50×50 child is laid
/// out under the INCOMING constraints (not the requested size) and aligned
/// within the claimed slot — proving the box's claimed size and its child's
/// alignment slot are the same rectangle, independent of the child's own
/// size.
///
/// Flutter parity: `'SizedOverflowBox alignment'` (`overflow_box_test.dart`,
/// tag `3.44.0`) — `Center` around a fixed `SizedOverflowBox(size: 100x100,
/// alignment: topRight)` around a 50×50 child on the 800×600 test surface;
/// asserts the child's own size (50x50, unaffected by the 100x100 claim) and
/// its global center point.
#[test]
fn sized_overflow_box_alignment_places_the_child_by_its_own_slot() {
    let laid = harness::pump_widget(
        Center::new().child(
            SizedOverflowBox::new(Size::new(px(100.0), px(100.0)))
                .with_alignment(Alignment::TOP_RIGHT)
                .child(SizedBox::new(50.0, 50.0)),
        ),
        harness::screen(),
    );

    let root = laid.root();
    let sized_overflow_box = laid.only_child(root);
    let leaf = laid.only_child(sized_overflow_box);

    assert_eq!(
        laid.size(leaf),
        size(50.0, 50.0),
        "the child keeps its own 50x50 size, unaffected by the box's 100x100 claim",
    );

    let leaf_offset = laid.absolute_offset(leaf);
    let leaf_size = laid.size(leaf);
    let center_x = leaf_offset.dx.get() + leaf_size.width.get() / 2.0;
    let center_y = leaf_offset.dy.get() + leaf_size.height.get() / 2.0;
    assert!(
        (center_x - 425.0).abs() < 1e-3 && (center_y - 275.0).abs() < 1e-3,
        "the child's global center must be (425, 275) -- Center puts the \
         100x100 box at (350, 250), then Alignment.topRight offsets the \
         50x50 child by (50, 0) within it; got ({center_x}, {center_y})",
    );
}

/// The same tree as [`sized_overflow_box_alignment_places_the_child_by_its_own_slot`],
/// but with `AlignmentDirectional::BOTTOM_START` resolved under RTL — which
/// resolves to `Alignment::BOTTOM_RIGHT` (`resolve` flips only the
/// horizontal sign for RTL, leaving `y` untouched), moving the child from
/// the top-right corner to the bottom-right corner of the claimed slot.
///
/// Flutter parity: `'SizedOverflowBox alignment (direction-sensitive)'`
/// (`overflow_box_test.dart`, tag `3.44.0`) — identical tree under
/// `Directionality(rtl)` with `alignment: AlignmentDirectional.bottomStart`.
/// FLUI's `SizedOverflowBox` (like `Align`/`Center` and the rest of this
/// widget family) takes an already-resolved `Alignment`, not an
/// `AlignmentGeometry` plus ambient `Directionality` — see the module doc's
/// case-5 note for why this test resolves at the call site instead of
/// threading a `Directionality` ancestor through the tree.
#[test]
fn sized_overflow_box_alignment_resolves_directional_alignment() {
    let resolved = AlignmentDirectional::BOTTOM_START.resolve(false); // RTL
    assert_eq!(
        resolved,
        Alignment::BOTTOM_RIGHT,
        "AlignmentDirectional::BOTTOM_START under RTL must resolve to \
         BOTTOM_RIGHT (only the horizontal sign flips)",
    );

    let laid = harness::pump_widget(
        Center::new().child(
            SizedOverflowBox::new(Size::new(px(100.0), px(100.0)))
                .with_alignment(resolved)
                .child(SizedBox::new(50.0, 50.0)),
        ),
        harness::screen(),
    );

    let root = laid.root();
    let sized_overflow_box = laid.only_child(root);
    let leaf = laid.only_child(sized_overflow_box);

    assert_eq!(laid.size(leaf), size(50.0, 50.0));

    let leaf_offset = laid.absolute_offset(leaf);
    let leaf_size = laid.size(leaf);
    let center_x = leaf_offset.dx.get() + leaf_size.width.get() / 2.0;
    let center_y = leaf_offset.dy.get() + leaf_size.height.get() / 2.0;
    assert!(
        (center_x - 425.0).abs() < 1e-3 && (center_y - 325.0).abs() < 1e-3,
        "the child's global center must be (425, 325) -- same x as the \
         topRight case (right-aligned in both), but bottom-aligned instead \
         of top-aligned; got ({center_x}, {center_y})",
    );
}
