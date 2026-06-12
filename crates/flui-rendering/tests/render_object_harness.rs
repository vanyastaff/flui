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
//! | `RenderPadding` | `harness_padding_*` | yes | — | — | yes | queries |
//! | `RenderCenter` | `harness_center_*` | yes | — | — | yes | — |
//! | `RenderAspectRatio` | `harness_aspect_ratio_*` | yes | — | — | yes | — |
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
        assert_has_committed_geometry, assert_has_committed_size, box_node, sliver_node,
    },
};
use flui_types::{
    Alignment, Offset, Point, Rect, Size,
    geometry::px,
    layout::{AxisDirection, BoxFit, StackFit},
    painting::Clip,
    styling::{BoxDecoration, Color},
    typography::{TextDirection, TextSpan},
};

/// Every concrete render-object type exported from `flui_rendering::objects`.
const RENDER_OBJECT_TYPES: &[&str] = &[
    "RenderSizedBox",
    "RenderColoredBox",
    "RenderImage",
    "RenderParagraph",
    "RenderPadding",
    "RenderCenter",
    "RenderAspectRatio",
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
