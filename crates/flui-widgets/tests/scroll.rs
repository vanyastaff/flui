//! Scroll-path parity tests:
#![allow(clippy::float_cmp)] // physics clamp + controller pixel reads return exact f32 literals
//!
//! 1. `SingleChildScrollView` viewport geometry (cross-protocol Box→Sliver path).
//! 2. `ScrollController` thumb geometry helpers.
//! 3. `Scrollable` interactive drag integration (gesture → offset change).
//! 4. `ClampingScrollPhysics` hard-boundary enforcement.

mod common;

use common::{lay_out, lay_out_with_arena, size, tight};
use flui_types::Color;
use flui_view::ViewExt;
use flui_widgets::{
    ClampingScrollPhysics, ColoredBox, ListView, ScrollController, ScrollPhysics, Scrollable,
    SharedScrollPhysics, SingleChildScrollView, SizedBox,
};
use std::sync::Arc;

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
    // thumb_fraction = 0.5.
    // At pixels = 200 (halfway): offset_fraction = (200/400) * (1 − 0.5) = 0.25
    let controller = ScrollController::new();
    controller.update_dimensions(400.0, 0.0, 400.0);
    controller.set_pixels(200.0);

    let frac = controller.thumb_offset_fraction();
    assert!(
        (frac - 0.25).abs() < 0.001,
        "thumb offset fraction at scroll midpoint should be 0.25, got {frac}"
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
