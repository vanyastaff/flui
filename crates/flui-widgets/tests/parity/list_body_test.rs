//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/basic.dart` `ListBody` over
//!   `RenderListBody` (`packages/flutter/lib/src/rendering/list_body.dart`).
//! - Tests: `packages/flutter/test/widgets/list_body_test.dart` (tag
//!   `3.44.0`), all six cases.
//!
//! `RenderListBody`'s contract (`rendering/list_body.dart`,
//! `performLayout`/`computeDryLayout`): children are laid out sequentially
//! along the main axis at their own intrinsic main-axis extent — the render
//! object hands each child `BoxConstraints.tightFor(<cross axis>: <its own
//! cross extent>)`, leaving the main axis of that per-child constraint fully
//! open (`0..∞`) — while forcing every child's cross axis tight to the box's
//! own cross extent. The box's own main-axis extent is the sum of its
//! children's main-axis extents; its cross extent is whatever the parent
//! constrains it to. It never clips or resizes a child's main-axis extent to
//! fit, and it asserts (`_debugCheckConstraints`) that its own incoming
//! constraints give it unlimited main-axis space and a bounded cross axis —
//! the two error cases below.
//!
//! ## Case ledger
//!
//! 1. `'ListBody down'` — ported, real green:
//!    [`list_body_down_lays_out_top_to_bottom_stretched_to_parent_width`].
//! 2. `'ListBody up'` — ported, real green:
//!    [`list_body_up_lays_out_bottom_to_top_stretched_to_parent_width`].
//! 3. `'ListBody right'` — ported, real green:
//!    [`list_body_right_lays_out_left_to_right_stretched_to_parent_height`].
//!    LTR ambient direction, non-reverse; on its own this case cannot
//!    distinguish "reads Directionality and got LTR" from "ignores
//!    Directionality and defaults to LTR" — see "Divergence found by this
//!    port" below for why case 4 is the one that actually proves the read.
//! 4. `'ListBody left'` — ported, real green:
//!    [`list_body_left_lays_out_right_to_left`]. See "Divergence found by
//!    this port" below for the bug this case catches.
//! 5. `'Limited space along main axis error'` — ported, real green, but via
//!    an observable that needed empirical discovery, not the mechanism the
//!    module doc originally assumed — see "The layout-poisoning gap found by
//!    this port" below:
//!    [`horizontal_list_body_without_unlimited_main_axis_panics`].
//! 6. `'Nested ListBody unbounded cross axis error'` — ported, real green,
//!    same gap:
//!    [`nested_list_body_with_unbounded_cross_axis_panics`].
//!
//! ## The layout-poisoning gap found by this port (cases 5 and 6)
//!
//! Both error cases trip a real `debug_assert!` in
//! `RenderListBody::debug_check_constraints`
//! (`crates/flui-objects/src/layout/list_body.rs`) — confirmed by the
//! `thread '...' panicked at .../list_body.rs:84:17: RenderListBody must
//! have unlimited space along its main axis` line that prints to stderr on
//! every run. But that panic never reaches `harness::pump_widget`'s caller:
//! `flui-rendering`'s layout walk
//! (`crates/flui-rendering/src/pipeline/owner/subtree_arena.rs`) wraps
//! `perform_layout_raw` in `catch_unwind` for BOTH the leaf and non-leaf
//! paths (not only the sliver child-manager build path
//! `error_widget_test.rs`'s module doc describes — this is a separate,
//! more general net), converts the panic to
//! `RenderError::Poisoned { render_object, phase: "layout" }`, and the
//! walk's caller swallows that `Err` entirely: the offending node is left
//! with NO committed geometry, its ancestors still complete their own
//! layout and the whole frame finishes as if nothing failed. `pump_widget`
//! itself never panics or errors — confirmed by writing this test first as
//! a bare `#[should_panic]` around the `pump_widget` call alone (no
//! `expected` string) and observing it fail with "test did not panic as
//! expected".
//!
//! The only place the failure becomes observable at all is the poisoned
//! node's own geometry: `LaidOut::size` (`crates/flui-widgets/tests/
//! common/mod.rs:322`) asserts the render node has committed box geometry,
//! and panics with `"render node should have box geometry after layout"`
//! when it does not — an unrelated-sounding message, not the original
//! assert text, because by the time a test can query it, the pipeline has
//! already discarded the original panic payload into a `tracing::error!`
//! log event instead of any queryable `Result`. Both tests below assert
//! that specific, real, reproducible symptom.
//!
//! This is a materially different (and arguably worse) divergence from
//! Flutter than the `debug_assert!`-vs-`FlutterError` mechanism gap
//! `table_test.rs`/`flow_test.rs` already document: Flutter's
//! `FlutterError` is caught by the framework and reported through
//! `FlutterError.onError`/`tester.takeException()` — a documented, typed,
//! application-observable channel. FLUI's equivalent invariant violation is
//! caught too, but the only trace left behind is a `tracing::error!` log
//! line and a node with permanently-missing geometry; nothing in the
//! `PipelineOwner`/`pump_widget` public surface reports that the frame
//! contains a poisoned node at all. Reported here, not fixed: closing it
//! means deciding where a poisoned-node signal should surface (a
//! `PipelineOwner` query? a diagnostics counter?), which is a `flui-rendering`
//! design question this test file has no standing to answer.
//!
//! ## Divergence found by this port — fixed
//!
//! `ListBody::axis_direction` (`crates/flui-widgets/src/layout/list_body.rs`)
//! used to compute its `AxisDirection` from `main_axis` and `reverse` alone:
//!
//! ```text
//! fn axis_direction(&self) -> AxisDirection {
//!     AxisDirection::from_axis(self.main_axis, self.reverse)
//! }
//! ```
//!
//! Flutter's `ListBody._getDirection(BuildContext context)` instead calls
//! `getAxisDirectionFromAxisReverseAndDirectionality(context, mainAxis,
//! reverse)` (`widgets/basic.dart`), which for `Axis.horizontal` reads
//! `Directionality.of(context)` and flips the resulting `AxisDirection`
//! between `left`/`right` depending on the ambient `TextDirection`. FLUI's
//! old version never looked at `Directionality` at all: for
//! `Axis::Horizontal` with `reverse: false` it was unconditionally
//! `AxisDirection::LeftToRight`, regardless of what `Directionality`
//! ancestor was in scope.
//!
//! The root cause was structural, not a one-line oversight:
//! `flui_view::RenderObjectContext` (`crates/flui-view/src/view/render.rs`)
//! — the only context `RenderView::create_render_object`/
//! `update_render_object` ever receive — carries nothing but an optional
//! interaction-dispatch handle. There is no ambient inherited-data lookup
//! capability at that call site at all, so no leaf `RenderView` in
//! `flui-widgets` can consult `Directionality` while building its render
//! object.
//!
//! The fix does not widen `RenderObjectContext` — every other FLUI widget
//! that needs ambient inherited data (`Text` reading `DefaultTextStyle` and
//! delegating to `RichText`, `Dismissible` reading `Directionality` in its
//! own `build`) resolves it the same way: at the `BuildContext` seam
//! (`build`), not at the render-object seam. `ListBody`
//! (`crates/flui-widgets/src/layout/list_body.rs`) is now a composing
//! `StatelessView`: its `build` resolves `Directionality::maybe_of(ctx)`
//! (defaulting to `Ltr` with no ancestor, matching every other FLUI widget
//! that reads `Directionality`) through `resolve_axis_direction` — the exact
//! `getAxisDirectionFromAxisReverseAndDirectionality` rule, including the
//! `reverse` flip on both the LTR and RTL base cases — and hands the
//! already-resolved `AxisDirection` to a private `ListBodyRenderView`, the
//! actual `RenderListBody`-backed `RenderView`. The render object itself
//! needed no change: its `AxisDirection::RightToLeft` branch was already
//! correct (`harness_list_body_horizontal_right_to_left_stretches_height`,
//! `crates/flui-objects/tests/render_object_harness.rs`); the widget layer
//! simply never selected it before.
//!
//! Effect before the fix: a horizontal `ListBody` with `reverse: false` under
//! an RTL `Directionality` ancestor laid out its children left-to-right
//! instead of Flutter's right-to-left — a real content-mirroring defect for
//! RTL locales, not a cosmetic one. Case 3 above (`'ListBody right'`) passed
//! even before this fix, but only because its ambient direction is LTR,
//! which coincided with FLUI's old hardcoded default; it never exercised, and
//! could not have caught, this gap. Case 4 (RTL ambient) is the one that
//! actually proves the read, and now passes for real.
//!
//! Not fixed here: `Flex`/`Row`/`Column`
//! (`crates/flui-widgets/src/flex/flex.rs`) share the identical gap —
//! grepping that file for `text_direction`/`Directionality`/`TextDirection`
//! returns zero hits, confirming FLUI's flex family still has no RTL-aware
//! `textDirection` concept. Reported for `flui-widgets` maintainers to port
//! next using the same `build`-seam pattern; out of scope for this change.

use flui_foundation::RenderId;
use flui_types::layout::Axis;
use flui_types::typography::TextDirection;
use flui_widgets::{Column, Directionality, ListBody, Row, SizedBox, row};

use crate::harness;

// ============================================================================
// Fixtures
// ============================================================================

const ITEM_WIDTH: f32 = 200.0;
const ITEM_HEIGHT: f32 = 150.0;

/// The oracle's shared `children` constant: four identically-sized boxes.
fn oracle_children() -> (SizedBox, SizedBox, SizedBox, SizedBox) {
    (
        SizedBox::new(ITEM_WIDTH, ITEM_HEIGHT),
        SizedBox::new(ITEM_WIDTH, ITEM_HEIGHT),
        SizedBox::new(ITEM_WIDTH, ITEM_HEIGHT),
        SizedBox::new(ITEM_WIDTH, ITEM_HEIGHT),
    )
}

// ============================================================================
// Case 1 — 'ListBody down'
// ============================================================================

/// Flutter parity: `list_body_test.dart` `'ListBody down'` (tag `3.44.0`).
///
/// A vertical `ListBody` nested in a vertical `Flex` (`Column`, which gives
/// it unlimited height and a bounded, 800px-wide cross axis) stacks its four
/// 200×150 children top to bottom, each stretched to the full 800px width.
///
/// Mutation-checked: temporarily changing child 2's expected `dy` from
/// `300.0` to `301.0` flips this red — captured verbatim:
/// `assertion `left == right` failed: child 2's absolute (x, y, width,
/// height) must match the oracle's Rect.fromLTWH left: (0.0, 300.0, 800.0,
/// 150.0) right: (0.0, 301.0, 800.0, 150.0)` — reverted after confirming.
#[test]
fn list_body_down_lays_out_top_to_bottom_stretched_to_parent_width() {
    let laid = harness::pump_widget(
        Column::new(row![ListBody::new(oracle_children())]),
        harness::screen(),
    );

    let list_body = laid.find_by_render_type("RenderListBody");
    let rect = |id: RenderId| {
        let offset = laid.absolute_offset(id);
        let size = laid.size(id);
        (
            offset.dx.get(),
            offset.dy.get(),
            size.width.get(),
            size.height.get(),
        )
    };

    let expected = [
        (0.0, 0.0, 800.0, 150.0),
        (0.0, 150.0, 800.0, 150.0),
        (0.0, 300.0, 800.0, 150.0),
        (0.0, 450.0, 800.0, 150.0),
    ];
    for (i, expected_rect) in expected.into_iter().enumerate() {
        let child = laid.child(list_body, i);
        assert_eq!(
            rect(child),
            expected_rect,
            "child {i}'s absolute (x, y, width, height) must match the \
             oracle's Rect.fromLTWH"
        );
    }
}

// ============================================================================
// Case 2 — 'ListBody up'
// ============================================================================

/// Flutter parity: `list_body_test.dart` `'ListBody up'` (tag `3.44.0`).
///
/// `reverse: true` on a vertical `ListBody` flips the main axis to
/// bottom-to-top: the first child ends up lowest on screen, the last child
/// highest — without changing any child's own size.
#[test]
fn list_body_up_lays_out_bottom_to_top_stretched_to_parent_width() {
    let laid = harness::pump_widget(
        Column::new(row![ListBody::new(oracle_children()).reverse(true)]),
        harness::screen(),
    );

    let list_body = laid.find_by_render_type("RenderListBody");
    let rect = |id: RenderId| {
        let offset = laid.absolute_offset(id);
        let size = laid.size(id);
        (
            offset.dx.get(),
            offset.dy.get(),
            size.width.get(),
            size.height.get(),
        )
    };

    let expected = [
        (0.0, 450.0, 800.0, 150.0),
        (0.0, 300.0, 800.0, 150.0),
        (0.0, 150.0, 800.0, 150.0),
        (0.0, 0.0, 800.0, 150.0),
    ];
    for (i, expected_rect) in expected.into_iter().enumerate() {
        let child = laid.child(list_body, i);
        assert_eq!(
            rect(child),
            expected_rect,
            "child {i}'s absolute (x, y, width, height) must match the \
             oracle's Rect.fromLTWH"
        );
    }
}

// ============================================================================
// Case 3 — 'ListBody right'
// ============================================================================

/// Flutter parity: `list_body_test.dart` `'ListBody right'` (tag `3.44.0`).
///
/// A horizontal `ListBody` under an LTR `Directionality`, nested in a
/// horizontal `Flex` (`Row`, which gives it unlimited width and a bounded,
/// 600px-tall cross axis), places its four 200×150 children left to right,
/// each stretched to the full 600px height.
///
/// This passes today even though FLUI's `ListBody` ignores ambient
/// `Directionality` entirely (see the module doc's divergence note): LTR is
/// also FLUI's hardcoded default for `Axis::Horizontal` + `reverse: false`,
/// so this scene cannot distinguish "reads Directionality and got LTR" from
/// "ignores Directionality and defaults to LTR". Case 4 below is the one
/// that actually exercises ambient direction.
#[test]
fn list_body_right_lays_out_left_to_right_stretched_to_parent_height() {
    let laid = harness::pump_widget(
        Row::new(row![Directionality::new(
            TextDirection::Ltr,
            ListBody::new(oracle_children()).main_axis(Axis::Horizontal),
        )]),
        harness::screen(),
    );

    let list_body = laid.find_by_render_type("RenderListBody");
    let rect = |id: RenderId| {
        let offset = laid.absolute_offset(id);
        let size = laid.size(id);
        (
            offset.dx.get(),
            offset.dy.get(),
            size.width.get(),
            size.height.get(),
        )
    };

    let expected = [
        (0.0, 0.0, 200.0, 600.0),
        (200.0, 0.0, 200.0, 600.0),
        (400.0, 0.0, 200.0, 600.0),
        (600.0, 0.0, 200.0, 600.0),
    ];
    for (i, expected_rect) in expected.into_iter().enumerate() {
        let child = laid.child(list_body, i);
        assert_eq!(
            rect(child),
            expected_rect,
            "child {i}'s absolute (x, y, width, height) must match the \
             oracle's Rect.fromLTWH"
        );
    }
}

// ============================================================================
// Case 4 — 'ListBody left'
// ============================================================================

/// Flutter parity: `list_body_test.dart` `'ListBody left'` (tag `3.44.0`).
///
/// Asserts the oracle's behaviour: a horizontal, non-reversed `ListBody`
/// under an RTL `Directionality` lays its children out right to left — the
/// first child (`children[0]`) ends up rightmost, at `x = 600`, and the last
/// ends up leftmost, at `x = 0`.
///
/// Before the fix (see the module doc's "Divergence found by this port"),
/// FLUI's `ListBody` ignored `Directionality` entirely and resolved
/// `Axis::Horizontal` + `reverse: false` to `AxisDirection::LeftToRight`
/// unconditionally — identical to case 3's LTR scene above, even though this
/// scene's ambient direction is RTL. The render object's own
/// `AxisDirection::RightToLeft` branch was already correct and already
/// exercised — by
/// `harness_list_body_horizontal_right_to_left_stretches_height`
/// (`crates/flui-objects/tests/render_object_harness.rs`), which drives the
/// render object directly. Only the widget layer never selected that branch.
///
/// Captured failure before the fix (this test run with the old
/// `AxisDirection::from_axis(self.main_axis, self.reverse)` body, verbatim
/// from the harness): the first assertion failed with `left: (0.0, 0.0,
/// 200.0, 600.0), right: (600.0, 0.0, 200.0, 600.0)` — FLUI placed
/// `children[0]` at the LTR position `x = 0` instead of the oracle's RTL
/// position `x = 600`.
#[test]
fn list_body_left_lays_out_right_to_left() {
    let laid = harness::pump_widget(
        Row::new(row![Directionality::new(
            TextDirection::Rtl,
            ListBody::new(oracle_children()).main_axis(Axis::Horizontal),
        )]),
        harness::screen(),
    );

    let list_body = laid.find_by_render_type("RenderListBody");
    let rect = |id: RenderId| {
        let offset = laid.absolute_offset(id);
        let size = laid.size(id);
        (
            offset.dx.get(),
            offset.dy.get(),
            size.width.get(),
            size.height.get(),
        )
    };

    let expected = [
        (600.0, 0.0, 200.0, 600.0),
        (400.0, 0.0, 200.0, 600.0),
        (200.0, 0.0, 200.0, 600.0),
        (0.0, 0.0, 200.0, 600.0),
    ];
    for (i, expected_rect) in expected.into_iter().enumerate() {
        let child = laid.child(list_body, i);
        assert_eq!(
            rect(child),
            expected_rect,
            "child {i}'s absolute (x, y, width, height) must match the \
             oracle's Rect.fromLTWH under RTL"
        );
    }
}

// ============================================================================
// Case 5 — 'Limited space along main axis error'
// ============================================================================

/// Flutter parity: `list_body_test.dart` `'Limited space along main axis
/// error'` (tag `3.44.0`).
///
/// The oracle wraps a horizontal `ListBody` directly in a `SizedBox.square`
/// (no `Flex` ancestor to relax the main axis to infinity), so the box's own
/// incoming main-axis (width) constraint stays finite/tight, and
/// `RenderListBody._debugCheckConstraints` throws.
///
/// FLUI's `RenderListBody::debug_check_constraints`
/// (`crates/flui-objects/src/layout/list_body.rs`) enforces the identical
/// invariant with a `debug_assert!`, but the layout pipeline catches and
/// swallows that panic (`RenderError::Poisoned`) rather than surfacing it to
/// `pump_widget`'s caller at all — see the module doc's "layout-poisoning
/// gap" section. The only observable symptom at this test's level is that
/// the poisoned `ListBody` node never receives committed geometry, so
/// querying its size panics with the harness's own
/// `"render node should have box geometry after layout"` message — that is
/// what this test actually pins, not the original assert text.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "render node should have box geometry after layout")]
fn horizontal_list_body_without_unlimited_main_axis_panics() {
    let laid = harness::pump_widget(
        SizedBox::square(100.0).child(Directionality::new(
            TextDirection::Rtl,
            ListBody::new(oracle_children()).main_axis(Axis::Horizontal),
        )),
        harness::screen(),
    );
    let list_body = laid.find_by_render_type("RenderListBody");
    let _ = laid.size(list_body);
}

// ============================================================================
// Case 6 — 'Nested ListBody unbounded cross axis error'
// ============================================================================

/// Flutter parity: `list_body_test.dart` `'Nested ListBody unbounded cross
/// axis error'` (tag `3.44.0`).
///
/// The outer horizontal `ListBody` is correctly nested in a horizontal
/// `Flex` (`Row`), so its own main axis is fine. Its single child is a
/// vertical `Flex` (`Column`) wrapping an inner, default (vertical)
/// `ListBody`. The outer `ListBody` hands its `Column` child an unconstrained
/// width (main-axis passthrough for a horizontal `ListBody`'s child); the
/// `Column`'s own non-stretch `CrossAxisAlignment::Center` default loosens
/// its own child's cross axis (width) to that SAME unbounded max — so the
/// inner `ListBody`'s cross axis (width, since its main axis is vertical)
/// arrives unbounded, and `RenderListBody._debugCheckConstraints` throws.
///
/// Same mechanism divergence as case 5 (see that test's doc comment): the
/// `debug_assert!` fires, gets caught by the layout pipeline's
/// `catch_unwind`, and is swallowed as a `RenderError::Poisoned` — invisible
/// to `pump_widget`'s caller. Querying the poisoned inner `ListBody`'s own
/// geometry is what surfaces it.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "render node should have box geometry after layout")]
fn nested_list_body_with_unbounded_cross_axis_panics() {
    let laid = harness::pump_widget(
        Row::new(row![Directionality::new(
            TextDirection::Ltr,
            ListBody::new(row![Column::new(row![Directionality::new(
                TextDirection::Ltr,
                ListBody::new(oracle_children()),
            )])])
            .main_axis(Axis::Horizontal),
        )]),
        harness::screen(),
    );

    // Two `RenderListBody` nodes exist (outer + inner), so
    // `find_by_render_type` is ambiguous here. The outer one laid out fine
    // (its own constraints were valid) — walk down from the root
    // (`Row` -> outer `ListBody` -> `Column` -> inner `ListBody`;
    // `Directionality` is transparent, no render node of its own) to reach
    // the poisoned inner one.
    let outer_list_body = laid.only_child(laid.root());
    let column = laid.only_child(outer_list_body);
    let inner_list_body = laid.only_child(column);
    let _ = laid.size(inner_list_body);
}
