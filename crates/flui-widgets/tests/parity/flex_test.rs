//! ## Test parity notes
//!
//! Flutter sources (tag `3.44.0`):
//! `packages/flutter/test/widgets/flex_test.dart`,
//! `packages/flutter/test/rendering/flex_test.dart`.
//!
//! Ported cases:
//! - `flex_test.dart` `'Flexible defaults to loose'` — `Flexible(child:
//!   SizedBox(100×200))` inside a `Row` receives loose constraints; its
//!   natural 100 px width wins.
//! - `flex_test.dart` `"Doesn't overflow because of floating point accumulated
//!   error"` — see `column_no_overflow_fp_test.rs` (ported there to keep file
//!   sizes small).
//! - `rendering/flex_test.dart` `'Space evenly'` —
//!   [`main_axis_alignment_space_evenly_distributes_free_space_symmetrically`].
//! - `rendering/flex_test.dart` `'Stretch'` —
//!   [`cross_axis_alignment_stretch_forces_children_to_fill_the_cross_axis`].
//! - `rendering/flex_test.dart` `'children with no baselines are top-aligned'`
//!   / the `Intrinsics` baseline group —
//!   [`cross_axis_alignment_baseline_aligns_children_by_reported_baseline`].
//! - `rendering/flex_test.dart` `'Flexible with MainAxisSize.min'` (the
//!   shrink-wrap half) —
//!   [`main_axis_size_min_shrink_wraps_while_max_fills_the_available_width`].
//! - `rendering/flex_test.dart` `'MainAxisSize.min inside unconstrained'`
//!   (third variant, asserting `isFlutterError`) — documented divergence, see
//!   [`unbounded_main_axis_degrades_flex_child_to_natural_size_instead_of_erroring`].
//! - The flex-factor allocation algorithm itself (`RenderFlex.performLayout`
//!   Pass 2, `flex.dart`), exercised generally by `rendering/flex_test.dart`'s
//!   `Intrinsics` group's flex=1/flex=2 setups —
//!   [`expanded_children_distribute_main_axis_by_flex_factor_two_to_one`].
//!
//! Widget → render-object mapping:
//! - `Row`/`Column` → `RenderFlex` (root)
//! - `Flexible`/`Expanded` → parent-data only (no render object of its own)
//! - `SizedBox(w, h)` → `RenderConstrainedBox` (child of `RenderFlex`)
//! - `Baseline` → `RenderBaseline` (reports a fixed baseline distance
//!   regardless of its child — FLUI's substitute for Flutter's
//!   `RenderFlowBaselineTestBox` test double, which the rendering-test file
//!   defines locally and this crate has no equivalent of; `RenderBaseline` is
//!   a real, shipped widget that happens to give the same deterministic
//!   baseline control)
//!
//! Divergence: Flutter's test uses `find.byType(SizedBox)` to locate the child;
//! FLUI uses `find_by_render_type("RenderConstrainedBox")` — the type-finder
//! operates on render objects, not widget types, per the documented finder design.
//! The geometry invariant (width == 100.0) is identical.
//!
//! **Unbounded main axis with flex children** (`unbounded_main_axis_degrades_flex_child_to_natural_size_instead_of_erroring`)
//! is a genuine, intentional divergence: Flutter raises a `FlutterError`
//! ("RenderFlex children have non-zero flex but incoming height constraints
//! are unbounded") — verified against `rendering/flex_test.dart`'s third
//! `'MainAxisSize.min inside unconstrained'` variant. FLUI's `RenderFlex`
//! (`crates/flui-objects/src/layout/flex.rs`, comment citing `flex.dart:1232`)
//! instead treats flex children as inflexible under an unbounded main axis —
//! no panic, no error signal, the child just takes its own natural size. This
//! is an existing, deliberate design choice already documented at the call
//! site, not a bug found during this port.

use crate::common::{offset, size, tight};
use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_view::ViewExt;
use flui_widgets::prelude::*;
// `row!` is intentionally absent from the prelude glob to avoid collision with
// `std`; import explicitly per the flui-widgets crate doc.
use flui_widgets::row;

use crate::harness;

/// `Flexible` (loose fit) wrapping a `SizedBox(100×200)` inside a `Row` must
/// let the child take its natural 100 px width.
///
/// Flutter parity: flex_test.dart line 70 — `box.size.width == 100.0`.
/// `Flexible` defaults to `FlexFit::Loose`: the child is given loose
/// constraints over its flex share, so it can be its natural width.
#[test]
fn flexible_defaults_to_loose_child_takes_natural_width() {
    let laid = harness::pump_widget(
        Row::new(row![Flexible::new(SizedBox::new(100.0, 200.0))]),
        harness::screen(),
    );

    // RenderFlex (Row) is the root; RenderConstrainedBox (SizedBox) is its child.
    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id).width,
        flui_types::geometry::px(100.0),
        "Flexible(loose) child SizedBox(100, 200) must retain its natural width of 100 px"
    );
}

/// `Expanded` (tight fit) forces its child to fill the flex share on the main axis.
///
/// Flutter parity: derived from flex_test.dart — `Expanded` is `Flexible`
/// with `FlexFit::Tight`, so the child must equal the full main-axis budget.
/// One `Expanded` child in an 800-wide `Row` must be 800 px wide.
#[test]
fn expanded_fills_available_main_axis_width() {
    let laid = harness::pump_widget(
        Row::new(row![flui_widgets::Expanded::new(SizedBox::shrink())]),
        harness::screen(),
    );

    let constrained_box_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(constrained_box_id),
        size(800.0, 0.0),
        "Expanded child must fill the full 800 px Row width; height is SizedBox::shrink height (0)"
    );
}

/// `MainAxisAlignment::SpaceEvenly` splits the free main-axis space into
/// `child_count + 1` equal gaps (leading, between-each-pair, trailing).
///
/// Flutter parity: `rendering/flex_test.dart` `'Space evenly'` — three
/// 100×100 boxes in a 500-wide row: free space = 500 − 300 = 200, split into
/// 4 gaps of 50 px each, giving child `dx` positions 50 / 200 / 350.
#[test]
fn main_axis_alignment_space_evenly_distributes_free_space_symmetrically() {
    let laid = harness::pump_widget(
        Row::new(vec![
            SizedBox::square(100.0).boxed(),
            SizedBox::square(100.0).boxed(),
            SizedBox::square(100.0).boxed(),
        ])
        .main_axis_alignment(MainAxisAlignment::SpaceEvenly),
        tight(500.0, 400.0),
    );

    let root = laid.root();
    // Cross axis (height) is 400 with default CrossAxisAlignment::Center:
    // (400 - 100) / 2 = 150 for every child.
    assert_eq!(laid.offset(laid.child(root, 0)), offset(50.0, 150.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(200.0, 150.0));
    assert_eq!(laid.offset(laid.child(root, 2)), offset(350.0, 150.0));
    for i in 0..3 {
        assert_eq!(laid.size(laid.child(root, i)), size(100.0, 100.0));
    }
}

/// `CrossAxisAlignment::Stretch` tightens the cross axis to its full bounded
/// extent, overriding a child's natural (shrink) cross size.
///
/// Flutter parity: `rendering/flex_test.dart` `'Stretch'` — a naturally
/// zero-sized box, once `crossAxisAlignment` becomes `stretch`, grows to the
/// full 100 px cross extent. FLUI's `SizedBox::shrink()` plays the same role
/// as Flutter's zero-sized `RenderDecoratedBox`.
#[test]
fn cross_axis_alignment_stretch_forces_children_to_fill_the_cross_axis() {
    let laid = harness::pump_widget(
        Row::new(vec![
            SizedBox::shrink().boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Stretch),
        BoxConstraints::loose(size(100.0, 100.0)),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(laid.child(root, 0)),
        size(0.0, 100.0),
        "a naturally 0×0 non-flex child must stretch to the full 100 px cross extent"
    );
    assert_eq!(
        laid.size(laid.child(root, 1)),
        size(100.0, 100.0),
        "the Expanded child fills the remaining 100 px main axis AND stretches cross-axis"
    );
}

/// Two `Expanded` children with flex factors 2 and 1 split the main axis
/// proportionally: 200 px and 100 px of a 300 px budget.
///
/// Flutter parity: the flex-factor allocation algorithm itself
/// (`RenderFlex.performLayout` Pass 2, `flex.dart`) — the same
/// `remaining * (flex / total_flex)` formula `rendering/flex_test.dart`'s
/// `Intrinsics` group exercises with flex 1/2 combinations (no single
/// dedicated 2:1 *layout* test exists upstream at this tag; the formula is
/// identical to the one already exercised for main-axis intrinsics).
#[test]
fn expanded_children_distribute_main_axis_by_flex_factor_two_to_one() {
    let laid = harness::pump_widget(
        Row::new(vec![
            Expanded::new(SizedBox::height(20.0)).flex(2).boxed(),
            Expanded::new(SizedBox::height(20.0)).flex(1).boxed(),
        ]),
        tight(300.0, 100.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(laid.child(root, 0)),
        size(200.0, 20.0),
        "flex=2 of total flex=3 over a 300px budget must get 200px"
    );
    assert_eq!(
        laid.size(laid.child(root, 1)),
        size(100.0, 20.0),
        "flex=1 of total flex=3 over a 300px budget must get 100px"
    );
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 40.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(200.0, 40.0));
}

/// `CrossAxisAlignment::Baseline` shifts each child so their reported
/// baselines land on the same horizontal line — the child with the smallest
/// baseline distance is pushed down the most.
///
/// Flutter parity: `rendering/flex_test.dart`'s baseline-alignment group
/// (`'children with no baselines are top-aligned'`, `'Vertical Flex
/// Baseline'`, and the `Intrinsics` baseline tests) all drive
/// `CrossAxisAlignment::baseline` through a test double
/// (`RenderFlowBaselineTestBox`) with a fixed, deterministic baseline.
/// FLUI has no equivalent test double — `Baseline` (`RenderBaseline`) is used
/// instead: it reports exactly its configured `baseline_offset` regardless of
/// its child, giving the same deterministic control.
#[test]
fn cross_axis_alignment_baseline_aligns_children_by_reported_baseline() {
    let laid = harness::pump_widget(
        Row::new(vec![
            Baseline::new(20.0, TextBaseline::Alphabetic)
                .child(SizedBox::new(30.0, 10.0))
                .boxed(),
            Baseline::new(50.0, TextBaseline::Alphabetic)
                .child(SizedBox::new(40.0, 10.0))
                .boxed(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Baseline),
        crate::common::loose(1000.0),
    );

    let root = laid.root();
    // max baseline = 50; child0 (baseline 20) shifts down by 50-20=30;
    // child1 (baseline 50) shifts down by 0 — both baselines now sit at y=50.
    assert_eq!(
        laid.offset(laid.child(root, 0)),
        offset(0.0, 30.0),
        "child with the smaller baseline distance (20) must shift down by 30px"
    );
    assert_eq!(
        laid.offset(laid.child(root, 1)),
        offset(30.0, 0.0),
        "child with the larger baseline distance (50) stays at the top"
    );
    assert_eq!(laid.size(laid.child(root, 0)), size(30.0, 20.0));
    assert_eq!(laid.size(laid.child(root, 1)), size(40.0, 50.0));
}

/// `MainAxisSize::Min` shrink-wraps to the sum of children; `MainAxisSize::Max`
/// (the default) fills the incoming bounded main-axis constraint instead.
///
/// Flutter parity: `rendering/flex_test.dart` `'Flexible with
/// MainAxisSize.min'` demonstrates the same min-vs-max size contrast
/// (`flex.size.width` 300 vs 500) via a `Flexible` fit toggle; this port
/// isolates the `MainAxisSize` half of that contrast directly, with plain
/// (non-flex) children.
#[test]
fn main_axis_size_min_shrink_wraps_while_max_fills_the_available_width() {
    let children = || {
        vec![
            SizedBox::new(50.0, 20.0).boxed(),
            SizedBox::new(80.0, 20.0).boxed(),
        ]
    };

    let min_laid = harness::pump_widget(
        Row::new(children()).main_axis_size(MainAxisSize::Min),
        crate::common::loose(500.0),
    );
    assert_eq!(
        min_laid.size(min_laid.root()),
        size(130.0, 20.0),
        "MainAxisSize::Min must shrink-wrap to the 50 + 80 = 130px child sum"
    );

    let max_laid = harness::pump_widget(
        Row::new(children()).main_axis_size(MainAxisSize::Max),
        crate::common::loose(500.0),
    );
    assert_eq!(
        max_laid.size(max_laid.root()),
        size(500.0, 20.0),
        "MainAxisSize::Max must fill the full 500px bounded main-axis constraint"
    );
}

/// Divergence: an `Expanded`/`Flexible` child under an unbounded main axis
/// does **not** raise an error in FLUI — it degrades to its own natural size.
///
/// Flutter parity: `rendering/flex_test.dart`'s third `'MainAxisSize.min
/// inside unconstrained'` variant asserts `exceptions.first is FlutterError`
/// ("RenderFlex children have non-zero flex but incoming height constraints
/// are unbounded") when a flex child sits under an unbounded main axis.
/// FLUI's `RenderFlex::compute_sizes` (`crates/flui-objects/src/layout/flex.rs`,
/// citing `flex.dart:1232`) instead treats flex children as inflexible in
/// that situation — a documented, intentional divergence, not a bug: the
/// `Expanded` child here simply reports its own 50×50 natural size, with no
/// panic and no error signal.
#[test]
fn unbounded_main_axis_degrades_flex_child_to_natural_size_instead_of_erroring() {
    let unbounded_height = BoxConstraints::new(
        px(0.0),
        px(300.0),
        px(0.0),
        flui_types::geometry::Pixels::INFINITY,
    );

    let laid = harness::pump_widget(
        Column::new(vec![Expanded::new(SizedBox::new(50.0, 50.0)).boxed()]),
        unbounded_height,
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root),
        size(50.0, 50.0),
        "Column must collapse to the flex child's natural 50×50 size, not throw, \
         under an unbounded main axis"
    );
    assert_eq!(
        laid.size(laid.child(root, 0)),
        size(50.0, 50.0),
        "the Expanded(SizedBox(50,50)) child must keep its own natural size"
    );
}
