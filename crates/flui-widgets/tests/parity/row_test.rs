//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/row_test.dart`, tag
//! `3.44.0` — 29 `testWidgets` cases across three axes (`- no textDirection`
//! / `- LTR` / `- RTL`) plus two `Row.spacing` cases.
//!
//! Widget → render-object mapping:
//! - `Row` → `RenderFlex` (`direction: Horizontal`)
//! - `Expanded` → parent-data only (`FlexFit::Tight`), no render object of
//!   its own — same mapping `flex_test.rs` already documents.
//! - `SizedBox(w, h)` → `RenderConstrainedBox`
//! - `Center` → `RenderCenter`
//!
//! Divergence (paint order): the oracle wraps every child in
//! `CustomPaint(painter: OrderPainter(n))` and asserts `OrderPainter.log ==
//! [1, 2, 3, ...]` after each pump, pinning sibling *paint* order alongside
//! layout. `LaidOut` has no paint-count/paint-order introspection — the same
//! gap `mouse_region_test.rs`'s own module doc documents ("No paint-count
//! introspection"). Every ported case below carries the full position/size
//! signal from its oracle counterpart; only the paint-order sub-assertion is
//! dropped.
//!
//! # Cross.H: flex text-direction gap
//!
//! `RenderFlex` (`crates/flui-objects/src/layout/flex.rs`) carries no
//! `text_direction` field at all, and `Row`/`Column`/`Flex`
//! (`crates/flui-widgets/src/flex/flex.rs`) lay out unconditionally as if
//! `TextDirection.ltr`, with no ambient `Directionality` lookup and no
//! "direction required" assertion. This blocks two whole axes of the
//! oracle: the 8 `- RTL` cases (mirrored positions) and 8 of the 9 `- no
//! textDirection` cases (a debug `AssertionError` (plain `assert`s in `flex.dart`, not a `FlutterError` construction), "has a null
//! textDirection", from `RenderFlex._debugHasNecessaryDirections`,
//! `rendering/flex.dart`, tag `3.44.0`). Filed as an extension of the
//! existing shifted-box `AlignmentDirectional`/ambient-`Directionality` gap
//! in `docs/ROADMAP.md`'s Cross.H section — see there for the shared root
//! cause, not repeated here.
//!
//! # Ledger (29 case names — every `testWidgets` in `row_test.dart` at tag
//! `3.44.0`; recounted against the accounting below — totals match)
//!
//! ## No textDirection (9)
//! 1. `'Row with one Flexible child - no textDirection'` — **out of
//!    scope**, Cross.H (asserts the `FlutterError`).
//! 2. `'Row with default main axis parameters - no textDirection'` — **out
//!    of scope**, Cross.H.
//! 3. `'Row with MainAxisAlignment.center - no textDirection'` — **out of
//!    scope**, Cross.H.
//! 4. `'Row with MainAxisAlignment.end - no textDirection'` — **out of
//!    scope**, Cross.H.
//! 5. `'Row with MainAxisAlignment.spaceBetween - no textDirection'` —
//!    **out of scope**, Cross.H.
//! 6. `'Row with MainAxisAlignment.spaceAround - no textDirection'` — **out
//!    of scope**, Cross.H.
//! 7. `'Row with MainAxisAlignment.spaceEvenly - no textDirection'` —
//!    **out of scope**, Cross.H.
//! 8. `'Row and MainAxisSize.min - no textDirection'` — **out of scope**,
//!    Cross.H.
//! 9. `'Row MainAxisSize.min layout at zero size - no textDirection'` —
//!    **duplicate, not separately ported.** Unlike cases 1–8, this one does
//!    NOT set `textDirection` and does NOT expect a `FlutterError` — with a
//!    single child under `MainAxisSize.min` the free main-axis space is
//!    always zero, so `mainAxisAlignment` has nothing to resolve and
//!    `RenderFlex._debugHasNecessaryDirections` never fires. Its assertion
//!    (`renderBox.size == Size(100, 0)`) never reads `parentData.offset`, so
//!    it is direction-independent — byte-for-byte the same check as case
//!    18. Ported once, as case 18.
//!
//! ## LTR (9)
//! 10. `'Row with one Flexible child - LTR'` — **duplicate**,
//!     cross-referenced against `flex_test.rs`'s
//!     `expanded_fills_available_main_axis_width` (`Expanded` fills the main
//!     axis) and
//!     `expanded_children_distribute_main_axis_by_flex_factor_two_to_one`
//!     (multi-child `Expanded` position math) — not re-ported. **Coverage
//!     gap noted:** neither existing test reproduces this oracle case's
//!     exact fixed-`Expanded`-fixed 3-child sandwich (`child0` dx=0,
//!     `Expanded` dx=100 w=600, `child2` dx=700) in one tree; the mechanism
//!     (fixed siblings keep their natural size, `Expanded` claims the rest)
//!     is exercised, the specific 3-child arrangement is not independently
//!     pinned.
//! 11. `'Row with default main axis parameters - LTR'` — **ported**:
//!     [`row_default_main_axis_parameters_positions_children_sequentially_from_start`].
//!     Re-scoped from "duplicate" to "port": no existing test (in
//!     `flex_test.rs` or the `flui-objects` render-object harness)
//!     reproduces a `Row`-*widget* with 3 equal, untagged children under
//!     default alignment/size —
//!     `harness_flex_row_positions_children_on_main_axis`
//!     (`flui-objects/tests/render_object_harness.rs`) checks the identical
//!     positioning mechanism one layer down, with `RenderFlex` built
//!     directly, bypassing the `Row` widget → render-object wiring this
//!     port exists to prove.
//! 12. `'Row with MainAxisAlignment.center - LTR'` — **ported**:
//!     [`row_main_axis_alignment_center_centers_the_children_block`].
//! 13. `'Row with MainAxisAlignment.end - LTR'` — **ported**:
//!     [`row_main_axis_alignment_end_pushes_children_to_the_far_edge`].
//! 14. `'Row with MainAxisAlignment.spaceBetween - LTR'` — **ported**:
//!     [`row_main_axis_alignment_space_between_flushes_the_first_and_last_children_to_the_edges`].
//! 15. `'Row with MainAxisAlignment.spaceAround - LTR'` — **ported**:
//!     [`row_main_axis_alignment_space_around_gives_edge_children_half_gaps`].
//! 16. `'Row with MainAxisAlignment.spaceEvenly - LTR'` — **duplicate**,
//!     cross-referenced against `flex_test.rs`'s
//!     `main_axis_alignment_space_evenly_distributes_free_space_symmetrically`
//!     — the same `n + 1`-equal-gaps formula, verified against different
//!     concrete numbers (200px children / 800px row here vs. 100px children
//!     / 500px row there); one delta: the oracle also asserts the row's own (800,100) size under loose constraints, which the cited test's tight(500,400) constraints cannot pin — redundantly covered by the alignment ports' row-size asserts here.
//! 17. `'Row and MainAxisSize.min - LTR'` — **duplicate**, cross-referenced
//!     against `flex_test.rs`'s
//!     `main_axis_size_min_shrink_wraps_while_max_fills_the_available_width`
//!     — same shrink-to-child-sum formula (`Row` size only). **Coverage gap
//!     noted:** the existing test does not assert child *positions*
//!     (`child0` dx=0, `child1` dx=100); default-`MainAxisAlignment::Start`
//!     cumulative-offset placement is exercised elsewhere (case 11's new
//!     port; `harness_flex_row_positions_children_on_main_axis`), so the
//!     residual gap is the specific min-size + position combination, not
//!     the underlying placement mechanism.
//! 18. `'Row MainAxisSize.min layout at zero size - LTR'` — **ported**:
//!     [`row_main_axis_size_min_layout_at_zero_size_collapses_the_child_to_zero_height`].
//!     Also accounts for cases 9 and 27 (see their entries above/below).
//!
//! ## RTL (9)
//! 19. `'Row with one Flexible child - RTL'` — **out of scope**, Cross.H
//!     (mirrored positions require `RenderFlex` to resolve `TextDirection`).
//! 20. `'Row with default main axis parameters - RTL'` — **out of scope**,
//!     Cross.H.
//! 21. `'Row with MainAxisAlignment.center - RTL'` — **out of scope**,
//!     Cross.H.
//! 22. `'Row with MainAxisAlignment.end - RTL'` — **out of scope**,
//!     Cross.H.
//! 23. `'Row with MainAxisAlignment.spaceBetween - RTL'` — **out of
//!     scope**, Cross.H.
//! 24. `'Row with MainAxisAlignment.spaceAround - RTL'` — **out of scope**,
//!     Cross.H.
//! 25. `'Row with MainAxisAlignment.spaceEvenly - RTL'` — **out of scope**,
//!     Cross.H.
//! 26. `'Row and MainAxisSize.min - RTL'` — **out of scope**, Cross.H.
//! 27. `'Row MainAxisSize.min layout at zero size - RTL'` — **duplicate,
//!     not separately ported.** Like case 9, this checks only
//!     `renderBox.size == Size(100, 0)` — `parentData.offset` (the one
//!     assertion RTL mirroring would actually change) is never read, so the
//!     `- RTL` label carries no RTL-specific signal here. Byte-for-byte the
//!     same check as case 18. Ported once, as case 18.
//!
//! ## `Row.spacing` (2, implementation-gated)
//! 28. `'Default Row.spacing value'` — **ported**:
//!     [`row_spacing_defaults_to_zero_on_the_mounted_render_object`].
//! 29. `'Can update Row.spacing value'` — **ported**:
//!     [`row_spacing_rebuild_changes_the_total_main_axis_extent`].
//!
//! **Total: 29 case names = 29 accounted for above** — 8 newly ported + 3
//! cross-file duplicates against `flex_test.rs` (cases 10, 16, 17) + 2
//! in-file duplicates folded into case 18 (cases 9, 27) + 16 out of scope
//! under the Cross.H flex text-direction gap (8 `- no textDirection` error
//! cases + 8 `- RTL` cases).
//!
//! # Implementation addition: `Row`/`Column`/`Flex::spacing`
//!
//! `RenderFlex` already carried a tested `spacing: f32` field
//! (`with_spacing`, `crates/flui-objects/src/layout/flex.rs`) with its own
//! default/setter unit tests; only the widget-level builder knob was
//! missing. Added `.spacing(f32)` to the shared `flex_style_builders!` macro
//! (`crates/flui-widgets/src/flex/flex.rs`, next to `main_axis_alignment`/
//! `cross_axis_alignment`/`main_axis_size`), so `Row`, `Column`, and `Flex`
//! all gained it from one edit — `FlexStyle::build` (called by both
//! `create_render_object` and `update_render_object`) now chains
//! `.with_spacing(self.spacing)`, so both wiring paths push it in one place.
//! Flutter parity: `widgets/basic.dart` `Flex.spacing`/`Row.spacing`/
//! `Column.spacing`, tag `3.44.0`, all backed by the same
//! `RenderFlex.spacing`. Red-check: deleting the `.with_spacing(self.spacing)`
//! chain call fails `crate::flex::flex::tests::row_spacing_builder_reaches_render_flex`
//! and `row_update_render_object_pushes_updated_spacing` in
//! `crates/flui-widgets/src/flex/flex.rs`, plus
//! [`row_spacing_rebuild_changes_the_total_main_axis_extent`] below.

use flui_rendering::testing::inspect::render_diagnostics;
use flui_view::ViewExt;
use flui_widgets::prelude::*;

use crate::common::{LaidOut, offset, size};
use crate::harness;

/// Reads a diagnostics property straight off the mounted render tree.
/// `LaidOut` has no such accessor of its own — this is the same
/// `find_descendant_unique` + `get_property_f64` lookup
/// `flui_rendering::testing::inspect::Probe::descendant_property_f64` uses
/// in the `flui-objects` render-object harness, applied here to a
/// widget-level [`LaidOut::pipeline_owner`].
fn descendant_property_f64(laid: &LaidOut, type_name: &str, property: &str) -> Option<f64> {
    let owner = laid.pipeline_owner();
    let owner = owner.read();
    render_diagnostics(&owner)
        .find_descendant_unique(type_name)
        .ok()?
        .get_property_f64(property)
}

/// Default `Row` (no explicit alignment/size) lays out 3 equal children
/// left-to-right from the leading edge, and fills the 800px surface width.
///
/// Flutter parity: row_test.dart `'Row with default main axis parameters -
/// LTR'` — `Row` size `(800, 100)`; three `SizedBox(100, 100)` children at
/// `dx` 0 / 100 / 200.
#[test]
fn row_default_main_axis_parameters_positions_children_sequentially_from_start() {
    let laid = harness::pump_widget(
        Center::new().child(Row::new(vec![
            SizedBox::new(100.0, 100.0).boxed(),
            SizedBox::new(100.0, 100.0).boxed(),
            SizedBox::new(100.0, 100.0).boxed(),
        ])),
        harness::screen(),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(
        laid.size(flex_id),
        size(800.0, 100.0),
        "default Row must fill the 800px surface width and take the 100px child height"
    );
    assert_eq!(laid.offset(laid.child(flex_id, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 1)), offset(100.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 2)), offset(200.0, 0.0));
    for i in 0..3 {
        assert_eq!(laid.size(laid.child(flex_id, i)), size(100.0, 100.0));
    }
}

/// `MainAxisAlignment::Center` centers the whole block of children as a
/// unit within the free main-axis space.
///
/// Flutter parity: row_test.dart `'Row with MainAxisAlignment.center -
/// LTR'` — two `SizedBox(100, 100)` children in an 800px `Row`; free space
/// 600 split evenly on both sides puts them at `dx` 300 / 400.
#[test]
fn row_main_axis_alignment_center_centers_the_children_block() {
    let laid = harness::pump_widget(
        Center::new().child(
            Row::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        ),
        harness::screen(),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 100.0));
    assert_eq!(laid.offset(laid.child(flex_id, 0)), offset(300.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 1)), offset(400.0, 0.0));
    assert_eq!(laid.size(laid.child(flex_id, 0)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 1)), size(100.0, 100.0));
}

/// `MainAxisAlignment::End` pushes every child flush against the trailing
/// edge, in list order.
///
/// Flutter parity: row_test.dart `'Row with MainAxisAlignment.end - LTR'`
/// — three `SizedBox(100, 100)` children packed against the 800px trailing
/// edge land at `dx` 500 / 600 / 700.
#[test]
fn row_main_axis_alignment_end_pushes_children_to_the_far_edge() {
    let laid = harness::pump_widget(
        Center::new().child(
            Row::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .main_axis_alignment(MainAxisAlignment::End),
        ),
        harness::screen(),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 100.0));
    assert_eq!(laid.offset(laid.child(flex_id, 0)), offset(500.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 1)), offset(600.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 2)), offset(700.0, 0.0));
    assert_eq!(laid.size(laid.child(flex_id, 0)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 1)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 2)), size(100.0, 100.0));
}

/// `MainAxisAlignment::SpaceBetween` flushes the first and last children to
/// the row's own edges and splits the remaining free space evenly between
/// the gaps.
///
/// Flutter parity: row_test.dart `'Row with MainAxisAlignment.spaceBetween
/// - LTR'` — three `SizedBox(100, 100)` children in an 800px `Row`; free
/// space 500 split across 2 gaps puts them at `dx` 0 / 350 / 700.
#[test]
fn row_main_axis_alignment_space_between_flushes_the_first_and_last_children_to_the_edges() {
    let laid = harness::pump_widget(
        Center::new().child(
            Row::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .main_axis_alignment(MainAxisAlignment::SpaceBetween),
        ),
        harness::screen(),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 100.0));
    assert_eq!(laid.offset(laid.child(flex_id, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 1)), offset(350.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 2)), offset(700.0, 0.0));
    assert_eq!(laid.size(laid.child(flex_id, 0)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 1)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 2)), size(100.0, 100.0));
}

/// `MainAxisAlignment::SpaceAround` gives every gap an equal share, but the
/// leading/trailing edges only get a *half* share.
///
/// Flutter parity: row_test.dart `'Row with MainAxisAlignment.spaceAround -
/// LTR'` — four `SizedBox(100, 100)` children in an 800px `Row`; free space
/// 400 split into 8 half-shares (2 per gap, 1 per edge) puts them at `dx`
/// 50 / 250 / 450 / 650.
#[test]
fn row_main_axis_alignment_space_around_gives_edge_children_half_gaps() {
    let laid = harness::pump_widget(
        Center::new().child(
            Row::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .main_axis_alignment(MainAxisAlignment::SpaceAround),
        ),
        harness::screen(),
    );

    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(laid.size(flex_id), size(800.0, 100.0));
    assert_eq!(laid.offset(laid.child(flex_id, 0)), offset(50.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 1)), offset(250.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 2)), offset(450.0, 0.0));
    assert_eq!(laid.offset(laid.child(flex_id, 3)), offset(650.0, 0.0));
    assert_eq!(laid.size(laid.child(flex_id, 0)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 1)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 2)), size(100.0, 100.0));
    assert_eq!(laid.size(laid.child(flex_id, 3)), size(100.0, 100.0));
}

/// A single child under `MainAxisSize::Min` inside a zero-sized ancestor
/// keeps its own natural main-axis extent but collapses to zero on the
/// cross axis, because the ancestor's tight-zero constraint caps how tall
/// the `Row` itself — and therefore its child — is allowed to be.
///
/// Flutter parity: row_test.dart `'Row MainAxisSize.min layout at zero size -
/// LTR'` (identical to the `- no textDirection` and `- RTL` variants, cases 9
/// and 27 — see the ledger above): `SizedBox.shrink(child: Row(mainAxisSize:
/// min, mainAxisAlignment: center, children: [SizedBox(100, 100)]))` measures
/// the inner child at `Size(100, 0)`.
#[test]
fn row_main_axis_size_min_layout_at_zero_size_collapses_the_child_to_zero_height() {
    let laid = harness::pump_widget(
        Center::new().child(
            SizedBox::shrink().child(
                Row::new(vec![SizedBox::new(100.0, 100.0).boxed()])
                    .main_axis_size(MainAxisSize::Min)
                    .main_axis_alignment(MainAxisAlignment::Center),
            ),
        ),
        harness::screen(),
    );

    let root = laid.root();
    let shrink_box_id = laid.child(root, 0);
    let flex_id = laid.child(shrink_box_id, 0);
    let child_id = laid.child(flex_id, 0);

    assert_eq!(
        laid.size(child_id),
        size(100.0, 0.0),
        "the child must keep its natural 100px width but collapse to 0px \
         height under the zero-sized ancestor's tight cross-axis constraint"
    );
}

/// A `Row` built with no explicit `.spacing()` call mounts a `RenderFlex`
/// with `spacing: 0.0` — no gap is inserted between children by default.
///
/// Flutter parity: row_test.dart `'Default Row.spacing value'` — asserts
/// `tester.widget<Row>(find.byType(Row)).spacing == 0.0`. `LaidOut` has no
/// widget-instance-field introspection (only render-tree geometry/
/// diagnostics), so this reads the equivalent `RenderFlex.spacing`
/// diagnostics property the widget wired through instead — the same value
/// Flutter's own `Row.spacing` is defined to forward unchanged into
/// `RenderFlex.spacing`.
#[test]
fn row_spacing_defaults_to_zero_on_the_mounted_render_object() {
    let row: Row = Row::new(Vec::new());
    let laid = harness::pump_widget(row, harness::screen());

    assert_eq!(
        descendant_property_f64(&laid, "RenderFlex", "spacing"),
        Some(0.0),
        "a Row built with no explicit .spacing() call must mount a RenderFlex \
         with spacing 0.0"
    );
}

/// Root-swapping to a new `.spacing()` value changes the `Row`'s total
/// main-axis extent (`MainAxisSize::Min` shrink-wraps to child-sum +
/// inter-child spacing) and the underlying `RenderFlex.spacing` alike.
///
/// Flutter parity: row_test.dart `'Can update Row.spacing value'` — three
/// 100×100 children under `MainAxisSize.min`; `spacing: 8.0` gives width
/// `3×100 + 2×8 = 316`, and rebuilding with `spacing: 18.0` gives `3×100 +
/// 2×18 = 336`. FLUI substitutes `SizedBox` for the oracle's `Container` —
/// color is irrelevant to the geometry assertion, the same substitution
/// this crate's other geometry-only ports already make.
#[test]
fn row_spacing_rebuild_changes_the_total_main_axis_extent() {
    fn row_with_spacing(spacing: f32) -> Center {
        Center::new().child(
            Row::new(vec![
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
                SizedBox::new(100.0, 100.0).boxed(),
            ])
            .main_axis_size(MainAxisSize::Min)
            .spacing(spacing),
        )
    }

    let mut laid = harness::pump_widget(row_with_spacing(8.0), harness::screen());
    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(
        laid.size(flex_id),
        size(316.0, 100.0),
        "3 × 100px children + 2 gaps × 8px spacing must total 316px"
    );
    assert_eq!(
        descendant_property_f64(&laid, "RenderFlex", "spacing"),
        Some(8.0)
    );

    laid.pump_widget(row_with_spacing(18.0));
    let flex_id = laid.find_by_render_type("RenderFlex");
    assert_eq!(
        laid.size(flex_id),
        size(336.0, 100.0),
        "3 × 100px children + 2 gaps × 18px spacing must total 336px"
    );
    assert_eq!(
        descendant_property_f64(&laid, "RenderFlex", "spacing"),
        Some(18.0)
    );
}
