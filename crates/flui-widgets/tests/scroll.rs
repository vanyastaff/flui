//! Scroll-path parity tests:
#![allow(clippy::float_cmp)] // physics clamp + controller pixel reads return exact f32 literals
//!
//! 1. `SingleChildScrollView` viewport geometry (cross-protocol Box→Sliver path).
//! 2. `ScrollController` thumb geometry helpers.
//! 3. `Scrollable` interactive drag integration (gesture → offset change).
//! 4. `ClampingScrollPhysics` hard-boundary enforcement.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{LaidOutScoped, lay_out, lay_out_with_arena, size, tight};
use flui_animation::Vsync;
use flui_rendering::constraints::BoxConstraints;
use flui_types::Color;
use flui_types::geometry::px;
use flui_view::ViewExt;
use flui_widgets::{
    BouncingScrollPhysics, ClampingScrollPhysics, ColoredBox, CustomScrollView, GridView, ListView,
    ScrollController, ScrollPhysics, Scrollable, SharedScrollPhysics, SingleChildScrollView,
    SizedBox, SliverFixedExtentList, VsyncScope,
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

#[test]
fn clamping_physics_clamps_proposed_offset_below_minimum() {
    let physics = ClampingScrollPhysics::default();
    // Proposing -50 (past the 0 minimum) must snap to 0.
    let result = physics.apply_boundary_conditions(-50.0, 0.0, 500.0);
    assert_eq!(
        result, 0.0,
        "clamping physics must clamp below-min proposals to min_scroll_extent"
    );
}

#[test]
fn clamping_physics_clamps_proposed_offset_above_maximum() {
    let physics = ClampingScrollPhysics::default();
    // Proposing 600 past the 500 maximum must snap to 500.
    let result = physics.apply_boundary_conditions(600.0, 0.0, 500.0);
    assert_eq!(
        result, 500.0,
        "clamping physics must clamp above-max proposals to max_scroll_extent"
    );
}

#[test]
fn clamping_physics_passes_through_in_range_offset() {
    let physics = ClampingScrollPhysics::default();
    let result = physics.apply_boundary_conditions(250.0, 0.0, 500.0);
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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Starting position: top of content.
    assert_eq!(controller.pixels(), 0.0, "initial scroll offset must be 0");

    // Simulate an upward pan: finger starts at y=200, first move to y=150 (50 px
    // upward, well past the 18 px drag slop — crosses slop, transitions to
    // Started, fires on_pan_start). Second move to y=140 fires on_pan_update
    // (on_update fires only after the slop-crossing move, in Started state).
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 150.0); // slop-crossing: 50 px > 18 px
    scoped.dispatch_pointer_move(150.0, 140.0); // in-progress update: delta dy = -10
    scoped.dispatch_pointer_up(150.0, 140.0);

    assert!(
        controller.pixels() > 0.0,
        "scroll offset must increase after dragging up 50 px; \
         got {:.1} — check that on_pan_update is wired to set_pixels",
        controller.pixels()
    );
}

/// A drag that does NOT cross the drag slop (< 18 px) must not move the
/// scroll position — only a genuine drag past the threshold triggers scrolling.
#[test]
fn scrollable_sub_slop_drag_does_not_move_scroll_offset() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);

    let widget = Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 800.0));

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Move only 5 px — below the 18 px drag slop; no drag recognized.
    scoped.dispatch_pointer_down(150.0, 150.0);
    scoped.dispatch_pointer_move(150.0, 145.0);
    scoped.dispatch_pointer_up(150.0, 145.0);

    assert_eq!(
        controller.pixels(),
        0.0,
        "a sub-slop movement must not change the scroll offset"
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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Drag upward: first move crosses slop (transitions Possible→Started, fires
    // on_pan_start). Second move fires on_pan_update — proposes 510 (past max) →
    // clamped physics holds at 500. Without the second move on_update never fires.
    scoped.dispatch_pointer_down(150.0, 200.0);
    scoped.dispatch_pointer_move(150.0, 140.0); // 60 px upward: slop-crossing
    scoped.dispatch_pointer_move(150.0, 130.0); // additional 10 px: fires on_update
    scoped.dispatch_pointer_up(150.0, 130.0);

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

    let mut scoped = lay_out_with_arena(widget, tight(300.0, 300.0));
    let box_before = scoped.laid().find_by_render_type("RenderConstrainedBox");
    let offset_before = scoped.laid().absolute_offset(box_before);

    // External mutation of the shared `ScrollPosition` — no gesture, no
    // `update_render_object` pixels push.
    controller.set_pixels(120.0);

    // `AnimatedBuilder`'s subscription to the same listenable schedules a
    // rebuild when `set_pixels` notifies; this drains it and re-runs layout.
    scoped.pump(Duration::ZERO);

    let box_after = scoped.laid().find_by_render_type("RenderConstrainedBox");
    let offset_after = scoped.laid().absolute_offset(box_after);

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

    let mut scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

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
    scoped.pump(Duration::ZERO);

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
        .viewport_builder(Arc::new(|position: flui_widgets::ScrollPosition| {
            let rows: Vec<_> = (0..12)
                .map(|_| SizedBox::new(300.0, 50.0).boxed())
                .collect();
            Viewport::new((SliverFixedExtentList::new(50.0, rows),))
                .position(position)
                .boxed()
        }));

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

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
/// [`LaidOutScoped::pump_for`] ticks the fling animation deterministically.
fn fling_scoped(widget: Scrollable, vsync: Vsync, constraints: BoxConstraints) -> LaidOutScoped {
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out_with_arena(wrapped, constraints);
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
    scoped.dispatch_pointer_cancel(150.0, 250.0);

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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    assert_eq!(controller.pixels(), 0.0, "initial scroll offset must be 0");

    // Thumb geometry at pixels=0:
    //   thumb_fraction = 300 / 600 = 0.5, thumb_height = 150, available_track = 150
    //   thumb_top = 0, thumb x = [280, 300], thumb y = [0, 150]
    //
    // Sequence:
    //   Down at (290, 10)       — inside thumb
    //   Move to (290, 60)  +50  — slop-crossing (>18 px): fires on_pan_start (no-op)
    //   Move to (290, 110) +50  — fires on_pan_update(delta_y=50) → content_delta=100
    //   Up   at (290, 110)      — within new thumb y=[50,200] after scroll, within widget
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 60.0);
    scoped.dispatch_pointer_move(290.0, 110.0);
    scoped.dispatch_pointer_up(290.0, 110.0);

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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Thumb at pixels=0 occupies x=[280,300], y=[0,200].
    // Slop-crossing: DOWN -> MOVE(+30) crosses 18px threshold.
    // Four on_pan_update calls of +30 track-px each accumulate 180 content-px -> clamped to 150.
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 40.0); // +30 px: slop-crossing -> on_pan_start
    scoped.dispatch_pointer_move(290.0, 70.0); // +30 px -> on_pan_update: pixels=45
    scoped.dispatch_pointer_move(290.0, 100.0); // +30 px -> on_pan_update: pixels=90
    scoped.dispatch_pointer_move(290.0, 130.0); // +30 px -> on_pan_update: pixels=135
    scoped.dispatch_pointer_move(290.0, 160.0); // +30 px -> on_pan_update: proposed=180, clamped=150
    scoped.dispatch_pointer_up(290.0, 160.0);

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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    assert!(!refresh_ctrl.is_refreshing(), "must start in idle state");

    // Pull down from the top: finger moves DOWN (y increases), delta.dy > 0, so
    // proposed = pixels - delta.dy = 0 - positive < min_scroll_extent -> overscroll.
    //
    // Sequence:
    //   Down at (150,  50)        -- inside widget, upper area
    //   Move to (150, 100) +50    -- slop-crossing: on_pan_start fires (stops fling)
    //   Move to (150, 170) +70    -- on_pan_update: proposed = 0 - 70 = -70 < 0
    //                              -> overscroll = 70 > threshold (50) tracked
    //   Up   at (150, 170)        -- on_pan_end: pull (70) >= threshold (50) -> fires
    scoped.dispatch_pointer_down(150.0, 50.0);
    scoped.dispatch_pointer_move(150.0, 100.0); // slop-crossing: dy=+50, on_pan_start fires
    scoped.dispatch_pointer_move(150.0, 170.0); // dy=+70: proposed=0-70=-70<0 -> overscroll=70
    scoped.dispatch_pointer_up(150.0, 170.0);

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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Pull only 30 px past top — below the 80 px threshold.
    // Finger moves DOWN (y increases): slop-crossing +50, then +30 actual overscroll.
    scoped.dispatch_pointer_down(150.0, 50.0);
    scoped.dispatch_pointer_move(150.0, 100.0); // slop-crossing: dy=+50
    scoped.dispatch_pointer_move(150.0, 130.0); // dy=+30: proposed=0-30=-30 -> overscroll=30<80
    scoped.dispatch_pointer_up(150.0, 130.0);

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

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Trigger a refresh with an over-threshold pull (finger moves DOWN: y increases).
    scoped.dispatch_pointer_down(150.0, 50.0);
    scoped.dispatch_pointer_move(150.0, 100.0); // slop-crossing: dy=+50
    scoped.dispatch_pointer_move(150.0, 170.0); // dy=+70: overscroll=70>=threshold(50)
    scoped.dispatch_pointer_up(150.0, 170.0);

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
