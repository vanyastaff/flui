//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/single_child_scroll_view_test.dart`
//! (tag `3.44.0`). No single upstream case isolates raw offset-clamping or
//! `reverse` geometry for `SingleChildScrollView` in a form this headless
//! harness can assert directly (the closest cases —
//! `'SingleChildScrollView overflow and clipRect test'` at line 53 and the
//! `getOffsetToReveal`/`showOnScreen` `reverse: true` cases at lines 611/721 —
//! assert paint-clip behavior and `RenderObject.showOnScreen`, both out of
//! scope per the paint-deferred-to-Phase-3 precedent, see `container_test.rs`).
//! This port instead derives its oracle directly from the render objects
//! `SingleChildScrollView` composes:
//! - Offset clamping:
//!   `crates/flui-rendering/src/view/viewport_offset.rs`
//!   `ScrollableViewportOffset::apply_content_dimensions` (lines 419-437) —
//!   `self.pixels = self.pixels.clamp(min_scroll_extent, max_scroll_extent)`,
//!   applied by `RenderViewport`'s layout correction loop
//!   (`crates/flui-objects/src/sliver/viewport.rs`,
//!   `MAX_LAYOUT_CYCLES_PER_CHILD`) once the sliver content establishes the
//!   scroll extents. Flutter parity: `ScrollPosition.applyContentDimensions`
//!   under `ClampingScrollPhysics`
//!   (`widgets/scroll_position_with_single_context.dart`) — FLUI folds the
//!   same clamp into `ViewportOffset` itself because this widget has no
//!   `Scrollable`/physics layer, only a programmatic `offset`.
//! - `reverse`: Flutter's `SingleChildScrollView.reverse` maps to
//!   `AxisDirection.up`/`AxisDirection.left` via
//!   `getAxisDirectionFromAxisReverseAndDirectionality`
//!   (`widgets/basic.dart:4513-4527`). The child-offset math for a reversed
//!   axis is `RenderSliverToBoxAdapter`'s `child_paint_offset`
//!   (`crates/flui-rendering/src/constraints/sliver_layout.rs:10-30`): for a
//!   non-right-way-up sliver, `main_axis_delta = paint_extent -
//!   child_main_extent - child_main_axis_position`.
//!
//! Widget → render-object mapping: `SingleChildScrollView` → `Viewport`
//! (`RenderViewport`) → `SliverToBoxAdapter` (`RenderSliverToBoxAdapter`) → box
//! child (`crates/flui-widgets/src/scroll/single_child_scroll_view.rs`).
//!
//! Fix applied: `SingleChildScrollView` had no `reverse` builder at all (a
//! genuine gap versus Flutter's `reverse` parameter) — added
//! `SingleChildScrollView::reverse(bool)`
//! (`crates/flui-widgets/src/scroll/single_child_scroll_view.rs`), a localized
//! widget-composition change mapping `(scroll_direction, reverse)` to the
//! appropriate `AxisDirection`; `RenderViewport` already supports all four
//! `AxisDirection` values, so no `flui-objects`/`flui-rendering` change was
//! needed (tripwire not crossed).
//!
//! Overlap: `tests/scroll.rs`
//! `single_child_scroll_view_lays_child_out_unbounded_on_scroll_axis` and
//! `_horizontal_lays_child_unbounded_on_width` already cover "child sized
//! unconstrained along the scroll axis" faithfully (this is the geometry half
//! of the upstream overflow test at line 53); this file does not duplicate
//! that case and instead cites the upstream test on those two functions.

use crate::common::{lay_out, offset, size, tight};
use flui_widgets::prelude::Axis;
use flui_widgets::{SingleChildScrollView, SizedBox};

/// An offset far past `max_scroll_extent` clamps to the maximum; an offset
/// below `min_scroll_extent` (0) clamps to zero. Read indirectly through the
/// scrolled box child's committed paint offset (`main_axis_delta = -pixels`
/// for a right-way-up `TopToBottom` viewport,
/// `sliver_layout.rs:18-24`) rather than a raw offset accessor, since the
/// harness exposes committed geometry, not internal `ViewportOffset` state.
///
/// Flutter parity: `ScrollableViewportOffset::apply_content_dimensions`
/// (`crates/flui-rendering/src/view/viewport_offset.rs:419-437`) — the same
/// clamp `ScrollPosition.applyContentDimensions` performs under
/// `ClampingScrollPhysics` (Flutter has no unclamped `ViewportOffset` in
/// normal use; FLUI's programmatic-offset `SingleChildScrollView` is the one
/// path where an out-of-range value can be set directly).
#[test]
fn offset_clamps_to_the_valid_scroll_range() {
    // Viewport 300 tall, child 600 tall: max_scroll_extent = 600 - 300 = 300.
    let over_max = lay_out(
        SingleChildScrollView::new()
            .offset(100_000.0)
            .child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );
    let viewport = over_max.root();
    let adapter = over_max.only_child(viewport);
    let child = over_max.only_child(adapter);
    assert_eq!(
        over_max.offset(child),
        offset(0.0, -300.0),
        "an offset far past max_scroll_extent (300) must clamp to 300, not stay at 100000"
    );

    let below_min = lay_out(
        SingleChildScrollView::new()
            .offset(-500.0)
            .child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );
    let viewport = below_min.root();
    let adapter = below_min.only_child(viewport);
    let child = below_min.only_child(adapter);
    assert_eq!(
        below_min.offset(child),
        offset(0.0, 0.0),
        "an offset below min_scroll_extent (0) must clamp to 0, not stay negative"
    );
}

/// `reverse(true)` flips which edge scroll position `0.0` anchors to: the
/// child's trailing (bottom) portion is what's visible at rest, not its
/// leading (top) portion — the child's main-axis paint offset within the
/// sliver becomes negative by the overflow amount instead of zero.
///
/// Flutter parity: `getAxisDirectionFromAxisReverseAndDirectionality`
/// (`widgets/scroll_view.dart`) maps `scrollDirection: Axis.vertical, reverse:
/// true` to `AxisDirection.up`. Child-offset math:
/// `child_paint_offset` (`sliver_layout.rs:18-24`) — for `AxisDirection::up`
/// (not right-way-up under forward growth), `main_axis_delta = paint_extent -
/// child_main_extent - child_main_axis_position`; at `pixels = 0` with
/// `paint_extent = 300` (clamped to the viewport) and `child_main_extent =
/// 600`, `main_axis_delta = 300 - 600 - 0 = -300`.
#[test]
fn reverse_flips_which_edge_the_child_anchors_to() {
    let forward = lay_out(
        SingleChildScrollView::new().child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );
    let forward_child = forward.only_child(forward.only_child(forward.root()));
    assert_eq!(
        forward.offset(forward_child),
        offset(0.0, 0.0),
        "forward (default) at rest anchors the child's leading edge to the viewport's top"
    );

    let reversed = lay_out(
        SingleChildScrollView::new()
            .reverse(true)
            .child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );
    let reversed_child = reversed.only_child(reversed.only_child(reversed.root()));
    assert_eq!(
        reversed.offset(reversed_child),
        offset(0.0, -300.0),
        "reverse(true) at rest must anchor the child's TRAILING edge to the viewport instead \
         of its leading edge — the visible window is the child's last 300px, not its first"
    );

    // The outer viewport size is unaffected by direction: it still fills the
    // tight constraints regardless of which edge the content anchors to.
    assert_eq!(reversed.size(reversed.root()), size(200.0, 300.0));
}

/// `reverse(true)` on the horizontal axis maps to `AxisDirection::RightToLeft`
/// (not the vertical `BottomToTop`) — confirms the `(scroll_direction,
/// reverse)` combination selects the correct one of all four `AxisDirection`
/// values, not just the vertical pair.
///
/// Flutter parity: `getAxisDirectionFromAxisReverseAndDirectionality` maps
/// `scrollDirection: Axis.horizontal, reverse: true` to `AxisDirection.left`.
#[test]
fn reverse_horizontal_flips_to_right_to_left_not_bottom_to_top() {
    let laid = lay_out(
        SingleChildScrollView::new()
            .scroll_direction(Axis::Horizontal)
            .reverse(true)
            .child(SizedBox::new(600.0, 200.0)),
        tight(300.0, 200.0),
    );
    let child = laid.only_child(laid.only_child(laid.root()));
    assert_eq!(
        laid.offset(child),
        offset(-300.0, 0.0),
        "horizontal reverse must shift on the X axis (RightToLeft), not the Y axis"
    );
}

/// A NONZERO offset moves the child in **opposite** directions depending on
/// `reverse`: increasing `pixels` shifts a forward viewport's child further
/// away from the origin (more negative), but shifts a reversed viewport's
/// child TOWARD the origin (less negative) — the full contract `reverse`
/// exposes, not just the rest-position (`pixels == 0`) flip covered by
/// `reverse_flips_which_edge_the_child_anchors_to` above.
///
/// Flutter parity: `getAxisDirectionFromAxisReverseAndDirectionality`
/// (`widgets/basic.dart:4513-4527`) — `reverse: true` maps
/// `Axis.vertical` to `AxisDirection.up` instead of `AxisDirection.down`;
/// scrolling `AxisDirection.up` content reveals what precedes the current
/// position by moving it toward the viewport's leading edge, the opposite of
/// `AxisDirection.down`. Numerically, from `child_paint_offset`
/// (`sliver_layout.rs:10-30`): forward gives `main_axis_delta = -pixels`
/// (decreasing in `pixels`); reverse gives `main_axis_delta = (paint_extent -
/// child_main_extent) + pixels` (increasing in `pixels`) — same slope
/// magnitude, opposite sign.
#[test]
fn reverse_with_nonzero_offset_shifts_content_the_opposite_way_from_forward() {
    // Viewport 300 tall, child 600 tall (max_scroll_extent = 300); pixels = 100
    // is within range on both viewports, so no clamping is in play here.
    let forward = lay_out(
        SingleChildScrollView::new()
            .offset(100.0)
            .child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );
    let forward_child = forward.only_child(forward.only_child(forward.root()));
    assert_eq!(
        forward.offset(forward_child),
        offset(0.0, -100.0),
        "forward at pixels=100 must shift the child up by exactly the offset"
    );

    let reversed = lay_out(
        SingleChildScrollView::new()
            .reverse(true)
            .offset(100.0)
            .child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );
    let reversed_child = reversed.only_child(reversed.only_child(reversed.root()));
    assert_eq!(
        reversed.offset(reversed_child),
        offset(0.0, -200.0),
        "reverse at pixels=100 must shift the child down (toward the viewport) from its \
         -300 rest position, not up further away from it like the forward case"
    );

    // Relative to each direction's own rest position (pixels=0: forward=0,
    // reverse=-300 — see `reverse_flips_which_edge_the_child_anchors_to`), the
    // SAME 100px offset input moves the two by the same magnitude but
    // opposite sign: forward's child offset decreases by 100 (shifts further
    // away), reverse's increases by 100 (shifts toward the viewport).
    let forward_delta = forward.offset(forward_child).dy.get() - 0.0;
    let reverse_delta = reversed.offset(reversed_child).dy.get() - (-300.0);
    assert!(
        (forward_delta - (-100.0)).abs() < 1e-4 && (reverse_delta - 100.0).abs() < 1e-4,
        "forward and reverse must move by equal magnitude but opposite sign for the same \
         offset: forward_delta={forward_delta}, reverse_delta={reverse_delta}"
    );
}
