//! Render-object harness catalog — every concrete render type is exercised
//! through [`RenderTester`] + [`Probe`] so CI can pin layout, hit-test, and
//! diagnostics contracts without visual inspection.
//!
//! # Coverage map (one row per exported render type)
//!
//! | Type | Harness test(s) | Layout | Hit-test | Paint | Diagnostics | Queries |
//! |------|-----------------|--------|----------|-------|-------------|---------|
//! | `RenderSizedBox` | `harness_sized_box_*` | yes | — | — | yes | queries |
//! | `RenderColoredBox` | `harness_colored_box_*` | yes | yes | yes | yes | — |
//! | `RenderImage` | `harness_image_*` | yes | — | yes | yes | — |
//! | `RenderParagraph` | `harness_paragraph_*` | yes | — | yes | yes | — |
//! | `RenderPadding` | `harness_padding_*` | yes | yes | — | yes | queries |
//! | `RenderCenter` | `harness_center_*` | yes | — | — | yes | — |
//! | `RenderAspectRatio` | `harness_aspect_ratio_*` | yes | — | — | yes | — |
//! | `RenderBaseline` | `harness_baseline_*` | yes | — | — | yes | queries |
//! | `RenderConstrainedBox` | `harness_constrained_box_*` | yes | — | — | yes | — |
//! | `RenderLimitedBox` | `harness_limited_box_*` | yes | — | — | yes | — |
//! | `RenderOffstage` | `harness_offstage_*` | yes | yes | — | yes | — |
//! | `RenderOpacity` | `harness_opacity_*` | yes | — | yes | yes | queries |
//! | `RenderTransform` | `harness_transform_*` | yes | — | yes | yes | — |
//! | `RenderFittedBox` | `harness_fitted_box_*` | yes | — | — | yes | — |
//! | `RenderFractionallySizedBox` | `harness_fractionally_sized_box_*` | yes | — | — | yes | — |
//! | `RenderFractionalTranslation` | `harness_fractional_translation_*` | yes | — | — | yes | — |
//! | `RenderDecoratedBox` | `harness_decorated_box_*` | yes | — | yes | yes | — |
//! | `RenderClipRect` | `harness_clip_rect_*` | yes | — | — | yes | — |
//! | `RenderClipRRect` | `harness_clip_rrect_*` | yes | — | — | yes | — |
//! | `RenderClipOval` | `harness_clip_oval_*` | yes | — | — | yes | — |
//! | `RenderClipPath` | `harness_clip_path_*` | yes | — | — | yes | — |
//! | `RenderRepaintBoundary` | `harness_repaint_boundary_*` | yes | — | yes | yes | — |
//! | `RenderMetaData` | `harness_metadata_*` | yes | — | — | yes | — |
//! | `RenderFlex` | `harness_flex_*` | yes | — | — | yes | queries |
//! | `RenderStack` | `harness_stack_*` | yes | yes | — | yes | queries |
//! | `RenderAbsorbPointer` | `harness_absorb_pointer_*` | yes | yes | — | yes | — |
//! | `RenderIgnorePointer` | `harness_ignore_pointer_*` | yes | yes | — | yes | — |
//! | `RenderSliverFixedExtentList` | `harness_sliver_fixed_extent_list_*` | yes | — | — | yes | — |
//! | `RenderSliverPadding` | `harness_sliver_padding_*` | yes | — | — | yes | — |
//! | `RenderSliverToBoxAdapter` | `harness_sliver_to_box_adapter_*` | yes | — | — | yes | — |
//! | `RenderSliverFillViewport` | `harness_sliver_fill_viewport_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemaining` | `harness_sliver_fill_remaining_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemainingAndOverscroll` | `harness_sliver_fill_remaining_and_overscroll_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemainingWithScrollable` | `harness_sliver_fill_remaining_with_scrollable_*` | yes | — | — | yes | — |
//! | `RenderSliverIgnorePointer` | `harness_sliver_ignore_pointer_*` | yes | yes | — | yes | — |
//! | `RenderSliverListLazy` | `harness_sliver_list_lazy_*` | yes | — | — | yes | — |
//! | `RenderSliverOffstage` | `harness_sliver_offstage_*` | yes | — | — | yes | — |
//! | `RenderSliverOpacity` | `harness_sliver_opacity_*` | yes | — | yes | yes | — |
//! | `RenderViewport` | `harness_viewport_*` | yes | — | — | yes | — |
//!
//! [`catalog_covers_every_render_object_name`] guards the table: every row's
//! type string must appear in this file so a missing harness test fails CI.

use flui_rendering::{
    constraints::BoxConstraints,
    objects::*,
    parent_data::{FlexParentData, StackParentData},
    testing::{
        BoxQueryRun, Probe, RenderTester, TreeNode, assert_descendant_properties,
        assert_has_committed_geometry, assert_has_committed_size, box_node, localize_hit_point,
        sliver_node,
    },
    traits::TextBaseline,
};
use flui_types::{
    Alignment, EdgeInsets, Offset, Point, Rect, Size,
    geometry::px,
    layout::{AxisDirection, BoxFit, StackFit},
    painting::Clip,
    styling::{BoxDecoration, Color},
    typography::{TextDirection, TextSpan},
};

/// Every concrete render-object type exported from `flui_rendering::objects`.
const RENDER_OBJECT_TYPES: &[&str] = &[
    "RenderAlign",
    "RenderSizedBox",
    "RenderColoredBox",
    "RenderImage",
    "RenderParagraph",
    "RenderPadding",
    "RenderCenter",
    "RenderAspectRatio",
    "RenderBaseline",
    "RenderConstrainedBox",
    "RenderLimitedBox",
    "RenderOffstage",
    "RenderOpacity",
    "RenderTransform",
    "RenderFittedBox",
    "RenderFractionallySizedBox",
    "RenderFractionalTranslation",
    "RenderDecoratedBox",
    "RenderClipRect",
    "RenderClipRRect",
    "RenderClipOval",
    "RenderClipPath",
    "RenderRepaintBoundary",
    "RenderMetaData",
    "RenderFlex",
    "RenderStack",
    "RenderAbsorbPointer",
    "RenderIgnorePointer",
    "RenderSliverFixedExtentList",
    "RenderSliverPadding",
    "RenderSliverToBoxAdapter",
    "RenderSliverFillViewport",
    "RenderSliverFillRemaining",
    "RenderSliverFillRemainingAndOverscroll",
    "RenderSliverFillRemainingWithScrollable",
    "RenderSliverIgnorePointer",
    "RenderSliverListLazy",
    "RenderSliverOffstage",
    "RenderSliverOpacity",
    "RenderViewport",
];

fn loose(max: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(max), px(0.0), px(max))
}

fn viewport(sliver: TreeNode) -> TreeNode {
    viewport_multi([sliver])
}

fn viewport_with_scroll(offset: f32, sliver: TreeNode) -> TreeNode {
    use flui_rendering::view::ScrollableViewportOffset;

    box_node(RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::new(offset),
    ))
    .label("viewport")
    .child(sliver)
}

fn viewport_multi(slivers: impl IntoIterator<Item = TreeNode>) -> TreeNode {
    let mut node = box_node(RenderViewport::new(AxisDirection::TopToBottom)).label("viewport");
    for sliver in slivers {
        node = node.child(sliver);
    }
    node
}

// ============================================================================
// Leaf box objects
// ============================================================================

#[test]
fn harness_sized_box_forces_dimensions() {
    let run = RenderTester::mount(box_node(RenderSizedBox::fixed(px(80.0), px(60.0))))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(80.0), px(60.0)));
    assert_descendant_properties(&run.diagnostics(), "RenderSizedBox", &["width", "height"]);
}

#[test]
fn harness_sized_box_expand_fills_parent() {
    let run = RenderTester::mount(box_node(RenderSizedBox::expand()))
        .with_size(Size::new(px(120.0), px(80.0)))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(120.0), px(80.0)));
}

#[test]
fn harness_sized_box_shrink_collapses() {
    let run = RenderTester::mount(box_node(RenderSizedBox::shrink()))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
}

#[test]
fn harness_sized_box_width_only_leaves_height_loose() {
    let run = RenderTester::mount(box_node(RenderSizedBox::new(Some(px(60.0)), None)))
        .with_constraints(BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(100.0)))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()).width, px(60.0));
    assert_eq!(run.box_geometry(run.root()).height, px(100.0));
}

#[test]
fn harness_sized_box_reports_fixed_queries() {
    let constraints = loose(200.0);
    let mut run = RenderTester::mount(box_node(RenderSizedBox::fixed(px(80.0), px(30.0))))
        .with_constraints(constraints)
        .run_layout();

    assert_eq!(run.min_intrinsic_width(run.root(), 0.0), 80.0);
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        Size::new(px(80.0), px(30.0))
    );
}

#[test]
fn harness_colored_box_self_describes_and_paints() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(50.0, 50.0)))
        .with_size(Size::new(px(100.0), px(100.0)))
        .run_frame();

    assert!(run.painted());
    assert_eq!(
        run.descendant_property("RenderColoredBox", "color")
            .as_deref(),
        Some("[1.0, 0.0, 0.0, 1.0]"),
    );
    let tree = run.diagnostics();
    assert_has_committed_size(
        tree.find_descendant("RenderColoredBox")
            .expect("colored box"),
    );
}

#[test]
fn harness_colored_box_hit_test_within_bounds() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_constraints(loose(200.0))
        .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.root()));
    assert!(run.hit(50.0, 50.0).is_empty());
}

#[test]
fn harness_image_placeholder_lays_out_from_intrinsic_size() {
    let run = RenderTester::mount(box_node(RenderImage::new(
        Size::new(px(100.0), px(50.0)),
        ImageFit::Contain,
        ImageAlignment::Center,
    )))
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_layout();

    assert!(run.box_geometry(run.root()).width.get() > 0.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderImage",
        &["intrinsic_size", "scale", "fit", "alignment"],
    );
}

#[test]
fn harness_image_paints_placeholder_frame() {
    let run = RenderTester::mount(box_node(RenderImage::new(
        Size::new(px(50.0), px(50.0)),
        ImageFit::Cover,
        ImageAlignment::Center,
    )))
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert!(run.painted());
}

#[test]
fn harness_paragraph_lays_out_text() {
    let run = RenderTester::mount(box_node(RenderParagraph::new(
        TextSpan::new("hello harness"),
        TextDirection::Ltr,
    )))
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert!(run.box_geometry(run.root()).height.get() > 0.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderParagraph",
        &["text_align", "text_direction"],
    );
}

#[test]
fn harness_paragraph_paints_text_frame() {
    let run = RenderTester::mount(box_node(RenderParagraph::new(
        TextSpan::new("paint me"),
        TextDirection::Ltr,
    )))
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_frame();

    assert!(run.painted());
}

// ============================================================================
// Single-child box proxies
// ============================================================================

#[test]
fn harness_padding_deflates_child_offset() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(12.0))
            .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(px(12.0), px(12.0)));
    assert!(
        run.descendant_property("RenderPadding", "padding")
            .is_some()
    );
}

#[test]
fn harness_padding_forwards_intrinsics_with_insets() {
    let mut run = RenderTester::mount(
        box_node(RenderPadding::all(10.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let padding = run.root();
    assert_eq!(
        run.min_intrinsic_width(padding, 100.0),
        60.0,
        "padding must add horizontal insets to the child's 40px min width"
    );
}

// Hit-test localization for RenderPadding: the recorded transform for the
// child entry must map a global hit point to the child's local coordinates.
//
// Setup: RenderPadding(all=12) with a 30×30 child in a 200×200 parent.
// Padding places the child at (12, 12).  Hit at global (20, 20).
// Expected child-local: (20−12, 20−12) = (8, 8).
#[test]
fn harness_padding_hit_localizes_to_padding_inset() {
    const PADDING_PX: f32 = 12.0;
    const HIT_X: f32 = 20.0;
    const HIT_Y: f32 = 20.0;

    let run = RenderTester::mount(
        box_node(RenderPadding::all(PADDING_PX))
            .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let child_id = run.id("child");

    let child_paint_offset = run.offset(child_id);
    assert_eq!(
        child_paint_offset,
        Offset::new(px(PADDING_PX), px(PADDING_PX)),
        "RenderPadding(all=12) must position child at (12, 12)"
    );

    let hit_entries = run.hit_with_transforms(HIT_X, HIT_Y);

    let child_transform = hit_entries
        .iter()
        .find(|(id, _)| *id == child_id)
        .map(|(_, t)| *t)
        .unwrap_or_else(|| panic!("child must be hit at ({HIT_X}, {HIT_Y})"));

    let recorded_transform = child_transform.expect(
        "child HitTestEntry must carry a recorded transform from hit_test_child_at_layout_offset",
    );

    let expected_local = Offset::new(
        px(HIT_X - child_paint_offset.dx.get()),
        px(HIT_Y - child_paint_offset.dy.get()),
    );

    let actual_local = localize_hit_point(recorded_transform, HIT_X, HIT_Y)
        .expect("recorded transform must be invertible");

    assert!(
        (actual_local.dx.get() - expected_local.dx.get()).abs() < 0.01
            && (actual_local.dy.get() - expected_local.dy.get()).abs() < 0.01,
        "child-local hit must equal global − padding_inset \
         (got ({:.2}, {:.2}), expected ({:.2}, {:.2}))",
        actual_local.dx.get(),
        actual_local.dy.get(),
        expected_local.dx.get(),
        expected_local.dy.get(),
    );
}

#[test]
fn harness_center_centers_child() {
    let run = RenderTester::mount(
        box_node(RenderCenter::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(px(30.0), px(30.0)));
    assert!(run.diagnostics().find_descendant("RenderCenter").is_some());
}

#[test]
fn harness_center_with_factors_shrinks_available_space() {
    let run = RenderTester::mount(
        box_node(
            RenderCenter::new()
                .with_width_factor(0.5)
                .with_height_factor(0.5),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(20.0), px(20.0)));
    assert!(
        run.descendant_property("RenderCenter", "width_factor")
            .is_some()
    );
    assert!(
        run.descendant_property("RenderCenter", "height_factor")
            .is_some()
    );
}

#[test]
fn harness_baseline_positions_text_at_offset() {
    let mut run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(0.0))).child(
            box_node(RenderParagraph::new(
                TextSpan::new("Ag"),
                TextDirection::Ltr,
            ))
            .label("text"),
        ),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    let tree = run.diagnostics();
    assert_has_committed_size(
        tree.find_descendant("RenderBaseline")
            .expect("RenderBaseline"),
    );
    assert_descendant_properties(&tree, "RenderBaseline", &["baseline"]);
    let constraints = BoxConstraints::loose(Size::new(px(200.0), px(100.0)));
    let baseline = run
        .dry_baseline(run.root(), constraints, TextBaseline::Alphabetic)
        .expect("paragraph reports a dry baseline");
    assert_eq!(baseline, 0.0);
}

#[test]
fn harness_flex_row_baseline_aligns_text_and_box() {
    let run = RenderTester::mount(
        box_node(
            RenderFlex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Baseline)
                .with_text_baseline(TextBaseline::Alphabetic),
        )
        .child(
            box_node(RenderParagraph::new(
                TextSpan::new("Ag"),
                TextDirection::Ltr,
            ))
            .label("text"),
        )
        .child(box_node(RenderColoredBox::red(20.0, 40.0)).label("box")),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    let text_y = run.offset(run.id("text")).dy.get();
    let box_y = run.offset(run.id("box")).dy.get();
    assert!(
        (text_y - box_y).abs() < 0.5,
        "baseline row should align text and box on the same cross offset (text={text_y}, box={box_y})",
    );
}

#[test]
fn harness_aspect_ratio_enforces_ratio() {
    // Loose constraints let `_apply_aspect_ratio` honour the ratio; tight
    // constraints return `constraints.smallest()` unchanged (Flutter parity).
    let run = RenderTester::mount(
        box_node(RenderAspectRatio::new(AspectRatio::new_unchecked(2.0)))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let size = run.box_geometry(run.root());
    assert!((size.width.get() / size.height.get() - 2.0).abs() < 0.01);
    assert_eq!(
        run.descendant_property_f64("RenderAspectRatio", "aspect_ratio"),
        Some(2.0)
    );
}

#[test]
fn harness_aspect_ratio_tight_constraints_use_smallest_size() {
    let run = RenderTester::mount(
        box_node(RenderAspectRatio::new(AspectRatio::new_unchecked(2.0)))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(200.0), px(200.0))
    );
}

#[test]
fn harness_constrained_box_enforces_minimums() {
    let extra = BoxConstraints::new(px(100.0), px(f32::INFINITY), px(100.0), px(f32::INFINITY));
    let run = RenderTester::mount(
        box_node(RenderConstrainedBox::new(extra))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let child = run.box_geometry(run.id("child"));
    assert!(child.width.get() >= 100.0);
    assert!(child.height.get() >= 100.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderConstrainedBox",
        &["additional_constraints"],
    );
}

#[test]
fn harness_limited_box_caps_unbounded_width_in_row() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row()).child(
            box_node(RenderLimitedBox::width(px(60.0)))
                .child(box_node(RenderColoredBox::green(200.0, 20.0)).label("child")),
        ),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("child")).width, px(60.0));
}

#[test]
fn harness_limited_box_self_describes_and_caps_unbounded_height() {
    let run = RenderTester::mount(
        box_node(RenderLimitedBox::height(px(40.0)))
            .child(box_node(RenderColoredBox::green(200.0, 200.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(f32::INFINITY),
    ))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("child")).height, px(40.0));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderLimitedBox",
        &["max_width", "max_height"],
    );
}

#[test]
fn harness_offstage_hidden_collapses_and_misses_hits() {
    let run = RenderTester::mount(
        box_node(RenderOffstage::hidden())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
    assert!(run.hit(10.0, 10.0).is_empty());
    assert!(
        run.descendant_property("RenderOffstage", "offstage")
            .is_some()
    );
}

#[test]
fn harness_offstage_visible_passes_child_size() {
    let run = RenderTester::mount(
        box_node(RenderOffstage::visible())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_offstage_visible_hit_test_reaches_child() {
    let run = RenderTester::mount(
        box_node(RenderOffstage::visible())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("child")));
}

#[test]
fn harness_opacity_passes_child_geometry() {
    let run = RenderTester::mount(
        box_node(RenderOpacity::new(0.5))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
    assert_eq!(
        run.descendant_property_f64("RenderOpacity", "opacity"),
        Some(0.5)
    );
}

#[test]
fn harness_opacity_forwards_box_queries() {
    let constraints = loose(200.0);
    let mut run = RenderTester::mount(
        box_node(RenderOpacity::opaque())
            .label("proxy")
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(constraints)
    .run_layout();

    let proxy = run.id("proxy");
    assert_eq!(
        run.min_intrinsic_width(proxy, 100.0),
        40.0,
        "opacity must forward child min intrinsic width"
    );
    assert_eq!(
        run.dry_layout(proxy, constraints),
        Size::new(px(40.0), px(40.0)),
        "opacity must forward child dry layout"
    );
}

#[test]
fn harness_opacity_paints_with_alpha_layer() {
    let run = RenderTester::mount(
        box_node(RenderOpacity::new(0.5))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(run.painted());
    assert!(run.structure().contains(&"Opacity"));
}

#[test]
fn harness_transform_passes_layout_and_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderTransform::uniform_scale(2.0))
            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(20.0), px(20.0)));
    assert!(
        run.descendant_property("RenderTransform", "transform")
            .is_some()
    );
}

#[test]
fn harness_transform_paints_with_transform_layer() {
    let run = RenderTester::mount(
        box_node(RenderTransform::uniform_scale(2.0))
            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(run.painted());
    assert!(run.structure().contains(&"Transform"));
}

#[test]
fn harness_fitted_box_sizes_to_parent() {
    let run = RenderTester::mount(
        box_node(RenderFittedBox::new(
            BoxFit::Contain,
            Alignment::CENTER,
            Clip::None,
        ))
        .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(50.0), px(50.0)))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(50.0), px(50.0)));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderFittedBox",
        &["fit", "clip_behavior"],
    );
}

#[test]
fn harness_fractionally_sized_box_applies_width_factor() {
    let run = RenderTester::mount(
        box_node(RenderFractionallySizedBox::new().with_width_factor(FractionFactor::HALF))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("child")).width, px(100.0));
}

#[test]
fn harness_fractionally_sized_box_height_factor_and_diagnostics() {
    let run = RenderTester::mount(
        box_node(
            RenderFractionallySizedBox::new()
                .with_height_factor(FractionFactor::HALF)
                .with_alignment(Alignment::BOTTOM_RIGHT),
        )
        .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("child")).height, px(50.0));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderFractionallySizedBox",
        &["width_factor", "height_factor", "alignment"],
    );
}

#[test]
fn harness_fractional_translation_passes_child_size() {
    let run = RenderTester::mount(
        box_node(RenderFractionalTranslation::translated(
            TranslationFraction::new(-0.5, 0.0),
        ))
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderFractionalTranslation",
        &["translation", "transform_hit_tests"],
    );
}

#[test]
fn harness_fractional_translation_without_hit_transform_uses_layout_bounds() {
    let run = RenderTester::mount(
        box_node(RenderFractionalTranslation::new(
            TranslationFraction::new(-0.5, 0.0),
            false,
        ))
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("child")));
    assert!(
        run.descendant_property("RenderFractionalTranslation", "transform_hit_tests")
            .is_none(),
        "false transform_hit_tests flags are omitted from diagnostics",
    );
}

#[test]
fn harness_decorated_box_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderDecoratedBox::new(BoxDecoration::with_color(
            Color::RED,
        )))
        .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert!(run.painted());
    assert_descendant_properties(&run.diagnostics(), "RenderDecoratedBox", &["decoration"]);
}

#[test]
fn harness_decorated_box_layout_wraps_child_geometry() {
    let run = RenderTester::mount(
        box_node(RenderDecoratedBox::new(BoxDecoration::with_color(
            Color::BLUE,
        )))
        .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(px(30.0), px(30.0))
    );
    assert_eq!(run.box_geometry(run.root()), Size::new(px(30.0), px(30.0)));
}

#[test]
fn harness_clip_rect_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderClipRect::new(Clip::HardEdge))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipRect", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_clip_rect_custom_clipper_flag() {
    let run = RenderTester::mount(
        box_node(
            RenderClipRect::anti_alias()
                .with_clipper(|size| Rect::from_origin_size(Point::ZERO, size)),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert!(
        run.descendant_property("RenderClipRect", "custom_clipper")
            .is_some()
    );
}

#[test]
fn harness_clip_rrect_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderClipRRect::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipRRect", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_clip_oval_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderClipOval::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipOval", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_clip_path_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderClipPath::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipPath", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_repaint_boundary_splits_layer_tree() {
    let run = RenderTester::mount(
        box_node(RenderRepaintBoundary::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.structure(), vec!["Offset", "Picture"]);
}

#[test]
fn harness_repaint_boundary_committed_size() {
    let run = RenderTester::mount(
        box_node(RenderRepaintBoundary::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let tree = run.diagnostics();
    assert_has_committed_size(
        tree.find_descendant("RenderRepaintBoundary")
            .expect("boundary"),
    );
}

#[test]
fn harness_metadata_with_payload() {
    let run = RenderTester::mount(
        box_node(RenderMetaData::new().with_metadata(42u32))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderMetaData",
        &["has_metadata", "behavior"],
    );
}

#[test]
fn harness_metadata_without_payload_omits_has_metadata_flag() {
    let run = RenderTester::mount(
        box_node(RenderMetaData::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert!(
        run.descendant_property("RenderMetaData", "has_metadata")
            .is_none()
    );
    assert!(
        run.descendant_property("RenderMetaData", "behavior")
            .is_some()
    );
}

// ============================================================================
// Multi-child box objects
// ============================================================================

#[test]
fn harness_flex_row_positions_children_on_main_axis() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(50.0, 20.0)).label("b")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    assert_eq!(run.offset(run.id("b")), Offset::new(px(30.0), px(0.0)));
    assert_eq!(
        run.descendant_property("RenderFlex", "direction")
            .as_deref(),
        Some("Horizontal"),
    );
}

#[test]
fn harness_flex_row_sums_child_min_intrinsic_widths() {
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(50.0, 20.0)).label("b")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.min_intrinsic_width(run.root(), 100.0), 80.0);
    assert_eq!(run.max_intrinsic_height(run.root(), 200.0), 20.0);
}

#[test]
fn harness_flex_row_weights_flexible_child_min_intrinsic_width() {
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(
                box_node(RenderColoredBox::red(100.0, 20.0))
                    .with_flex_parent_data(FlexParentData::flexible(1))
                    .label("flex"),
            )
            .child(box_node(RenderColoredBox::green(40.0, 20.0)).label("fixed")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(
        run.min_intrinsic_width(run.root(), 100.0),
        140.0,
        "flex child min 100 at flex 1 plus fixed 40",
    );
}

#[test]
fn harness_flex_column_stacks_children_vertically() {
    let run = RenderTester::mount(
        box_node(RenderFlex::column())
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(30.0, 25.0)).label("b")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    assert_eq!(run.offset(run.id("b")), Offset::new(px(0.0), px(20.0)));
    assert_eq!(
        run.descendant_property("RenderFlex", "direction")
            .as_deref(),
        Some("Vertical"),
    );
}

#[test]
fn harness_flex_column_max_child_intrinsic_width() {
    let mut run = RenderTester::mount(
        box_node(RenderFlex::column())
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(50.0, 20.0)).label("b")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(
        run.min_intrinsic_width(run.root(), 100.0),
        50.0,
        "column cross-axis width is max child width, not sum",
    );
    assert_eq!(run.max_intrinsic_width(run.root(), 100.0), 50.0);
}

#[test]
fn harness_stack_max_child_intrinsic_width() {
    let mut run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(50.0, 25.0)).label("b")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.min_intrinsic_width(run.root(), 100.0), 50.0);
    assert_eq!(run.max_intrinsic_height(run.root(), 200.0), 25.0);
}

#[test]
fn harness_stack_hit_tests_top_child_first() {
    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("bottom"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("top")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("top")));
    assert_descendant_properties(&run.diagnostics(), "RenderStack", &["fit", "clip_behavior"]);
}

#[test]
fn harness_stack_expand_fit_stretches_non_positioned_child() {
    let run = RenderTester::mount(
        box_node(RenderStack::new().with_fit(StackFit::Expand))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(80.0)))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(px(100.0), px(80.0))
    );
}

#[test]
fn harness_stack_positioned_child_layout_and_hit_test() {
    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(60.0, 60.0)).label("base"))
            .child(
                box_node(RenderColoredBox::green(20.0, 20.0))
                    .with_stack_parent_data(StackParentData::new().with_top(8.0).with_left(16.0))
                    .label("badge"),
            ),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.offset(run.id("badge")), Offset::new(px(16.0), px(8.0)));
    assert_eq!(run.hit_first(20.0, 12.0), Some(run.id("badge")));
    assert_eq!(run.hit_first(5.0, 5.0), Some(run.id("base")));
}

// ============================================================================
// Pointer semantics
// ============================================================================

#[test]
fn harness_absorb_pointer_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderAbsorbPointer::new(true))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert!(
        run.descendant_property("RenderAbsorbPointer", "absorbing")
            .is_some()
    );
}

#[test]
fn harness_ignore_pointer_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderIgnorePointer::new(true))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert!(
        run.descendant_property("RenderIgnorePointer", "ignoring")
            .is_some()
    );
}

#[test]
fn harness_absorb_pointer_blocks_child_hits() {
    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("below"))
            .child(
                box_node(RenderAbsorbPointer::new(true))
                    .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("inner"))
                    .label("absorb"),
            ),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    let path = run.hit(20.0, 20.0);
    assert!(path.contains(&run.id("absorb")));
    assert!(!path.contains(&run.id("inner")));
}

#[test]
fn harness_ignore_pointer_lets_hits_pass_to_sibling_below() {
    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("below"))
            .child(
                box_node(RenderIgnorePointer::new(true))
                    .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("inner"))
                    .label("ignore"),
            ),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("below")));
}

// ============================================================================
// Sliver objects (via viewport host)
// ============================================================================

#[test]
fn harness_sliver_fixed_extent_list_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFixedExtentList::new(25.0))
            .label("list")
            .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("item0"))
            .child(box_node(RenderColoredBox::green(300.0, 1000.0))),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("list")).scroll_extent, 50.0);
    assert_eq!(run.box_geometry(run.id("item0")).height, px(25.0));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderSliverFixedExtentList",
        &["item_extent"],
    );
    let tree = run.diagnostics();
    let sliver = tree.find_descendant("RenderSliverFixedExtentList").unwrap();
    assert_has_committed_geometry(sliver);
}

#[test]
fn harness_sliver_padding_insets_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverPadding::symmetric(10.0, 0.0))
            .label("pad")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(20.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert!(
        run.descendant_property("RenderSliverPadding", "padding")
            .is_some()
    );
    assert!(run.sliver_geometry(run.id("pad")).scroll_extent > 0.0);
    let tree = run.diagnostics();
    assert_has_committed_geometry(
        tree.find_descendant("RenderSliverPadding")
            .expect("padding"),
    );
}

#[test]
fn harness_sliver_padding_scrolled_viewport_applies_leading_padding() {
    let run = RenderTester::mount(viewport_with_scroll(
        5.0,
        sliver_node(RenderSliverPadding::new(EdgeInsets {
            top: px(10.0),
            right: px(0.0),
            bottom: px(20.0),
            left: px(0.0),
        }))
        .label("pad")
        .child(
            sliver_node(RenderSliverToBoxAdapter::new())
                .label("adapter")
                .child(box_node(RenderSizedBox::fixed(px(300.0), px(80.0))).label("box")),
        ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    let pad = run.sliver_geometry(run.id("pad"));
    assert_eq!(pad.scroll_extent, 110.0);
    assert_eq!(
        pad.paint_extent, 100.0,
        "paint extent is clamped to the 100px viewport main axis",
    );
    assert_eq!(run.offset(run.id("adapter")).dy, px(5.0));
    assert_has_committed_geometry(
        run.diagnostics()
            .find_descendant("RenderSliverToBoxAdapter")
            .expect("adapter"),
    );
}

#[test]
fn harness_sliver_to_box_adapter_scroll_extent_matches_child() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverToBoxAdapter::new())
            .label("adapter")
            .child(box_node(RenderSizedBox::fixed(px(300.0), px(42.0))).label("box")),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("adapter")).scroll_extent, 42.0);
    let tree = run.diagnostics();
    assert_has_committed_geometry(
        tree.find_descendant("RenderSliverToBoxAdapter")
            .expect("adapter"),
    );
}

#[test]
fn harness_sliver_fill_viewport_fraction() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillViewport::new(0.5))
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("page")),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 50.0);
    assert_eq!(run.box_geometry(run.id("page")).height, px(50.0));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderSliverFillViewport",
        &["viewport_fraction"],
    );
}

#[test]
fn harness_sliver_fill_remaining_uses_viewport_remainder() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillRemaining::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("child")),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 100.0);
    let tree = run.diagnostics();
    let node = tree.find_descendant("RenderSliverFillRemaining").unwrap();
    assert_has_committed_geometry(node);
}

#[test]
fn harness_sliver_fill_remaining_and_overscroll_fills_viewport() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillRemainingAndOverscroll::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("child")),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 100.0);
    assert_eq!(run.box_geometry(run.id("child")).height, px(100.0));
    let tree = run.diagnostics();
    let node = tree
        .find_descendant("RenderSliverFillRemainingAndOverscroll")
        .unwrap();
    assert_has_committed_geometry(node);
}

#[test]
fn harness_sliver_fill_remaining_with_scrollable_reports_full_scroll_extent() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillRemainingWithScrollable::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("child")),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 100.0);
    assert_eq!(run.box_geometry(run.id("child")).height, px(100.0));
    let tree = run.diagnostics();
    let node = tree
        .find_descendant("RenderSliverFillRemainingWithScrollable")
        .unwrap();
    assert_has_committed_geometry(node);
}

#[test]
fn harness_sliver_ignore_pointer_blocks_hits_when_active() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverIgnorePointer::new(true))
            .label("ignore")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("item")),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    assert!(run.hit(20.0, 20.0).is_empty());
    assert!(
        run.descendant_property("RenderSliverIgnorePointer", "ignoring")
            .is_some()
    );
}

#[test]
fn harness_sliver_ignore_pointer_passes_hits_when_inactive() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverIgnorePointer::new(false))
            .label("ignore")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("item")),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("item")));
}

#[test]
fn harness_sliver_list_lazy_zero_items_reports_zero_geometry() {
    // Empty source — build closure always returns None, so perform_layout
    // produces zero scroll_extent and self-describes via diagnostics.
    let list = RenderSliverListLazy::new(0, 48.0, std::sync::Arc::new(|_| None), None);
    let run = RenderTester::mount(viewport(sliver_node(list).label("lazy")))
        .with_size(Size::new(px(300.0), px(400.0)))
        .run_layout();

    assert_eq!(
        run.sliver_geometry(run.id("lazy")).scroll_extent,
        0.0,
        "empty RenderSliverListLazy must report zero scroll extent",
    );
    assert_descendant_properties(&run.diagnostics(), "RenderSliverListLazy", &["item_count"]);
}

#[test]
fn harness_sliver_offstage_hidden_reports_zero_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOffstage::hidden())
            .label("off")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("off")).scroll_extent, 0.0);
    assert!(
        run.descendant_property("RenderSliverOffstage", "offstage")
            .is_some()
    );
}

#[test]
fn harness_sliver_offstage_visible_reports_child_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOffstage::visible())
            .label("off")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("item")),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("off")).scroll_extent, 30.0);
}

#[test]
fn harness_sliver_opacity_repaints_on_paint_mutation() {
    let mut run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOpacity::new(1.0))
            .label("opacity")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    let opacity = run.id("opacity");
    let report = run.advance_paint::<RenderSliverOpacity>(opacity, |o| {
        o.set_opacity(0.5);
    });
    assert!(
        report.painted,
        "sliver opacity change must repaint: {report}"
    );
    assert!(
        run.structure().contains(&"Opacity"),
        "semi-opaque sliver must pay for an OpacityLayer: {:?}",
        run.structure(),
    );
    assert!(
        (run.opacity_alpha().expect("opacity layer present") - 0.5).abs() < 0.01,
        "opacity layer alpha must track the animated value",
    );
}

#[test]
fn harness_sliver_opacity_passes_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOpacity::new(0.5))
            .label("opacity")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("opacity")).scroll_extent, 30.0);
    assert_eq!(
        run.descendant_property_f64("RenderSliverOpacity", "opacity"),
        Some(0.5)
    );
}

// 1.3 RED test (behavior fix): alpha=0 must NOT need compositing.
// Flutter proxy_sliver.dart: `alwaysNeedsCompositing => alpha > 0`.
// Currently `needs_compositing` returns true for alpha=0 (condition:
// `always || alpha != 255`), which diverges from Flutter's rule.
#[test]
fn sliver_opacity_alpha_zero_does_not_need_compositing() {
    let opacity = RenderSliverOpacity::transparent(); // alpha = 0
    assert!(
        !opacity.needs_compositing(),
        "RenderSliverOpacity at alpha=0 must not need compositing \
         (Flutter: alwaysNeedsCompositing => alpha > 0)"
    );
}

// 1.3 mirror RED test: box RenderOpacity at alpha=0 must also not need
// compositing (needs_compositing false at alpha=0 without always flag).
#[test]
fn box_opacity_alpha_zero_does_not_need_compositing() {
    // RenderOpacity::needs_compositing() currently returns true for alpha=0
    // (condition: alpha != 255), which diverges from Flutter's
    // alwaysNeedsCompositing => alpha > 0. This test pins the correct behavior.
    let opacity = RenderOpacity::transparent(); // alpha = 0
    assert!(
        !opacity.needs_compositing(),
        "RenderOpacity at alpha=0 must not report needs_compositing \
         (Flutter: alwaysNeedsCompositing => alpha > 0)"
    );
}

// 1.3 paint_alpha RED→GREEN test: alpha=0 sliver must not emit an Opacity layer.
// Flutter proxy_sliver.dart: alpha 0 → layer=null, return — no layer painted.
// Before the paint_alpha fix, paint_alpha returned Some(0) for alpha=0, causing
// the owner to wrap the child in a 0-alpha OpacityLayer (present in structure).
// After fix: paint_alpha returns None at alpha=0, no OpacityLayer emitted.
#[test]
fn harness_sliver_opacity_alpha_zero_emits_no_opacity_layer() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOpacity::transparent()) // alpha = 0
            .label("opacity")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    assert!(
        !run.structure().contains(&"Opacity"),
        "fully-transparent sliver (alpha=0) must NOT emit an OpacityLayer \
         (Flutter: alpha=0 → layer=null): {:?}",
        run.structure(),
    );
}

// 1.3 paint_alpha RED→GREEN test: alpha=0 box must not emit an Opacity layer.
// Mirrors the sliver test above for RenderOpacity (box variant).
#[test]
fn harness_opacity_alpha_zero_emits_no_opacity_layer() {
    let run = RenderTester::mount(
        box_node(RenderOpacity::transparent())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(
        !run.structure().contains(&"Opacity"),
        "fully-transparent box (alpha=0) must NOT emit an OpacityLayer \
         (Flutter: alpha=0 → layer=null): {:?}",
        run.structure(),
    );
}

#[test]
fn harness_viewport_self_describes() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFixedExtentList::new(20.0))
            .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderViewport",
        &["axis_direction", "scroll_offset", "cache_extent"],
    );
}

#[test]
fn harness_viewport_stacks_two_slivers() {
    let run = RenderTester::mount(viewport_multi([
        sliver_node(RenderSliverFixedExtentList::new(20.0))
            .label("header")
            .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
        sliver_node(RenderSliverFillRemaining::new())
            .label("body")
            .child(box_node(RenderColoredBox::green(300.0, 10.0)).label("fill_child")),
    ]))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("header")).scroll_extent, 20.0);
    assert_eq!(run.sliver_geometry(run.id("body")).scroll_extent, 80.0);
}

// ============================================================================
// RenderAlign harness tests
// ============================================================================

// Verify that TOP_LEFT alignment places the child at (0,0) inside a 100×100
// parent with a 40×40 child → free space = 60×60 → TOP_LEFT offset = (0,0).
#[test]
fn harness_align_top_left_places_child_at_origin() {
    let run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::TOP_LEFT))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(px(0.0), px(0.0)));
}

// BOTTOM_RIGHT alignment: free space = 60×60 → offset = (60,60).
#[test]
fn harness_align_bottom_right_places_child_at_free_space() {
    let run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::BOTTOM_RIGHT))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(px(60.0), px(60.0)));
}

// CENTER alignment: free space = 60×60 → offset = (30,30).
#[test]
fn harness_align_center_matches_render_center_offset() {
    let run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::CENTER))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(px(30.0), px(30.0)));
}

// Intrinsics scale by the width factor.
#[test]
fn harness_align_intrinsics_scale_with_factor() {
    let constraints = loose(200.0);
    let mut run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::CENTER).with_width_factor(2.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(constraints)
    .run_layout();

    // min_intrinsic_width = child 40 * factor 2.0 = 80
    assert_eq!(run.min_intrinsic_width(run.root(), 0.0), 80.0);
}

// Dry baseline = child baseline + child_offset.dy.
// Uses BOTTOM_RIGHT alignment (dy = free_h * 1.0) so that the +offset.dy term
// is non-zero and the test fails if that addition is deleted.
// Layout: parent 200×200, child dry ~line-height → free_h > 0 → dy > 0.
// If the `+ child_offset_dy` line in RenderAlign::compute_dry_baseline were
// replaced with `+ 0.0`, this test would fail because child_bl + 0 ≠ child_bl + free_h.
#[test]
fn align_dry_baseline_adds_child_offset_dy() {
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let mut run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::BOTTOM_RIGHT)).child(
            box_node(RenderParagraph::new(
                flui_types::typography::TextSpan::new("A"),
                flui_types::typography::TextDirection::Ltr,
            ))
            .label("text"),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    let child_constraints = constraints.loosen();
    // Dry layout the child to get its size and baseline.
    let child_size = run.dry_layout(run.id("text"), child_constraints);
    let child_bl = run
        .dry_baseline(run.id("text"), child_constraints, TextBaseline::Alphabetic)
        .expect("paragraph has a baseline");

    // BOTTOM_RIGHT: free_h = parent_h - child_h; offset.dy = free_h * 1.0.
    // parent_size = constrain(200×200, child_size, None, None) = 200×200.
    let free_h = 200.0_f32 - child_size.height.get();
    let expected_dy = free_h; // BOTTOM_RIGHT factor = 1.0
    let expected = child_bl + expected_dy;

    let dry_bl = run
        .dry_baseline(run.root(), constraints, TextBaseline::Alphabetic)
        .expect("align with paragraph child reports dry baseline");
    assert!(
        (dry_bl - expected).abs() < 0.5,
        "BOTTOM_RIGHT dry baseline must be child_baseline + free_h (got {dry_bl}, expected {expected})"
    );
}

// Live baseline = child live baseline + child_offset.dy (FIX 1 — parity with
// Flutter RenderShiftedBox.computeDistanceToActualBaseline).
//
// Strategy: wrap RenderAlign in a RenderBaseline probe at a fixed offset.
// RenderBaseline::perform_layout calls child_distance_to_actual_baseline on
// RenderAlign, then positions it at `baseline_offset_px - live_baseline` from
// the top.  Before the fix RenderAlign returns None so the child lands at dy=0.
// After the fix RenderAlign returns child_bl + align_dy (non-zero for CENTER),
// so the child lands at baseline_offset_px - (child_bl + align_dy) ≠ 0.
//
// Layout: outer 200×200, RenderAlign(CENTER), RenderParagraph child.
// child_size ≈ text line-height (much less than 200); CENTER places child at
// dy = free_h / 2, which is large and well above 0.
// probe_offset is set to 100 so the expected child dy = 100 - (child_bl + align_dy).
#[test]
fn align_live_baseline_adds_child_offset_dy() {
    use flui_types::geometry::px;
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    const PROBE_OFFSET_PX: f32 = 100.0;

    let mut run = RenderTester::mount(
        box_node(RenderBaseline::new(
            TextBaseline::Alphabetic,
            px(PROBE_OFFSET_PX),
        ))
        .label("probe")
        .child(
            box_node(RenderAlign::new(Alignment::CENTER))
                .label("align")
                .child(
                    box_node(RenderParagraph::new(
                        flui_types::typography::TextSpan::new("A"),
                        flui_types::typography::TextDirection::Ltr,
                    ))
                    .label("text"),
                ),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    // Query the child (RenderAlign) dry baseline to know what the live baseline
    // SHOULD be after the fix: child_bl + align_dy.
    let align_constraints = constraints.loosen();
    let align_bl_dry = run
        .dry_baseline(run.id("align"), align_constraints, TextBaseline::Alphabetic)
        .expect("RenderAlign with paragraph child must report a dry baseline");

    // RenderBaseline positions its child at: child_offset.dy = probe_offset - live_bl_of_align.
    // Before fix: live_bl_of_align = None → child_offset.dy = 0.
    // After fix:  live_bl_of_align = align_bl_dry (live == dry for a statically laid-out tree)
    //             → child_offset.dy = PROBE_OFFSET_PX - align_bl_dry.
    let align_offset_dy = run.offset(run.id("align")).dy.get();
    let expected_dy = PROBE_OFFSET_PX - align_bl_dry;

    assert!(
        (align_offset_dy - expected_dy).abs() < 0.5,
        "RenderAlign must forward live baseline so RenderBaseline positions it at \
         probe_offset - (child_bl + align_dy) (got dy={align_offset_dy}, expected {expected_dy})"
    );
}

// Diagnostics includes width_factor and height_factor when set.
#[test]
fn harness_align_self_describes() {
    let run = RenderTester::mount(
        box_node(
            RenderAlign::new(Alignment::CENTER)
                .with_width_factor(1.5)
                .with_height_factor(2.0),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert!(
        run.descendant_property("RenderAlign", "width_factor")
            .is_some(),
        "RenderAlign must report width_factor in diagnostics"
    );
    assert!(
        run.descendant_property("RenderAlign", "height_factor")
            .is_some(),
        "RenderAlign must report height_factor in diagnostics"
    );
}

// Hit-test localization: the transform recorded in the child's HitTestEntry
// must map the global hit point back to the child's local coordinate space.
//
// Setup: RenderAlign(CENTER) with a 40×40 child in a 100×100 parent.
// Center places the child at offset (30, 30).
// Hit at root (50, 50) — inside the child.
//
// Before commit 2e69d275 (when hit_test used hit_test_child_at_offset):
//   the entry's transform was recorded as the identity (no offset pushed
//   onto the HitTestResult stack), so localizing (50, 50) via the recorded
//   transform returned (50, 50) — wrong.
//
// After the fix (hit_test_child_at_layout_offset):
//   the child's paint offset (30, 30) is pushed before recursing, so the
//   recorded global-to-local transform is a translation by (-30, -30).
//   Localizing (50, 50) gives (20, 20) — the correct child-local position.
#[test]
fn harness_align_hit_localizes_to_child_offset() {
    const PARENT_PX: f32 = 100.0;
    const CHILD_PX: f32 = 40.0;
    const HIT_X: f32 = 50.0;
    const HIT_Y: f32 = 50.0;

    let run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::CENTER))
            .child(box_node(RenderColoredBox::red(CHILD_PX, CHILD_PX)).label("child")),
    )
    .with_size(Size::new(px(PARENT_PX), px(PARENT_PX)))
    .run_layout();

    let child_id = run.id("child");

    // Confirm layout placed the child at (30, 30).
    let child_paint_offset = run.offset(child_id);
    assert_eq!(
        child_paint_offset,
        Offset::new(px(30.0), px(30.0)),
        "CENTER alignment must place a 40×40 child in a 100×100 parent at (30, 30)"
    );

    // Retrieve the hit path with recorded transforms.
    let hit_entries = run.hit_with_transforms(HIT_X, HIT_Y);

    let child_transform = hit_entries
        .iter()
        .find(|(id, _)| *id == child_id)
        .map(|(_, t)| *t)
        .unwrap_or_else(|| panic!("child must be in the hit path at ({HIT_X}, {HIT_Y})"));

    let recorded_transform = child_transform.unwrap_or_else(|| {
        panic!(
            "child HitTestEntry must carry a recorded transform \
             (hit_test_child_at_layout_offset pushes the paint offset)"
        )
    });

    // The expected child-local position is global − child_paint_offset.
    let expected_local = Offset::new(
        px(HIT_X - child_paint_offset.dx.get()),
        px(HIT_Y - child_paint_offset.dy.get()),
    );

    let actual_local = localize_hit_point(recorded_transform, HIT_X, HIT_Y)
        .expect("recorded transform must be invertible");

    assert!(
        (actual_local.dx.get() - expected_local.dx.get()).abs() < 0.01
            && (actual_local.dy.get() - expected_local.dy.get()).abs() < 0.01,
        "child-local hit point must equal global − child_paint_offset \
         (got ({:.2}, {:.2}), expected ({:.2}, {:.2}))",
        actual_local.dx.get(),
        actual_local.dy.get(),
        expected_local.dx.get(),
        expected_local.dy.get(),
    );
}

// ============================================================================
// RenderCenter FIX tests (behaviors that changed in this PR)
// ============================================================================

// FIX A: unbounded axis with no factor must shrink-wrap to child size.
// Before: returned Pixels::INFINITY; after: returns child width.
#[test]
fn center_unbounded_shrink_wraps_to_child() {
    // Unconstrained width (max = ∞), bounded height.
    let constraints = BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(200.0));
    let run = RenderTester::mount(
        box_node(RenderCenter::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(constraints)
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()).width,
        px(40.0),
        "unbounded Center with no factor must shrink-wrap to child width"
    );
}

// FIX B: factor > 1.0 must not be clamped.
// Before: with_width_factor(2.0) stored 1.0 (clamped); after: stores 2.0.
#[test]
fn center_width_factor_above_one_not_clamped() {
    let center = RenderCenter::new().with_width_factor(2.0);
    assert_eq!(
        center.width_factor(),
        Some(2.0),
        "width_factor of 2.0 must not be clamped to 1.0"
    );
}

// Anchor for center dry-baseline: bounded layout, verify the offset.dy addition.
// 100×100 parent, 40×40 child → parent_size = 100×100 → free_h = 60 → dy = 30.
// dry_baseline = child_baseline + 30.
#[test]
fn center_dry_baseline_adds_half_free_height() {
    let constraints = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0));
    let mut run = RenderTester::mount(
        box_node(RenderCenter::new()).child(
            box_node(RenderParagraph::new(
                flui_types::typography::TextSpan::new("A"),
                flui_types::typography::TextDirection::Ltr,
            ))
            .label("text"),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    let center_bl = run
        .dry_baseline(run.root(), constraints, TextBaseline::Alphabetic)
        .expect("center with paragraph reports dry baseline");

    let child_constraints = constraints.loosen();
    let child_bl = run
        .dry_baseline(run.id("text"), child_constraints, TextBaseline::Alphabetic)
        .expect("paragraph has dry baseline");

    // parent_size = constrain(100×100); child_size = paragraph dry.
    // free_h = parent.height - child.height.  dy = free_h * 0.5.
    let child_size = run.dry_layout(run.id("text"), child_constraints);
    let free_h = 100.0_f32 - child_size.height.get();
    let expected = child_bl + free_h * 0.5;
    assert!(
        (center_bl - expected).abs() < 0.5,
        "center dry baseline must be child_baseline + free_h/2 (got {center_bl}, expected {expected})"
    );
}

// ============================================================================
// Catalog guard — every exported render type must be exercised above
// ============================================================================

#[test]
fn catalog_covers_every_render_object_name() {
    let source = include_str!("render_object_harness.rs");
    for &type_name in RENDER_OBJECT_TYPES {
        let covered = source
            .split("#[test]")
            .skip(1)
            .any(|chunk| chunk.contains("fn harness_") && chunk.contains(type_name));
        assert!(
            covered,
            "{type_name} must appear in at least one `#[test] fn harness_*` block",
        );
    }
}

#[test]
fn render_object_types_match_exports() {
    let objects_mod = include_str!("../src/objects/mod.rs");
    let mut exported: Vec<&str> = Vec::new();
    let mut in_pub_use = false;
    for line in objects_mod.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub use ") {
            in_pub_use = true;
        }
        if in_pub_use {
            for word in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if word.starts_with("Render") {
                    exported.push(word);
                }
            }
            if trimmed.ends_with(';') {
                in_pub_use = false;
            }
        }
    }
    exported.sort_unstable();
    exported.dedup();
    // Generic clip family root — harness catalog targets the concrete variants.
    exported.retain(|name| *name != "RenderClip");

    let mut catalog: Vec<&str> = RENDER_OBJECT_TYPES.to_vec();
    catalog.sort_unstable();

    assert_eq!(
        catalog, exported,
        "RENDER_OBJECT_TYPES must match `pub use` exports in objects/mod.rs",
    );
}
