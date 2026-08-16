//! Scroll-path parity tests:
#![allow(clippy::float_cmp)] // physics clamp + controller pixel reads return exact f32 literals
//!
//! 1. `SingleChildScrollView` viewport geometry (cross-protocol Box→Sliver path).
//! 2. `ScrollController` thumb geometry helpers.
//! 3. `Scrollable` interactive drag integration (gesture → offset change).
//! 4. `ClampingScrollPhysics` hard-boundary enforcement.
//! 5. `ScrollController::animate_to` (ADR-0037) — curve-driven animation,
//!    grab-to-cancel, and jump_to-cancels-in-flight.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out, offset, size, tight};
use flui_animation::{Curves, Vsync};
use flui_interaction::events::{PointerEvent, PointerEventExt as _};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::view::ScrollDirection;
use flui_types::Color;
use flui_types::geometry::px;
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, IntoView, ViewExt};
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, ColoredBox, CustomScrollView, GestureDetector,
    GridView, ListView, Listener, ScrollController, ScrollMetrics, ScrollPhysics, Scrollable,
    SharedScrollPhysics, SingleChildScrollView, SizedBox, SliverFixedExtentList, VsyncScope,
};

/// Flutter parity (tag `3.44.0`):
/// `packages/flutter/test/widgets/single_child_scroll_view_test.dart:53`
/// `'SingleChildScrollView overflow and clipRect test'` — the geometry half of
/// that test (a child taller than the viewport lays out unbounded on the
/// scroll axis and overflows) is what this asserts; the paint-clip-behavior
/// half is out of scope because the headless harness asserts committed
/// geometry, not paint output — `parity/container_test.rs` documents the same
/// limitation for its paint assertions.
#[test]
fn single_child_scroll_view_lays_child_out_unbounded_on_scroll_axis() {
    // Viewport bounded to 200×300; a 200×600 child is taller than the viewport.
    let laid = lay_out(
        SingleChildScrollView::new().child(SizedBox::new(200.0, 600.0)),
        tight(200.0, 300.0),
    );

    // The viewport (root box) sizes to its constraints — it does NOT grow to
    // the child; the overflow is what gets scrolled/clipped.
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 300.0));

    // Viewport → SliverToBoxAdapter (sliver) → the box child. The child keeps
    // its full 600 height: it was laid out with an unbounded main axis, the
    // essence of scrollability.
    let adapter = laid.only_child(viewport);
    let child = laid.only_child(adapter);
    assert_eq!(laid.size(child), size(200.0, 600.0));
}

/// Flutter parity (tag `3.44.0`):
/// `packages/flutter/test/widgets/single_child_scroll_view_test.dart:53`
/// `'SingleChildScrollView overflow and clipRect test'`, 4th/5th sub-cases
/// (horizontal width-overflow) — geometry half only, see the citation on
/// `single_child_scroll_view_lays_child_out_unbounded_on_scroll_axis` above.
#[test]
fn single_child_scroll_view_horizontal_lays_child_unbounded_on_width() {
    use flui_widgets::prelude::Axis;
    let laid = lay_out(
        SingleChildScrollView::new()
            .scroll_direction(Axis::Horizontal)
            .child(SizedBox::new(800.0, 100.0)),
        tight(300.0, 100.0),
    );
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(300.0, 100.0));
    let child = laid.only_child(laid.only_child(viewport));
    assert_eq!(laid.size(child), size(800.0, 100.0));
}

#[test]
fn list_view_gives_each_row_the_fixed_item_extent() {
    // 4 rows at item_extent 50 → 200 total scroll extent in a 120-tall viewport.
    // Each childless ColoredBox fills its slot: viewport-wide × item_extent.
    let rows: Vec<_> = [
        Color::rgb(229, 57, 53),
        Color::rgb(30, 136, 229),
        Color::rgb(67, 160, 71),
        Color::rgb(255, 193, 7),
    ]
    .into_iter()
    .map(|c| ColoredBox::new(c).boxed())
    .collect();

    let laid = lay_out(ListView::new(50.0, rows), tight(200.0, 120.0));
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 120.0));

    // Viewport → SliverFixedExtentList → first row: forced to item_extent (50)
    // on the main axis, viewport-wide on the cross axis.
    let list = laid.only_child(viewport);
    let first_row = laid.child(list, 0);
    assert_eq!(laid.size(first_row), size(200.0, 50.0));
}

#[test]
fn list_view_shrink_wrap_sizes_to_static_fixed_extent_content() {
    let rows: Vec<_> = (0..4).map(|_| SizedBox::shrink().boxed()).collect();
    let laid = lay_out(
        ListView::new(50.0, rows).shrink_wrap(true),
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(500.0)),
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert!(laid.find_all_by_render_type("RenderViewport").is_empty());
    assert_eq!(
        laid.size(viewport),
        size(200.0, 200.0),
        "4 fixed-extent rows at 50px must shrink-wrap to 200px high"
    );
}

#[test]
fn list_view_builder_shrink_wrap_sizes_to_settled_lazy_content() {
    // The shrink_wrap + lazy-builder combination is otherwise never exercised
    // together: `list_view_builder_builds_all_visible_items` covers lazy
    // without shrink_wrap, `list_view_shrink_wrap_sizes_to_static_fixed_extent_content`
    // covers shrink_wrap without lazy.
    let mut laid = lay_out(
        ListView::builder(3, 50.0, |index| {
            (index < 3).then(|| SizedBox::new(200.0, 50.0).boxed())
        })
        .shrink_wrap(true),
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(500.0)),
    );

    laid.tick();
    laid.tick();

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert!(laid.find_all_by_render_type("RenderViewport").is_empty());
    assert_eq!(
        laid.size(viewport),
        size(200.0, 150.0),
        "3 settled lazy items at a 50px estimate must shrink-wrap to 150px high"
    );
}

#[test]
fn list_view_horizontal_lays_rows_out_along_the_width() {
    use flui_widgets::prelude::Axis;

    let rows: Vec<_> = (0..2).map(|_| SizedBox::shrink().boxed()).collect();
    let laid = lay_out(
        ListView::new(50.0, rows).scroll_direction(Axis::Horizontal),
        tight(200.0, 120.0),
    );

    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 120.0));

    // Horizontal axis_direction: each row is forced to item_extent (50) on
    // the horizontal main axis, viewport-tall on the cross axis.
    let list = laid.only_child(viewport);
    let first_row = laid.child(list, 0);
    assert_eq!(laid.size(first_row), size(50.0, 120.0));
}

#[test]
fn custom_scroll_view_shrink_wrap_sizes_to_sliver_content() {
    let laid = lay_out(
        CustomScrollView::new((SliverFixedExtentList::new(
            30.0,
            vec![SizedBox::shrink().boxed(), SizedBox::shrink().boxed()],
        ),))
        .shrink_wrap(true),
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(500.0)),
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert!(laid.find_all_by_render_type("RenderViewport").is_empty());
    assert_eq!(
        laid.size(viewport),
        size(200.0, 60.0),
        "2 fixed-extent sliver rows at 30px must shrink-wrap to 60px high"
    );
}

#[test]
fn grid_view_shrink_wrap_sizes_to_grid_rows() {
    let tiles: Vec<_> = (0..4).map(|_| SizedBox::shrink().boxed()).collect();
    let laid = lay_out(
        GridView::count(2, tiles).shrink_wrap(true),
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(500.0)),
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert!(laid.find_all_by_render_type("RenderViewport").is_empty());
    assert_eq!(
        laid.size(viewport),
        size(200.0, 200.0),
        "4 square grid tiles in 2 columns must form 2 rows at 100px each"
    );
}

#[test]
fn sliver_padding_insets_its_sliver_child() {
    use flui_widgets::{SliverPadding, SliverToBoxAdapter, Viewport};
    // Viewport → SliverPadding(10) → SliverToBoxAdapter → box: the padding's
    // 10-per-side cross inset shrinks the box's cross axis to 200-20 = 180.
    let laid = lay_out(
        Viewport::new((SliverPadding::all(10.0)
            .child(SliverToBoxAdapter::new().child(SizedBox::new(180.0, 100.0))),)),
        tight(200.0, 300.0),
    );
    let viewport = laid.root();
    assert_eq!(laid.size(viewport), size(200.0, 300.0));

    let padding = laid.only_child(viewport);
    let adapter = laid.only_child(padding);
    let box_child = laid.only_child(adapter);
    assert_eq!(laid.size(box_child), size(180.0, 100.0));
}

// ============================================================================
// Viewport — Position/Pixels mode switching
// ============================================================================

/// Regression: a `Viewport` reused across a Position-mode build (offset
/// injected from a `ScrollController`) followed by a Pixels-mode rebuild
/// (`.offset(constant)`) must not keep pushing that constant into the
/// PRIOR build's shared, controller-owned `ScrollPosition` — `update_render_object`
/// only ever sees the new build's config, not the old one's, so the render
/// object must detect that the currently-installed offset is foreign
/// (`ScrollPosition::is_uniquely_held` is false — the controller also holds
/// a clone) and swap in a fresh, privately-owned position before pushing.
///
/// Without the fix, the Pixels arm called `set_pixels` on whatever offset
/// was already installed — after a prior Position-mode build that is the
/// controller's shared position, so this test's `controller.pixels()`
/// assertion catches the stomp, and the widget's own geometry check catches
/// the case where the switch is detected but the constant is never actually
/// applied.
#[test]
fn viewport_position_to_pixels_mode_switch_does_not_stomp_the_shared_controller_position() {
    use flui_widgets::Viewport;

    // 10 rows at 50px = 500px content in a 120px viewport -> real
    // max_scroll_extent = 380, comfortably above the 200 seeded below (no
    // incidental clamp from the first layout's own apply_content_dimensions
    // muddying the mode-switch assertion).
    let controller = ScrollController::new();
    controller.set_pixels(200.0);

    fn rows() -> Vec<flui_view::BoxedView> {
        (0..10)
            .map(|_| SizedBox::new(200.0, 50.0).boxed())
            .collect()
    }

    // First build: Position mode, injecting the controller's shared
    // position (currently at pixels=200).
    let position_widget =
        Viewport::new((SliverFixedExtentList::new(50.0, rows()),)).position(controller.position());
    let mut laid = lay_out(position_widget, tight(200.0, 120.0));

    // Second build, same tree position — the element/render object is
    // REUSED (not remounted), so this exercises the mode-switch path:
    // Pixels mode at a constant (42.0) distinct from the controller's 200.
    let pixels_widget = Viewport::new((SliverFixedExtentList::new(50.0, rows()),)).offset(42.0);
    laid.pump_widget(pixels_widget);

    assert_eq!(
        controller.pixels(),
        200.0,
        "a Position-to-Pixels mode switch must not push into the controller's shared \
         ScrollPosition; got {:.1}",
        controller.pixels()
    );

    // And the widget must genuinely be scrolled to its OWN 42px constant
    // (not stuck at 200, and not silently reset to 0): compare its item
    // geometry against a widget built fresh, directly in Pixels mode, at
    // the same 42.0 constant — a correct mode switch makes these identical.
    let switched_sliver = laid.only_child(laid.root());
    let switched_item_offset = laid.absolute_offset(laid.child(switched_sliver, 0));

    let fresh_widget = Viewport::new((SliverFixedExtentList::new(50.0, rows()),)).offset(42.0);
    let fresh_laid = lay_out(fresh_widget, tight(200.0, 120.0));
    let fresh_sliver = fresh_laid.only_child(fresh_laid.root());
    let fresh_item_offset = fresh_laid.absolute_offset(fresh_laid.child(fresh_sliver, 0));

    assert_eq!(
        switched_item_offset, fresh_item_offset,
        "after the mode switch the widget must be scrolled to its own 42px constant, matching \
         a viewport built fresh directly in Pixels mode at the same offset"
    );
}

/// The render-side counterpart to
/// `scrollable_position_mode_relayouts_from_external_mutation_with_no_pixels_push`:
/// that test's relayout rides `Scrollable`'s `AnimatedBuilder` subscription,
/// which schedules a widget rebuild when the shared `ScrollPosition`
/// notifies. A BARE `Viewport::position(...)` — no `Scrollable`, no
/// `AnimatedBuilder`, nothing subscribed to the position at the widget layer
/// at all — has no such rebuild path: [`LaidOut::tick`] drives a frame
/// WITHOUT marking anything dirty at the widget level (the headless
/// equivalent of an idle event loop with no `setState` anywhere), so before
/// `RenderViewport` listened to its own `ViewportOffset`, an external
/// `position.set_pixels(...)` here was dead on arrival — nothing observed
/// it, and committed paint never moved. `RenderViewport::attach` (Flutter
/// parity: `rendering/viewport.dart`'s `offset.addListener(markNeedsLayout)`)
/// closes that gap: the render object marks its OWN layout dirty straight
/// off the offset's notification, no widget rebuild required.
#[test]
fn bare_viewport_position_mode_relayouts_via_the_render_side_listener_with_no_widget_rebuild_path()
{
    use flui_rendering::view::ScrollPosition;
    use flui_widgets::Viewport;

    fn rows() -> Vec<flui_view::BoxedView> {
        (0..10)
            .map(|_| SizedBox::new(200.0, 50.0).boxed())
            .collect()
    }

    // 10 rows at 50px = 500px content in a 120px viewport -> 380px of real
    // scroll range, comfortably above the 120px jump below.
    let position = ScrollPosition::new(0.0);
    let widget =
        Viewport::new((SliverFixedExtentList::new(50.0, rows()),)).position(position.clone());
    let mut laid = lay_out(widget, tight(200.0, 120.0));

    let sliver = laid.only_child(laid.root());
    let offset_before = laid.absolute_offset(laid.child(sliver, 0));

    // External mutation: no gesture, no `Scrollable`, no widget anywhere
    // subscribed to `position` — nothing schedules a rebuild.
    position.set_pixels(120.0);

    // `tick()` (unlike `pump()`) does NOT mark the root needing build — only
    // a render-object-level self-mark can move committed paint here.
    laid.tick();

    let offset_after = laid.absolute_offset(laid.child(sliver, 0));
    assert_ne!(
        offset_before, offset_after,
        "a bare Viewport in Position mode (no Scrollable/AnimatedBuilder anywhere) must \
         relayout on an external ScrollPosition mutation via the render-side offset \
         listener alone, with zero widget-level rebuild path involved"
    );
}

// ============================================================================
// ListView / GridView — `.position()` passthrough
// ============================================================================

/// Mirrors `scrollable_content_dimension_feedback_supplies_extents_and_notifies_a_listener`'s
/// zero-`update_dimensions` pin, for `ListView` itself rather than
/// `Scrollable`: `ListView::position` must hand the injected `ScrollPosition`
/// straight through to the composed `Viewport`, so
/// `RenderViewport::perform_layout`'s committed content extents land in the
/// SAME controller a caller reads — no manual extent feed anywhere in this
/// test — and a subsequent `set_pixels` must move the committed paint. This
/// widget's own tree wraps a `ListView` bare (no `Scrollable`, so the
/// `AnimatedBuilder` rebuild path isn't in play here) and drives the
/// relayout via `.pump()` (root-dirty), so it exercises the ordinary
/// widget-rebuild path rather than `RenderViewport`'s render-side offset
/// listener specifically —
/// `bare_viewport_position_mode_relayouts_via_the_render_side_listener_with_no_widget_rebuild_path`
/// isolates that listener on its own with `.tick()` (no root-dirty).
#[test]
fn list_view_position_passthrough_feeds_the_content_dimension_feedback_loop() {
    let controller = ScrollController::new();
    // 12 rows at 50px = 600px content in a 120px viewport -> 480px scroll extent.
    let rows: Vec<_> = (0..12)
        .map(|_| SizedBox::new(200.0, 50.0).boxed())
        .collect();
    let widget = ListView::new(50.0, rows).position(controller.position());

    let mut laid = lay_out(widget, tight(200.0, 120.0));
    laid.pump();

    assert!(
        controller.max_scroll_extent() > 0.0,
        "ListView::position must feed RenderViewport::perform_layout's committed content \
         extents into the injected ScrollPosition with zero update_dimensions calls; got {:.1}",
        controller.max_scroll_extent()
    );

    let viewport = laid.root();
    let sliver = laid.only_child(viewport);
    let offset_before = laid.absolute_offset(laid.child(sliver, 0));

    controller.set_pixels(100.0);
    laid.pump();

    let offset_after = laid.absolute_offset(laid.child(sliver, 0));
    assert_ne!(
        offset_before, offset_after,
        "controller.set_pixels must move ListView's committed paint after the next rebuild \
         picks up the shared ScrollPosition; got {offset_before:?} both before and after"
    );
}

/// Same pin as
/// `list_view_position_passthrough_feeds_the_content_dimension_feedback_loop`,
/// for `GridView`.
#[test]
fn grid_view_position_passthrough_feeds_the_content_dimension_feedback_loop() {
    let controller = ScrollController::new();
    // 8 square tiles in 2 columns = 4 rows; 200px viewport width / 2 columns =
    // 100px tiles -> 400px content in a 200px viewport -> 200px scroll extent.
    let tiles: Vec<_> = (0..8).map(|_| SizedBox::shrink().boxed()).collect();
    let widget = GridView::count(2, tiles).position(controller.position());

    let mut laid = lay_out(widget, tight(200.0, 200.0));
    laid.pump();

    assert!(
        controller.max_scroll_extent() > 0.0,
        "GridView::position must feed RenderViewport::perform_layout's committed content \
         extents into the injected ScrollPosition with zero update_dimensions calls; got {:.1}",
        controller.max_scroll_extent()
    );

    let viewport = laid.root();
    let sliver = laid.only_child(viewport);
    let offset_before = laid.absolute_offset(laid.child(sliver, 0));

    controller.set_pixels(80.0);
    laid.pump();

    let offset_after = laid.absolute_offset(laid.child(sliver, 0));
    assert_ne!(
        offset_before, offset_after,
        "controller.set_pixels must move GridView's committed paint after the next rebuild \
         picks up the shared ScrollPosition; got {offset_before:?} both before and after"
    );
}

/// Same pin as `list_view_position_passthrough_feeds_the_content_dimension_feedback_loop`,
/// under [`ListView::shrink_wrap`] — the Business.1 remainder this closes.
/// Before the fix, the shrink_wrap arm snapshotted `position.pixels()` once
/// per rebuild into a private `ShrinkWrappingViewport::offset(f32)`, so
/// `RenderShrinkWrappingViewport`'s committed content extents never flushed
/// back into `controller` (`max_scroll_extent()` stayed `0.0`) and a
/// subsequent `set_pixels` never moved committed paint until the next
/// rebuild happened to re-snapshot. Content (600px) exceeds the 120px main-
/// axis bound, so the shrink-wrapped viewport clamps to 120px and genuinely
/// scrolls — same shape as the non-shrink-wrap pin above.
#[test]
fn list_view_shrink_wrap_position_passthrough_feeds_the_content_dimension_feedback_loop() {
    let controller = ScrollController::new();
    // 12 rows at 50px = 600px content, bounded to a 120px main-axis max.
    let rows: Vec<_> = (0..12)
        .map(|_| SizedBox::new(200.0, 50.0).boxed())
        .collect();
    let widget = ListView::new(50.0, rows)
        .shrink_wrap(true)
        .position(controller.position());

    let mut laid = lay_out(
        widget,
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(120.0)),
    );
    laid.pump();

    assert!(
        controller.max_scroll_extent() > 0.0,
        "ListView::shrink_wrap(true).position must feed \
         RenderShrinkWrappingViewport::perform_layout's committed content extents into the \
         injected ScrollPosition with zero update_dimensions calls; got {:.1}",
        controller.max_scroll_extent()
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    let sliver = laid.only_child(viewport);
    let offset_before = laid.absolute_offset(laid.child(sliver, 0));

    controller.set_pixels(100.0);
    laid.pump();

    let offset_after = laid.absolute_offset(laid.child(sliver, 0));
    assert_ne!(
        offset_before, offset_after,
        "controller.set_pixels must move a shrink-wrapped ListView's committed paint after the \
         next rebuild picks up the shared ScrollPosition; got {offset_before:?} both before and \
         after"
    );
}

/// Same pin as `list_view_shrink_wrap_position_passthrough_feeds_the_content_dimension_feedback_loop`,
/// for `GridView`.
#[test]
fn grid_view_shrink_wrap_position_passthrough_feeds_the_content_dimension_feedback_loop() {
    let controller = ScrollController::new();
    // 8 square tiles in 2 columns = 4 rows at 100px each = 400px content,
    // bounded to a 200px main-axis max.
    let tiles: Vec<_> = (0..8).map(|_| SizedBox::shrink().boxed()).collect();
    let widget = GridView::count(2, tiles)
        .shrink_wrap(true)
        .position(controller.position());

    let mut laid = lay_out(
        widget,
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(200.0)),
    );
    laid.pump();

    assert!(
        controller.max_scroll_extent() > 0.0,
        "GridView::shrink_wrap(true).position must feed \
         RenderShrinkWrappingViewport::perform_layout's committed content extents into the \
         injected ScrollPosition with zero update_dimensions calls; got {:.1}",
        controller.max_scroll_extent()
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    let sliver = laid.only_child(viewport);
    let offset_before = laid.absolute_offset(laid.child(sliver, 0));

    controller.set_pixels(80.0);
    laid.pump();

    let offset_after = laid.absolute_offset(laid.child(sliver, 0));
    assert_ne!(
        offset_before, offset_after,
        "controller.set_pixels must move a shrink-wrapped GridView's committed paint after the \
         next rebuild picks up the shared ScrollPosition; got {offset_before:?} both before and \
         after"
    );
}

// ============================================================================
// ScrollController — thumb geometry helpers
// ============================================================================

#[test]
fn scroll_controller_thumb_fraction_is_proportional_to_viewport_over_content() {
    // viewport = 300, content = 600 (300 viewport + 300 scroll extent).
    // thumb_fraction = viewport / content = 300 / 600 = 0.5
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 300.0);

    let fraction = controller.thumb_fraction();
    assert!(
        (fraction - 0.5).abs() < 0.001,
        "thumb fraction should be 0.5 when viewport equals scroll extent (content = 2×viewport), got {fraction}"
    );
}

#[test]
fn scroll_controller_thumb_fraction_is_one_when_content_fits_in_viewport() {
    // max_scroll_extent = 0 → content fits entirely; thumb fills the track.
    let controller = ScrollController::new();
    controller.update_dimensions(400.0, 0.0, 0.0);

    assert_eq!(
        controller.thumb_fraction(),
        1.0,
        "thumb fraction must be 1.0 when scroll_extent is zero (content shorter than viewport)"
    );
}

#[test]
fn scroll_controller_thumb_offset_fraction_at_scroll_midpoint() {
    // viewport = 400, scroll_extent = 400, content = 800.
    // offset_fraction = (pixels - min_scroll_extent) / scroll_extent — a
    // fraction of the AVAILABLE track, independent of thumb_fraction (see
    // `ScrollController::thumb_offset_fraction`'s doc for why folding in
    // `(1 - thumb_fraction)` here would be a double-application once
    // `Scrollbar` multiplies by `available_track`, which already contains
    // that factor).
    // At pixels = 200 (halfway): offset_fraction = 200/400 = 0.5
    let controller = ScrollController::new();
    controller.update_dimensions(400.0, 0.0, 400.0);
    controller.set_pixels(200.0);

    let frac = controller.thumb_offset_fraction();
    assert!(
        (frac - 0.5).abs() < 0.001,
        "thumb offset fraction at scroll midpoint should be 0.5, got {frac}"
    );
}

// ============================================================================
// ScrollPhysics — clamping boundary enforcement
// ============================================================================

/// Minimal metrics fixture for these boundary-clamp tests: only
/// `min_scroll_extent`/`max_scroll_extent` matter to `ClampingScrollPhysics`;
/// `pixels`/`viewport_dimension` are passed explicitly as `0.0` (unused here).
fn metrics_with_extents(min_scroll_extent: f32, max_scroll_extent: f32) -> ScrollMetrics {
    ScrollMetrics::new(0.0, min_scroll_extent, max_scroll_extent, 0.0)
}

#[test]
fn clamping_physics_clamps_proposed_offset_below_minimum() {
    let physics = ClampingScrollPhysics::default();
    // Proposing -50 (past the 0 minimum) must snap to 0.
    let result = physics.apply_boundary_conditions(&metrics_with_extents(0.0, 500.0), -50.0);
    assert_eq!(
        result, 0.0,
        "clamping physics must clamp below-min proposals to min_scroll_extent"
    );
}

#[test]
fn clamping_physics_clamps_proposed_offset_above_maximum() {
    let physics = ClampingScrollPhysics::default();
    // Proposing 600 past the 500 maximum must snap to 500.
    let result = physics.apply_boundary_conditions(&metrics_with_extents(0.0, 500.0), 600.0);
    assert_eq!(
        result, 500.0,
        "clamping physics must clamp above-max proposals to max_scroll_extent"
    );
}

#[test]
fn clamping_physics_passes_through_in_range_offset() {
    let physics = ClampingScrollPhysics::default();
    let result = physics.apply_boundary_conditions(&metrics_with_extents(0.0, 500.0), 250.0);
    assert_eq!(
        result, 250.0,
        "clamping physics must pass through in-range proposals unchanged"
    );
}

// ============================================================================
// Scrollable — drag gesture integration
// ============================================================================

/// A drag upward (finger moves toward smaller y-values, delta.dy < 0) must
/// increase the scroll offset because upward drag reveals content below the
/// current viewport position. This test FAILS if the pan callback is not
/// wired: `controller.pixels()` stays 0.0 when no gesture fires.
#[test]
fn scrollable_drag_up_increases_scroll_offset() {
    let controller = ScrollController::new();
    // 300px viewport, 800px content → 500px scroll extent.
    controller.update_dimensions(300.0, 0.0, 500.0);

    let physics: SharedScrollPhysics = Arc::new(ClampingScrollPhysics::default());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Starting position: top of content.
    assert_eq!(controller.pixels(), 0.0, "initial scroll offset must be 0");

    // With no competing recognizer, the arena awards the drag after Down.
    // The first 50px upward move is therefore delivered in full.
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);

    assert_eq!(
        controller.pixels(),
        50.0,
        "an upward 50px finger move must increase the scroll offset by exactly 50px"
    );
}

/// With a tap recognizer competing below the Scrollable, a sub-slop move
/// leaves the arena unresolved and must not move the scroll position.
#[test]
fn scrollable_sub_slop_drag_waits_while_a_tap_competitor_remains() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);

    let widget = GestureDetector::new().on_tap(|| {}).child(
        Scrollable::new()
            .controller(controller.clone())
            .child(SizedBox::new(300.0, 800.0)),
    );

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Move only 5px — below the 18px drag slop while tap still competes.
    scoped.dispatch_pointer_down(150.0, 150.0);
    scoped.dispatch_pointer_move(150.0, 145.0);
    scoped.dispatch_pointer_up(150.0, 145.0);

    assert_eq!(
        controller.pixels(),
        0.0,
        "a sub-slop movement must not move while a tap competitor remains"
    );
}

/// Without a competitor, Flutter's arena awards the lone drag recognizer
/// after Down and `onlyAcceptDragOnThreshold` remains false. Its first
/// sub-slop move is therefore a real scroll update.
#[test]
fn scrollable_lone_drag_applies_the_first_sub_slop_move() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);

    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 800.0));
    let scoped = lay_out(widget, tight(300.0, 300.0));

    scoped.dispatch_pointer_down(150.0, 150.0);
    scoped.dispatch_pointer_move(150.0, 145.0);
    scoped.dispatch_pointer_up(150.0, 145.0);

    assert_eq!(
        controller.pixels(),
        5.0,
        "the lone recognizer's first -5px move must scroll forward by 5px"
    );
}

/// A drag at the bottom edge (offset = max_scroll_extent) must not scroll
/// further: clamping physics holds the position at the maximum.
#[test]
fn scrollable_drag_up_at_max_extent_is_clamped_by_physics() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);
    // Start at the very bottom.
    controller.set_pixels(500.0);

    let physics: SharedScrollPhysics = Arc::new(ClampingScrollPhysics::default());
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // The lone drag starts after Down. A 10px upward move proposes 510,
    // and clamping physics holds it at 500.
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 190.0);
    scoped.dispatch_pointer_up(150.0, 190.0);

    assert!(
        controller.pixels() <= 500.0,
        "clamping physics must not allow the offset to exceed max_scroll_extent (500); \
         got {:.1}",
        controller.pixels()
    );
}

/// Pins that Position-mode scrolling rides `RenderBehavior::on_update`'s
/// UNCONDITIONAL relayout mark (`flui-view/src/element/behavior.rs`, the
/// `mark_render_needs_layout_and_paint` call that follows every
/// `update_render_object`, regardless of whether anything about the widget's
/// own configuration changed), not a value comparison inside
/// `Viewport::update_render_object` — in Position mode that method never
/// pushes pixels at all (the injected `ScrollPosition`'s `Arc` identity is
/// unchanged between rebuilds, so its `ptr_eq` guard skips `set_offset` too).
///
/// The mutation below goes through `ScrollController::set_pixels` directly —
/// deliberately NOT through this widget's own `on_pan_update` gesture
/// callback — to prove the relayout does not depend on that code path
/// either: it is driven purely by the unconditional dirty-mark that fires
/// whenever `AnimatedBuilder`'s listenable-driven rebuild re-diffs the
/// (structurally unchanged) `Viewport` view against the mounted render
/// object.
///
/// A future compare-and-mark memoization — e.g. skipping
/// `mark_render_needs_layout_and_paint` when the `Viewport` view "looks
/// unchanged" between rebuilds — would leave the render tree at its
/// pre-mutation offset here, and this test FAILS.
#[test]
fn scrollable_position_mode_relayouts_from_external_mutation_with_no_pixels_push() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);

    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = lay_out(widget, tight(300.0, 300.0));
    let box_before = scoped.find_by_render_type("RenderConstrainedBox");
    let offset_before = scoped.absolute_offset(box_before);

    // External mutation of the shared `ScrollPosition` — no gesture, no
    // `update_render_object` pixels push.
    controller.set_pixels(120.0);

    // `AnimatedBuilder`'s subscription to the same listenable schedules a
    // rebuild when `set_pixels` notifies; this drains it and re-runs layout.
    scoped.pump_for(Duration::ZERO);

    let box_after = scoped.find_by_render_type("RenderConstrainedBox");
    let offset_after = scoped.absolute_offset(box_after);

    assert_ne!(
        offset_before, offset_after,
        "an external ScrollPosition mutation with no gesture and no pixels push from \
         update_render_object must still relayout the child to the new offset"
    );
}

/// Loop-termination pin: the post-frame content-dimension flush now has TWO
/// listeners on the same shared `ScrollPosition` — the pre-existing
/// `AnimatedBuilder` widget-rebuild subscription `Scrollable` installs, and
/// `RenderViewport`'s own render-side offset listener (this change). Both
/// can fire off the SAME coalesced flush; this proves they don't keep
/// re-triggering each other into an unbounded relayout loop.
///
/// Mechanism (why this terminates): `ViewportOffset::apply_content_dimensions`
/// only marks the position's metrics dirty — and so only schedules another
/// flush — on a REAL extent change (`scroll_position.rs`'s epsilon guards).
/// Once a relayout re-commits the SAME extents, nothing schedules a further
/// flush, nothing notifies, and the render listener has nothing left to
/// fire — matching `set_pixels`'s own epsilon guard against no-op writes.
#[test]
fn scrollable_offset_listener_settles_within_a_bounded_number_of_ticks_after_external_mutation() {
    let controller = ScrollController::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 800.0));

    let mut laid = lay_out(widget, tight(300.0, 300.0));
    let box_before = laid.find_by_render_type("RenderConstrainedBox");
    let offset_before = laid.absolute_offset(box_before);

    // External mutation — same shape as
    // `scrollable_position_mode_relayouts_from_external_mutation_with_no_pixels_push`.
    controller.set_pixels(120.0);

    const SETTLE_BUDGET: usize = 5;
    let mut offsets = Vec::with_capacity(SETTLE_BUDGET);
    for _ in 0..SETTLE_BUDGET {
        laid.tick();
        let box_now = laid.find_by_render_type("RenderConstrainedBox");
        offsets.push(laid.absolute_offset(box_now));
    }

    assert_ne!(
        offsets[0], offset_before,
        "the mutation must actually move committed paint within the settle budget"
    );
    assert_eq!(
        offsets[SETTLE_BUDGET - 1],
        offsets[SETTLE_BUDGET - 2],
        "geometry must settle to a fixed point well within {SETTLE_BUDGET} idle ticks after \
         the external mutation — a still-changing value here would mean the post-frame flush \
         and the render-side offset listener are re-triggering each other in an unbounded \
         relayout loop instead of going quiescent"
    );
}

/// Pins the content-dimension feedback loop end-to-end, with **zero**
/// `update_dimensions` calls anywhere in this test — every existing
/// `update_dimensions`-seeded test in this file (and `scroll_controller.rs`'s
/// unit tests) exercises the legacy explicit-feed path, which would keep
/// passing even if the feedback loop itself were dead. This test is the one
/// that would catch that: extents must arrive purely from
/// `RenderViewport::perform_layout`'s `apply_viewport_dimension`/
/// `apply_content_dimensions` writing into the controller's shared
/// `ScrollPosition`, and a listener must observe the coalesced post-frame
/// flush `ScrollableState::init_state` installs.
///
/// FAILS if `apply_content_dimensions` stops writing through to the shared
/// position (the `max_scroll_extent` assertion), or if the coalesced flush
/// never fires (the listener-count assertion) — e.g. a flush handle that
/// silently isn't installed, or a flush that never calls `notify()`.
#[test]
fn scrollable_content_dimension_feedback_supplies_extents_and_notifies_a_listener() {
    let controller = ScrollController::new();
    let listener_fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&listener_fired);
    controller.as_listenable().add_listener(Arc::new(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));

    // 300px viewport, 800px content — the exact geometry
    // `scrollable_drag_up_increases_scroll_offset` seeds by hand via
    // `update_dimensions(300.0, 0.0, 500.0)`. Here nothing seeds it.
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = lay_out(widget, tight(300.0, 300.0));

    // Extents write through to the shared state SYNCHRONOUSLY during layout
    // (only the listener notification is deferred) — readable immediately,
    // no pump required.
    assert!(
        controller.max_scroll_extent() > 0.0,
        "RenderViewport::perform_layout must commit a nonzero max_scroll_extent (300px \
         viewport, 800px content -> 500px scroll extent) into the shared ScrollPosition with \
         zero update_dimensions calls; got {:.1}",
        controller.max_scroll_extent()
    );
    assert_eq!(
        listener_fired.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the coalesced flush must not have run before any frame completed"
    );

    // Drive a completed frame: drains the scheduler's post-frame queue,
    // firing the coalesced flush.
    scoped.pump_for(Duration::ZERO);

    assert!(
        listener_fired.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "a listener registered via ScrollController::as_listenable() must observe the \
         content-dimension feedback loop's coalesced post-frame flush after a completed frame"
    );

    // The extents the feedback loop supplied are real clamp bounds, not just
    // readable numbers: a drag past them must still clamp, purely off this
    // loop's output (again, no update_dimensions in this test).
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: 70 px upward
    scoped.dispatch_pointer_move(150.0, 10.0); // 170 px more upward: on_pan_update
    scoped.dispatch_pointer_up(150.0, 10.0);

    assert!(
        controller.pixels() <= controller.max_scroll_extent() + 0.01,
        "a drag past the feedback-loop-supplied max_scroll_extent ({:.1}) must clamp there, \
         not exceed it; got {:.1}",
        controller.max_scroll_extent(),
        controller.pixels()
    );
    assert!(
        controller.pixels() > 0.0,
        "the drag must have moved the scroll position at all; got {:.1}",
        controller.pixels()
    );
}

/// `Scrollable::viewport_builder` composes an ARBITRARY scrollable widget
/// (here: a `Viewport` over a `SliverFixedExtentList`, bypassing
/// `SingleChildScrollView` entirely) instead of the default single-child
/// fast path — and the drag/fling gesture wiring and content-dimension
/// feedback loop must still drive it, because the closure was handed the
/// controller's own shared `ScrollPosition` to inject.
#[test]
fn scrollable_viewport_builder_composes_a_custom_viewport_with_working_drag_and_feedback() {
    use flui_widgets::Viewport;

    let controller = ScrollController::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .viewport_builder(std::rc::Rc::new(
            |position: flui_widgets::ScrollPosition| {
                let rows: Vec<_> = (0..12)
                    .map(|_| SizedBox::new(300.0, 50.0).boxed())
                    .collect();
                Viewport::new((SliverFixedExtentList::new(50.0, rows),))
                    .position(position)
                    .boxed()
            },
        ));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // No update_dimensions anywhere: extents must arrive from the custom
    // viewport's own layout — the same feedback loop the SCSV fast path
    // uses, proving the builder path isn't a separate, unwired mechanism.
    // 12 rows * 50px = 600px content in a 300px viewport -> 300px extent.
    assert!(
        controller.max_scroll_extent() > 0.0,
        "the custom viewport_builder composition must feed extents back into the controller \
         via the same content-dimension feedback loop; got {:.1}",
        controller.max_scroll_extent()
    );

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: 70 px upward
    scoped.dispatch_pointer_move(150.0, 140.0); // 40 px more: on_pan_update
    scoped.dispatch_pointer_up(150.0, 140.0);

    assert!(
        controller.pixels() > 0.0,
        "dragging through a Scrollable composed via viewport_builder must move the scroll \
         position; got {:.1}",
        controller.pixels()
    );
}

// ============================================================================
// Scrollable — fling ballistic simulation integration
// ============================================================================

/// Wrap `widget` in a [`VsyncScope`] so its `ScrollableState::init_state` can
/// register the fling controller, then lay it out under `constraints` with a
/// gesture arena. Adopts the same vsync in the tree binding so
/// [`LaidOut::pump_for`] ticks the fling animation deterministically.
fn fling_scoped(widget: Scrollable, vsync: Vsync, constraints: BoxConstraints) -> LaidOut {
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, constraints);
    scoped.adopt_vsync(vsync);
    scoped
}

/// After a pan gesture ends with sufficient velocity the scroll offset must
/// continue to advance beyond the release position when the binding pumps
/// animation frames — confirming that the fling animation controller is wired
/// to the scroll controller and the vsync is driving it.
#[test]
fn scrollable_fling_advances_offset_past_release() {
    let controller = ScrollController::new();
    // Large extent prevents the fling from hitting the boundary on the first
    // frame — we want to observe forward motion, not clamping.
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Upward drag well past the 18 px slop to establish a recognizable fling
    // velocity. The first move crosses slop (on_pan_start). The second fires
    // on_pan_update, advancing the offset. The pointer_up triggers on_pan_end
    // which calls animate_with on the fling controller.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 150.0); // 30 px more: on_pan_update
    scoped.dispatch_pointer_up(150.0, 150.0);

    let pixels_at_release = controller.pixels();
    assert!(
        pixels_at_release > 0.0,
        "pan drag must advance the offset before release; got {pixels_at_release}"
    );

    // First pump: vsync detects the new run generation from `animate_with` and
    // anchors the run start at t=0. The controller ticks at elapsed=0, which
    // gives x(0) = start (the release position). No net advance yet.
    scoped.pump_for(Duration::from_millis(16));
    // Second pump: advances to t=16 ms. The ballistic simulation gives
    // x(0.016) > start (friction deceleration carries the position forward).
    scoped.pump_for(Duration::from_millis(16));

    assert!(
        controller.pixels() > pixels_at_release,
        "scroll offset must continue past the release position after two fling frames; \
         release={pixels_at_release:.1}, now={:.1}",
        controller.pixels()
    );
}

/// Same gesture as [`scrollable_fling_advances_offset_past_release`], but with
/// deliberately irregular REAL delays inserted between the dispatch calls —
/// standing in for the scheduler jitter a loaded CI runner introduces between
/// one Rust statement and the next. The sleeps are adversarial input, not
/// synchronization: nothing about this test's outcome should depend on how
/// long they actually took.
///
/// A pointer's own velocity samples must come from the binding's virtual
/// clock, never from wall time — so however long the real gaps between
/// `dispatch_pointer_*` calls turn out to be, the recorded sample spacing
/// (and therefore the fling velocity) is unaffected. This is the deterministic
/// reproduction of the flake `scrollable_fling_advances_offset_past_release`
/// hit intermittently under load: that test's own real dispatch gaps were
/// implicitly whatever the machine happened to schedule; this test makes the
/// worst case explicit and asserts it still doesn't corrupt the fling.
#[test]
fn scrollable_fling_survives_irregular_real_dispatch_timing() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    scoped.dispatch_pointer_down(150.0, 250.0);
    // Wildly uneven real gaps — nothing a genuine pointer's report rate would
    // ever produce, chosen to make the corruption obvious if wall time leaks
    // into the recorded sample timestamps.
    std::thread::sleep(Duration::from_millis(2));
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    std::thread::sleep(Duration::from_millis(41));
    scoped.dispatch_pointer_move(150.0, 150.0); // 30 px more: on_pan_update
    scoped.dispatch_pointer_up(150.0, 150.0);

    let pixels_at_release = controller.pixels();
    assert!(
        pixels_at_release > 0.0,
        "pan drag must advance the offset before release regardless of real \
         dispatch timing; got {pixels_at_release}"
    );

    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));

    assert!(
        controller.pixels() > pixels_at_release,
        "the fling must keep advancing past the release position no matter how \
         irregular the real time between dispatch calls was; \
         release={pixels_at_release:.1}, now={:.1}",
        controller.pixels()
    );
}

/// Clamping physics must never allow the fling to carry the scroll position
/// past `max_scroll_extent` regardless of the initial fling velocity.
///
/// The drag leaves the position at `max_scroll_extent` (clamped during the pan
/// update phase). The ballistic simulation starts there; even with an extreme
/// velocity the `BoundedFrictionSimulation` respects its upper bound.
#[test]
fn clamping_physics_fling_stays_within_max_extent() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f32;
    controller.update_dimensions(300.0, 0.0, max_extent);

    let physics: SharedScrollPhysics = Arc::new(ClampingScrollPhysics::new());
    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Large upward drag: clamping physics clamps at max_extent during
    // on_pan_update, so we release from the boundary.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: 70 px upward
    scoped.dispatch_pointer_move(150.0, 10.0); // 170 px more upward: on_pan_update
    scoped.dispatch_pointer_up(150.0, 10.0);

    // Pump many frames — even with extreme fling velocity the clamping
    // simulation bounds the final position.
    for _ in 0..30 {
        scoped.pump_for(Duration::from_millis(16));
    }

    assert!(
        controller.pixels() <= max_extent,
        "clamping physics must hold scroll at or below max_extent ({max_extent}); \
         got {:.1}",
        controller.pixels()
    );
}

/// Bouncing physics allows the drag to carry the scroll position past
/// `max_scroll_extent` with spring damping. On release, a
/// `ScrollSpringSimulation` springs the position back to the boundary. After
/// enough frames the position must be within 1 px of `max_scroll_extent`.
#[test]
fn bouncing_physics_fling_springs_back_after_overscroll() {
    let controller = ScrollController::new();
    let max_extent = 500.0_f32;
    controller.update_dimensions(300.0, 0.0, max_extent);

    let physics: SharedScrollPhysics = Arc::new(BouncingScrollPhysics::new());
    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .physics(physics)
        .child(SizedBox::new(300.0, 800.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Pre-position the scroll just below max_extent so a moderate in-bounds
    // upward drag pushes it past the boundary under bouncing physics.
    controller.set_pixels(480.0);

    // Upward drag past slop, then a further in-bounds move that applies
    // `apply_boundary_conditions` and lets pixels exceed max_extent (damped
    // by the overscroll spring coefficient 0.52):
    //   proposed = 480 − (−60) = 540 → clamped = 500 + 40×0.52 = 520.8
    // on_pan_end sees pixels = 520.8 > max_extent and returns a
    // ScrollSpringSimulation that springs the position back to max_extent.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // 70 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 120.0); // 60 px more upward: on_pan_update
    scoped.dispatch_pointer_up(150.0, 120.0);

    // Pump 100 frames (1.6 s) — sufficient for the critically-damped spring
    // (SpringDescription with damping_ratio ≥ 0.75) to settle.
    for _ in 0..100 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let final_pixels = controller.pixels();
    assert!(
        final_pixels <= max_extent + 1.0,
        "bouncing spring-back must return scroll to within 1 px of max_extent ({max_extent}); \
         got {final_pixels:.3}"
    );
}

/// A pan gesture that crosses drag-slop during an active fling fires
/// `on_pan_start`, which calls `fling_controller.stop()`. Subsequent animation
/// frames must not advance the scroll offset — the fling must be dead.
#[test]
fn pan_start_during_fling_halts_momentum() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // --- First gesture: start a fling ---
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: 70 px
    scoped.dispatch_pointer_move(150.0, 150.0); // in-progress update
    scoped.dispatch_pointer_up(150.0, 150.0);

    // Let the fling run for one frame so we know it advanced.
    scoped.pump_for(Duration::from_millis(16));
    let pixels_mid_fling = controller.pixels();
    assert!(
        pixels_mid_fling > 0.0,
        "fling must advance the offset on the first frame; got {pixels_mid_fling:.1}"
    );

    // --- Second gesture: cross slop to fire on_pan_start → fling.stop() ---
    // Using a downward drag (positive dy) so it doesn't overlap with the
    // already-advanced scroll position numerically.
    scoped.dispatch_pointer_down(150.0, 200.0);
    // 50 px downward — well past the 18 px slop, fires on_pan_start which
    // stops the fling. Does NOT fire on_pan_update (slop-crossing move only
    // fires on_start in the DragGestureRecognizer FSM).
    scoped.dispatch_pointer_move(150.0, 250.0);
    // Cancel to avoid triggering on_pan_end (and a new fling).
    scoped.dispatch_pointer_cancel();

    let pixels_after_grab = controller.pixels();

    // --- Pump more frames: fling is stopped, no value-listener fire ---
    for _ in 0..5 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let drift = (controller.pixels() - pixels_after_grab).abs();
    assert!(
        drift <= 1.0,
        "halting the fling via on_pan_start must freeze the scroll offset; \
         offset drifted by {drift:.3} px after grab \
         (from {pixels_after_grab:.1} to {:.1})",
        controller.pixels()
    );
}

// ============================================================================
// Scrollable — animate_to (ADR-0037)
// ============================================================================

/// `animate_to` drives the SAME fling `AnimationController` a ballistic fling
/// uses, through a curve/duration tween: pumping frames must show the offset
/// moving continuously between the start and target (not jumping straight to
/// the end), landing EXACTLY on the target once the duration has elapsed.
///
/// Three pumps of warm-up before real advance begins, one more than
/// `scrollable_fling_advances_offset_past_release`'s direct `animate_with`
/// call needs: `animate_to` queues a command rather than driving the fling
/// controller synchronously (see `scroll_controller.rs`'s module docs), so
/// pump 1 is what services that queue (`flui-testing::pump_frame` ticks
/// registered controllers BEFORE draining the rebuild that services it —
/// `AnimationController::animate_to_curved` only runs during pump 1's
/// rebuild step, too late for pump 1's OWN tick step to see it running).
/// Pump 2 is then the vsync registry's own "detect the new run generation,
/// anchor `t = 0`" pump (same as the direct-`animate_with` fling case);
/// pump 3 is the first tick that actually advances the value.
///
/// Flutter parity: `ScrollController.animateTo`/`ScrollPositionWithSingleContext
/// .animateTo` (`scroll_controller.dart`/`scroll_position_with_single_context.dart`,
/// tag `3.44.0`) drive a `DrivenScrollActivity`'s curve/duration tween from
/// the current position to the target.
#[test]
fn scrollable_animate_to_reaches_the_target_through_the_curve() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    controller.animate_to(1000.0, Duration::from_millis(100), Arc::new(Curves::Linear));

    // Pump 1: services the queued command (starts the run). Pump 2: vsync
    // anchors the new run generation at t=0. Pump 3: the first real tick,
    // 16ms into a 100ms run (t = 0.16).
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    let mid = controller.pixels();
    assert!(
        mid > 0.0 && mid < 1000.0,
        "part-way through a 100ms animate_to, the offset must sit strictly \
         between the start (0.0) and the target (1000.0); got {mid:.2}"
    );

    // Pump comfortably past the 100ms duration.
    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.pixels(),
        1000.0,
        "once the duration has fully elapsed, animate_to must land EXACTLY on \
         the target; got {:.2}",
        controller.pixels()
    );
}

/// A pan gesture that crosses drag-slop DURING an in-flight `animate_to` must
/// halt it at the finger's contact position — `on_pan_start` calls
/// `fling_controller.stop()`, and `animate_to` drives that EXACT SAME
/// controller (the whole reason `ScrollableState` reuses its fling
/// controller instead of a separate one — see `scrollable.rs`'s module
/// docs), so the cancellation falls out of the existing grab-to-stop
/// discipline for free.
#[test]
fn scrollable_grab_during_animate_to_halts_it() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    controller.animate_to(1000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    // Three pumps of warm-up — see `scrollable_animate_to_reaches_the_target_through_the_curve`'s
    // doc for why this needs one more pump than a direct `animate_with` fling.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    let pixels_mid_animation = controller.pixels();
    assert!(
        pixels_mid_animation > 0.0 && pixels_mid_animation < 1000.0,
        "sanity: the animation must be genuinely in flight before the grab; \
         got {pixels_mid_animation:.2}"
    );

    // Grab: cross slop to fire on_pan_start -> fling_controller.stop(). A
    // downward drag so it doesn't overlap numerically with the already-
    // advanced scroll position, then cancel to avoid firing on_pan_end (and
    // starting a new fling).
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 250.0); // 50px downward: past the 18px slop
    scoped.dispatch_pointer_cancel();

    let pixels_after_grab = controller.pixels();

    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let drift = (controller.pixels() - pixels_after_grab).abs();
    assert!(
        drift <= 1.0,
        "grabbing mid-animate_to must halt the run; offset drifted by {drift:.3} px \
         after the grab (from {pixels_after_grab:.1} to {:.1})",
        controller.pixels()
    );
    assert!(
        controller.pixels() < 1000.0,
        "a halted animate_to must never reach its original target (1000.0); got {:.2}",
        controller.pixels()
    );
}

/// `jump_to` called while an `animate_to` is in flight must cancel it
/// SYNCHRONOUSLY — a subsequent frame must not resume driving toward the
/// original target, and must not even transiently show a stale fling-tick
/// value before the cancellation "catches up" (see `ScrollController`'s
/// `stop_hook` field docs for the one-frame race a merely QUEUED
/// cancellation would otherwise leave open, since `flui-testing::pump_frame`
/// ticks registered controllers before draining the rebuild queue that
/// services a queued command).
///
/// Flutter parity: `ScrollPosition.jumpTo` calls `goIdle()` — cancelling
/// whatever activity currently owns the position — before touching `pixels`
/// (`scroll_position_with_single_context.dart`, tag `3.44.0`).
#[test]
fn scrollable_jump_to_during_animate_to_cancels_it_synchronously() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    controller.animate_to(1000.0, Duration::from_millis(300), Arc::new(Curves::Linear));
    // Three pumps of warm-up — see `scrollable_animate_to_reaches_the_target_through_the_curve`'s
    // doc for why this needs one more pump than a direct `animate_with` fling.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        controller.pixels() > 0.0 && controller.pixels() < 1000.0,
        "sanity: the animation must be in flight before jump_to"
    );

    controller.jump_to(42.0);
    assert_eq!(
        controller.pixels(),
        42.0,
        "jump_to must move to its own target immediately"
    );

    // The very next frame tick must NOT resume the old animation — if the
    // cancellation were only queued (not synchronous), this frame's tick step
    // (which runs BEFORE the rebuild that would service the queued Cancel)
    // would advance the still-running fling controller once more, stomping
    // the 42.0 this assertion checks.
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(
        controller.pixels(),
        42.0,
        "the frame immediately after jump_to must not have resumed the \
         canceled animate_to even transiently; got {:.2}",
        controller.pixels()
    );

    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.pixels(),
        42.0,
        "a jump_to mid-animate_to must cancel the run for good — later frames \
         must not resume driving toward the original 1000.0 target; got {:.2}",
        controller.pixels()
    );
}

/// A second `animate_to`, issued before the first has finished, must replace
/// it outright — the position must end up at the SECOND target, never
/// pausing at or passing through the first.
///
/// Flutter parity: `ScrollPositionWithSingleContext.animateTo` calls
/// `beginActivity`, which disposes whatever `DrivenScrollActivity` (or
/// ballistic activity) was previously running before installing the new one
/// (`scroll_position_with_single_context.dart`, tag `3.44.0`).
#[test]
fn scrollable_second_animate_to_supersedes_the_first() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));

    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    controller.animate_to(500.0, Duration::from_millis(100), Arc::new(Curves::Linear));
    // Three pumps of warm-up — see `scrollable_animate_to_reaches_the_target_through_the_curve`'s
    // doc for why the run only starts genuinely ticking on the third pump.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    let pixels_before_supersede = controller.pixels();
    assert!(
        pixels_before_supersede > 0.0 && pixels_before_supersede < 500.0,
        "sanity: the first animate_to must be genuinely in flight before it is \
         superseded; got {pixels_before_supersede:.2}"
    );

    // Replace it before it ever reaches 500.0.
    controller.animate_to(2000.0, Duration::from_millis(100), Arc::new(Curves::Linear));

    for _ in 0..20 {
        scoped.pump_for(Duration::from_millis(16));
    }

    assert_eq!(
        controller.pixels(),
        2000.0,
        "a second animate_to must supersede the first outright, landing on \
         the SECOND target (2000.0), never settling at the first (500.0); \
         got {:.2}",
        controller.pixels()
    );
}

/// A controller SWAP (via `did_update_view` — same root `Scrollable` type
/// before and after, so this reconciles as an update, not a remount) must
/// move the synchronous `jump_to` cancellation hook onto the NEW controller.
/// Without `ScrollableState::did_update_view` re-installing it there, the new
/// controller's `jump_to` would find no hook installed at all (a fresh
/// `ScrollController` starts with none) and fail to stop the SHARED fling
/// controller synchronously.
///
/// This starts the fling via a REAL drag+release on the OLD controller
/// BEFORE the swap (`Scrollable`'s `AnimatedBuilder`-rebuilt gesture
/// callbacks are only known-good against the controller active at gesture
/// time), then swaps, then exercises the stop hook.
///
/// The fling VALUE LISTENER is ALSO re-wired onto the new controller by the
/// same swap (`ScrollableState::install_fling_listener`, called from
/// `did_update_view` right alongside the stop-hook re-install — the fix for
/// the swap-blindness gap `scrollable.rs`'s `scroll_controller` field doc
/// used to name). That's why the OLD controller's own pixels stop moving
/// right after the swap below, and the NEW controller's pixels are what the
/// still-in-flight fling — and later the stop hook — are observed against.
/// See `scrollable_reinstalls_the_fling_listener_after_a_controller_swap`
/// for a test isolating just that half via a post-swap `animate_to`.
#[test]
fn scrollable_reinstalls_the_stop_hook_after_a_controller_swap() {
    let old_controller = ScrollController::new();
    let new_controller = ScrollController::new();
    old_controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(old_controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync.clone(), tight(300.0, 300.0));

    // Start a REAL fling on the OLD controller — the shared fling
    // `AnimationController` this drives is the SAME instance
    // `ScrollableState` keeps across a controller swap (created once in
    // `create_state`), so it stays running straight through the swap below.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: 70 px upward
    scoped.dispatch_pointer_move(150.0, 150.0); // 30 px more: on_pan_update
    scoped.dispatch_pointer_up(150.0, 150.0);
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    let old_pixels_mid_fling = old_controller.pixels();
    assert!(
        old_pixels_mid_fling > 0.0,
        "sanity: the fling must be genuinely advancing the OLD controller \
         before the swap; got {old_pixels_mid_fling:.2}"
    );

    // Swap to a DIFFERENT controller. The scoped harness preserves the
    // outer GestureArenaScope, so this reconciles through did_update_view.
    scoped.pump_widget(VsyncScope::new(
        vsync,
        Scrollable::new()
            .controller(new_controller.clone())
            .child(SizedBox::new(300.0, 5000.0)),
    ));

    // The fling is STILL running on the shared `fling_controller` post-swap,
    // but its value listener now writes into the NEW controller — the OLD
    // one is frozen at whatever it reached right before the swap.
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(
        old_controller.pixels(),
        old_pixels_mid_fling,
        "the OLD controller must stop moving once the fling listener has \
         been re-wired onto the new controller by the swap"
    );
    let new_pixels_mid_fling = new_controller.pixels();
    assert!(
        new_pixels_mid_fling > 0.0,
        "sanity: the still-in-flight fling must now be advancing the NEW \
         controller after the swap; got {new_pixels_mid_fling:.2}"
    );

    // The SYNCHRONOUS cancellation path: `new_controller.jump_to` must find
    // the stop hook `did_update_view` re-installed on it, and stop the
    // SHARED fling controller THIS INSTANT.
    new_controller.jump_to(0.0);
    let new_pixels_after_jump = new_controller.pixels();

    for _ in 0..5 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let drift = (new_controller.pixels() - new_pixels_after_jump).abs();
    assert!(
        drift <= 1.0,
        "jump_to on the controller installed by a did_update_view SWAP must \
         still stop the shared fling controller synchronously — the NEW \
         controller's pixels (now fed by the fling's value listener) must \
         stop drifting once jump_to is called on it; drifted {drift:.3} px \
         after jump_to (from {new_pixels_after_jump:.1} to {:.1})",
        new_controller.pixels()
    );
}

/// Isolates the fling-listener half of the swap fix: a post-swap
/// `animate_to` on the NEW controller must move the NEW controller's own
/// `pixels()`.
///
/// Before the fix, `ScrollableState::init_state` captured the scroll
/// controller once into the fling value listener's closure and never
/// re-captured it; `did_update_view` only ever re-installed the `stop_hook`.
/// A controller swap therefore left the listener writing into the OLD
/// controller forever — `animate_to`/a fling driven on the NEW controller
/// still moved the shared `fling_controller`'s own value (queued and
/// serviced correctly, see `scroll_controller.rs`'s ADR-0037 docs), but
/// nothing ever copied that value into the NEW controller's `ScrollPosition`,
/// so its `pixels()` never moved at all.
///
/// Red-check: comment out `did_update_view`'s `self.install_fling_listener()`
/// call — this test's first assertion fails (`new_controller.pixels()` stays
/// at its pre-`animate_to` value).
#[test]
fn scrollable_reinstalls_the_fling_listener_after_a_controller_swap() {
    let old_controller = ScrollController::new();
    let new_controller = ScrollController::new();
    old_controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(old_controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync.clone(), tight(300.0, 300.0));

    // Swap to a DIFFERENT controller — same shape as
    // `scrollable_reinstalls_the_stop_hook_after_a_controller_swap` above,
    // but with NO pre-swap gesture: the very first fling this test ever
    // drives is via `animate_to` on the NEW controller, after the swap.
    scoped.pump_widget(VsyncScope::new(
        vsync,
        Scrollable::new()
            .controller(new_controller.clone())
            .child(SizedBox::new(300.0, 5000.0)),
    ));

    let new_pixels_before = new_controller.pixels();

    // Drives the SAME shared `fling_controller` `ScrollableState` has kept
    // since `create_state` (queued and serviced regardless of this bug) —
    // it's the value listener's re-wiring that this test actually pins.
    new_controller.animate_to(500.0, Duration::from_millis(100), Arc::new(Curves::Linear));

    // Same 3-pump warm-up as `scrollable_animate_to_reaches_the_target_through_the_curve`.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));

    assert!(
        new_controller.pixels() > new_pixels_before,
        "a post-swap animate_to must move the NEW controller's own position \
         — did_update_view must re-wire the fling value listener onto it, \
         not leave it writing into the old controller forever; got \
         {new_pixels_before:.2} -> {:.2}",
        new_controller.pixels()
    );
    assert_eq!(
        old_controller.pixels(),
        0.0,
        "the OLD controller must receive no ticks at all once the listener \
         has been re-wired onto the new one by the swap"
    );

    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        new_controller.pixels(),
        500.0,
        "once the duration has fully elapsed, the post-swap animate_to must \
         land EXACTLY on the target on the NEW controller; got {:.2}",
        new_controller.pixels()
    );
}

/// A `StatelessView` host that can unmount `Scrollable` entirely (`show:
/// false`) — the same "stable root TYPE, varying build output" pattern
/// `PageViewHost` (`tests/parity/page_view_test.rs`) uses, since `pump_widget`
/// reconciling two DIFFERENT concrete ROOT types does not run the normal
/// unmount/dispose path; toggling the INNER build output under a stable
/// outer type does.
#[derive(Clone, StatelessView)]
struct ScrollableHost {
    controller: ScrollController,
    show: bool,
}

impl StatelessView for ScrollableHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.show {
            return SizedBox::new(1.0, 1.0).into_view().boxed();
        }
        Scrollable::new()
            .controller(self.controller.clone())
            .child(SizedBox::new(300.0, 5000.0))
            .into_view()
            .boxed()
    }
}

/// Disposing a `Scrollable` must clear any not-yet-serviced pending command
/// from its controller — otherwise an `animate_to` queued before dispose
/// would replay against a DIFFERENT (freshly mounted) `ScrollableState`'s
/// fling controller if the same `ScrollController` is later re-attached to a
/// new `Scrollable`, instead of leaving the fresh, untouched state a caller
/// re-attaching a controller would expect.
#[test]
fn disposing_a_scrollable_clears_the_controllers_pending_command_before_a_reattach() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let mut scoped = lay_out(
        VsyncScope::new(
            vsync.clone(),
            ScrollableHost {
                controller: controller.clone(),
                show: true,
            },
        ),
        tight(300.0, 300.0),
    );
    scoped.adopt_vsync(vsync.clone());

    // Queue an animate_to but dispose before ANY pump services it.
    controller.animate_to(1000.0, Duration::from_millis(100), Arc::new(Curves::Linear));

    // Unmount: `show: false` toggles the inner build output. The scoped
    // harness retains its GestureArenaScope around this matching inner root.
    scoped.pump_widget(VsyncScope::new(
        vsync.clone(),
        ScrollableHost {
            controller: controller.clone(),
            show: false,
        },
    ));

    // Re-attach the SAME controller to a brand-new mounted Scrollable.
    scoped.pump_widget(VsyncScope::new(
        vsync,
        ScrollableHost {
            controller: controller.clone(),
            show: true,
        },
    ));

    for _ in 0..10 {
        scoped.pump_for(Duration::from_millis(16));
    }

    assert_eq!(
        controller.pixels(),
        0.0,
        "an animate_to queued before dispose must NOT replay against a \
         freshly re-attached Scrollable; got {:.2}",
        controller.pixels()
    );
}

// ============================================================================
// Scrollbar — thumb drag
// ============================================================================

/// Dragging the scrollbar thumb by N track-pixels must scroll the content
/// by the proportional number of content-pixels. This test FAILS if the
/// `on_pan_update` wired to the thumb's `GestureDetector` does not call
/// `set_pixels` — the controller would remain at 0.
///
/// Mapping (`ScrollController::thumb_offset_fraction`'s doc, matching
/// Flutter's `ScrollbarPainter` thumb-drag contract in
/// `widgets/scrollbar.dart`, 3.44.0): `dP/d(thumb_top) = scroll_extent /
/// available_track`. With scroll_extent=300, available_track=150:
///   50 track-px × (300 / 150) = 100 content-px
#[test]
fn scrollbar_thumb_drag_moves_scroll_offset_proportionally() {
    use flui_widgets::Scrollbar;

    let controller = ScrollController::new();
    // viewport=300, content=600 → scroll_extent=300, thumb occupies half the track.
    controller.update_dimensions(300.0, 0.0, 300.0);

    // Use a wider thumb (20 px) for comfortable hit-testing.
    let widget = Scrollbar::new()
        .controller(controller.clone())
        .thumb_width(20.0)
        .child(SizedBox::new(300.0, 300.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    assert_eq!(controller.pixels(), 0.0, "initial scroll offset must be 0");

    // Thumb geometry at pixels=0:
    //   thumb_fraction = 300 / 600 = 0.5, thumb_height = 150, available_track = 150
    //   thumb_top = 0, thumb x = [280, 300], thumb y = [0, 150]
    //
    // The thumb has no competing recognizer, so its first +50px move is an
    // update and maps to +100 content pixels.
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 60.0);
    scoped.dispatch_pointer_up(290.0, 60.0);

    let final_pixels = controller.pixels();
    assert!(
        (final_pixels - 100.0).abs() < 1.0,
        "dragging the thumb 50 track-px must scroll 100 content-px \
         (scroll_extent=300, available_track=150); got {final_pixels:.2}"
    );
}

/// Chaining small thumb-drag moves accumulates content-delta until `max_scroll_extent`
/// is hit, and `clamp` prevents the position from exceeding the maximum.
///
/// Geometry: viewport=300, scroll_extent=150 (max_scroll_extent=150) →
/// thumb_fraction = 300/450 = 0.6667, thumb_height=200, available_track=100.
/// Each +30 track-px move gives content_delta = (30/100)*150 = 45 px
/// (`dP/d(thumb_top) = scroll_extent / available_track`, this file's
/// `scrollbar_thumb_drag_moves_scroll_offset_proportionally` above).
/// After 4 `on_pan_update` calls: accumulated proposed = 180, clamped to 150.
///
/// All pointer positions stay within the thumb's original Positioned bounds
/// (y in [0, 200]) so every re-hit-test succeeds.
#[test]
fn scrollbar_thumb_drag_clamps_at_max_scroll_extent() {
    use flui_widgets::Scrollbar;

    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 150.0);

    let widget = Scrollbar::new()
        .controller(controller.clone())
        .thumb_width(20.0)
        .child(SizedBox::new(300.0, 300.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Thumb at pixels=0 occupies x=[280,300], y=[0,200]. Four +30px
    // updates accumulate 180 content pixels and clamp to 150.
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 40.0); // pixels=45
    scoped.dispatch_pointer_move(290.0, 70.0); // pixels=90
    scoped.dispatch_pointer_move(290.0, 100.0); // pixels=135
    scoped.dispatch_pointer_move(290.0, 130.0); // proposed=180, clamped=150
    scoped.dispatch_pointer_up(290.0, 130.0);

    assert!(
        controller.pixels() <= 150.0,
        "thumb drag must not carry scroll past max_scroll_extent (150); got {:.2}",
        controller.pixels()
    );
    assert!(
        controller.pixels() > 0.0,
        "thumb drag must have moved the scroll position; got {:.2}",
        controller.pixels()
    );
}

// ============================================================================
// RefreshIndicator — pull-to-refresh
// ============================================================================

/// An over-threshold pull at the top + release must fire `on_refresh` and
/// transition the controller to the refreshing state.
///
/// This test FAILS if `on_pan_end` does not detect the overscroll or does not
/// call the callback.
#[test]
fn refresh_indicator_over_threshold_pull_fires_on_refresh() {
    use flui_widgets::{RefreshController, RefreshIndicator};
    use std::sync::atomic::{AtomicBool, Ordering};

    let refreshed = std::sync::Arc::new(AtomicBool::new(false));
    let refreshed_cb = refreshed.clone();

    let scroll_ctrl = ScrollController::new();
    // viewport=300, content=800 → scroll_extent=500.
    scroll_ctrl.update_dimensions(300.0, 0.0, 500.0);

    let refresh_ctrl = RefreshController::new();

    let widget = RefreshIndicator::new()
        .scroll_controller(scroll_ctrl.clone())
        .controller(refresh_ctrl.clone())
        // Default threshold is 80 px; use 50 px for a smaller test pull.
        .threshold_px(50.0)
        .on_refresh(move || {
            refreshed_cb.store(true, Ordering::SeqCst);
        })
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    assert!(!refresh_ctrl.is_refreshing(), "must start in idle state");

    // Pull down from the top: finger moves DOWN (y increases), delta.dy > 0, so
    // proposed = pixels - delta.dy = 0 - positive < min_scroll_extent -> overscroll.
    //
    // A lone +70px move produces 70px of overscroll, above the 50px
    // threshold, then release triggers refresh.
    scoped.dispatch_pointer_down(150.0, 50.0);
    scoped.dispatch_pointer_move(150.0, 120.0);
    scoped.dispatch_pointer_up(150.0, 120.0);

    assert!(
        refreshed.load(Ordering::SeqCst),
        "on_refresh must fire after an over-threshold pull and release"
    );
    assert!(
        refresh_ctrl.is_refreshing(),
        "controller must be in refreshing state after on_refresh fires"
    );
}

/// A pull that stays below the threshold must NOT fire `on_refresh`.
#[test]
fn refresh_indicator_under_threshold_pull_does_not_fire_on_refresh() {
    use flui_widgets::{RefreshController, RefreshIndicator};
    use std::sync::atomic::{AtomicBool, Ordering};

    let refreshed = std::sync::Arc::new(AtomicBool::new(false));
    let refreshed_cb = refreshed.clone();

    let scroll_ctrl = ScrollController::new();
    scroll_ctrl.update_dimensions(300.0, 0.0, 500.0);

    let refresh_ctrl = RefreshController::new();

    let widget = RefreshIndicator::new()
        .scroll_controller(scroll_ctrl.clone())
        .controller(refresh_ctrl.clone())
        .threshold_px(80.0)
        .on_refresh(move || {
            refreshed_cb.store(true, Ordering::SeqCst);
        })
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Pull only 30px past top — below the 80px threshold.
    scoped.dispatch_pointer_down(150.0, 50.0);
    scoped.dispatch_pointer_move(150.0, 80.0);
    scoped.dispatch_pointer_up(150.0, 80.0);

    assert!(
        !refreshed.load(Ordering::SeqCst),
        "on_refresh must NOT fire for a sub-threshold pull (30 px < 80 px threshold)"
    );
    assert!(
        !refresh_ctrl.is_refreshing(),
        "controller must remain in idle state after a sub-threshold pull"
    );
}

/// After a successful refresh, `RefreshController::finish()` must return the
/// controller to the idle state, hiding the spinner.
#[test]
fn refresh_indicator_finish_dismisses_spinner() {
    use flui_widgets::{RefreshController, RefreshIndicator};
    use std::sync::atomic::{AtomicBool, Ordering};

    let refreshed = std::sync::Arc::new(AtomicBool::new(false));
    let refreshed_cb = refreshed.clone();

    let scroll_ctrl = ScrollController::new();
    scroll_ctrl.update_dimensions(300.0, 0.0, 500.0);

    let refresh_ctrl = RefreshController::new();

    let widget = RefreshIndicator::new()
        .scroll_controller(scroll_ctrl.clone())
        .controller(refresh_ctrl.clone())
        .threshold_px(50.0)
        .on_refresh(move || {
            refreshed_cb.store(true, Ordering::SeqCst);
        })
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Trigger a refresh with an over-threshold pull (finger moves DOWN: y increases).
    scoped.dispatch_pointer_down(150.0, 50.0);
    scoped.dispatch_pointer_move(150.0, 120.0); // +70: overscroll >= threshold
    scoped.dispatch_pointer_up(150.0, 120.0);

    assert!(
        refreshed.load(Ordering::SeqCst),
        "on_refresh must fire before testing finish()"
    );
    assert!(
        refresh_ctrl.is_refreshing(),
        "spinner must be present (is_refreshing=true) while refresh is in progress"
    );

    // Caller signals completion.
    refresh_ctrl.finish();

    assert!(
        !refresh_ctrl.is_refreshing(),
        "spinner must be gone (is_refreshing=false) after finish() is called"
    );
}

// ============================================================================
// Hit-test transform-stack composition through the sliver hit-test walk
// ============================================================================
//
// `PipelineOwner::hit_test_sliver_subtree` computes each child's position and
// recurses into it, but (before the fix this section pins down) never pushed
// the paint offset it consumed onto the `HitTestResult` transform stack --
// unlike the box-side walk, which does for `RenderBox` ancestors
// (`crates/flui-rendering/src/pipeline/owner/accessors.rs`). A `Listener`
// living anywhere below a sliver therefore received the raw, un-localized
// GLOBAL dispatch position instead of one local to its own box -- wrong for
// any sliver whose child paints at a nonzero offset, the common case for a
// scrolled list.
//
// Flutter's own regression test for the same class of bug --
// `'SliverMainAxisGroup pointer event positions'`
// (`packages/flutter/test/widgets/sliver_main_axis_group_test.dart`, tag
// `3.44.0`, filed as flutter/flutter#173029) -- asserts
// `TapDownDetails.localPosition` stays scroll-correct through nested slivers.
// FLUI has no `SliverMainAxisGroup` yet, so these are non-parity regression
// tests reproducing the same class of defect through the sliver widgets FLUI
// does have: `ListView` (the sliver->box leg, under a real scroll offset,
// vertical and horizontal), `SliverPadding` wrapping a `SliverToBoxAdapter`
// (the sliver->sliver leg), and a reversed `AxisDirection` (the sliver->box
// leg's sign, not just its axis).

const ROW_COLOR: Color = Color::rgb(200, 30, 30);

/// The `dx`/`dy` local position a `Listener`'s `on_pointer_down` callback
/// records, readable back by the test.
type RecordedPosition = Rc<Cell<Option<(f32, f32)>>>;

/// A `Listener` that records the local position its `on_pointer_down`
/// callback receives into a fresh, independently readable cell.
fn recording_listener() -> (RecordedPosition, Listener) {
    let recorded = Rc::new(Cell::new(None));
    let probe = Rc::clone(&recorded);
    let listener = Listener::new().on_pointer_down(move |event: &PointerEvent| {
        let position = event.position();
        probe.set(Some((position.dx.get(), position.dy.get())));
    });
    (recorded, listener)
}

/// A hit-testable leaf of the given size: `ColoredBox` wraps a childless
/// `SizedBox` so it actually hit-tests true -- a bare `SizedBox` does not.
/// See `crates/flui-widgets/tests/parity/pointer_local_position_test.rs::target`
/// for the same convention.
fn hit_testable_leaf(width: f32, height: f32) -> ColoredBox {
    ColoredBox::new(ROW_COLOR).child(SizedBox::new(width, height))
}

/// Builds `count` rows, each a `Listener` wrapping a `width x height`
/// hit-testable leaf, alongside a `RecordedPosition` per row (same index).
fn recording_rows(
    count: usize,
    width: f32,
    height: f32,
) -> (Vec<RecordedPosition>, Vec<flui_view::BoxedView>) {
    let mut recorders = Vec::with_capacity(count);
    let rows = (0..count)
        .map(|_| {
            let (recorded, listener) = recording_listener();
            recorders.push(recorded);
            listener.child(hit_testable_leaf(width, height)).boxed()
        })
        .collect();
    (recorders, rows)
}

/// Asserts `actual` is within floating-point tolerance of `expected`.
fn assert_local_position(actual: (f32, f32), expected: (f32, f32), what: &str) {
    const TOLERANCE: f32 = 1e-3;
    assert!(
        (actual.0 - expected.0).abs() < TOLERANCE && (actual.1 - expected.1).abs() < TOLERANCE,
        "{what}: expected ({:.4}, {:.4}), got ({:.4}, {:.4})",
        expected.0,
        expected.1,
        actual.0,
        actual.1,
    );
}

/// A `Listener` inside a scrolled `ListView` row must receive a position
/// local to its OWN box, not the raw global dispatch position.
///
/// 10 rows at 50px in a 120px-tall viewport (380px of real scroll range).
/// Row 3 spans layout y=150..200; after scrolling by 100px it paints at
/// screen y=50..100, so a tap at screen (100, 60) lands 10px into row 3's
/// own box.
#[test]
fn scrolled_list_view_row_listener_receives_a_locally_transformed_position() {
    let (recorders, rows) = recording_rows(10, 200.0, 50.0);

    let controller = ScrollController::new();
    let mut laid = lay_out(
        ListView::new(50.0, rows).position(controller.position()),
        tight(200.0, 120.0),
    );

    laid.dispatch_pointer_down(100.0, 25.0);
    laid.dispatch_pointer_up(100.0, 25.0);
    assert_local_position(
        recorders[0]
            .get()
            .expect("row 0's on_pointer_down must have fired for the unscrolled tap"),
        (100.0, 25.0),
        "unscrolled tap on row 0",
    );
    for (index, recorder) in recorders.iter().enumerate().skip(1) {
        assert!(
            recorder.get().is_none(),
            "only row 0 should have received the unscrolled tap; row {index} also fired"
        );
    }

    controller.set_pixels(100.0);
    laid.pump();

    laid.dispatch_pointer_down(100.0, 60.0);
    laid.dispatch_pointer_up(100.0, 60.0);
    assert_local_position(
        recorders[3]
            .get()
            .expect("row 3's on_pointer_down must have fired after scrolling by 100px"),
        (100.0, 10.0),
        "scrolled tap on row 3 must localize to its own box, not the raw global dispatch position",
    );
}

/// The horizontal-axis sibling of
/// [`scrolled_list_view_row_listener_receives_a_locally_transformed_position`]
/// -- proves the sliver->box leg's offset decomposition picks the correct
/// axis (`LeftToRight`) rather than only ever having been exercised on the
/// vertical default.
#[test]
fn scrolled_horizontal_list_view_column_listener_receives_a_locally_transformed_position() {
    use flui_widgets::prelude::Axis;

    let (recorders, columns) = recording_rows(10, 50.0, 200.0);

    let controller = ScrollController::new();
    let mut laid = lay_out(
        ListView::new(50.0, columns)
            .scroll_direction(Axis::Horizontal)
            .position(controller.position()),
        tight(120.0, 200.0),
    );

    laid.dispatch_pointer_down(25.0, 100.0);
    laid.dispatch_pointer_up(25.0, 100.0);
    assert_local_position(
        recorders[0]
            .get()
            .expect("column 0's on_pointer_down must have fired for the unscrolled tap"),
        (25.0, 100.0),
        "unscrolled tap on column 0",
    );
    for (index, recorder) in recorders.iter().enumerate().skip(1) {
        assert!(
            recorder.get().is_none(),
            "only column 0 should have received the unscrolled tap; column {index} also fired"
        );
    }

    controller.set_pixels(100.0);
    laid.pump();

    laid.dispatch_pointer_down(60.0, 100.0);
    laid.dispatch_pointer_up(60.0, 100.0);
    assert_local_position(
        recorders[3]
            .get()
            .expect("column 3's on_pointer_down must have fired after scrolling by 100px"),
        (10.0, 100.0),
        "scrolled tap on column 3 must localize to its own box",
    );
}

/// The sliver->sliver leg (`SliverPadding` wrapping a `SliverToBoxAdapter`)
/// must localize too. FLUI has no `SliverMainAxisGroup` (the sliver Flutter's
/// own regression test cited above uses), so this reproduces the
/// sliver->sliver class of the defect through the nested-sliver widgets FLUI
/// does have.
///
/// The first tap is unscrolled: `SliverPadding`'s consumed offset is a fixed
/// 20px padding constant, deterministic and independent of scroll --
/// isolating the sliver->sliver leg cleanly. The second tap scrolls PAST the
/// padding (30px > the 20px padding), so the padding's own consumed offset
/// drops to 0 and the `SliverToBoxAdapter`->box (sliver->box) leg picks up a
/// nonzero one instead -- the same nested tree exercises both not-yet-fixed
/// legs.
///
/// Both taps target the same `local_point` inside the leaf's own box; the
/// global dispatch coordinate is derived from [`LaidOut::absolute_offset`],
/// which sums committed PAINT offsets set by `perform_layout`/
/// `position_child` -- entirely independent of the HIT-TEST transform-stack
/// walk this test exists to check, so recovering `local_point` back out is
/// not a tautology against the code under test.
#[test]
fn scrolled_nested_sliver_listener_receives_a_locally_transformed_position() {
    use flui_widgets::{SliverPadding, SliverToBoxAdapter, Viewport};

    let (recorded, listener) = recording_listener();
    let content = listener.child(hit_testable_leaf(160.0, 200.0));

    let controller = ScrollController::new();
    let widget =
        Viewport::new((SliverPadding::all(20.0).child(SliverToBoxAdapter::new().child(content)),))
            .position(controller.position());

    let mut laid = lay_out(widget, tight(200.0, 150.0));

    let padding = laid.only_child(laid.root());
    let adapter = laid.only_child(padding);
    let listener = laid.only_child(adapter);
    let local_point = (10.0_f32, 10.0_f32);

    // Anchors the geometry independently of `absolute_offset`: a 20px
    // uniform `SliverPadding` places its sliver child at (20, 20)
    // (cross-axis padding.left, main-axis padding.top) regardless of
    // scroll, so this can't pass merely because two accumulations of
    // the same (possibly wrong) `node.offset()` agree with each other.
    assert_eq!(
        laid.offset(adapter),
        offset(20.0, 20.0),
        "SliverPadding::all(20.0) must place its child 20px in on both axes"
    );
    let listener_offset = laid.absolute_offset(listener);
    laid.dispatch_pointer_down(
        listener_offset.dx.get() + local_point.0,
        listener_offset.dy.get() + local_point.1,
    );
    laid.dispatch_pointer_up(
        listener_offset.dx.get() + local_point.0,
        listener_offset.dy.get() + local_point.1,
    );
    assert_local_position(
        recorded
            .get()
            .expect("on_pointer_down must have fired through the padding"),
        local_point,
        "sliver->sliver: tap must localize past the SliverPadding offset",
    );

    recorded.set(None);
    controller.set_pixels(30.0); // past the 20px padding, into the content
    laid.pump();

    let listener_offset = laid.absolute_offset(listener);
    laid.dispatch_pointer_down(
        listener_offset.dx.get() + local_point.0,
        listener_offset.dy.get() + local_point.1,
    );
    laid.dispatch_pointer_up(
        listener_offset.dx.get() + local_point.0,
        listener_offset.dy.get() + local_point.1,
    );
    assert_local_position(
        recorded
            .get()
            .expect("on_pointer_down must have fired after scrolling past the padding"),
        local_point,
        "sliver->box (within a nested-sliver chain): tap must localize past the \
         scroll-shifted content offset",
    );
}

/// A reversed [`AxisDirection`] (`BottomToTop`) must not flip the localized
/// sign the wrong way -- `box_hit_offset_from_sliver_position`'s
/// `right_way_up` branch treats a reversed direction specially, and the
/// hit-test transform-stack fix must compose with it correctly rather than
/// only ever having been exercised in the (default) `TopToBottom` direction
/// the other tests in this section use.
///
/// As with the nested-sliver case, the expected local position is derived
/// from [`LaidOut::absolute_offset`] (paint, not hit-test), so no
/// hand-derived reversed-axis arithmetic is load-bearing here.
#[test]
fn scrolled_reversed_list_row_listener_receives_a_locally_transformed_position() {
    use flui_types::layout::AxisDirection;
    use flui_widgets::Viewport;

    let (recorders, rows) = recording_rows(10, 200.0, 50.0);

    let controller = ScrollController::new();
    let widget = Viewport::new((SliverFixedExtentList::new(50.0, rows),))
        .axis_direction(AxisDirection::BottomToTop)
        .position(controller.position());
    let mut laid = lay_out(widget, tight(200.0, 120.0));

    let list = laid.only_child(laid.root());
    let row0 = laid.child(list, 0);
    let local_point = (20.0_f32, 15.0_f32);

    // Anchors the geometry independently of `absolute_offset`: with a
    // 120px viewport and 50px rows, `BottomToTop` places row 0's top-left
    // at physical dy = 120 - 50 = 70 unscrolled -- computed from first
    // principles, not from summing the same `node.offset()` values the
    // fix under test pushes.
    let row0_offset = laid.absolute_offset(row0);
    assert_eq!(
        row0_offset,
        offset(0.0, 70.0),
        "reversed-axis (BottomToTop) row 0 must sit at physical dy = 70 unscrolled"
    );
    laid.dispatch_pointer_down(
        row0_offset.dx.get() + local_point.0,
        row0_offset.dy.get() + local_point.1,
    );
    laid.dispatch_pointer_up(
        row0_offset.dx.get() + local_point.0,
        row0_offset.dy.get() + local_point.1,
    );
    assert_local_position(
        recorders[0]
            .get()
            .expect("row 0's on_pointer_down must have fired for the reversed-axis unscrolled tap"),
        local_point,
        "reversed-axis (BottomToTop), unscrolled",
    );
    recorders[0].set(None);

    controller.set_pixels(20.0);
    laid.pump();

    // Scrolling 20px moves the viewport's visible window, shifting row 0's
    // physical dy to 120 - 50 - (0 - 20) = 90.
    let row0_offset = laid.absolute_offset(row0);
    assert_eq!(
        row0_offset,
        offset(0.0, 90.0),
        "reversed-axis (BottomToTop) row 0 must sit at physical dy = 90 after scrolling by 20px"
    );
    laid.dispatch_pointer_down(
        row0_offset.dx.get() + local_point.0,
        row0_offset.dy.get() + local_point.1,
    );
    laid.dispatch_pointer_up(
        row0_offset.dx.get() + local_point.0,
        row0_offset.dy.get() + local_point.1,
    );
    assert_local_position(
        recorders[0]
            .get()
            .expect("row 0's on_pointer_down must have fired for the reversed-axis scrolled tap"),
        local_point,
        "reversed-axis (BottomToTop), scrolled by 20px",
    );
}

/// The sliver->sliver override arm in `accessors.rs`'s `hit_child` closure
/// (the `Some(child_position)` branch, guarded by a `debug_assert!` that the
/// child's committed offset is zero) has no driver-level coverage anywhere
/// else in the workspace. The sole supplier is
/// `RenderSliverOffstage::hit_test`
/// (`crates/flui-objects/src/sliver/sliver_offstage.rs`), which forwards its
/// own main-axis position unchanged to a sliver child; its existing harness
/// coverage (`flui-objects/tests/render_object_harness.rs`) is
/// layout/geometry only, never a driven hit test.
///
/// Drives a hit through a non-offstage (`visible`) `SliverOffstage` wrapping
/// a `SliverToBoxAdapter` and confirms the delivered local position, so the
/// override branch is exercised end to end rather than only by the
/// `debug_assert!` it carries.
#[test]
fn hit_through_a_visible_sliver_offstage_reaches_its_child_at_the_correct_position() {
    use flui_widgets::{SliverOffstage, SliverToBoxAdapter, Viewport};

    let (recorded, listener) = recording_listener();
    let content = listener.child(hit_testable_leaf(160.0, 200.0));

    let controller = ScrollController::new();
    let widget =
        Viewport::new((SliverOffstage::visible().child(SliverToBoxAdapter::new().child(content)),))
            .position(controller.position());

    let laid = lay_out(widget, tight(200.0, 150.0));

    let offstage = laid.only_child(laid.root());
    let adapter = laid.only_child(offstage);
    let target = laid.only_child(adapter);
    let local_point = (10.0_f32, 10.0_f32);

    // A visible `SliverOffstage` is a transparent passthrough -- it never
    // calls `position_child`, so this is the ACTUAL invariant the
    // `debug_assert!` on the override arm depends on, not merely assumed.
    assert_eq!(
        laid.offset(adapter),
        offset(0.0, 0.0),
        "a visible SliverOffstage must not reposition its sliver child"
    );

    let target_offset = laid.absolute_offset(target);
    laid.dispatch_pointer_down(
        target_offset.dx.get() + local_point.0,
        target_offset.dy.get() + local_point.1,
    );
    laid.dispatch_pointer_up(
        target_offset.dx.get() + local_point.0,
        target_offset.dy.get() + local_point.1,
    );
    assert_local_position(
        recorded
            .get()
            .expect("on_pointer_down must have fired through the visible SliverOffstage"),
        local_point,
        "sliver->sliver override arm (RenderSliverOffstage): tap must localize to the child's own box",
    );
}

// ============================================================================
// Scrollable — scroll-activity signal (Flutter: isScrollingNotifier)
// ============================================================================

/// The scroll-activity signal across a full gesture: idle before, live
/// from the grab with the user's direction recorded, live through the
/// ballistic run past the release, and idle again — direction reset — once
/// the run settles. The signal a floating header's snap trigger keys on.
#[test]
fn scroll_activity_tracks_the_whole_gesture_lifecycle() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let position = controller.position();

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    assert!(!position.is_scrolling(), "idle before any gesture");
    assert_eq!(position.user_scroll_direction(), ScrollDirection::Idle);

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0); // slop-crossing: on_pan_start
    assert!(
        position.is_scrolling(),
        "the grab begins the scroll activity"
    );
    scoped.dispatch_pointer_move(150.0, 150.0); // on_pan_update
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Reverse,
        "an upward finger drag increases the offset — Reverse"
    );
    scoped.dispatch_pointer_up(150.0, 150.0);

    // Whether the release produced a ballistic run (fast release) or not,
    // the signal must return to idle once everything settles; the bounded
    // pump keeps a stuck-true regression a loud failure, not a hang.
    let mut frames = 0;
    while position.is_scrolling() && frames < 2_000 {
        scoped.pump_for(Duration::from_millis(16));
        frames += 1;
    }
    assert!(
        !position.is_scrolling(),
        "the activity must end after the release settles (still scrolling \
         after {frames} frames)"
    );
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Idle,
        "ending the scroll resets the direction"
    );
}

/// A driven `animate_to` IS a scroll activity — Flutter parity:
/// `animateTo` begins a `DrivenScrollActivity`, so `isScrollingNotifier`
/// holds true for the run's whole duration (`scroll_position.dart`,
/// `beginActivity`) — while `userScrollDirection` stays `Idle` throughout,
/// because a driven run is not a USER scroll. Both halves matter to a
/// floating header: the activity keeps its snap trigger honest, and the
/// idle direction is what keeps the header from float-revealing during a
/// programmatic backward animation.
#[test]
fn a_driven_animate_to_is_a_scroll_activity_but_not_a_user_scroll() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let position = controller.position();

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    assert!(!position.is_scrolling(), "idle before the call");
    controller.animate_to(1000.0, Duration::from_millis(100), Arc::new(Curves::Linear));

    // Pump 1 services the queued command and starts the run — the activity
    // must be live from that point.
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        position.is_scrolling(),
        "a driven animation is a scroll activity from the frame that starts it"
    );
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Idle,
        "a driven run is not a USER scroll — the direction stays idle"
    );

    // Mid-run the signal holds.
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        position.is_scrolling(),
        "the activity holds through the run"
    );

    // Bounded settle: completion must end the activity through the same
    // status-listener half a ballistic fling uses.
    let mut frames = 0;
    while position.is_scrolling() && frames < 2_000 {
        scoped.pump_for(Duration::from_millis(16));
        frames += 1;
    }
    assert!(
        !position.is_scrolling(),
        "the run's completion must end the activity (still scrolling after {frames} frames)"
    );
    assert_eq!(
        controller.pixels(),
        1000.0,
        "and the run itself still lands on the target"
    );
}

/// A wheel tick scrolls IMMEDIATELY — no drag slop, no arena, no
/// hold-and-release — mirroring the oracle's `Listener.onPointerSignal` →
/// `position.pointerScroll(delta)` wire: the pixel write clamps hard to
/// the extents, and the scroll activity pulses around it with the USER
/// direction (what a floating header's reveal keys on), ending after the
/// frame that consumes it.
#[test]
fn a_wheel_tick_scrolls_without_a_drag() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let position = controller.position();

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Wheel-down: a POSITIVE normalized delta (the oracle's
    // `scrollDelta` convention); content scrolls down, the offset
    // increases.
    scoped.dispatch_scroll(150.0, 150.0, 0.0, 53.0);
    assert_eq!(
        controller.pixels(),
        53.0,
        "one wheel-down tick scrolls by its pixel delta, immediately"
    );
    assert!(
        position.is_scrolling(),
        "the wheel pulse raises the scroll activity for the consuming frame"
    );
    assert_eq!(
        position.user_scroll_direction(),
        ScrollDirection::Reverse,
        "a wheel-down tick is a USER scroll toward the end"
    );

    // The pulse ends once the frame that consumed the write completes.
    scoped.pump_for(Duration::from_millis(16));
    assert!(
        !position.is_scrolling(),
        "the wheel pulse must end after the consuming frame"
    );
    assert_eq!(position.user_scroll_direction(), ScrollDirection::Idle);

    // Wheel-up past the start clamps hard: no overscroll from a wheel.
    scoped.dispatch_scroll(150.0, 150.0, 0.0, -200.0);
    assert_eq!(
        controller.pixels(),
        0.0,
        "a wheel tick clamps to the extents — it never overscrolls"
    );
    // Let that tick's own pulse end before probing the no-op case.
    scoped.pump_for(Duration::from_millis(16));

    // At the boundary, a further wheel-up is a no-op and must not pulse.
    scoped.dispatch_scroll(150.0, 150.0, 0.0, -10.0);
    assert!(
        !position.is_scrolling(),
        "a wheel tick that cannot move the position must not raise activity"
    );
}

/// A wheel tick during a driven animation WINS: the tick stops the run
/// before writing (the oracle's `pointerScroll` starts from `goIdle()`)
/// — otherwise the animation's value listener overwrites the wheel
/// write on its very next tick and the wheel appears dead mid-fling.
#[test]
fn a_wheel_tick_interrupts_a_driven_animation() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    // Start well away from the extents so the wheel write below cannot
    // clamp: this test pins the interrupt, not the boundary.
    controller.jump_to(1_000.0);
    scoped.pump_for(Duration::from_millis(16));
    controller.animate_to(4_000.0, Duration::from_secs(10), Arc::new(Curves::Linear));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    scoped.pump_for(Duration::from_millis(16));
    let mid_animation = controller.pixels();
    assert!(
        mid_animation > 1_000.0 && mid_animation < 4_000.0,
        "premise: the run is in flight past the start (got {mid_animation})"
    );

    // Wheel-up against the animation's direction.
    scoped.dispatch_scroll(150.0, 150.0, 0.0, -53.0);
    let after_wheel = controller.pixels();
    assert_eq!(
        after_wheel,
        mid_animation - 53.0,
        "the wheel write lands relative to where the run was stopped"
    );

    // Frames later the position must NOT have resumed the animation.
    for _ in 0..5 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.pixels(),
        after_wheel,
        "the interrupted animation must not keep driving the position"
    );
}

/// A fling keeps the activity alive through the ballistic run and ends it
/// when the simulation settles — the status-listener half of the signal.
/// Without it, `is_scrolling` would stick true forever after any fling.
#[test]
fn a_fling_keeps_scrolling_until_the_ballistic_run_settles() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let position = controller.position();

    let vsync = Vsync::new();
    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 5000.0));
    let mut scoped = fling_scoped(widget, vsync, tight(300.0, 300.0));

    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 180.0);
    scoped.dispatch_pointer_move(150.0, 150.0);
    scoped.dispatch_pointer_up(150.0, 150.0);

    assert!(
        position.is_scrolling(),
        "the ballistic run keeps the activity alive past the release"
    );

    // Drive frames until the friction simulation settles (bounded so a
    // never-settling regression fails loudly instead of hanging).
    let mut frames = 0;
    while position.is_scrolling() && frames < 2_000 {
        scoped.pump_for(Duration::from_millis(16));
        frames += 1;
    }
    assert!(
        !position.is_scrolling(),
        "the fling's completion must end the activity (still scrolling after {frames} frames)"
    );
}

// ============================================================================
// Scrollable — arbitrated pointer-signal (wheel) routing
// ============================================================================

/// Two vertically nested scrollables under one wheel tick: the outer's
/// 300×300 viewport holds a 300×200 inner scrollable at the top of its
/// content, and the tick lands over the inner.
///
/// Layout used by the three arbitration tests below:
/// outer content = Column[SizedBox(300×200){inner Scrollable}, 300×4800
/// filler], inner content = 300×1000, so the inner can travel 0..=800 and
/// the outer 0..=4700.
fn nested_scrollables(outer: &ScrollController, inner: &ScrollController, vsync: Vsync) -> LaidOut {
    outer.update_dimensions(300.0, 0.0, 4700.0);
    inner.update_dimensions(200.0, 0.0, 800.0);

    let inner_scrollable = Scrollable::new()
        .controller(inner.clone())
        .child(SizedBox::new(300.0, 1000.0));
    let outer_scrollable =
        Scrollable::new()
            .controller(outer.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(300.0, 200.0)
                    .child(inner_scrollable)
                    .into_view()
                    .boxed(),
                SizedBox::new(300.0, 4800.0).into_view().boxed(),
            ]));

    let wrapped = VsyncScope::new(vsync.clone(), outer_scrollable);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);
    scoped
}

/// One wheel tick over nested scrollables moves ONLY the innermost one that
/// can move — the oracle's `PointerSignalResolver` contract: every scrollable
/// on the hit path registers interest, the first (leaf-most) registrant wins,
/// and the rest never act (`gestures/pointer_signal_resolver.dart`,
/// `widgets/scrollable.dart` `_receivedPointerSignal`). Without arbitration
/// the same tick advances BOTH controllers (issue #717's double-scroll).
#[test]
fn a_wheel_tick_over_nested_scrollables_moves_only_the_inner() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let scoped = nested_scrollables(&outer, &inner, Vsync::new());

    // Over the inner scrollable (its region is 0..200 in root coordinates).
    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);

    assert_eq!(
        inner.pixels(),
        53.0,
        "the inner scrollable claims the tick and scrolls"
    );
    assert_eq!(
        outer.pixels(),
        0.0,
        "the outer scrollable must NOT also scroll — the inner claimed the tick"
    );
}

/// When the inner scrollable sits at the extent the tick pushes toward, it
/// declines the claim and the OUTER scrollable takes the tick — the oracle
/// registers interest "only ... if it would actually result in a scroll"
/// (`widgets/scrollable.dart:962`), which is what makes a wheel keep working
/// once an inner list bottoms out.
#[test]
fn a_wheel_tick_hands_off_to_the_outer_when_the_inner_is_at_its_extent() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let mut scoped = nested_scrollables(&outer, &inner, Vsync::new());

    inner.jump_to(800.0);
    scoped.pump_for(Duration::from_millis(16));
    assert_eq!(inner.pixels(), 800.0, "precondition: inner at max extent");

    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);

    assert_eq!(
        inner.pixels(),
        800.0,
        "an inner scrollable at its extent cannot consume a further tick"
    );
    assert_eq!(
        outer.pixels(),
        53.0,
        "the outer scrollable takes the tick the inner declined"
    );
}

/// A claimed tick is still OBSERVED by every `on_pointer_signal` listener on
/// the path — Flutter dispatches the signal to the whole hit path first and
/// resolves the single actor afterwards (`GestureBinding.dispatchEvent` then
/// `pointerSignalResolver.resolve`), so observation and arbitration are two
/// channels, not one.
#[test]
fn a_claimed_wheel_tick_is_still_observed_by_the_whole_path() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 4700.0);
    let observed = Rc::new(Cell::new(0u32));

    let observed_in_listener = Rc::clone(&observed);
    let widget = Listener::new()
        .on_pointer_signal(move |_event| {
            observed_in_listener.set(observed_in_listener.get() + 1);
        })
        .child(
            Scrollable::new()
                .controller(controller.clone())
                .child(SizedBox::new(300.0, 5000.0)),
        );

    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    scoped.dispatch_scroll(150.0, 150.0, 0.0, 53.0);

    assert_eq!(
        controller.pixels(),
        53.0,
        "the scrollable still consumes the tick"
    );
    assert_eq!(
        observed.get(),
        1,
        "an enclosing plain listener still observes the signal the scrollable claimed"
    );
}

/// An `InteractiveViewer` nested in a scrollable claims the wheel tick it
/// zooms with, so one tick does not BOTH zoom and scroll — a documented
/// divergence from Flutter, whose `InteractiveViewer` acts on
/// `onPointerSignal` without registering in the `PointerSignalResolver` and
/// therefore double-acts (the resolver's own class documentation names this
/// exact scrollable-plus-custom-widget conflict as its reason to exist).
#[test]
fn a_wheel_tick_over_an_interactive_viewer_zooms_without_scrolling_the_outer() {
    use flui_types::Matrix4;
    use flui_widgets::{InteractiveViewer, TransformationController};

    let outer = ScrollController::new();
    outer.update_dimensions(300.0, 0.0, 4700.0);
    let transformation = TransformationController::new();

    // An infinite boundary margin so a zoom-out from identity is a REAL
    // transform change; with the default zero margin the boundary clamp
    // collapses it to a no-op and the viewer (correctly) declines the claim
    // — the second half of this test pins that fall-through.
    let viewer = InteractiveViewer::new()
        .controller(transformation.clone())
        .boundary_margin(flui_types::EdgeInsets::all(px(f32::INFINITY)))
        .child(SizedBox::new(300.0, 200.0));
    let widget = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(300.0, 200.0)
                .child(viewer)
                .into_view()
                .boxed(),
            SizedBox::new(300.0, 4800.0).into_view().boxed(),
        ]));

    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    // Wheel-DOWN (positive dy) over the viewer zooms out (clamped at the
    // default min scale, still a transform change). Chosen over wheel-up
    // deliberately: the outer scrollable sits at 0 and a wheel-up tick could
    // not have moved it anyway, so only the DOWN direction makes the
    // no-outer-scroll assertion below load-bearing (red-verified against a
    // sabotaged claim walk that never stops).
    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);

    assert_ne!(
        transformation.value().m,
        Matrix4::identity().m,
        "the viewer consumes the tick as a zoom"
    );
    assert_eq!(
        outer.pixels(),
        0.0,
        "the outer scrollable must not also scroll the tick the viewer claimed"
    );
}

/// The complement: when the viewer's zoom clamps to a NO-OP (zoom-out at
/// identity under the default zero boundary margin), the viewer declines the
/// claim and the tick falls through to the enclosing scrollable — the wheel
/// never goes dead over a viewer that cannot zoom any further. This is the
/// same net behavior Flutter reaches by the opposite route (its viewer acts
/// unarbitrated and no-ops, while the scrollable wins the resolver).
#[test]
fn a_no_op_zoom_falls_through_to_the_outer_scrollable() {
    use flui_types::Matrix4;
    use flui_widgets::{InteractiveViewer, TransformationController};

    let outer = ScrollController::new();
    outer.update_dimensions(300.0, 0.0, 4700.0);
    let transformation = TransformationController::new();

    // Default (zero) boundary margin: zoom-out below identity is clamped.
    let viewer = InteractiveViewer::new()
        .controller(transformation.clone())
        .child(SizedBox::new(300.0, 200.0));
    let widget = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(300.0, 200.0)
                .child(viewer)
                .into_view()
                .boxed(),
            SizedBox::new(300.0, 4800.0).into_view().boxed(),
        ]));

    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);

    assert_eq!(
        transformation.value().m,
        Matrix4::identity().m,
        "the clamped zoom-out is a no-op on the transform"
    );
    assert_eq!(
        outer.pixels(),
        53.0,
        "the tick the viewer could not use must scroll the outer scrollable"
    );
}

/// A wheel tick that arrives MID-DRAG is observed by the widgets under the
/// CURSOR, not by the route captured at Down — the oracle hit-tests every
/// `PointerSignalEvent` fresh at the signal position and asserts a signal's
/// pointer has no stored result (`gestures/binding.dart`
/// `_handlePointerEventImmediately`), so the observation and claim channels
/// always walk the same fresh path.
#[test]
fn a_wheel_tick_mid_drag_is_observed_under_the_cursor_not_the_captured_route() {
    let controller = ScrollController::new();
    controller.update_dimensions(150.0, 0.0, 4850.0);
    let observed_over_listener = Rc::new(Cell::new(0u32));

    // Top half (0..150): a plain observing listener over a hittable surface.
    // Bottom half (150..300): a scrollable to capture a drag.
    let observed_in_listener = Rc::clone(&observed_over_listener);
    let widget = flui_widgets::Column::new(vec![
        Listener::new()
            .on_pointer_signal(move |_event| {
                observed_in_listener.set(observed_in_listener.get() + 1);
            })
            .child(SizedBox::new(300.0, 150.0).child(ColoredBox::new(Color::rgb(0x40, 0x40, 0x40))))
            .into_view()
            .boxed(),
        SizedBox::new(300.0, 150.0)
            .child(
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(300.0, 5000.0)),
            )
            .into_view()
            .boxed(),
    ]);

    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    // Start a drag on the scrollable and HOLD it (no release): the pointer
    // now has a captured Down route through the bottom half.
    scoped.dispatch_pointer_down(150.0, 250.0);
    scoped.dispatch_pointer_move(150.0, 220.0);
    assert!(
        controller.pixels() > 0.0,
        "precondition: the drag is live and captured by the scrollable"
    );

    // A wheel tick over the TOP half, while the drag still holds.
    scoped.dispatch_scroll(150.0, 75.0, 0.0, 53.0);

    assert_eq!(
        observed_over_listener.get(),
        1,
        "the listener under the cursor observes the signal — not the captured drag route"
    );
}

/// The desktop wheel contract, composable at last: a ctrl-gated
/// `InteractiveViewer` inside a `Scrollable` — a PLAIN wheel tick scrolls
/// the list (the viewer declines it under `WheelScaleGate::CtrlWheel`),
/// and a CTRL+wheel tick zooms the viewer (the scrollable declines every
/// ctrl tick, zoom chord or not). Before this split, whichever widget was
/// inner claimed everything and the two gestures could not coexist.
#[test]
fn plain_wheel_scrolls_and_ctrl_wheel_zooms_under_the_ctrl_gate() {
    use flui_interaction::events::Modifiers;
    use flui_types::Matrix4;
    use flui_widgets::{InteractiveViewer, TransformationController, WheelScaleGate};

    let outer = ScrollController::new();
    outer.update_dimensions(300.0, 0.0, 4700.0);
    let transformation = TransformationController::new();

    let viewer = InteractiveViewer::new()
        .controller(transformation.clone())
        .wheel_scale_gate(WheelScaleGate::CtrlWheel)
        .boundary_margin(flui_types::EdgeInsets::all(px(f32::INFINITY)))
        .child(SizedBox::new(300.0, 200.0));
    let widget = Scrollable::new()
        .controller(outer.clone())
        .child(flui_widgets::Column::new(vec![
            SizedBox::new(300.0, 200.0)
                .child(viewer)
                .into_view()
                .boxed(),
            SizedBox::new(300.0, 4800.0).into_view().boxed(),
        ]));

    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    // A PLAIN wheel-down over the viewer: the viewer declines, the list
    // scrolls.
    scoped.dispatch_scroll(150.0, 100.0, 0.0, 53.0);
    assert_eq!(
        outer.pixels(),
        53.0,
        "a plain tick over the ctrl-gated viewer scrolls the list"
    );
    assert_eq!(
        transformation.value().m,
        Matrix4::identity().m,
        "…and does not zoom"
    );

    scoped.pump_for(Duration::from_millis(16));

    // CTRL+wheel over the viewer: the scrollable declines, the viewer zooms.
    scoped.dispatch_scroll_with_modifiers(150.0, 50.0, 0.0, 53.0, Modifiers::CONTROL);
    assert_eq!(
        outer.pixels(),
        53.0,
        "a ctrl tick never scrolls the list, even with scroll room"
    );
    assert_ne!(
        transformation.value().m,
        Matrix4::identity().m,
        "the ctrl tick zooms the viewer"
    );

    scoped.pump_for(Duration::from_millis(16));

    // CTRL+wheel over the plain LIST region (no viewer under the cursor):
    // the scrollable itself must decline the chord — this leg is what pins
    // the scrollable's own gate, since over the viewer the inner claim
    // wins before the scrollable is ever asked.
    let pixels_before = outer.pixels();
    scoped.dispatch_scroll_with_modifiers(150.0, 250.0, 0.0, 53.0, Modifiers::CONTROL);
    assert_eq!(
        outer.pixels(),
        pixels_before,
        "a ctrl tick over the bare list is declined by the scrollable"
    );
}
