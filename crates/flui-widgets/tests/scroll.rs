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
/// Mapping: `dP/d(thumb_top) = (viewport + scroll_extent) / available_track`
/// With viewport=300, scroll_extent=300, available_track=150:
///   50 track-px × (600 / 150) = 200 content-px
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
    //   Move to (290, 110) +50  — fires on_pan_update(delta_y=50) → content_delta=200
    //   Up   at (290, 110)      — within new thumb y=[50,200] after scroll, within widget
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 60.0);
    scoped.dispatch_pointer_move(290.0, 110.0);
    scoped.dispatch_pointer_up(290.0, 110.0);

    let final_pixels = controller.pixels();
    assert!(
        (final_pixels - 200.0).abs() < 1.0,
        "dragging the thumb 50 track-px must scroll 200 content-px \
         (viewport=300, scroll_extent=300, available_track=150); got {final_pixels:.2}"
    );
}

/// Chaining small thumb-drag moves accumulates content-delta until `max_scroll_extent`
/// is hit, and `clamp` prevents the position from exceeding the maximum.
///
/// Geometry: viewport=300, scroll_extent=300, thumb_height=150, available_track=150.
/// Each +20 track-px move gives content_delta = (20/150)*600 = 80 px.
/// After 4 `on_pan_update` calls: accumulated proposed = 320, clamped to 300.
///
/// All pointer positions stay within the thumb's original Positioned bounds
/// (y in [0, 150]) so every re-hit-test succeeds.
#[test]
fn scrollbar_thumb_drag_clamps_at_max_scroll_extent() {
    use flui_widgets::Scrollbar;

    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 300.0);

    let widget = Scrollbar::new()
        .controller(controller.clone())
        .thumb_width(20.0)
        .child(SizedBox::new(300.0, 300.0));

    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Thumb at pixels=0 occupies x=[280,300], y=[0,150].
    // Slop-crossing: DOWN -> MOVE(+20) crosses 18px threshold.
    // Four on_pan_update calls of +20 track-px each accumulate 320 content-px -> clamped to 300.
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 30.0); // +20 px: slop-crossing -> on_pan_start
    scoped.dispatch_pointer_move(290.0, 50.0); // +20 px -> on_pan_update: pixels=80
    scoped.dispatch_pointer_move(290.0, 70.0); // +20 px -> on_pan_update: pixels=160
    scoped.dispatch_pointer_move(290.0, 90.0); // +20 px -> on_pan_update: pixels=240
    scoped.dispatch_pointer_move(290.0, 110.0); // +20 px -> on_pan_update: proposed=320, clamped=300
    scoped.dispatch_pointer_up(290.0, 110.0);

    assert!(
        controller.pixels() <= 300.0,
        "thumb drag must not carry scroll past max_scroll_extent (300); got {:.2}",
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
