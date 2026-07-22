//! Self-tests for the test harness itself.
//!
//! Covers the full matrix — both protocols (Box, Sliver) crossed with both
//! run depths (`run_layout`, `run_frame`) — so the harness is proven against
//! known-good built-in render objects before any other test relies on it.
//! Assertions mirror values already validated in `tests/pipeline_scenarios.rs`,
//! `tests/layout_offset_commit.rs`, and `tests/sliver_fixed_extent_list.rs`.
//!
//! Previously lived at `src/testing/tests.rs` (a `#[cfg(test)]` internal
//! module). Moved here after the `flui-objects` extraction (ADR-0008):
//! internal lib tests cannot import from `flui_objects` without triggering
//! a duplicate-crate-version error (flui-objects has a production dep on
//! flui-rendering, so the lib-under-test and flui-objects' copy of
//! flui-rendering are distinct compiled artifacts). Integration tests do not
//! have this problem — they link the already-built library.

use flui_objects::{
    RenderColoredBox, RenderFlex, RenderOpacity, RenderPadding, RenderRepaintBoundary,
    RenderSliverFixedExtentList, RenderStack, RenderViewport,
};
use flui_rendering::{
    constraints::BoxConstraints,
    parent_data::{FlexParentData, StackParentData},
    testing::{BoxQueryRun, Probe, RenderTester, box_node, sliver_node},
};
use flui_types::{EdgeInsets, Offset, Rect, Size, geometry::px, layout::AxisDirection};

/// Loose `0..=200 x 0..=200` constraints: children settle at their natural
/// size rather than being forced to fill (the box-pipeline test default).
fn loose_200() -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0))
}

// ============================================================================
// Box x run_frame
// ============================================================================

#[test]
fn box_run_frame_padding_offsets_and_single_picture() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose_200())
    .run_frame();

    assert!(run.painted(), "first frame must paint");
    assert!(run.is_clean(), "no dirty residue after a settled frame");

    let child = run.id("child");
    assert_eq!(run.offset(child), Offset::new(px(5.0), px(5.0)));
    assert_eq!(run.box_geometry(child), Size::new(px(40.0), px(40.0)));
    assert_eq!(run.structure(), vec!["Offset", "Picture"]);
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(5.0), px(5.0), px(45.0), px(45.0))),
    );
}

#[test]
fn box_run_frame_flex_row_lays_children_along_main_axis() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("red"))
            .child(box_node(RenderColoredBox::green(60.0, 40.0)).label("green"))
            .child(box_node(RenderColoredBox::blue(20.0, 40.0)).label("blue")),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    assert_eq!(run.offset(run.id("red")), Offset::new(px(0.0), px(0.0)));
    assert_eq!(run.offset(run.id("green")), Offset::new(px(40.0), px(0.0)));
    assert_eq!(run.offset(run.id("blue")), Offset::new(px(100.0), px(0.0)));
}

#[test]
fn box_run_layout_stack_positioned_child_respects_parent_data_seed() {
    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(80.0, 80.0)).label("base"))
            .child(
                box_node(RenderColoredBox::green(20.0, 20.0))
                    .with_stack_parent_data(StackParentData::new().with_top(12.0).with_left(18.0))
                    .label("positioned"),
            ),
    )
    .with_size(Size::new(px(120.0), px(120.0)))
    .run_layout();

    assert_eq!(
        run.offset(run.id("positioned")),
        Offset::new(px(18.0), px(12.0))
    );
    assert_eq!(run.hit_first(25.0, 20.0), Some(run.id("positioned")));
}

#[test]
fn box_run_layout_flex_child_honors_flex_parent_data_seed() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(40.0, 20.0)).label("fixed"))
            .child(
                box_node(RenderColoredBox::green(10.0, 20.0))
                    .with_flex_parent_data(FlexParentData::flexible(1))
                    .label("flex"),
            ),
    )
    .with_size(Size::new(px(200.0), px(60.0)))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("flex")).width, px(160.0));
    assert_eq!(run.offset(run.id("flex")), Offset::new(px(40.0), px(0.0)));
}

#[test]
fn box_run_frame_repaint_boundary_splits_subtree() {
    let run = RenderTester::mount(box_node(RenderPadding::all(5.0)).child(
        box_node(RenderRepaintBoundary::new()).child(box_node(RenderColoredBox::red(40.0, 40.0))),
    ))
    .with_constraints(loose_200())
    .run_frame();

    assert_eq!(
        run.structure(),
        vec!["Offset", "Offset", "Picture"],
        "the boundary subtree splits under its own OffsetLayer",
    );
}

#[test]
fn box_run_frame_clean_frame_after_settle_produces_no_tree() {
    let mut run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(100.0), px(100.0)))
        .run_frame();

    assert!(run.painted(), "frame 1 paints");

    let report = run.pump();
    assert!(
        !report.painted,
        "a frame with no dirty work must produce no layer tree",
    );
    assert!(run.is_clean());
}

#[test]
fn box_run_frame_hit_path_routes_to_child() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose_200())
    .run_frame();

    let child = run.id("child");
    assert_eq!(run.hit_first(10.0, 10.0), Some(child));
    assert!(
        run.hit(2.0, 2.0).is_empty(),
        "the padding border (outside the child) misses",
    );
}

#[test]
fn render_dump_carries_object_properties_and_geometry() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose_200())
    .run_frame();

    let dump = run.dump();
    assert!(dump.contains("RenderPadding"), "names the padding: {dump}");
    assert!(dump.contains("RenderColoredBox"), "names the leaf: {dump}");
    assert!(
        dump.contains("padding"),
        "padding self-describes its insets: {dump}"
    );
    assert!(
        dump.contains("color"),
        "leaf self-describes its color: {dump}"
    );
    assert!(
        dump.contains("size"),
        "committed size is layered on: {dump}"
    );
}

#[test]
fn structured_diagnostics_queries() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row()).child(box_node(RenderColoredBox::red(40.0, 40.0)).label("red")),
    )
    .with_constraints(loose_200())
    .run_frame();

    // Per-node property lookup (no substring matching on the dump).
    assert_eq!(
        run.property(run.root(), "direction").as_deref(),
        Some("Horizontal"),
    );
    assert_eq!(
        run.property(run.id("red"), "color").as_deref(),
        Some("[1.0, 0.0, 0.0, 1.0]"),
    );

    // Depth-first tree navigation (works regardless of nesting depth).
    let tree = run.diagnostics();
    let leaf = tree
        .find_descendant("RenderColoredBox")
        .expect("tree has a colored leaf");
    assert_eq!(
        leaf.get_property("color").as_deref(),
        Some("[1.0, 0.0, 0.0, 1.0]")
    );

    assert_eq!(
        run.descendant_property("RenderFlex", "direction")
            .as_deref(),
        Some("Horizontal"),
    );
    assert_eq!(
        run.descendant_property("RenderColoredBox", "color")
            .as_deref(),
        Some("[1.0, 0.0, 0.0, 1.0]"),
    );
}

#[test]
fn pump_idle_frames_skips_settled_frames() {
    let mut run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(100.0), px(100.0)))
        .run_frame();

    run.pump_idle_frames(3);
}

#[test]
fn simulate_advances_layout_across_ticks() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose_200())
    .run_frame();

    let child = run.id("child");
    let pad = run.root();
    let reports = run.simulate([0.25, 0.5, 1.0], |t, run| {
        let padding = 5.0 + 50.0 * t as f32;
        run.update::<RenderPadding>(pad, |p| {
            p.set_padding(EdgeInsets::all(px(padding)));
        });
    });

    assert_eq!(reports.len(), 3);
    assert!(reports.iter().all(|r| r.painted));
    assert_eq!(run.offset(child), Offset::new(px(55.0), px(55.0)));
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(px(55.0), px(55.0), px(95.0), px(95.0))),
    );
}

#[test]
fn advance_paint_changes_color_without_layout() {
    let mut run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)).label("leaf"))
        .with_size(Size::new(px(100.0), px(100.0)))
        .run_frame();

    let leaf = run.id("leaf");
    let report = run.advance_paint::<RenderColoredBox>(leaf, |box_| {
        box_.set_color([0.0, 1.0, 0.0, 1.0]);
    });

    assert!(report.painted);
    assert_eq!(
        run.property(leaf, "color").as_deref(),
        Some("[0.0, 1.0, 0.0, 1.0]"),
    );
}

#[test]
fn advance_paint_opacity_tracks_layer_alpha() {
    let mut run = RenderTester::mount(
        box_node(RenderOpacity::new(1.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    let fade = run.root();
    let report = run.advance_paint::<RenderOpacity>(fade, |o| o.set_opacity(0.5));
    assert!(report.painted, "opacity change must repaint: {report}");
    assert!(
        run.structure().contains(&"Opacity"),
        "semi-opaque subtree must pay for an OpacityLayer: {:?}",
        run.structure(),
    );
    assert!(
        (run.opacity_alpha().expect("opacity layer present") - 0.5).abs() < 0.01,
        "opacity layer alpha must track the animated value",
    );
    assert!(run.has_picture_layer());
}

#[test]
fn update_then_pump_relayouts() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose_200())
    .run_frame();

    let child = run.id("child");
    assert_eq!(run.offset(child), Offset::new(px(5.0), px(5.0)));

    run.update::<RenderPadding>(run.root(), |padding| {
        padding.set_padding(EdgeInsets::all(px(20.0)));
    });
    run.pump();

    assert_eq!(run.offset(child), Offset::new(px(20.0), px(20.0)));
}

// ============================================================================
// Box x run_layout
// ============================================================================

#[test]
fn box_run_layout_commits_geometry_without_a_frame() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(8.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("inner")),
    )
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(200.0), px(200.0))
    );

    let inner = run.id("inner");
    assert_eq!(run.offset(inner), Offset::new(px(8.0), px(8.0)));
    assert_eq!(run.box_geometry(inner), Size::new(px(184.0), px(184.0)));
}

// ============================================================================
// Sliver x run_layout
// ============================================================================

#[test]
fn sliver_run_layout_fixed_extent_list_geometry_and_child_sizes() {
    let run = RenderTester::mount(
        box_node(RenderViewport::new(AxisDirection::TopToBottom)).child(
            sliver_node(RenderSliverFixedExtentList::new(30.0))
                .label("list")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("item0"))
                .child(box_node(RenderColoredBox::green(300.0, 1000.0)))
                .child(box_node(RenderColoredBox::blue(300.0, 1000.0))),
        ),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    let geometry = run.sliver_geometry(run.id("list"));
    assert_eq!(geometry.scroll_extent, 90.0, "3 items x 30px main extent");

    // Each box child is sized to the cross extent x the item extent.
    assert_eq!(
        run.box_geometry(run.id("item0")),
        Size::new(px(300.0), px(30.0))
    );
}

// ============================================================================
// Sliver x run_frame (smoke)
// ============================================================================

#[test]
fn sliver_run_frame_viewport_paints() {
    let run = RenderTester::mount(
        box_node(RenderViewport::new(AxisDirection::TopToBottom)).child(
            sliver_node(RenderSliverFixedExtentList::new(30.0))
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)))
                .child(box_node(RenderColoredBox::green(300.0, 1000.0))),
        ),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    assert!(
        run.painted(),
        "a viewport-rooted sliver tree paints a frame"
    );
    assert!(run.is_clean(), "no dirty residue after the frame settles");
}

// ============================================================================
// Label registry
// ============================================================================

#[test]
fn unknown_label_resolves_to_none() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(10.0, 10.0)))
        .with_size(Size::new(px(50.0), px(50.0)))
        .run_layout();
    assert!(run.try_id("missing").is_none());
}

// ============================================================================
// Box query helpers
// ============================================================================

#[test]
fn layout_run_box_queries_match_pipeline() {
    let constraints = loose_200();
    let mut run = RenderTester::mount(
        box_node(RenderOpacity::opaque())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(constraints)
    .run_layout();

    assert_eq!(run.min_intrinsic_width(run.root(), 100.0), 40.0);
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        Size::new(px(40.0), px(40.0))
    );
}

#[test]
fn frame_run_box_queries_work_after_paint() {
    let constraints = loose_200();
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(5.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(constraints)
    .run_frame();

    assert_eq!(run.min_intrinsic_width(run.root(), 100.0), 50.0);
    assert!(run.painted());
}
