//! Self-tests for the test harness itself.
//!
//! Covers the full matrix — both protocols (Box, Sliver) crossed with both
//! run depths (`run_layout`, `run_frame`) — so the harness is proven against
//! known-good built-in render objects before any other test relies on it.
//! Assertions mirror values already validated in `tests/pipeline_scenarios.rs`,
//! `tests/layout_offset_commit.rs`, and `tests/sliver_fixed_extent_list.rs`.
//!
//! Previously lived at `src/testing/tests.rs` (a `#[cfg(test)]` internal
//! module). Moved here after the `flui-objects` extraction:
//! internal lib tests cannot import from `flui_objects` without triggering
//! a duplicate-crate-version error (flui-objects has a production dep on
//! flui-rendering, so the lib-under-test and flui-objects' copy of
//! flui-rendering are distinct compiled artifacts). Integration tests do not
//! have this problem — they link the already-built library.

use flui_foundation::geometry::{Offset, Rect, Size};
use flui_objects::{
    RenderColoredBox, RenderPadding, RenderSliverFixedExtentList, RenderStack, RenderViewport,
};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
use flui_rendering::testing::ParentDataSeed;
use flui_rendering::{
    constraints::BoxConstraints,
    parent_data::StackParentData,
    testing::{Probe, RenderTester, box_node, sliver_node},
};

/// Loose `0..=200 x 0..=200` constraints: children settle at their natural
/// size rather than being forced to fill (the box-pipeline test default).
fn loose_200() -> BoxConstraints {
    BoxConstraints::new(0.0, 200.0, 0.0, 200.0)
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
    assert_eq!(run.offset(child), Offset::new(5.0, 5.0));
    assert_eq!(run.box_geometry(child), Size::new(40.0, 40.0));
    assert_eq!(run.structure(), vec!["Offset", "Picture"]);
    assert_eq!(
        run.picture_bounds(),
        Some(Rect::from_ltrb(5.0, 5.0, 45.0, 45.0)),
    );
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
    .with_size(Size::new(120.0, 120.0))
    .run_layout();

    assert_eq!(run.offset(run.id("positioned")), Offset::new(18.0, 12.0));
    assert_eq!(run.hit_first(25.0, 20.0), Some(run.id("positioned")));
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

// ============================================================================
// Box x run_layout
// ============================================================================

// ============================================================================
// Sliver x run_layout
// ============================================================================

#[test]
fn sliver_run_layout_fixed_extent_list_geometry_and_child_sizes() {
    let run = RenderTester::mount(
        box_node(RenderViewport::new(AxisDirection::TopToBottom)).child(
            sliver_node(RenderSliverFixedExtentList::new(30.0, 3))
                .label("list")
                .child(
                    box_node(RenderColoredBox::red(300.0, 1000.0))
                        .label("item0")
                        .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                            SliverMultiBoxAdaptorParentData::new(0),
                        )),
                )
                .child(
                    box_node(RenderColoredBox::green(300.0, 1000.0)).with_parent_data_seed(
                        ParentDataSeed::SliverMultiBoxAdaptor(
                            SliverMultiBoxAdaptorParentData::new(1),
                        ),
                    ),
                )
                .child(
                    box_node(RenderColoredBox::blue(300.0, 1000.0)).with_parent_data_seed(
                        ParentDataSeed::SliverMultiBoxAdaptor(
                            SliverMultiBoxAdaptorParentData::new(2),
                        ),
                    ),
                ),
        ),
    )
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    let geometry = run.sliver_geometry(run.id("list"));
    assert_eq!(geometry.scroll_extent, 90.0, "3 items x 30px main extent");

    // Each box child is sized to the cross extent x the item extent.
    assert_eq!(run.box_geometry(run.id("item0")), Size::new(300.0, 30.0));
}

// ============================================================================
// Sliver x run_frame (smoke)
// ============================================================================

// ============================================================================
// Label registry
// ============================================================================

// ============================================================================
// Box query helpers
// ============================================================================
