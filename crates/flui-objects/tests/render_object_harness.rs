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
//! | `RenderCustomPaint` | `harness_custom_paint_*` | yes | yes | yes | yes | order |
//! | `RenderImage` | `harness_image_*` | yes | — | yes | yes | — |
//! | `RenderParagraph` | `harness_paragraph_*` | yes | — | yes | yes | — |
//! | `RenderEditable` | `harness_editable_*` | yes | yes | yes | yes | — |
//! | `RenderPadding` | `harness_padding_*` | yes | yes | — | yes | queries |
//! | `RenderCustomSingleChildLayoutBox` | `harness_custom_single_child_layout_*` | yes | yes | yes | yes | queries, baseline |
//! | `RenderCustomMultiChildLayoutBox` | `harness_custom_multi_child_layout_*` | yes | yes | yes | yes | queries |
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
//! | `RenderShaderMask` | `harness_shader_mask_*` | yes | yes | yes | yes | — |
//! | `RenderBackdropFilter` | `harness_backdrop_filter_*` | yes | yes | yes | yes | — |
//! | `RenderLeaderLayer` | `harness_leader_layer_*` | yes | yes | yes | yes | — |
//! | `RenderFollowerLayer` | `harness_follower_layer_*` | yes | yes | yes | yes | — |
//! | `RenderPhysicalModel` | `harness_physical_model_*` | yes | yes | yes | yes | — |
//! | `RenderPhysicalShape` | `harness_physical_shape_*` | yes | yes | yes | yes | — |
//! | `RenderRepaintBoundary` | `harness_repaint_boundary_*` | yes | — | yes | yes | — |
//! | `RenderSemanticsAnnotations` | `harness_semantics_annotations_*` | yes | — | — | yes | semantics |
//! | `RenderMergeSemantics` | `harness_merge_semantics_*` | yes | — | — | yes | semantics |
//! | `RenderExcludeSemantics` | `harness_exclude_semantics_*` | yes | — | — | yes | semantics |
//! | `RenderMetaData` | `harness_metadata_*` | yes | — | — | yes | — |
//! | `RenderFlex` | `harness_flex_*` | yes | — | — | yes | queries, baseline |
//! | `RenderStack` | `harness_stack_*` | yes | yes | — | yes | queries |
//! | `RenderIndexedStack` | `harness_indexed_stack_*` | yes | yes | yes | yes | baseline |
//! | `RenderListBody` | `harness_list_body_*` | yes | yes | — | yes | dry baseline |
//! | `RenderFlow` | `harness_flow_*` | yes | yes | yes | yes | order |
//! | `RenderTable` | `harness_table_*` | yes | yes | yes | yes | column widths |
//! | `RenderAbsorbPointer` | `harness_absorb_pointer_*` | yes | yes | — | yes | — |
//! | `RenderIgnorePointer` | `harness_ignore_pointer_*` | yes | yes | — | yes | — |
//! | `RenderListener` | `harness_listener_*` | yes | yes | — | yes | — |
//! | `RenderMouseRegion` | `harness_mouse_region_*` | yes | yes | — | yes | cursor/annotation |
//! | `RenderSliverFixedExtentList` | `harness_sliver_fixed_extent_list_*` | yes | — | — | yes | — |
//! | `RenderSliverGrid` | `harness_render_sliver_grid_*` | yes | — | — | yes | — |
//! | `RenderSliverGridLazy` | `harness_render_sliver_grid_lazy_*` | yes | — | — | yes | — |
//! | `RenderSliverPadding` | `harness_sliver_padding_*` | yes | — | — | yes | — |
//! | `RenderSliverToBoxAdapter` | `harness_sliver_to_box_adapter_*` | yes | — | — | yes | — |
//! | `RenderSliverFillViewport` | `harness_sliver_fill_viewport_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemaining` | `harness_sliver_fill_remaining_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemainingAndOverscroll` | `harness_sliver_fill_remaining_and_overscroll_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemainingWithScrollable` | `harness_sliver_fill_remaining_with_scrollable_*` | yes | — | — | yes | — |
//! | `RenderSliverIgnorePointer` | `harness_sliver_ignore_pointer_*` | yes | yes | — | yes | — |
//! | `RenderSliverList` | `harness_sliver_list_*` | yes | — | — | yes | — |
//! | `RenderSliverListLazy` | `harness_sliver_list_lazy_*` | yes | — | — | yes | — |
//! | `RenderSliverOffstage` | `harness_sliver_offstage_*` | yes | — | — | yes | — |
//! | `RenderSliverOpacity` | `harness_sliver_opacity_*` | yes | — | yes | yes | compositing |
//! | `RenderViewport` | `harness_viewport_*` | yes | — | — | yes | — |
//! | `RenderShrinkWrappingViewport` | `harness_shrink_wrapping_viewport_*` | yes | — | — | yes | — |
//! | `RenderWrap` | `harness_render_wrap_*` | yes | yes | — | yes | — |
//! | `RenderIntrinsicWidth` | `harness_intrinsic_width_*` | yes | — | — | yes | — |
//! | `RenderIntrinsicHeight` | `harness_intrinsic_height_*` | yes | — | — | yes | — |
//! | `RenderConstrainedOverflowBox` | `harness_constrained_overflow_box_*` | yes | — | — | yes | — |
//! | `RenderSizedOverflowBox` | `harness_sized_overflow_box_*` | yes | — | — | yes | — |
//! | `RenderRotatedBox` | `harness_rotated_box_*` | yes | yes | — | yes | — |
//! | `RenderAnimatedSize` | `harness_render_animated_size_*` | yes | — | yes | yes | state machine |
//! | `RenderSliverScrollingPersistentHeader` | `harness_sliver_persistent_header_scrolling_*` | yes | — | — | — | — |
//! | `RenderSliverPinnedPersistentHeader` | `harness_sliver_persistent_header_pinned_*` | yes | — | — | — | viewport wiring |
//! | `RenderSliverFloatingPersistentHeader` | `harness_sliver_persistent_header_floating_*` | yes | — | — | — | state machine |
//! | `RenderSliverFloatingPinnedPersistentHeader` | `harness_sliver_persistent_header_floating_pinned_*` | yes | — | — | — | state machine |
//!
//! [`catalog_covers_every_render_object_name`] guards the table: every row's
//! type string must appear in this file so a missing harness test fails CI.

// Single-binary consolidation (`autotests = false` in `Cargo.toml`): the
// snapshot dogfood suite compiles as a module of this target instead of
// linking the full dependency stack a second time. Its insta snapshots are
// prefixed `render_object_harness__harness_snapshot__` accordingly.
#[path = "harness_snapshot.rs"]
mod harness_snapshot;

use std::{
    any::Any,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use flui_animation::curve::ArcCurve;
use flui_animation::{AnimationController, Curves, Scheduler};
use flui_interaction::MouseTracker;
use flui_objects::*;
use flui_painting::{Canvas, Paint};
use flui_rendering::{
    constraints::BoxConstraints,
    delegates::{
        CustomPainter, FlowDelegate, FlowPaintingContext, MultiChildLayoutContext,
        MultiChildLayoutDelegate, SingleChildLayoutDelegate,
        SliverGridDelegateWithFixedCrossAxisCount,
    },
    hit_testing::{
        CursorIcon, EventPropagation, HitTestBehavior, HitTestResult, InputEvent,
        MouseEnterCallback, MouseExitCallback, MouseHoverCallback, PointerEventHandler,
    },
    layer::LayerLink,
    parent_data::{
        FlexParentData, MultiChildLayoutParentData, SliverMultiBoxAdaptorParentData,
        StackParentData, TableCellParentData,
    },
    semantics::SemanticsProperties,
    testing::{
        BoxQueryRun, DrawKind, ParentDataSeed, Probe, RenderTester, TreeNode,
        assert_descendant_properties, assert_has_committed_geometry, assert_has_committed_size,
        box_node, localize_hit_point, sliver_node,
    },
    traits::{RenderBox, TextBaseline},
    view::{ScrollDirection, ScrollableViewportOffset},
};
use flui_types::{
    Alignment, EdgeInsets, Matrix4, Offset, Point, Rect, Size,
    geometry::px,
    layout::{
        AxisDirection, BoxFit, BoxShape, StackFit, TableCellVerticalAlignment, TableColumnWidth,
    },
    painting::{BlendMode, Clip, ImageFilter, Path, Shader},
    styling::{
        BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, Color, TableBorder,
    },
    typography::{TextDirection, TextSpan},
};

/// Every concrete render-object type exported from `flui_objects`.
const RENDER_OBJECT_TYPES: &[&str] = &[
    "RenderAlign",
    "RenderSizedBox",
    "RenderColoredBox",
    "RenderCustomPaint",
    "RenderImage",
    "RenderParagraph",
    "RenderEditable",
    "RenderPadding",
    "RenderCustomSingleChildLayoutBox",
    "RenderCustomMultiChildLayoutBox",
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
    "RenderShaderMask",
    "RenderBackdropFilter",
    "RenderLeaderLayer",
    "RenderFollowerLayer",
    "RenderPhysicalModel",
    "RenderPhysicalShape",
    "RenderRepaintBoundary",
    "RenderSemanticsAnnotations",
    "RenderMergeSemantics",
    "RenderExcludeSemantics",
    "RenderMetaData",
    "RenderFlex",
    "RenderStack",
    "RenderIndexedStack",
    "RenderListBody",
    "RenderFlow",
    "RenderTable",
    "RenderAbsorbPointer",
    "RenderIgnorePointer",
    "RenderListener",
    "RenderMouseRegion",
    "RenderSliverFixedExtentList",
    "RenderSliverGrid",
    "RenderSliverGridLazy",
    "RenderSliverPadding",
    "RenderSliverToBoxAdapter",
    "RenderSliverFillViewport",
    "RenderSliverFillRemaining",
    "RenderSliverFillRemainingAndOverscroll",
    "RenderSliverFillRemainingWithScrollable",
    "RenderSliverIgnorePointer",
    "RenderSliverList",
    "RenderSliverListLazy",
    "RenderSliverOffstage",
    "RenderSliverOpacity",
    "RenderViewport",
    "RenderShrinkWrappingViewport",
    "RenderWrap",
    "RenderIntrinsicWidth",
    "RenderIntrinsicHeight",
    "RenderConstrainedOverflowBox",
    "RenderSizedOverflowBox",
    "RenderRotatedBox",
    "RenderAnimatedSize",
    "RenderSliverScrollingPersistentHeader",
    "RenderSliverPinnedPersistentHeader",
    "RenderSliverFloatingPersistentHeader",
    "RenderSliverFloatingPinnedPersistentHeader",
];

fn loose(max: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(max), px(0.0), px(max))
}

#[derive(Debug)]
struct HarnessPainter {
    color: Color,
    hit: Option<bool>,
}

impl HarnessPainter {
    fn new(color: Color) -> Self {
        Self { color, hit: None }
    }

    fn with_hit(mut self, hit: Option<bool>) -> Self {
        self.hit = hit;
        self
    }
}

impl CustomPainter for HarnessPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        canvas.draw_rect(
            Rect::from_origin_size(Point::ZERO, size),
            &Paint::fill(self.color),
        );
    }

    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.color != self.color || old.hit != self.hit)
    }

    fn hit_test(&self, _position: Offset) -> Option<bool> {
        self.hit
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn custom_painter(color: Color) -> Arc<dyn CustomPainter> {
    Arc::new(HarnessPainter::new(color))
}

fn custom_hit_painter(color: Color, hit: Option<bool>) -> Arc<dyn CustomPainter> {
    Arc::new(HarnessPainter::new(color).with_hit(hit))
}

#[derive(Debug)]
struct HarnessSingleChildLayoutDelegate {
    size: Size,
    child_constraints: BoxConstraints,
    offset: Offset,
}

impl HarnessSingleChildLayoutDelegate {
    fn new(size: Size, child_constraints: BoxConstraints, offset: Offset) -> Self {
        Self {
            size,
            child_constraints,
            offset,
        }
    }
}

impl SingleChildLayoutDelegate for HarnessSingleChildLayoutDelegate {
    fn get_size(&self, _constraints: BoxConstraints) -> Size {
        self.size
    }

    fn get_constraints_for_child(&self, _constraints: BoxConstraints) -> BoxConstraints {
        self.child_constraints
    }

    fn get_position_for_child(&self, _size: Size, _child_size: Size) -> Offset {
        self.offset
    }

    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| {
                self.size != old.size
                    || self.child_constraints != old.child_constraints
                    || self.offset != old.offset
            })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn custom_single_child_delegate(
    size: Size,
    child_constraints: BoxConstraints,
    offset: Offset,
) -> Arc<dyn SingleChildLayoutDelegate> {
    Arc::new(HarnessSingleChildLayoutDelegate::new(
        size,
        child_constraints,
        offset,
    ))
}

#[derive(Debug)]
struct HarnessMultiChildLayoutDelegate {
    size: Size,
}

impl HarnessMultiChildLayoutDelegate {
    fn new(size: Size) -> Self {
        Self { size }
    }
}

impl MultiChildLayoutDelegate for HarnessMultiChildLayoutDelegate {
    fn get_size(&self, _constraints: BoxConstraints) -> Size {
        self.size
    }

    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
        if context.has_child("header") {
            context.layout_child(
                "header",
                BoxConstraints::tight(Size::new(size.width, px(20.0))),
            );
            context.position_child("header", Offset::ZERO);
        }
        if context.has_child("body") {
            context.layout_child("body", BoxConstraints::tight(Size::new(px(70.0), px(30.0))));
            context.position_child("body", Offset::new(px(10.0), px(25.0)));
        }
    }

    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| self.size != old.size)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn custom_multi_child_delegate(size: Size) -> Arc<dyn MultiChildLayoutDelegate> {
    Arc::new(HarnessMultiChildLayoutDelegate::new(size))
}

fn multi_child_layout_parent_data(id: &str) -> MultiChildLayoutParentData {
    MultiChildLayoutParentData::zero().with_id(id.to_owned())
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

fn shrink_wrapping_viewport(sliver: TreeNode) -> TreeNode {
    box_node(RenderShrinkWrappingViewport::new(
        AxisDirection::TopToBottom,
    ))
    .label("shrink_viewport")
    .child(sliver)
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
fn harness_custom_paint_childless_uses_preferred_size_and_paints() {
    let run = RenderTester::mount(box_node(RenderCustomPaint::new(
        Some(custom_painter(Color::RED)),
        None,
        Size::new(px(30.0), px(20.0)),
    )))
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(30.0), px(20.0)));
    assert!(run.hit_first(10.0, 10.0).is_some());
    assert!(
        run.display_commands()
            .iter()
            .any(|cmd| cmd.line.contains("#FF0000FF")),
        "background painter must emit a red draw command",
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderCustomPaint",
        &["preferred_size", "has_painter"],
    );
}

#[test]
fn harness_custom_paint_orders_background_child_foreground() {
    let run = RenderTester::mount(
        box_node(RenderCustomPaint::new(
            Some(custom_painter(Color::RED)),
            Some(custom_painter(Color::BLUE)),
            Size::ZERO,
        ))
        .child(box_node(RenderColoredBox::green(20.0, 10.0))),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let painted = run
        .display_commands()
        .into_iter()
        .map(|cmd| cmd.line)
        .collect::<Vec<_>>();
    let rects = painted
        .iter()
        .filter(|line| line.contains("DrawRect"))
        .collect::<Vec<_>>();
    assert_eq!(
        rects.len(),
        3,
        "expected background painter, child, foreground painter; commands:\n{}",
        painted.join("\n"),
    );
    assert!(
        rects[0].contains("#FF0000FF")
            && rects[1].contains("#00FF00FF")
            && rects[2].contains("#0000FFFF"),
        "paint order must be background red -> child green -> foreground blue; commands:\n{}",
        painted.join("\n"),
    );
}

#[test]
fn harness_custom_paint_foreground_hit_test_wins() {
    let run = RenderTester::mount(box_node(RenderCustomPaint::new(
        Some(custom_hit_painter(Color::RED, Some(false))),
        Some(custom_hit_painter(Color::BLUE, Some(true))),
        Size::new(px(30.0), px(20.0)),
    )))
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.hit_first(10.0, 10.0), Some(run.root()));
}

#[test]
fn harness_listener_passes_layout_through_and_attaches_handler() {
    // A no-op handler — the harness verifies it reaches the hit entry (the new
    // pipeline wiring); that it FIRES end-to-end is covered by the Listener
    // widget's dispatch test.
    let handler: PointerEventHandler = Arc::new(|_event| EventPropagation::Continue);
    let run = RenderTester::mount(
        // DeferToChild over a hittable ColoredBox: the listener registers when
        // the child is hit.
        box_node(RenderListener::new(handler, HitTestBehavior::DeferToChild))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    // Layout is a pure pass-through: the listener sizes to its 40×40 child.
    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));

    // A pointer landing on the child hits the listener (it registers itself in
    // the leaf-first path alongside its child), and its hit entry carries the
    // handler the pipeline attached from `pointer_event_handler()`.
    assert!(
        run.hit(20.0, 20.0).contains(&run.root()),
        "the listener registers itself in the hit path",
    );
    let mut result = HitTestResult::new();
    run.pipeline()
        .hit_test(Offset::new(px(20.0), px(20.0)), &mut result);
    assert!(
        result.path().iter().any(|entry| entry.handler.is_some()),
        "the listener's hit entry must carry a pointer handler:\n{}",
        run.diagnostics(),
    );
}

#[test]
fn harness_listener_childless_fills_parent() {
    let handler: PointerEventHandler = Arc::new(|_event| EventPropagation::Continue);
    let constraints = loose(200.0);
    let mut run = RenderTester::mount(box_node(RenderListener::new(
        handler,
        HitTestBehavior::Opaque,
    )))
    .with_constraints(constraints)
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(200.0), px(200.0)),
        "childless RenderListener must use constraints.biggest like Flutter's RenderPointerListener",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        Size::new(px(200.0), px(200.0)),
        "childless RenderListener dry layout must mirror computeSizeForNoChild",
    );
}

#[test]
fn harness_listener_translucent_adds_entry_without_blocking_lower_sibling() {
    let handler: PointerEventHandler = Arc::new(|_event| EventPropagation::Continue);
    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("bottom"))
            .child(
                box_node(RenderListener::new(handler, HitTestBehavior::Translucent))
                    .label("top_listener"),
            ),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(
        run.hit(20.0, 20.0),
        vec![run.id("top_listener"), run.id("bottom"), run.root()],
        "translucent RenderListener must contribute a hit entry without \
         stopping siblings visually behind it",
    );
}

#[test]
fn harness_mouse_region_childless_fills_parent_and_self_describes() {
    let run = RenderTester::mount(box_node(RenderMouseRegion::new()))
        .with_constraints(BoxConstraints::tight(Size::new(px(80.0), px(40.0))))
        .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(80.0), px(40.0)),
        "childless RenderMouseRegion must use constraints.biggest like Flutter's computeSizeForNoChild",
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderMouseRegion",
        &["cursor", "valid_for_mouse_tracker", "opaque", "behavior"],
    );
}

#[test]
fn harness_mouse_region_hit_entry_carries_cursor_and_annotation() {
    let enters = Arc::new(AtomicUsize::new(0));
    let enter_counter = Arc::clone(&enters);
    let on_enter: MouseEnterCallback = Arc::new(move |_device, _position| {
        enter_counter.fetch_add(1, Ordering::SeqCst);
    });

    let mut region = RenderMouseRegion::new();
    region.set_cursor(CursorIcon::Pointer);
    region.set_on_enter(Some(on_enter));

    let run = RenderTester::mount(box_node(region))
        .with_constraints(BoxConstraints::tight(Size::new(px(60.0), px(30.0))))
        .run_frame();

    let mut result = HitTestResult::new();
    run.pipeline()
        .hit_test(Offset::new(px(10.0), px(10.0)), &mut result);

    let entry = result
        .path()
        .iter()
        .find(|entry| entry.target == run.root())
        .expect("mouse region must contribute a hit-test entry");
    assert_eq!(
        entry.cursor,
        CursorIcon::Pointer,
        "mouse region cursor must be copied into the hit-test entry",
    );
    let annotation = entry
        .mouse_annotation
        .as_ref()
        .expect("mouse region must contribute MouseTrackerAnnotation");
    assert_eq!(annotation.region_id, run.root());
    assert!(annotation.on_enter.is_some());
}

#[test]
fn harness_mouse_region_opaque_false_adds_entry_without_blocking_lower_sibling() {
    let mut region = RenderMouseRegion::new();
    region.set_opaque(false);

    let run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("bottom"))
            .child(box_node(region).label("top_region")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(
        run.hit(20.0, 20.0),
        vec![run.id("top_region"), run.id("bottom"), run.root()],
        "MouseRegion opaque=false must contribute its hit entry without \
         suppressing mouse regions or targets behind it",
    );
}

#[test]
fn harness_mouse_region_hover_dispatches_move_event_and_tracker_enter_exit() {
    let hovers = Arc::new(AtomicUsize::new(0));
    let hover_counter = Arc::clone(&hovers);
    let on_hover: MouseHoverCallback = Arc::new(move |_device, _position| {
        hover_counter.fetch_add(1, Ordering::SeqCst);
    });
    let exits = Arc::new(AtomicUsize::new(0));
    let exit_counter = Arc::clone(&exits);
    let on_exit: MouseExitCallback = Arc::new(move |_device, _position| {
        exit_counter.fetch_add(1, Ordering::SeqCst);
    });

    let mut region = RenderMouseRegion::new();
    region.set_on_hover(Some(on_hover));
    region.set_on_exit(Some(on_exit));

    let run = RenderTester::mount(box_node(region))
        .with_constraints(BoxConstraints::tight(Size::new(px(60.0), px(30.0))))
        .run_frame();

    let mut inside = HitTestResult::new();
    let inside_position = Offset::new(px(10.0), px(10.0));
    run.pipeline().hit_test(inside_position, &mut inside);
    inside.dispatch(&flui_interaction::events::make_move_event(
        inside_position,
        flui_interaction::events::PointerType::Mouse,
    ));
    assert_eq!(
        hovers.load(Ordering::SeqCst),
        1,
        "PointerEvent::Move dispatch should invoke RenderMouseRegion's hover handler",
    );

    let tracker = MouseTracker::new();
    tracker.update_with_event(
        &InputEvent::DeviceAdded {
            device_id: 0,
            pointer_type: flui_interaction::events::PointerType::Mouse,
        },
        &HitTestResult::new(),
    );
    tracker.update_with_event(
        &InputEvent::Pointer(flui_interaction::events::make_move_event(
            inside_position,
            flui_interaction::events::PointerType::Mouse,
        )),
        &inside,
    );
    assert_eq!(
        hovers.load(Ordering::SeqCst),
        1,
        "first tracker update is an enter, not a hover",
    );

    tracker.update_with_event(
        &InputEvent::Pointer(flui_interaction::events::make_move_event(
            inside_position,
            flui_interaction::events::PointerType::Mouse,
        )),
        &inside,
    );
    assert_eq!(
        hovers.load(Ordering::SeqCst),
        2,
        "second tracker update over the same region is a hover",
    );

    let mut outside = HitTestResult::new();
    let outside_position = Offset::new(px(80.0), px(10.0));
    run.pipeline().hit_test(outside_position, &mut outside);
    tracker.update_with_event(
        &InputEvent::Pointer(flui_interaction::events::make_move_event(
            outside_position,
            flui_interaction::events::PointerType::Mouse,
        )),
        &outside,
    );
    assert_eq!(
        exits.load(Ordering::SeqCst),
        1,
        "tracker must retain the prior annotation long enough to fire exit",
    );
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

#[test]
fn harness_editable_lays_out_and_paints_collapsed_caret() {
    let run = RenderTester::mount(box_node(
        RenderEditable::new(TextSpan::new("edit me"), TextDirection::Ltr)
            .with_caret_byte_offset(7)
            .with_show_caret(true)
            .with_caret_width(2.0)
            .with_caret_height(18.0),
    ))
    .with_constraints(loose(160.0))
    .run_frame();

    let size = run.box_geometry(run.root());
    assert_eq!(size.width, px(160.0));
    assert!(size.height.get() >= 18.0);

    let commands = run.display_commands();
    assert!(
        commands.iter().any(
            |command| command.line.contains("DrawTextSpan") && command.line.contains("edit me")
        ),
        "RenderEditable must paint its text span; commands: {commands:#?}"
    );
    assert!(
        commands
            .iter()
            .any(|command| command.line.contains("DrawRect")),
        "RenderEditable must paint the collapsed caret; commands: {commands:#?}"
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderEditable",
        &["text", "caret_byte_offset", "force_line"],
    );
}

#[test]
fn harness_editable_hit_tests_self() {
    let run = RenderTester::mount(box_node(RenderEditable::new(
        TextSpan::new("hit me"),
        TextDirection::Ltr,
    )))
    .with_size(Size::new(px(120.0), px(40.0)))
    .run_layout();

    assert_eq!(run.hit_first(10.0, 10.0), Some(run.root()));
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
fn harness_custom_single_child_layout_positions_child_with_delegate() {
    let delegate = custom_single_child_delegate(
        Size::new(px(120.0), px(80.0)),
        BoxConstraints::tight(Size::new(px(30.0), px(20.0))),
        Offset::new(px(70.0), px(50.0)),
    );
    let run = RenderTester::mount(
        box_node(RenderCustomSingleChildLayoutBox::new(delegate))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(120.0), px(80.0)),
        "parent size must come from delegate.get_size constrained by incoming constraints",
    );
    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(px(30.0), px(20.0)),
        "child must be laid out under delegate.get_constraints_for_child",
    );
    assert_eq!(run.offset(run.id("child")), Offset::new(px(70.0), px(50.0)));
    assert_eq!(run.hit_first(75.0, 55.0), Some(run.id("child")));
    assert!(run.hit(10.0, 10.0).is_empty());
    assert!(
        run.display_commands()
            .iter()
            .any(|cmd| cmd.line.contains("#FF0000FF")),
        "delegated child must still paint through the parent",
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderCustomSingleChildLayoutBox",
        &["delegate"],
    );
}

#[test]
fn harness_custom_single_child_layout_queries_use_delegate_size_formula() {
    let constraints = loose(200.0);
    let delegate = custom_single_child_delegate(
        Size::new(px(120.0), px(80.0)),
        BoxConstraints::loose(Size::new(px(50.0), px(40.0))),
        Offset::new(px(0.0), px(0.0)),
    );
    let mut run = RenderTester::mount(box_node(RenderCustomSingleChildLayoutBox::new(delegate)))
        .with_constraints(constraints)
        .run_layout();

    let root = run.root();
    assert_eq!(
        run.dry_layout(root, constraints),
        Size::new(px(120.0), px(80.0)),
        "compute_dry_layout must use the same constrained delegate size as perform_layout",
    );
    assert_eq!(run.min_intrinsic_width(root, 50.0), 120.0);
    assert_eq!(run.max_intrinsic_width(root, 50.0), 120.0);
    assert_eq!(run.min_intrinsic_height(root, 90.0), 80.0);
    assert_eq!(run.max_intrinsic_height(root, 90.0), 80.0);
}

#[test]
fn harness_custom_single_child_layout_dry_baseline_adds_delegate_offset() {
    let constraints = loose(200.0);
    let child_constraints = BoxConstraints::loose(Size::new(px(100.0), px(40.0)));
    let delegate = custom_single_child_delegate(
        Size::new(px(120.0), px(80.0)),
        child_constraints,
        Offset::new(px(5.0), px(30.0)),
    );
    let mut run = RenderTester::mount(
        box_node(RenderCustomSingleChildLayoutBox::new(delegate)).child(
            box_node(RenderParagraph::new(
                TextSpan::new("Ag"),
                TextDirection::Ltr,
            ))
            .label("text"),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    let child_baseline = run
        .dry_baseline(run.id("text"), child_constraints, TextBaseline::Alphabetic)
        .expect("paragraph child reports a dry baseline");
    let custom_baseline = run
        .dry_baseline(run.root(), constraints, TextBaseline::Alphabetic)
        .expect("paragraph child reports a dry baseline");
    assert!(
        custom_baseline > child_baseline,
        "custom dry baseline must include delegate dy offset; child={child_baseline}, custom={custom_baseline}",
    );
    assert_eq!(
        custom_baseline - child_baseline,
        30.0,
        "delegate offset dy must be added to the child's dry baseline",
    );
}

#[test]
fn harness_custom_single_child_layout_actual_baseline_adds_delegate_offset() {
    let delegate = custom_single_child_delegate(
        Size::new(px(120.0), px(80.0)),
        BoxConstraints::loose(Size::new(px(100.0), px(40.0))),
        Offset::new(px(5.0), px(30.0)),
    );
    let run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(100.0))).child(
            box_node(RenderCustomSingleChildLayoutBox::new(delegate))
                .label("custom")
                .child(
                    box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(10.0)))
                        .child(box_node(RenderColoredBox::red(20.0, 20.0))),
                ),
        ),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.offset(run.id("custom")).dy,
        px(60.0),
        "outer baseline should place custom at 100 - (child baseline 10 + delegate dy 30)",
    );
}

#[test]
fn harness_custom_multi_child_layout_positions_children_by_layout_id() {
    let delegate = custom_multi_child_delegate(Size::new(px(120.0), px(90.0)));
    let run = RenderTester::mount(
        box_node(RenderCustomMultiChildLayoutBox::new(delegate))
            .child(
                box_node(RenderColoredBox::red(10.0, 10.0))
                    .with_multi_child_layout_parent_data(multi_child_layout_parent_data("header"))
                    .label("header"),
            )
            .child(
                box_node(RenderColoredBox::green(10.0, 10.0))
                    .with_multi_child_layout_parent_data(multi_child_layout_parent_data("body"))
                    .label("body"),
            ),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(120.0), px(90.0)),
        "parent size must come from delegate.get_size constrained by incoming constraints",
    );
    assert_eq!(
        run.box_geometry(run.id("header")),
        Size::new(px(120.0), px(20.0)),
        "header receives tight constraints from the delegate",
    );
    assert_eq!(
        run.box_geometry(run.id("body")),
        Size::new(px(70.0), px(30.0)),
        "body receives different tight constraints from the delegate",
    );
    assert_eq!(run.offset(run.id("header")), Offset::ZERO);
    assert_eq!(run.offset(run.id("body")), Offset::new(px(10.0), px(25.0)));
    assert_eq!(run.hit_first(15.0, 30.0), Some(run.id("body")));
    assert_eq!(run.hit_first(5.0, 5.0), Some(run.id("header")));
    assert!(
        run.display_commands()
            .iter()
            .any(|cmd| cmd.line.contains("#00FF00FF")),
        "delegated body child must still paint through the parent",
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderCustomMultiChildLayoutBox",
        &["delegate"],
    );
}

#[test]
fn harness_custom_multi_child_layout_queries_use_delegate_size_formula() {
    let constraints = loose(200.0);
    let delegate = custom_multi_child_delegate(Size::new(px(120.0), px(90.0)));
    let mut run = RenderTester::mount(box_node(RenderCustomMultiChildLayoutBox::new(delegate)))
        .with_constraints(constraints)
        .run_layout();

    let root = run.root();
    assert_eq!(
        run.dry_layout(root, constraints),
        Size::new(px(120.0), px(90.0)),
        "compute_dry_layout must use the same constrained delegate size as perform_layout",
    );
    assert_eq!(run.min_intrinsic_width(root, 50.0), 120.0);
    assert_eq!(run.max_intrinsic_width(root, 50.0), 120.0);
    assert_eq!(run.min_intrinsic_height(root, 50.0), 90.0);
    assert_eq!(run.max_intrinsic_height(root, 50.0), 90.0);
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
        .map_or_else(
            || panic!("child must be hit at ({HIT_X}, {HIT_Y})"),
            |(_, t)| *t,
        );

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
fn harness_baseline_loosens_child_constraints() {
    // Flutter RenderBaseline.performLayout lays the child out under
    // `constraints.loosen()`, so a tight incoming axis does not stretch a small
    // child. Tight width 100 with a 20×20 child → child stays 20×20 (before the
    // fix the un-loosened tight width forced it to 100×20).
    let run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(50.0)))
            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(
        px(100.0),
        px(100.0),
        px(0.0),
        px(f32::INFINITY),
    ))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(px(20.0), px(20.0)),
        "tight incoming width must be loosened so the child keeps its 20×20 size",
    );
}

#[test]
fn harness_baseline_dry_baseline_handles_cross_kind_query() {
    // The box's baseline type is Alphabetic; a parent querying a DIFFERENT kind
    // (Ideographic) must still get a value — Flutter computes
    // `baseline_offset + child(requested) - child(own)`. The prior code returned
    // None for any cross-kind dry-baseline query.
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

    let root = run.root();
    let constraints = BoxConstraints::loose(Size::new(px(200.0), px(100.0)));
    assert!(
        run.dry_baseline(root, constraints, TextBaseline::Ideographic)
            .is_some(),
        "cross-kind dry baseline query must return a value, not None",
    );
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
        box_node(RenderAspectRatio::new(AspectRatioFactor::new_unchecked(
            2.0,
        )))
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
        box_node(RenderAspectRatio::new(AspectRatioFactor::new_unchecked(
            2.0,
        )))
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
fn harness_semantics_annotations_builds_semantics_node_and_passes_layout() {
    let mut properties = SemanticsProperties::new()
        .with_label("Submit")
        .with_button(true)
        .with_enabled(true);
    properties.toggled = Some(false);

    let run = RenderTester::mount(
        box_node(RenderSemanticsAnnotations::new(properties).with_container(true))
            .label("semantics")
            .child(box_node(RenderSizedBox::new(
                Some(px(40.0)),
                Some(px(20.0)),
            ))),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    assert_eq!(
        run.box_geometry(run.id("semantics")),
        Size::new(px(40.0), px(20.0)),
    );
    assert_eq!(
        run.property(run.id("semantics"), "container"),
        Some("container".to_string()),
    );

    let owner = run.semantics_owner().expect("semantics enabled");
    let root_id = owner.root().expect("root semantics node");
    let node = owner.get(root_id).expect("root id must resolve");

    assert_eq!(owner.tree().len(), 1);
    assert_eq!(node.label(), Some("Submit"));
    assert!(node.config().is_button());
    assert_eq!(node.config().is_enabled(), Some(true));
    assert_eq!(node.config().is_toggled(), Some(false));
}

#[test]
fn harness_merge_semantics_collapses_descendant_boundaries() {
    let alpha = SemanticsProperties::new().with_label("Alpha");
    let beta = SemanticsProperties::new()
        .with_label("Beta")
        .with_button(true);

    let run = RenderTester::mount(
        box_node(RenderMergeSemantics::default())
            .label("merge")
            .child(box_node(RenderSemanticsAnnotations::new(alpha)))
            .child(box_node(
                RenderSemanticsAnnotations::new(beta).with_container(true),
            )),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");
    let root_id = owner.root().expect("merge semantics root");
    let node = owner.get(root_id).expect("root id must resolve");

    assert_eq!(
        owner.tree().len(),
        1,
        "RenderMergeSemantics must collapse both descendants into one node",
    );
    assert!(node.children().is_empty());
    assert!(node.config().is_button());
    let label = node.label().expect("merged label");
    assert!(label.contains("Alpha") && label.contains("Beta"));
}

#[test]
fn harness_exclude_semantics_drops_descendant_content_but_keeps_layout() {
    let hidden = SemanticsProperties::new().with_label("Hidden");

    let run = RenderTester::mount(
        box_node(RenderExcludeSemantics::default())
            .label("exclude")
            .child(
                box_node(RenderSemanticsAnnotations::new(hidden)).child(box_node(
                    RenderSizedBox::new(Some(px(24.0)), Some(px(16.0))),
                )),
            ),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    assert_eq!(
        run.box_geometry(run.id("exclude")),
        Size::new(px(24.0), px(16.0)),
    );
    assert_eq!(
        run.property(run.id("exclude"), "excluding"),
        Some("excluding".to_string()),
    );

    let owner = run.semantics_owner().expect("semantics enabled");
    let root_id = owner.root().expect("root semantics node");
    let node = owner.get(root_id).expect("root id must resolve");

    assert_eq!(owner.tree().len(), 1);
    assert!(
        node.label().is_none(),
        "excluded descendant label must not merge into the root semantics node",
    );
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
fn harness_fitted_box_preserves_aspect_ratio_when_sizing_box() {
    // child 100×50 (aspect 2.0); under maxW=60 with loose height, Contain sizes
    // the BOX preserving aspect → (60, 30), not a plain clamp (60, 50). Flutter
    // uses constrainSizeAndAttemptToPreserveAspectRatio. Before the fix
    // perform_layout used a plain constrain → (60, 50), disagreeing with
    // compute_dry_layout.
    let run = RenderTester::mount(
        box_node(RenderFittedBox::new(
            BoxFit::Contain,
            Alignment::CENTER,
            Clip::None,
        ))
        .child(box_node(RenderColoredBox::red(100.0, 50.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(
        px(0.0),
        px(60.0),
        px(0.0),
        px(f32::INFINITY),
    ))
    .run_layout();
    assert_eq!(run.box_geometry(run.root()), Size::new(px(60.0), px(30.0)));
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
fn harness_fractional_translation_hits_shifted_child_outside_own_bounds() {
    // translation (1.0, 0.0) shifts the 40×40 child to visual x ∈ [40, 80). A
    // pointer at (50, 20) is OUTSIDE the box's own [0,40) bounds but inside the
    // shifted child → must hit (child-local (10, 20)). Flutter's
    // RenderFractionalTranslation.hitTest skips the own-bounds check; the prior
    // `is_within_own_size` gate returned no hit here.
    let run = RenderTester::mount(
        box_node(RenderFractionalTranslation::translated(
            TranslationFraction::new(1.0, 0.0),
        ))
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.hit_first(50.0, 20.0), Some(run.id("child")));
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
fn harness_decorated_box_hit_tests_child_before_decoration_shape() {
    // Flutter tests the child before hitTestSelf: a rounded decoration excludes
    // the rect's corners from its own shape, but a child hittable in a cut
    // corner must still hit. (2,2) is inside the 100×100 box yet outside the
    // r=50 rounded shape; before the fix the decoration shape was tested first
    // and rejected it.
    let run = RenderTester::mount(
        box_node(RenderDecoratedBox::new(
            BoxDecoration::with_color(Color::RED)
                .set_border_radius(Some(BorderRadius::circular(px(50.0)))),
        ))
        .child(box_node(RenderColoredBox::blue(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(2.0, 2.0), Some(run.id("child")));
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

// ============================================================================
// RenderShaderMask / RenderBackdropFilter
// ============================================================================

/// A trivial shader callback for tests that don't care about the produced
/// shader itself, only that the mask machinery ran.
fn solid_white_shader(_bounds: Rect) -> Shader {
    Shader::solid(Color::WHITE)
}

#[test]
fn harness_shader_mask_layout_passes_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderShaderMask::new(solid_white_shader))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_shader_mask_no_child_paints_nothing() {
    let run = RenderTester::mount(box_node(RenderShaderMask::new(solid_white_shader)))
        .with_constraints(loose(200.0))
        .run_frame();

    assert!(
        !run.structure().contains(&"ShaderMask"),
        "a childless ShaderMask must not push a layer (oracle: layer = null): {:?}",
        run.structure(),
    );
}

#[test]
fn harness_shader_mask_paints_with_shader_mask_layer() {
    let run = RenderTester::mount(
        box_node(RenderShaderMask::new(solid_white_shader))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(run.painted());
    assert!(run.structure().contains(&"ShaderMask"));
}

#[test]
fn harness_shader_mask_callback_receives_local_not_offset_rect() {
    // Regression test for the highest-risk trap in the design research
    // plan (§4.3): the shader callback must see the node's LOCAL bounds
    // rect even when the ShaderMask itself sits at a non-zero origin
    // within its parent — nesting under RenderPadding gives the
    // ShaderMask a non-zero accumulated origin (20, 20) so a bug that
    // passed the origin-shifted (global) rect to the callback instead of
    // the local one would be caught here.
    let captured: Arc<std::sync::Mutex<Option<Rect>>> = Arc::new(std::sync::Mutex::new(None));
    let captured_write = Arc::clone(&captured);

    let run = RenderTester::mount(
        box_node(RenderPadding::all(20.0)).child(
            box_node(RenderShaderMask::new(move |bounds: Rect| {
                *captured_write.lock().expect("mutex poisoned") = Some(bounds);
                Shader::solid(Color::WHITE)
            }))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
        ),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(run.painted());
    let bounds = captured
        .lock()
        .expect("mutex poisoned")
        .expect("shader callback must have been invoked during paint");
    assert_eq!(
        bounds,
        Rect::from_origin_size(Point::ZERO, Size::new(px(40.0), px(40.0))),
        "shader callback must receive the LOCAL bounds rect, not the \
         parent-origin-shifted global rect",
    );
}

#[test]
fn harness_shader_mask_layer_field_round_trip() {
    let run = RenderTester::mount(
        box_node(RenderShaderMask::new(solid_white_shader).with_blend_mode(BlendMode::Multiply))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let (_, node) = run
        .layer_tree()
        .expect("frame must have painted a layer tree")
        .iter()
        .find(|(_, n)| n.layer().is_shader_mask())
        .expect("ShaderMask layer must be present");
    assert_eq!(
        node.layer().as_shader_mask().unwrap().blend_mode(),
        BlendMode::Multiply,
        "blend_mode must reach the composed ShaderMaskLayer unchanged"
    );
}

#[test]
fn harness_shader_mask_hit_tests_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderShaderMask::new(solid_white_shader))
            .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(50.0, 50.0), Some(run.id("child")));
}

#[test]
fn harness_shader_mask_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderShaderMask::new(solid_white_shader).with_blend_mode(BlendMode::Screen))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderShaderMask", &["blend_mode"]);
}

#[test]
fn harness_backdrop_filter_layout_passes_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderBackdropFilter::new(ImageFilter::blur(5.0)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_backdrop_filter_no_child_paints_nothing() {
    let run = RenderTester::mount(box_node(RenderBackdropFilter::new(ImageFilter::blur(5.0))))
        .with_constraints(loose(200.0))
        .run_frame();

    assert!(
        !run.structure().contains(&"BackdropFilter"),
        "a childless BackdropFilter must not push a layer (oracle: layer = null): {:?}",
        run.structure(),
    );
}

#[test]
fn harness_backdrop_filter_paints_with_backdrop_filter_layer() {
    let run = RenderTester::mount(
        box_node(RenderBackdropFilter::new(ImageFilter::blur(5.0)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(run.painted());
    assert!(run.structure().contains(&"BackdropFilter"));
}

#[test]
fn harness_backdrop_filter_disabled_bypasses_filter_but_still_paints_child() {
    // Regression test for trap §4.4: `enabled` and "has a child" are TWO
    // INDEPENDENT gates. enabled=false must bypass the filter layer
    // entirely while the child STILL paints (unfiltered) — a naive
    // combined `enabled && has_child` condition would wrongly skip
    // painting the child too.
    let run = RenderTester::mount(
        box_node(RenderBackdropFilter::new(ImageFilter::blur(5.0)).with_enabled(false))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(
        !run.structure().contains(&"BackdropFilter"),
        "enabled=false must bypass the filter layer entirely: {:?}",
        run.structure(),
    );
    assert_eq!(
        run.structure(),
        vec!["Offset", "Picture"],
        "enabled=false must still paint the child unfiltered, not skip \
         painting entirely: {:?}",
        run.structure(),
    );
}

#[test]
fn harness_backdrop_filter_layer_field_round_trip() {
    let run = RenderTester::mount(
        box_node(
            RenderBackdropFilter::new(ImageFilter::blur(5.0)).with_blend_mode(BlendMode::Screen),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let (_, node) = run
        .layer_tree()
        .expect("frame must have painted a layer tree")
        .iter()
        .find(|(_, n)| n.layer().is_backdrop_filter())
        .expect("BackdropFilter layer must be present");
    assert_eq!(
        node.layer().as_backdrop_filter().unwrap().blend_mode(),
        BlendMode::Screen,
        "blend_mode must reach the composed BackdropFilterLayer unchanged"
    );
}

#[test]
fn harness_backdrop_filter_hit_tests_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderBackdropFilter::new(ImageFilter::blur(5.0)))
            .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(50.0, 50.0), Some(run.id("child")));
}

#[test]
fn harness_backdrop_filter_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderBackdropFilter::new(ImageFilter::blur(5.0)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderBackdropFilter",
        &["filter", "blend_mode", "enabled"],
    );
}

// ============================================================================
// RenderLeaderLayer / RenderFollowerLayer
// ============================================================================

#[test]
fn harness_leader_layer_layout_passes_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderLeaderLayer::new(LayerLink::new()))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_leader_layer_layout_uses_smallest_when_no_child() {
    let run = RenderTester::mount(box_node(RenderLeaderLayer::new(LayerLink::new())))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
}

#[test]
fn harness_leader_layer_always_pushes_layer_even_with_zero_children() {
    // Regression test for the highest-risk trap in the design research
    // plan (§7.1/§7.2): unlike ShaderMask/BackdropFilter's OWN no-child
    // test (which asserts the layer is ABSENT), oracle's
    // `RenderLeaderLayer.paint` pushes its `LeaderLayer` UNCONDITIONALLY
    // (`proxy_box.dart:4513-4528`) — a childless leader is still a
    // coordinate anchor and must still appear in the structure.
    let run = RenderTester::mount(box_node(RenderLeaderLayer::new(LayerLink::new())))
        .with_constraints(loose(200.0))
        .run_frame();

    assert!(
        run.structure().contains(&"Leader"),
        "a childless Leader MUST still push its layer (oracle: unconditional \
         push, unlike ShaderMask/BackdropFilter): {:?}",
        run.structure(),
    );
}

#[test]
fn harness_leader_layer_field_round_trip() {
    let link = LayerLink::new();
    let run = RenderTester::mount(
        box_node(RenderLeaderLayer::new(link))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let (_, node) = run
        .layer_tree()
        .expect("frame must have painted a layer tree")
        .iter()
        .find(|(_, n)| n.layer().is_leader())
        .expect("Leader layer must be present");
    let leader = node.layer().as_leader().unwrap();
    assert_eq!(
        leader.link(),
        link,
        "link must reach the composed LeaderLayer unchanged"
    );
    assert_eq!(
        leader.size(),
        Size::new(px(40.0), px(40.0)),
        "size must be published as this node's committed paint size"
    );
}

#[test]
fn harness_leader_layer_always_needs_compositing_is_unconditional() {
    // Contrasts with ShaderMask/BackdropFilter's `self.has_child`-gated
    // version (oracle `proxy_box.dart:4498-4499`).
    assert!(RenderLeaderLayer::new(LayerLink::new()).always_needs_compositing());
}

#[test]
fn harness_leader_layer_hit_tests_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderLeaderLayer::new(LayerLink::new()))
            .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(50.0, 50.0), Some(run.id("child")));
}

#[test]
fn harness_leader_layer_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderLeaderLayer::new(LayerLink::new()))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderLeaderLayer", &["link"]);
}

#[test]
fn harness_follower_layer_layout_passes_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderFollowerLayer::new(LayerLink::new()))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(40.0)));
}

#[test]
fn harness_follower_layer_layout_uses_smallest_when_no_child() {
    let run = RenderTester::mount(box_node(RenderFollowerLayer::new(LayerLink::new())))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
}

#[test]
fn harness_follower_layer_always_pushes_layer_even_with_zero_children() {
    // Regression test for the highest-risk trap (design research plan
    // §7.1/§7.2), the direct opposite of ShaderMask/BackdropFilter's own
    // no-child test: oracle's `RenderFollowerLayer.paint` pushes its
    // `FollowerLayer` UNCONDITIONALLY (`proxy_box.dart:4708-4721`) — the
    // no-leader/hidden decision is resolved later, not by skipping the
    // push here.
    let run = RenderTester::mount(box_node(RenderFollowerLayer::new(LayerLink::new())))
        .with_constraints(loose(200.0))
        .run_frame();

    assert!(
        run.structure().contains(&"Follower"),
        "a childless Follower MUST still push its layer (oracle: unconditional \
         push, unlike ShaderMask/BackdropFilter): {:?}",
        run.structure(),
    );
}

#[test]
fn harness_follower_layer_field_round_trip() {
    // Non-default values for every field, catching a composer wiring bug
    // that drops or defaults a field (the same class of test the
    // ShaderMask/BackdropFilter plan used for `blend_mode`).
    let link = LayerLink::new();
    let target_offset = Offset::new(px(3.0), px(7.0));
    let run = RenderTester::mount(
        box_node(
            RenderFollowerLayer::new(link)
                .with_show_when_unlinked(false)
                .with_offset(target_offset)
                .with_leader_anchor(Alignment::BOTTOM_CENTER)
                .with_follower_anchor(Alignment::TOP_CENTER),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let (_, node) = run
        .layer_tree()
        .expect("frame must have painted a layer tree")
        .iter()
        .find(|(_, n)| n.layer().is_follower())
        .expect("Follower layer must be present");
    let follower = node.layer().as_follower().unwrap();
    assert_eq!(follower.link(), link);
    assert!(!follower.show_when_unlinked());
    assert_eq!(follower.target_offset(), target_offset);
    assert_eq!(follower.leader_anchor(), Alignment::BOTTOM_CENTER);
    assert_eq!(follower.follower_anchor(), Alignment::TOP_CENTER);
}

#[test]
fn harness_follower_layer_always_needs_compositing_is_unconditional() {
    // Contrasts with ShaderMask/BackdropFilter's `self.has_child`-gated
    // version (oracle `proxy_box.dart:4656`).
    assert!(RenderFollowerLayer::new(LayerLink::new()).always_needs_compositing());
}

#[test]
fn harness_follower_layer_hit_tests_through_to_child_structurally_only() {
    // Structural-forward half ONLY: a child positioned at the follower's
    // own layout-relative offset is hit. This does NOT cover
    // resolved-transform-aware hit-testing — that is the genuinely
    // deferred ADR-level gap (design research plan §4.4/§8), not
    // implemented by this render object today.
    let run = RenderTester::mount(
        box_node(RenderFollowerLayer::new(LayerLink::new()))
            .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(50.0, 50.0), Some(run.id("child")));
}

#[test]
fn harness_follower_layer_hit_test_misses_when_no_child() {
    let run = RenderTester::mount(box_node(RenderFollowerLayer::new(LayerLink::new())))
        .with_size(Size::new(px(100.0), px(100.0)))
        .run_frame();

    assert_eq!(run.hit_first(50.0, 50.0), None);
}

#[test]
fn harness_follower_layer_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderFollowerLayer::new(LayerLink::new()))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderFollowerLayer",
        &[
            "link",
            "show_when_unlinked",
            "offset",
            "leader_anchor",
            "follower_anchor",
        ],
    );
}

/// ★ MILESTONE (ADR-0015): a leader+follower pair under two DIFFERENT
/// `Stack`-positioned `RenderRepaintBoundary` branches — the
/// cross-repaint-boundary case that motivated the whole render-time
/// resolution design — must hit-test the follower's child at the
/// follower's RESOLVED on-screen position, NOT at its plain tree-relative
/// position.
///
/// Tree (both branches are repaint boundaries, so paint.rs wraps each in
/// its own `Layer::Offset`):
///
/// ```text
/// RenderStack (300x300)
///  ├─ "branch_a" @ Stack(top:60, left:50) = RenderRepaintBoundary
///  │     └─ RenderLeaderLayer(link)         (no child — a pure anchor)
///  └─ "branch_b" @ Stack(top:0, left:0, width:300, height:300) = RenderRepaintBoundary
///        └─ RenderFollowerLayer(link)
///              └─ RenderAlign(TOP_LEFT)       (mirrors Flutter's own
///                    └─ "follower_child"       `Positioned.fill` + `Align`
///                       = RenderColoredBox      idiom for a follower whose
///                         (30x30)               resolved position can land
///                                                anywhere in the overlay)
/// ```
///
/// `branch_b` is given an explicit large size (rather than sizing tightly to
/// its own small content) for the SAME reason a real `CompositedTransformFollower`
/// is conventionally wrapped in `Positioned.fill`: the hit-test walk's
/// ancestor chain gates on each node's OWN untransformed bounds before the
/// follower's resolved-offset shift ever applies, so an ancestor sized only
/// to the follower's natural content could never geometrically reach a
/// resolved position that lands elsewhere. `RenderAlign` then hands the
/// small child loose constraints again, so `follower_child` keeps its
/// natural 30x30 size and precise position within the (now large) follower.
///
/// With default TOP_LEFT/TOP_LEFT anchors and zero target offset, the
/// resolved offset is exactly `branch_a`'s own Stack offset (50,60) —
/// `resolve_follower_offset` must sum BOTH ancestor chains to their common
/// ancestor (summing `branch_a`'s (50,60) and subtracting `branch_b`'s
/// (0,0)) rather than assuming a shared parent or a same-numbered offset.
#[test]
fn harness_follower_layer_hit_tests_at_resolved_position_across_repaint_boundaries() {
    let link = LayerLink::new();

    let branch_a = box_node(RenderRepaintBoundary::new())
        .label("branch_a")
        .with_stack_parent_data(StackParentData::new().with_top(60.0).with_left(50.0))
        .child(box_node(RenderLeaderLayer::new(link)).label("leader"));

    let branch_b = box_node(RenderRepaintBoundary::new())
        .label("branch_b")
        .with_stack_parent_data(
            StackParentData::new()
                .with_top(0.0)
                .with_left(0.0)
                .with_width(300.0)
                .with_height(300.0),
        )
        .child(
            box_node(RenderFollowerLayer::new(link))
                .label("follower")
                .child(
                    box_node(RenderAlign::new(Alignment::TOP_LEFT))
                        .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("follower_child")),
                ),
        );

    let run = RenderTester::mount(box_node(RenderStack::new()).child(branch_a).child(branch_b))
        .with_size(Size::new(px(300.0), px(300.0)))
        .run_frame();

    // (a) A hit at the follower's RESOLVED on-screen position — the
    // leader's absolute anchor at `branch_a`'s Stack offset (50,60), well
    // inside the follower_child's resolved (50,60)-(80,90) rect — reaches
    // the follower's child.
    assert_eq!(
        run.hit_first(60.0, 70.0),
        Some(run.id("follower_child")),
        "a hit at the follower's RESOLVED on-screen position must reach \
         its child — this is the whole point of ADR-0015"
    );

    // (b) A hit at the follower's plain TREE-RELATIVE position (inside
    // `follower_child`'s NATURAL (0,0)-(30,30) rect, where `branch_b` and
    // the follower itself sit) does NOT reach the child — a naive
    // structural-only forward (the pre-ADR-0015 behavior) would have hit
    // it here instead, exactly backwards.
    assert_eq!(
        run.hit_first(10.0, 10.0),
        None,
        "a hit at the follower's plain TREE-RELATIVE position must NOT \
         reach its child — this is the regression proof that the fix is \
         real, not a no-op (a naive structural-only forward hits here)"
    );
}

/// ★ MILESTONE (ADR-0015): an unlinked follower with
/// `show_when_unlinked = false` has NO hittable subtree at all — the
/// hit-test walk must skip it entirely, mirroring
/// `resolve_follower_offset -> None -> don't descend` on the render path,
/// rather than falling through to the structural forward.
#[test]
fn harness_follower_layer_hidden_when_unlinked_has_no_hittable_subtree() {
    // A link with NO leader registered anywhere in this tree.
    let link = LayerLink::new();

    let run = RenderTester::mount(
        box_node(RenderFollowerLayer::new(link).with_show_when_unlinked(false))
            .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("hidden_child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.hit_first(5.0, 5.0),
        None,
        "an unlinked follower with show_when_unlinked = false must have \
         no hittable subtree at all"
    );
}

// ============================================================================
// RenderPhysicalModel / RenderPhysicalShape
// ============================================================================

#[test]
fn harness_physical_model_layout_passes_through_to_child() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalModel::new(Color::WHITE))
            .child(box_node(RenderColoredBox::red(40.0, 30.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(40.0), px(30.0)));
    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(px(40.0), px(30.0))
    );
}

#[test]
fn harness_physical_model_no_child_paints_nothing() {
    let run = RenderTester::mount(box_node(RenderPhysicalModel::new(Color::WHITE)))
        .with_size(Size::new(px(50.0), px(50.0)))
        .run_frame();

    assert!(
        run.display_commands().is_empty(),
        "no child means nothing is drawn at all, not even a background \
         fill (oracle proxy_box.dart:2206-2209)",
    );
}

#[test]
fn harness_physical_model_zero_elevation_paints_no_shadow() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalModel::new(Color::WHITE))
            .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(
        !run.display_commands()
            .iter()
            .any(|cmd| cmd.kind == DrawKind::Shadow),
        "elevation == 0.0 must not cast a shadow",
    );
}

#[test]
fn harness_physical_model_elevation_casts_shadow_before_fill_and_child() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalModel::new(Color::WHITE).with_elevation(4.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let commands = run.display_commands();
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.kind == DrawKind::Shadow)
            .count(),
        1,
        "elevation != 0.0 must cast exactly one shadow; commands:\n{commands:#?}",
    );
    let shadow_idx = commands
        .iter()
        .position(|c| c.kind == DrawKind::Shadow)
        .expect("shadow must be present");
    let fill_idx = commands
        .iter()
        .position(|c| c.kind == DrawKind::RRect)
        .expect("fill must be present");
    let child_idx = commands
        .iter()
        .position(|c| c.kind == DrawKind::Rect)
        .expect("child paint must be present");
    assert!(
        shadow_idx < fill_idx,
        "shadow must paint before the fill; commands:\n{commands:#?}",
    );
    assert!(
        fill_idx < child_idx,
        "fill must paint before the child; commands:\n{commands:#?}",
    );
}

// The `usesSaveLayer` fork (research plan trap §4.3) — controls WHERE the
// fill is drawn, not just whether. These two tests are the direct check
// that a naive port didn't collapse the fork into "always fill outside"
// or "always fill inside" (either would double-paint or bleed an edge).
//
// `PaintCx::with_clip_rrect`/`with_clip_path` push a genuine `Layer::ClipRRect`/
// `ClipPath` tree node (`flui-rendering/src/pipeline/owner/paint.rs::clip_layer`),
// not a `DrawCommand::ClipRRect` embedded in a `Picture`'s display list — so
// `display_commands()` (which only extracts commands from `Picture` layers)
// never surfaces a `DrawKind::Clip` entry for this path. The fork is instead
// verified by (a) `run.structure()` proving a real clip layer was pushed
// regardless of `clip_behavior`, and (b) exactly one fill of the *expected*
// kind — the shape-specific `RRect`/`Path` draw outside, or the `draw_paint`
// (`Other`) fill inside — with the other kind entirely absent.
#[test]
fn harness_physical_model_fills_before_clip_when_not_save_layer() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalModel::new(Color::WHITE).with_clip_behavior(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(
        run.structure().contains(&"ClipRRect"),
        "AntiAlias must still push a real clip layer; structure: {:?}",
        run.structure(),
    );

    let commands = run.display_commands();
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.kind == DrawKind::RRect)
            .count(),
        1,
        "!uses_save_layer must fill via the shape-specific RRect draw call \
         exactly once (outside the clip, on the parent canvas); commands:\n{commands:#?}",
    );
    assert!(
        !commands.iter().any(|c| c.kind == DrawKind::Other),
        "!uses_save_layer must not also draw_paint inside the clip — that \
         is the save-layer-only branch; commands:\n{commands:#?}",
    );
}

#[test]
fn harness_physical_model_fills_inside_clip_when_save_layer() {
    let run = RenderTester::mount(
        box_node(
            RenderPhysicalModel::new(Color::WHITE).with_clip_behavior(Clip::AntiAliasWithSaveLayer),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(
        run.structure().contains(&"ClipRRect"),
        "AntiAliasWithSaveLayer must still push a real clip layer; structure: {:?}",
        run.structure(),
    );

    let commands = run.display_commands();
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.kind == DrawKind::Other)
            .count(),
        1,
        "uses_save_layer must fill via draw_paint exactly once (inside the \
         clip scope); commands:\n{commands:#?}",
    );
    assert!(
        !commands.iter().any(|c| c.kind == DrawKind::RRect),
        "uses_save_layer must not also draw the shape-specific RRect fill \
         outside the clip — that is the non-save-layer-only branch; \
         commands:\n{commands:#?}",
    );
}

// Trap §4.4 regression at the render-object level (see the unit-level
// regression in `proxy::physical_model::tests` for the formula check) plus
// the hit-test divergence trap §4.2: `RenderPhysicalModel` ALWAYS tests the
// clip shape, even though it never exposes a public clipper — a deliberate,
// precedent-backed divergence from the oracle's `_clipper != null` gate,
// which for `RenderPhysicalModel` specifically never engages (see the
// module doc on `RenderPhysicalModelBase::hit_test`). A circular/rounded
// `RenderPhysicalModel` hits its full bounding box in real Flutter; this
// port hit-tests the visible shape instead.
#[test]
fn harness_physical_model_hit_test_always_tests_circle_shape_excludes_bbox_corner() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalModel::new(Color::WHITE).with_shape(BoxShape::Circle))
            .child(box_node(RenderColoredBox::red(100.0, 40.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(40.0)))
    .run_layout();

    // (1, 1) is inside the 100x40 bounding box but outside the inscribed
    // ellipse (rx=50, ry=20 centered at (50, 20)) — the ellipse-not-circle
    // formula from trap §4.4 makes this exclusion asymmetric per axis.
    assert_eq!(run.hit_first(1.0, 1.0), None);
    // The ellipse center is always inside.
    assert_eq!(run.hit_first(50.0, 20.0), Some(run.id("child")));
}

#[test]
fn harness_physical_shape_hit_test_triangular_clipper() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalShape::new(
            |size: Size| {
                let mut p = Path::new();
                p.move_to(Point::new(size.width * 0.5, px(0.0)));
                p.line_to(Point::new(size.width, size.height));
                p.line_to(Point::new(px(0.0), size.height));
                p.close();
                p
            },
            Color::WHITE,
        ))
        .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    // The oracle and the "always test shape" convention already agree for
    // `RenderPhysicalShape` (it always has a clipper), so this is a plain
    // shape hit-test, not a divergence test.
    assert_eq!(
        run.hit_first(1.0, 1.0),
        None,
        "top-left bounding-box corner is outside the triangle"
    );
    assert_eq!(
        run.hit_first(50.0, 90.0),
        Some(run.id("child")),
        "near the base midpoint must be inside the triangle"
    );
}

#[test]
fn harness_physical_shape_falls_back_to_whole_rect_when_clipper_cleared() {
    let mut run = RenderTester::mount(
        box_node(RenderPhysicalShape::new(
            |size: Size| {
                // A clipper covering only the top-left quadrant.
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(
                    Point::ZERO,
                    Size::new(size.width * 0.5, size.height * 0.5),
                ));
                p
            },
            Color::WHITE,
        ))
        .label("shape")
        .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    // Before clearing: outside the top-left-quadrant clip, no hit.
    assert_eq!(run.hit_first(90.0, 90.0), None);
    assert!(
        run.descendant_property("RenderPhysicalShape", "custom_clipper")
            .is_some()
    );

    run.update::<RenderPhysicalShape>(run.id("shape"), |node| {
        assert!(node.set_clipper::<fn(Size) -> Path>(None));
    });
    run.relayout();

    // After clearing: falls back to the whole-box rectangle (oracle `:2296`).
    assert_eq!(run.hit_first(90.0, 90.0), Some(run.id("child")));
    assert!(
        run.descendant_property("RenderPhysicalShape", "custom_clipper")
            .is_none(),
        "custom_clipper flag must be omitted once cleared",
    );
}

#[test]
fn harness_physical_model_self_describes_shape_border_radius_and_colors() {
    let run = RenderTester::mount(
        box_node(
            RenderPhysicalModel::new(Color::WHITE)
                .with_elevation(2.0)
                .with_shadow_color(Color::BLUE)
                .with_border_radius(BorderRadius::circular(px(8.0))),
        )
        .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderPhysicalModel",
        &[
            "elevation",
            "color",
            "shadow_color",
            "clip_behavior",
            "shape",
            "border_radius",
        ],
    );
    // Trap §4.1 regression: the oracle's own `debugFillProperties` bug
    // passes `color` a second time instead of `shadowColor` — this must
    // read back the real shadow color, not the fill color.
    assert_eq!(
        run.descendant_property("RenderPhysicalModel", "shadow_color"),
        Some(format!("{:?}", Color::BLUE)),
    );
    assert_eq!(
        run.descendant_property("RenderPhysicalModel", "color"),
        Some(format!("{:?}", Color::WHITE)),
    );
}

#[test]
fn harness_physical_shape_self_describes_custom_clipper_and_colors() {
    let run = RenderTester::mount(
        box_node(RenderPhysicalShape::new(
            |size: Size| {
                let mut p = Path::new();
                p.add_rect(Rect::from_origin_size(Point::ZERO, size));
                p
            },
            Color::WHITE,
        ))
        .child(box_node(RenderColoredBox::red(40.0, 40.0))),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderPhysicalShape",
        &[
            "elevation",
            "color",
            "shadow_color",
            "clip_behavior",
            "custom_clipper",
        ],
    );
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
fn harness_repaint_boundary_hit_tests_through_to_child() {
    // A repaint boundary must pass hit-tests through to its child, not absorb
    // them. Before the fix the trait-default hit_test returned the boundary
    // itself and never recursed, blocking the entire subtree from pointer events.
    let run = RenderTester::mount(
        box_node(RenderRepaintBoundary::new())
            .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(50.0, 50.0), Some(run.id("child")));
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
fn harness_flex_empty_row_max_fills_main_axis() {
    // An empty Row with the default MainAxisSize::Max still fills the bounded
    // main axis (cross collapses to 0). Flutter flex.dart idealMainSize.
    // Before the fix the childless short-circuit returned smallest() → (0,0).
    let run = RenderTester::mount(box_node(RenderFlex::row()))
        .with_constraints(BoxConstraints::new(px(0.0), px(500.0), px(0.0), px(300.0)))
        .run_layout();
    assert_eq!(run.box_geometry(run.root()), Size::new(px(500.0), px(0.0)));
}

#[test]
fn harness_flex_empty_row_min_collapses() {
    // MainAxisSize::Min collapses both axes even under a bounded main axis.
    let run = RenderTester::mount(box_node(
        RenderFlex::row().with_main_axis_size(MainAxisSize::Min),
    ))
    .with_constraints(BoxConstraints::new(px(0.0), px(500.0), px(0.0), px(300.0)))
    .run_layout();
    assert_eq!(run.box_geometry(run.root()), Size::new(px(0.0), px(0.0)));
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

/// `compute_dry_layout` returns the real flex size, not `Size::ZERO`.
///
/// Oracle: a 500×300 tight box containing a 200px fixed child and a flex=1
/// Tight child distributes the remaining 300px to the flex child, giving a
/// total of 500px main × 300px cross = (500, 300). This test would fail with
/// the default trait implementation (`Size::ZERO`) and passes only once
/// `RenderFlex::compute_dry_layout` is wired through `compute_sizes`.
#[test]
fn harness_flex_dry_layout_returns_real_size() {
    let constraints = BoxConstraints::tight(Size::new(px(500.0), px(300.0)));
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderSizedBox::fixed(px(200.0), px(300.0))).label("fixed"))
            .child(
                box_node(RenderSizedBox::fixed(px(100.0), px(300.0)))
                    .with_flex_parent_data(FlexParentData::flexible(1))
                    .label("flex_child"),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    // Tight 500×300: fixed child takes 200px, flex=1 Tight child takes the
    // remaining 300px. The container fills its bounded main axis (MainAxisSize::Max
    // default), so the dry size equals the tight constraint size.
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        Size::new(px(500.0), px(300.0)),
        "flex dry layout must return the real sized result, not Size::ZERO",
    );
}

/// A horizontal flex reports its own Alphabetic baseline as the **highest** —
/// meaning the minimum `child_baseline + child_offset.dy` across all children
/// (oracle: `box.dart:3336-3348`, `flex.dart:806-812`).
///
/// Tree: `RenderBaseline(100px)` → `RenderFlex::row` → two `RenderBaseline`
/// children with baseline offsets 10 and 30 over fixed-size boxes.
/// After fix the outer baseline positions the flex so its baseline (10) sits at
/// 100 → `flex.offset.dy == 90`.  Before the fix the flex returned `None`,
/// so the outer fell back to the flex's height (30px) and placed it at 70.
///
/// Red before Slice A (flex has no `compute_distance_to_actual_baseline` override,
/// returns `None`, outer baseline falls back to child height → offset 70 ≠ 90).
/// Green after.
#[test]
fn harness_flex_row_reports_highest_baseline() {
    let run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(100.0)))
            .label("outer")
            .child(
                box_node(RenderFlex::row())
                    .label("row")
                    .child(
                        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(10.0)))
                            .child(box_node(RenderColoredBox::red(40.0, 20.0))),
                    )
                    .child(
                        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(30.0)))
                            .child(box_node(RenderColoredBox::green(40.0, 40.0))),
                    ),
            ),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // Oracle: highest(row) = min(10 + 0, 30 + 0) = 10.
    // Outer RenderBaseline(100px): top = 100 - 10 = 90.
    assert_eq!(
        run.offset(run.id("row")).dy.get(),
        90.0,
        "flex row must report highest baseline (10) so outer baseline places it at dy=90; \
         before the fix flex returned None → dy was 70",
    );
}

/// A vertical flex reports its own Alphabetic baseline as the **first** child
/// baseline in list order (oracle: `box.dart:3318-3330`, `flex.dart:806-812`).
///
/// Tree: `RenderBaseline(50px)` → `RenderFlex::column` → two `RenderBaseline`
/// children with baseline offsets 5 and 25.
/// After fix the outer baseline positions the flex so its baseline (5) sits at
/// 50 → `flex.offset.dy == 45`.  Before the fix the flex returned `None` → 20.
///
/// Red before Slice A, green after.
#[test]
fn harness_flex_column_reports_first_baseline() {
    let run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(50.0)))
            .label("outer")
            .child(
                box_node(RenderFlex::column())
                    .label("col")
                    .child(
                        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(5.0)))
                            .child(box_node(RenderColoredBox::red(30.0, 10.0))),
                    )
                    .child(
                        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(25.0)))
                            .child(box_node(RenderColoredBox::green(30.0, 10.0))),
                    ),
            ),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // Oracle: first(col) = child_0_baseline + child_0_offset.dy = 5 + 0 = 5.
    // Outer RenderBaseline(50px): top = 50 - 5 = 45.
    assert_eq!(
        run.offset(run.id("col")).dy.get(),
        45.0,
        "flex column must report first baseline (5) so outer baseline places it at dy=45; \
         before the fix flex returned None → dy was 20",
    );
}

/// The flex's dry Alphabetic baseline equals the committed baseline (dry==committed
/// invariant, ADR-0012 D-B3).
///
/// Uses `RenderBaseline` over `RenderParagraph` children so both the live and dry
/// paths have a real baseline to compute from: `RenderBaseline.compute_dry_baseline`
/// returns `baseline_offset + requested - own = baseline_offset` when the requested
/// kind matches the box's own kind and the child (paragraph) reports the same value
/// for both reads.
///
/// Expected dry baseline: `min(10 + 0, 30 + 0) = 10.0`.
///
/// Red before Slice B (`compute_dry_baseline` not overridden → returns `None`).
/// Green after.
#[test]
fn harness_flex_dry_baseline_equals_committed() {
    let constraints = BoxConstraints::loose(Size::new(px(300.0), px(100.0)));
    let mut run = RenderTester::mount(
        box_node(RenderFlex::row())
            .label("row")
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(10.0))).child(box_node(
                    RenderParagraph::new(TextSpan::new("Ag"), TextDirection::Ltr),
                )),
            )
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(30.0))).child(box_node(
                    RenderParagraph::new(TextSpan::new("Ag"), TextDirection::Ltr),
                )),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    // RenderBaseline.compute_dry_baseline(Alphabetic) = baseline_offset + para - para = offset.
    // Flex highest dry baseline = min(10, 30) ≈ 10.  A sub-pixel floating-point
    // drift in the paragraph baseline cancellation is tolerated (< 0.1px).
    let dry = run
        .dry_baseline(run.id("row"), constraints, TextBaseline::Alphabetic)
        .expect("flex row must report a dry Alphabetic baseline");
    assert!(
        (dry - 10.0).abs() < 0.1,
        "flex dry baseline must equal committed baseline (~10.0); got {dry}; \
         before Slice B compute_dry_baseline was not overridden and returned None",
    );
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
fn harness_wrap_max_intrinsic_width_omits_spacing() {
    // Flutter wrap.dart computeMaxIntrinsicWidth sums child max-intrinsic widths
    // with NO inter-child spacing term. 3 children (30+50+40 = 120) with
    // spacing 10 → 120, not the pre-fix spacing-inclusive 140.
    let mut run = RenderTester::mount(
        box_node(RenderWrap::new().with_spacing(10.0))
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(50.0, 20.0)).label("b"))
            .child(box_node(RenderColoredBox::red(40.0, 20.0)).label("c")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_layout();

    assert_eq!(run.max_intrinsic_width(run.root(), 100.0), 120.0);
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

#[test]
fn harness_indexed_stack_sizes_like_stack_but_only_paints_and_hits_selected_child() {
    let run = RenderTester::mount(
        box_node(RenderIndexedStack::new().with_index(Some(1)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("bottom"))
            .child(box_node(RenderColoredBox::green(80.0, 60.0)).label("selected"))
            .child(box_node(RenderColoredBox::blue(30.0, 30.0)).label("hidden_top")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(80.0), px(60.0)),
        "indexed stack must size with the same all-child Stack layout pass",
    );
    assert_eq!(
        run.hit_first(10.0, 10.0),
        Some(run.id("selected")),
        "hit testing must visit only the selected child, not the later hidden child",
    );

    let painted = run
        .display_commands()
        .into_iter()
        .map(|cmd| cmd.line)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        painted.contains("#00FF00FF"),
        "selected green child must paint; commands:\n{painted}",
    );
    assert!(
        !painted.contains("#FF0000FF") && !painted.contains("#0000FFFF"),
        "hidden red/blue children must not paint; commands:\n{painted}",
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderIndexedStack",
        &["fit", "clip_behavior", "index"],
    );
}

#[test]
fn harness_indexed_stack_none_lays_out_but_displays_no_child() {
    let run = RenderTester::mount(
        box_node(RenderIndexedStack::new().with_index(None))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("a"))
            .child(box_node(RenderColoredBox::green(80.0, 60.0)).label("b")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(80.0), px(60.0)),
        "index None must not skip layout of children",
    );
    assert_eq!(
        run.hit_first(10.0, 10.0),
        None,
        "index None must display and hit-test no child",
    );
    assert!(
        run.display_commands().is_empty(),
        "index None must produce no child paint commands",
    );
}

#[test]
fn harness_indexed_stack_reports_selected_child_baseline() {
    let constraints = loose(200.0);
    let mut run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(100.0)))
            .label("outer")
            .child(
                box_node(RenderIndexedStack::new().with_index(Some(1)))
                    .label("indexed")
                    .child(box_node(RenderColoredBox::red(40.0, 50.0)).label("hidden"))
                    .child(
                        box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(10.0)))
                            .child(box_node(RenderParagraph::new(
                                TextSpan::new("Ag"),
                                TextDirection::Ltr,
                            )))
                            .label("selected"),
                    ),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    assert_eq!(
        run.offset(run.id("indexed")).dy.get(),
        90.0,
        "outer baseline must use the selected child's 10px baseline, not the \
         hidden child's larger stack height",
    );
    let dry = run
        .dry_baseline(run.id("indexed"), constraints, TextBaseline::Alphabetic)
        .expect("indexed stack must report the selected child's dry baseline");
    assert!(
        (dry - 10.0).abs() < 0.01,
        "dry baseline must also resolve through the selected child only; got {dry}",
    );
}

#[test]
fn harness_list_body_vertical_down_stretches_cross_axis_and_hits_children() {
    let constraints = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(f32::INFINITY));
    let run = RenderTester::mount(
        box_node(RenderListBody::new())
            .child(box_node(RenderSizedBox::fixed(px(20.0), px(10.0))).label("first"))
            .child(box_node(RenderSizedBox::fixed(px(30.0), px(20.0))).label("second")),
    )
    .with_constraints(constraints)
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(100.0), px(30.0)),
        "vertical ListBody must take the bounded cross-axis width and summed child heights",
    );
    assert_eq!(
        run.box_geometry(run.id("first")),
        Size::new(px(100.0), px(10.0)),
        "children are tight to the cross-axis width",
    );
    assert_eq!(run.offset(run.id("first")), Offset::ZERO);
    assert_eq!(run.offset(run.id("second")), Offset::new(px(0.0), px(10.0)));
    assert_eq!(run.hit_first(5.0, 15.0), Some(run.id("second")));
    assert_descendant_properties(&run.diagnostics(), "RenderListBody", &["axis_direction"]);
}

#[test]
fn harness_list_body_vertical_up_positions_children_from_bottom() {
    let constraints = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(f32::INFINITY));
    let run = RenderTester::mount(
        box_node(RenderListBody::with_axis_direction(
            AxisDirection::BottomToTop,
        ))
        .child(box_node(RenderSizedBox::fixed(px(20.0), px(10.0))).label("first"))
        .child(box_node(RenderSizedBox::fixed(px(30.0), px(20.0))).label("second")),
    )
    .with_constraints(constraints)
    .run_frame();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(100.0), px(30.0)));
    assert_eq!(
        run.offset(run.id("first")),
        Offset::new(px(0.0), px(20.0)),
        "first child is visually last for AxisDirection::BottomToTop",
    );
    assert_eq!(run.offset(run.id("second")), Offset::ZERO);
    assert_eq!(run.hit_first(5.0, 5.0), Some(run.id("second")));
}

#[test]
fn harness_list_body_horizontal_right_to_left_stretches_height() {
    let constraints = BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(50.0));
    let run = RenderTester::mount(
        box_node(RenderListBody::with_axis_direction(
            AxisDirection::RightToLeft,
        ))
        .child(box_node(RenderSizedBox::fixed(px(20.0), px(10.0))).label("first"))
        .child(box_node(RenderSizedBox::fixed(px(30.0), px(20.0))).label("second")),
    )
    .with_constraints(constraints)
    .run_frame();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(50.0), px(50.0)));
    assert_eq!(
        run.box_geometry(run.id("first")),
        Size::new(px(20.0), px(50.0)),
    );
    assert_eq!(run.offset(run.id("first")), Offset::new(px(30.0), px(0.0)));
    assert_eq!(run.offset(run.id("second")), Offset::ZERO);
}

#[test]
fn harness_list_body_dry_layout_and_baseline_follow_oracle_order() {
    let constraints = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(f32::INFINITY));
    let mut dry_run = RenderTester::mount(
        box_node(RenderListBody::new())
            .label("list")
            .child(box_node(RenderSizedBox::fixed(px(20.0), px(10.0))))
            .child(box_node(RenderSizedBox::fixed(px(30.0), px(20.0)))),
    )
    .with_constraints(constraints)
    .run_layout();

    assert_eq!(
        dry_run.dry_layout(dry_run.id("list"), constraints),
        Size::new(px(100.0), px(30.0)),
        "dry layout must take the bounded cross axis and sum child main extents",
    );

    let mut baseline_run = RenderTester::mount(
        box_node(RenderListBody::new())
            .label("list")
            .child(box_node(RenderSizedBox::fixed(px(20.0), px(10.0))).label("box"))
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(5.0)))
                    .child(box_node(RenderParagraph::new(
                        TextSpan::new("Ag"),
                        TextDirection::Ltr,
                    )))
                    .label("baseline"),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    let dry = baseline_run
        .dry_baseline(
            baseline_run.id("list"),
            constraints,
            TextBaseline::Alphabetic,
        )
        .expect("second child reports a baseline");
    assert!(
        (dry - 15.0).abs() < 0.01,
        "vertical down dry baseline must skip the first non-baseline child and add its height; got {dry}",
    );
}

// ── RenderStack dry layout ────────────────────────────────────────────────────

/// `compute_dry_layout` for a stack with a non-positioned and a positioned child.
///
/// Oracle (stack.dart:619-675): positioned children are EXCLUDED from the
/// sizing pass, so the stack shrink-wraps to the non-positioned 40×40 child.
/// This test would return `Size::ZERO` with the default trait implementation
/// and passes only once `RenderStack::compute_dry_layout` delegates to
/// `compute_size`.
#[test]
fn harness_stack_dry_layout_shrink_wraps_and_excludes_positioned() {
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let mut run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderSizedBox::fixed(px(40.0), px(40.0))).label("nonpos"))
            .child(
                box_node(RenderSizedBox::fixed(px(80.0), px(80.0)))
                    .with_stack_parent_data(StackParentData::new().with_top(0.0).with_left(0.0))
                    .label("pos"),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    let expected = Size::new(px(40.0), px(40.0));
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        expected,
        "dry layout must shrink-wrap to the non-positioned child and ignore the positioned one",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        run.box_geometry(run.root()),
        "dry layout must agree with committed layout geometry",
    );
}

/// `compute_dry_layout` for `StackFit::Expand`: the non-positioned child is
/// stretched to the biggest constraint, so the container reports (200, 200).
#[test]
fn harness_stack_dry_layout_expand_fit() {
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let mut run = RenderTester::mount(
        box_node(RenderStack::new().with_fit(StackFit::Expand))
            .child(box_node(RenderSizedBox::fixed(px(10.0), px(10.0))).label("child")),
    )
    .with_constraints(constraints)
    .run_layout();

    let expected = Size::new(px(200.0), px(200.0));
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        expected,
        "StackFit::Expand dry layout must fill the incoming constraints",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        run.box_geometry(run.root()),
        "dry layout must agree with committed layout geometry",
    );
}

/// `compute_dry_layout` when all children are positioned: no non-positioned
/// children contribute to sizing, so the stack takes `constraints.biggest()`.
#[test]
fn harness_stack_dry_layout_all_positioned_takes_biggest() {
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let mut run = RenderTester::mount(
        box_node(RenderStack::new()).child(
            box_node(RenderSizedBox::fixed(px(20.0), px(20.0)))
                .with_stack_parent_data(StackParentData::new().with_top(0.0))
                .label("pos"),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    let expected = Size::new(px(200.0), px(200.0));
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        expected,
        "all-positioned stack dry layout must take constraints.biggest()",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        run.box_geometry(run.root()),
        "dry layout must agree with committed layout geometry",
    );
}

/// A width-only Stack child IS positioned (Flutter stack.dart:242-249): it is
/// excluded from sizing (so the stack shrink-wraps to the non-positioned base)
/// and sized to its explicit width. Regression guard for the is_positioned fix
/// — before it, a width-only child was treated as non-positioned and its size
/// leaked into the stack size (would be 100×50 instead of 50×50 here).
#[test]
fn harness_stack_dry_layout_width_only_child_is_positioned() {
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let mut run = RenderTester::mount(
        box_node(RenderStack::new())
            .child(box_node(RenderSizedBox::fixed(px(50.0), px(50.0))).label("base"))
            .child(
                box_node(RenderSizedBox::fixed(px(100.0), px(30.0)))
                    .with_stack_parent_data(StackParentData::new().with_width(80.0))
                    .label("width_only"),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    let expected = Size::new(px(50.0), px(50.0));
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        expected,
        "a width-only child is positioned and excluded from sizing; the stack \
         shrink-wraps to the non-positioned 50x50 base",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        run.box_geometry(run.root()),
        "dry layout must agree with committed layout geometry",
    );
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

// ── RenderSliverGrid ─────────────────────────────────────────────────────────

#[test]
fn harness_render_sliver_grid_lays_out_two_column_grid() {
    // 4 children, 2 columns, viewport 200×200: 2 rows of 100×100 tiles.
    // scroll_extent = compute_max_scroll_offset(4) = 100*2 - 0 = 200.
    // All 4 tiles fit in the 200px viewport so all receive layout.
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverGrid::new(Arc::new(
            SliverGridDelegateWithFixedCrossAxisCount::new(2),
        )))
        .label("grid")
        .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("tile0"))
        .child(box_node(RenderColoredBox::green(100.0, 100.0)).label("tile1"))
        .child(box_node(RenderColoredBox::blue(100.0, 100.0)).label("tile2"))
        .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("tile3")),
    ))
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_layout();

    // Sliver geometry.
    let geom = run.sliver_geometry(run.id("grid"));
    assert_eq!(
        geom.scroll_extent, 200.0,
        "4 children × 2 columns = 2 rows × 100px = 200px total extent",
    );
    assert!(geom.paint_extent > 0.0);

    // Each tile must receive tight 100×100 constraints from the delegate.
    assert_eq!(
        run.box_geometry(run.id("tile0")),
        Size::new(px(100.0), px(100.0)),
        "tile0 must be sized 100×100 by the delegate",
    );
    assert_eq!(
        run.box_geometry(run.id("tile2")),
        Size::new(px(100.0), px(100.0)),
        "tile2 (second row) must also be 100×100",
    );

    // Diagnostics must surface child_count and committed geometry.
    assert_descendant_properties(&run.diagnostics(), "RenderSliverGrid", &["child_count"]);
    let tree = run.diagnostics();
    let sliver_node_diag = tree.find_descendant("RenderSliverGrid").unwrap();
    assert_has_committed_geometry(sliver_node_diag);
}

// ── RenderSliverGridLazy ──────────────────────────────────────────────────────

#[test]
fn harness_render_sliver_grid_lazy_zero_items_reports_zero_geometry() {
    // Empty source — no build requests should be emitted and the reported
    // scroll extent must be zero.
    let grid = RenderSliverGridLazy::new(
        Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
        0,
    );
    let run = RenderTester::mount(viewport(sliver_node(grid).label("lazy_grid")))
        .with_size(Size::new(px(200.0), px(400.0)))
        .run_layout();

    assert_eq!(
        run.sliver_geometry(run.id("lazy_grid")).scroll_extent,
        0.0,
        "empty RenderSliverGridLazy must report zero scroll extent",
    );
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderSliverGridLazy",
        &["item_count", "attached_child_count"],
    );
}

#[test]
fn harness_render_sliver_grid_lazy_pre_seeded_tiles_lay_out_correctly() {
    // 4 items, 2 columns, 200×200 viewport → 2 rows of 100×100 tiles.
    // All 4 tiles are pre-seeded with correct SliverMultiBoxAdaptorParentData so
    // they are "resident" during layout; no build requests should be emitted.
    // scroll_extent = compute_max_scroll_offset(4) = 2 rows × 100px = 200px.
    let grid = RenderSliverGridLazy::new(
        Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
        4,
    );
    let mut run = RenderTester::mount(viewport(
        sliver_node(grid)
            .label("lazy_grid")
            .child(
                box_node(RenderColoredBox::red(100.0, 100.0))
                    .label("tile0")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(0),
                    )),
            )
            .child(
                box_node(RenderColoredBox::green(100.0, 100.0))
                    .label("tile1")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(1),
                    )),
            )
            .child(
                box_node(RenderColoredBox::blue(100.0, 100.0))
                    .label("tile2")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(2),
                    )),
            )
            .child(
                box_node(RenderColoredBox::red(100.0, 100.0))
                    .label("tile3")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(3),
                    )),
            ),
    ))
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_layout();

    // Scroll extent: 2 rows × 100px.
    assert_eq!(
        run.sliver_geometry(run.id("lazy_grid")).scroll_extent,
        200.0,
        "4 items in a 2-column 100px-tile grid = 2 rows × 100px = 200px scroll extent",
    );

    // Every tile must receive tight 100×100 constraints from the delegate.
    assert_eq!(
        run.box_geometry(run.id("tile0")),
        Size::new(px(100.0), px(100.0)),
        "tile0 must be sized 100×100 by the delegate",
    );
    assert_eq!(
        run.box_geometry(run.id("tile2")),
        Size::new(px(100.0), px(100.0)),
        "tile2 (second row) must also be 100×100",
    );

    // All 4 tiles are resident — no build requests should be pending.
    let pending = run.owner_mut().take_pending_child_requests();
    assert!(
        pending.is_empty(),
        "all tiles are pre-seeded; no build requests should be emitted but got {pending:?}",
    );

    let tree = run.diagnostics();
    let grid_diag = tree
        .find_descendant("RenderSliverGridLazy")
        .expect("RenderSliverGridLazy must appear in diagnostics");
    assert_has_committed_geometry(grid_diag);
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

// ─── RenderSliverList (U4.2 request seam — INERT without U4.3 child manager) ─

#[test]
fn harness_sliver_list_zero_items_reports_zero_geometry() {
    // An empty RenderSliverList (item_count = 0) must produce zero geometry and
    // emit no requests.  This is the structural baseline; it would fail if
    // perform_layout panicked or returned non-zero geometry for an empty source.
    let mut run = RenderTester::mount(viewport(
        sliver_node(RenderSliverList::new(0, 48.0)).label("list"),
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    assert_eq!(
        run.sliver_geometry(run.id("list")).scroll_extent,
        0.0,
        "empty RenderSliverList must report zero scroll extent",
    );
    let requests = run.owner_mut().take_pending_child_requests();
    assert!(
        requests.is_empty(),
        "empty list must emit no child requests, got {requests:?}",
    );
    assert_descendant_properties(&run.diagnostics(), "RenderSliverList", &["item_count"]);
}

#[test]
fn harness_sliver_list_layout_emits_absent_requests() {
    // RenderSliverList with 3 items visible in a 300×400 viewport (48 px
    // estimate each; all 3 fit in the 400 px paint extent).  Because no
    // arena children exist yet, every in-band slot fires request_child_build.
    // The test asserts the request buffer contains exactly the logical indices
    // 0, 1, 2 — this fails before request_child_build is wired (Unwired
    // returns, no requests are recorded, buffer is empty).
    let mut run = RenderTester::mount(viewport(
        sliver_node(RenderSliverList::new(3, 48.0)).label("list"),
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let mut requests = run.owner_mut().take_pending_child_requests();
    // Sort by logical index for deterministic comparison.
    requests.sort_by_key(|&(_id, logical_index)| logical_index);
    let logical_indices: Vec<usize> = requests
        .iter()
        .map(|&(_id, logical_index)| logical_index)
        .collect();
    assert_eq!(
        logical_indices,
        &[0, 1, 2],
        "expected requests for logical indices [0, 1, 2], got {logical_indices:?}",
    );
    // All requests must carry the same sliver id (the list node itself).
    let list_id = run.id("list");
    for &(sliver_id, _) in &requests {
        assert_eq!(
            sliver_id, list_id,
            "all requests must originate from the list sliver",
        );
    }
}

#[test]
fn harness_sliver_list_seeded_residents_laid_out_at_expected_offsets() {
    // Pre-seed 2 arena-resident children at logical indices 0 and 1 (48 px
    // each).  With no scrolling and a 3-item list, items 0 and 1 are present
    // in the tree; the band walk lays them out from `logical_to_slot` and only
    // emits a request for the absent index 2.
    //
    // Fails before SliverMultiBoxAdaptor seeding is wired: without the seed the
    // parent-data index is never stamped, logical_to_slot stays empty, and the
    // walk fires requests for ALL 3 indices instead of just index 2.
    let mut run = RenderTester::mount(viewport(
        sliver_node(RenderSliverList::new(3, 48.0))
            .label("list")
            .child(
                box_node(RenderColoredBox::red(300.0, 48.0))
                    .label("item0")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(0),
                    )),
            )
            .child(
                box_node(RenderColoredBox::red(300.0, 48.0))
                    .label("item1")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(1),
                    )),
            ),
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    // Items 0 and 1 are in the tree and laid out: offsets must reflect their
    // virtualizer-assigned layout offsets (0 and 48 px) minus scroll_offset=0.
    assert_eq!(
        run.offset(run.id("item0")).dy,
        px(0.0),
        "resident at logical index 0 must be positioned at dy=0"
    );
    assert_eq!(
        run.offset(run.id("item1")).dy,
        px(48.0),
        "resident at logical index 1 must be positioned at dy=48 (one estimate below index 0)"
    );

    // Only the absent item — index 2 — should be requested.
    let pending = run.owner_mut().take_pending_child_requests();
    let indices: Vec<usize> = {
        let mut v: Vec<usize> = pending.iter().map(|&(_, i)| i).collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        indices,
        &[2],
        "only logical index 2 should be requested; got {indices:?}"
    );
}

#[test]
fn harness_sliver_list_off_band_resident_enqueued_for_removal() {
    // Pre-seed a resident at logical index 0 (48 px).  With scroll=300 the
    // cache window starts at 300-250=50 px; item 0 ends at 48 px, so it is
    // just outside the cache → the band walk enqueues it for deferred removal.
    //
    // Fails before disposal is wired: without the seeded logical index in
    // `logical_to_slot` the walk never identifies item 0 as out-of-band and
    // never disposes it.
    let run = RenderTester::mount(viewport_with_scroll(
        300.0,
        sliver_node(RenderSliverList::new(20, 48.0))
            .label("list")
            .child(
                box_node(RenderColoredBox::red(300.0, 48.0))
                    .label("item0")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(0),
                    )),
            ),
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    // After layout the deferred removal is applied: the node must be gone.
    assert!(
        run.try_box_geometry(run.id("item0")).is_none(),
        "off-band resident at index 0 must be removed from the tree after layout"
    );
}

#[test]
fn harness_sliver_list_scroll_extent_equals_virtualizer_estimate() {
    // A 3-item list with no arena residents (all absent → requests emitted)
    // reports scroll_extent = item_count × estimate = 3 × 48 = 144 px.
    //
    // Fails if item_count or default_extent_estimate is mis-wired: a zero
    // estimate or zero count would give scroll_extent = 0.
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverList::new(3, 48.0)).label("list"),
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    assert_eq!(
        run.sliver_geometry(run.id("list")).scroll_extent,
        144.0,
        "3 items × 48 px estimate must give scroll_extent = 144.0"
    );
}

#[test]
fn harness_sliver_list_anchor_correction_forward_emits_backward_suppresses() {
    // Two-pass test for the anchor-correction state machine.
    //
    // Setup: 10-item list (48 px estimate), item 0 pre-seeded at 60 px.
    // With scroll=100 the viewport tight-visible range starts at item 2
    // (estimated start 96 px < 100 < 144 px = its end) → anchor=(2, 0).
    // Item 0 is in the cache-above band (cache_before = 100 px, cache
    // starts at 0).  set_measured(0, 60, (2,0)) accumulates pending=12.
    // Forward scroll (last=0 → current=100) → correction EMITTED.
    //
    // The viewport absorbs the correction in a three-pass correction loop:
    //   Pass 1 (scroll=100): correction=12 fires → correct_by(12) → pixels=112.
    //   Pass 2 (scroll=112): no new correction; apply_content_dimensions clamps
    //     pixels 112→92 (max_scroll = total_extent(492) − viewport(400) = 92),
    //     returns false → re-run.
    //   Pass 3 (scroll=92): accepted; last_scroll_offset finalised to 92.
    // Observable: item 0's paint dy = layout_offset(0) − scroll(92) = −92 px.
    //
    // Pass 2 of this test: grow item 0 to 84 px, scroll BACKWARD to 72 px.
    // Virtualizer item 0 is now Measured at 60 px.  With scroll=72,
    // visible range starts at item 1 (item 0 ends at 60 < 72) → anchor=(1,0).
    // set_measured(0, 84, (1,0)) accumulates pending=24.  But backward
    // scroll (72 < 92 = last_scroll_offset) → SUPPRESSED.  Viewport keeps
    // scroll=72.  Item 0 paint dy = 0 − 72 = −72 px.
    //
    // Fails when anchor-correction is not wired, when forward/backward
    // detection is inverted, or when the viewport's correction loop is broken.
    let mut run = RenderTester::mount(viewport_with_scroll(
        100.0,
        sliver_node(RenderSliverList::new(10, 48.0))
            .label("list")
            .child(
                box_node(RenderColoredBox::red(300.0, 60.0))
                    .label("item0")
                    .with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                        SliverMultiBoxAdaptorParentData::new(0),
                    )),
            ),
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let item0_id = run.id("item0");
    let vp_id = run.id("viewport");

    // Pass 1 check: the 12 px forward correction was absorbed by the viewport.
    // Correction loop: scroll 100→112 (correct_by), 112→92 (clamped by
    // apply_content_dimensions, max_scroll=492-400=92), 92 accepted.
    // Item 0 at layout_offset=0 with final scroll=92 gets paint dy = -92 px.
    assert_eq!(
        run.offset(item0_id).dy,
        px(-92.0),
        "forward correction loop: scroll 100→112→92 (clamped); \
         item 0 (layout_offset=0) must have dy=0-92=-92; got {:?}",
        run.offset(item0_id).dy,
    );

    // Pass 2: grow item 0 to 84 px, scroll backward to 72 px.
    run.update::<RenderColoredBox>(item0_id, |b| {
        b.set_preferred_size(Size::new(px(300.0), px(84.0)));
    });
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(72.0);
    });
    run.relayout();

    // Pass 2 check: backward scroll (72 < 92 = last_scroll_offset) suppresses
    // the 24 px correction → viewport stays at scroll=72.  Item 0 at
    // layout_offset=0 gets paint dy = 0 - 72 = -72 px.
    assert_eq!(
        run.offset(item0_id).dy,
        px(-72.0),
        "backward suppression: viewport stays at scroll=72; \
         item 0 (layout_offset=0) must have dy=0-72=-72; got {:?}",
        run.offset(item0_id).dy,
    );
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

// Compositing-hooks forwarding: RED→GREEN pipeline test.
//
// The `RenderSliver` blanket impl must forward `always_needs_compositing` from
// `dyn RenderObject<SliverProtocol>` to the concrete override — matching what
// the `RenderBox` blanket impl already does (render_box.rs:630).
//
// The pipeline compositing-bits walk (`PipelineOwner::update_subtree_compositing_bits`,
// owner/mod.rs:2355) calls `node.always_needs_compositing()`, which dispatches
// through `RenderNode` → `dyn RenderObject<SliverProtocol>::always_needs_compositing()`.
// Without the forward the vtable returns the default `false`, so a
// `RenderSliverOpacity` with partial alpha never gets its own compositing layer
// (silent correctness gap — tests still pass but the frame tree is wrong).
//
// Flutter parity: `RenderSliverOpacity.alwaysNeedsCompositing`
// (proxy_sliver.dart:128) = `child != null && _alpha > 0`.
// FLUI's `needs_compositing()` = `always_flag || (alpha > 0 && alpha != 255)`.
// The `alpha != 255` narrowing is intentional (opaque fast path; no layer needed).
#[test]
fn harness_sliver_opacity_always_needs_compositing_reaches_pipeline() {
    // After compositing phase runs, the pipeline node for the opacity sliver
    // must report `always_needs_compositing() == true` through `RenderNode`
    // (the exact path `owner/mod.rs:2355` uses).  Before the blanket-impl
    // forward was added this returned `false` regardless of alpha.
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOpacity::new(0.5)) // alpha = 128 — partial, needs compositing
            .label("opacity")
            .child(
                sliver_node(RenderSliverFixedExtentList::new(30.0))
                    .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
            ),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_to_compositing();

    let opacity_id = run.id("opacity");
    let node = run
        .pipeline()
        .render_tree()
        .get(opacity_id)
        .expect("opacity node must exist after compositing phase");
    assert!(
        node.always_needs_compositing(),
        "RenderSliverOpacity with partial alpha (0.5) must report \
         always_needs_compositing=true through RenderNode (the pipeline \
         compositing-bits walk path); blanket impl must forward via UFCS \
         (Flutter parity: alwaysNeedsCompositing = child != null && alpha > 0)"
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

// Regression coverage for the `RenderViewport::attempt_layout` sign bug
// documented in docs/research/widget-renderobject-map.md ("Two pre-existing
// infrastructure defects"): the forward sequence's `overlap` used
// `center_offset.min(0.0)` (== `(-corrected_offset).min(0.0)`) instead of the
// oracle's `corrected_offset.min(0.0)` (`rendering/viewport.dart:1834`,
// `overlap: leadingNegativeChild == null ? math.min(0.0, -centerOffset) :
// 0.0`). At a positive scroll offset with no leading reverse-growth group,
// `overlap` must be `0.0`; with one, it must be `0.0` for BOTH sequences.
// `RenderSliverFillRemainingWithScrollable` reads `constraints.overlap.min(0.0)`
// directly into its `extent` formula, so a wrong sign inflates `extent` and
// silently un-clamps `paint_extent` — the exact failure mode this guards.

#[test]
fn harness_viewport_forward_overlap_is_zero_without_leading_reverse_group() {
    let run = RenderTester::mount(viewport_with_scroll(
        50.0,
        sliver_node(RenderSliverFillRemainingWithScrollable::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("fill_child")),
    ))
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    assert_eq!(
        run.sliver_geometry(run.id("fill")).paint_extent,
        50.0,
        "overlap == -50.0 (the sign bug) inflates `extent` to 150.0, which \
         un-clamps `paint_extent` to 100.0 instead of the correct 50.0 \
         (100px viewport minus the 50px already-consumed scroll offset)",
    );
}

#[test]
fn harness_viewport_reverse_group_overlap_is_always_zero() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::new(50.0),
    );
    // `center_sliver_index(1)` splits the two children: child 0 (a plain
    // filler) lays out with forward growth, giving the viewport real forward
    // scroll range so `50.0` is a valid, unclamped offset; child 1 lays out
    // with reverse growth — the oracle's `leadingNegativeChild` case, where
    // `overlap` must be `0.0` for BOTH sequences (not just the forward one).
    viewport.set_center_sliver_index(Some(1));
    let node = box_node(viewport)
        .label("viewport")
        .child(
            sliver_node(RenderSliverToBoxAdapter::new())
                .label("forward_filler")
                .child(box_node(RenderColoredBox::red(300.0, 250.0)).label("forward_child")),
        )
        .child(
            sliver_node(RenderSliverFillRemainingWithScrollable::new())
                .label("fill")
                .child(box_node(RenderColoredBox::green(300.0, 10.0)).label("fill_child")),
        );

    let run = RenderTester::mount(node)
        .with_size(Size::new(px(300.0), px(100.0)))
        .run_layout();

    assert_eq!(
        run.sliver_geometry(run.id("fill")).paint_extent,
        50.0,
        "the reverse sequence must also report overlap == 0.0 (oracle: \
         unconditionally, `rendering/viewport.dart:1818`); before the fix \
         this reverse-group sliver saw the same wrongly-negative overlap \
         (inherited from the forward sequence's buggy value) and reported an \
         un-clamped paint_extent of 100.0",
    );
}

#[test]
fn harness_shrink_wrapping_viewport_sizes_to_sliver_extent_under_unbounded_main_axis() {
    let run = RenderTester::mount(shrink_wrapping_viewport(
        sliver_node(RenderSliverFixedExtentList::new(25.0))
            .label("list")
            .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("item0"))
            .child(box_node(RenderColoredBox::green(300.0, 1000.0)).label("item1")),
    ))
    .with_constraints(BoxConstraints::new(
        px(300.0),
        px(300.0),
        px(0.0),
        flui_types::Pixels::INFINITY,
    ))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(300.0), px(50.0)),
        "shrink-wrapping viewport must take its main-axis size from child max_paint_extent"
    );
    assert_eq!(run.sliver_geometry(run.id("list")).scroll_extent, 50.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderShrinkWrappingViewport",
        &["axis_direction", "scroll_offset", "shrink_wrap_extent"],
    );
}

#[test]
fn harness_shrink_wrapping_viewport_empty_uses_cross_axis_max_and_main_axis_min() {
    let run = RenderTester::mount(box_node(RenderShrinkWrappingViewport::new(
        AxisDirection::TopToBottom,
    )))
    .with_constraints(BoxConstraints::new(
        px(20.0),
        px(300.0),
        px(12.0),
        flui_types::Pixels::INFINITY,
    ))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(300.0), px(12.0)),
        "empty shrink-wrapping viewport follows Flutter's empty-size branch"
    );
}

#[test]
fn harness_shrink_wrapping_viewport_clamps_to_bounded_max_extent() {
    let run = RenderTester::mount(shrink_wrapping_viewport(
        sliver_node(RenderSliverFixedExtentList::new(50.0))
            .label("list")
            .child(box_node(RenderColoredBox::red(300.0, 1000.0)))
            .child(box_node(RenderColoredBox::green(300.0, 1000.0)))
            .child(box_node(RenderColoredBox::blue(300.0, 1000.0)))
            .child(box_node(RenderColoredBox::red(300.0, 1000.0))),
    ))
    .with_constraints(BoxConstraints::new(
        px(300.0),
        px(300.0),
        px(0.0),
        px(120.0),
    ))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(300.0), px(120.0)),
        "parent max height must clamp the shrink-wrapped viewport"
    );
    assert_eq!(
        run.sliver_geometry(run.id("list")).scroll_extent,
        200.0,
        "content scroll extent remains the full sliver extent after viewport clamp"
    );
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
        .map_or_else(
            || panic!("child must be in the hit path at ({HIT_X}, {HIT_Y})"),
            |(_, t)| *t,
        );

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
// Wrap
// ============================================================================

#[test]
fn harness_render_wrap_wraps_to_second_run() {
    // Three 40×40 boxes in a max-100-wide loose constraint.
    // Run 1: a(40) + b(40) = 80 ≤ 100. Run 2: c(40) wraps.
    // Container: constrain(80 main, 80 cross) within [0,100]×[0,100] = (80,80).
    //
    // This assertion FAILS if wrapping is not implemented — without wrapping,
    // c would be placed at main=80 instead of starting a new run at cross=40.
    let run = RenderTester::mount(
        box_node(RenderWrap::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("a"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("c")),
    )
    .with_constraints(loose(100.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(80.0), px(80.0)));
    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    assert_eq!(run.offset(run.id("b")), Offset::new(px(40.0), px(0.0)));
    // Wrap proof: c must be on a new row, not overflowing the first.
    assert_eq!(
        run.offset(run.id("c")),
        Offset::new(px(0.0), px(40.0)),
        "c must wrap to a second run, not overflow the first",
    );
}

#[test]
fn harness_render_wrap_spacing_and_run_spacing_add_gaps() {
    // Three 30×20 boxes, spacing=10, run_spacing=5, loose(100).
    // Run 1: a(30) + gap(10) + b(30) = 70. Next: 70+10+30=110 > 100 → wrap.
    // Run 2: c. max_run_main=70, total_cross=20+5+20=45.
    // Container: (70, 45). a@(0,0), b@(40,0), c@(0,25).
    let run = RenderTester::mount(
        box_node(RenderWrap::new().with_spacing(10.0).with_run_spacing(5.0))
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(30.0, 20.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(30.0, 20.0)).label("c")),
    )
    .with_constraints(loose(100.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(70.0), px(45.0)));
    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    // b is offset by 30 (a width) + 10 (spacing).
    assert_eq!(run.offset(run.id("b")), Offset::new(px(40.0), px(0.0)));
    // c is on the second run: cross_offset = run 1 cross(20) + run_spacing(5).
    assert_eq!(run.offset(run.id("c")), Offset::new(px(0.0), px(25.0)));
}

// ── RenderWrap dry layout ─────────────────────────────────────────────────────

/// `compute_dry_layout` for a three-child wrap that breaks into two runs.
///
/// Oracle sizes from `harness_render_wrap_wraps_to_second_run`: three 40×40
/// children in a loose-100 container. Run 1: a(40)+b(40)=80. Run 2: c(40)
/// wraps. Container: constrain(80 main, 80 cross) = (80, 80). This test
/// returns `Size::ZERO` with the default trait implementation and passes only
/// once `RenderWrap::compute_dry_layout` delegates to `compute_runs`.
#[test]
fn harness_render_wrap_dry_layout_multi_run() {
    let constraints = loose(100.0);
    let mut run = RenderTester::mount(
        box_node(RenderWrap::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("a"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("c")),
    )
    .with_constraints(constraints)
    .run_layout();

    let expected = Size::new(px(80.0), px(80.0));
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        expected,
        "wrap dry layout must break into two runs and report the correct container size",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        run.box_geometry(run.root()),
        "dry layout must agree with committed layout geometry",
    );
}

/// `compute_dry_layout` for a wrap with `spacing` and `run_spacing`.
///
/// Oracle sizes from `harness_render_wrap_spacing_and_run_spacing_add_gaps`:
/// three 30×20 children, spacing=10, run_spacing=5, loose(100).
/// Run 1: a(30)+gap(10)+b(30)=70; next child (30+10+30=110 > 100) wraps.
/// Run 2: c(30). max_run_main=70, total_cross=20+5+20=45. Container: (70, 45).
#[test]
fn harness_render_wrap_dry_layout_with_spacing_and_run_spacing() {
    let constraints = loose(100.0);
    let mut run = RenderTester::mount(
        box_node(RenderWrap::new().with_spacing(10.0).with_run_spacing(5.0))
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(30.0, 20.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(30.0, 20.0)).label("c")),
    )
    .with_constraints(constraints)
    .run_layout();

    let expected = Size::new(px(70.0), px(45.0));
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        expected,
        "wrap dry layout with spacing/run_spacing must report the correct container size",
    );
    assert_eq!(
        run.dry_layout(run.root(), constraints),
        run.box_geometry(run.root()),
        "dry layout must agree with committed layout geometry",
    );
}

#[test]
fn harness_render_wrap_center_alignment_distributes_main_axis_free_space() {
    // Two 30×20 boxes in a tight-100-wide container, alignment=Center.
    // Run 1 main_extent=60. container_main=100. free=40.
    // Center: leading=20, between=0.
    // a@(20,0), b@(50,0).
    let run = RenderTester::mount(
        box_node(RenderWrap::new().with_alignment(WrapAlignment::Center))
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(30.0, 20.0)).label("b")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_layout();

    assert_eq!(run.offset(run.id("a")), Offset::new(px(20.0), px(0.0)));
    assert_eq!(run.offset(run.id("b")), Offset::new(px(50.0), px(0.0)));
}

#[test]
fn harness_render_wrap_cross_axis_alignment_centers_short_child_within_run() {
    // Two children in one run: a=40×40, b=40×10. Run cross=40.
    // WrapCrossAlignment::Center: b's cross offset = (40−10)/2 = 15.
    let run = RenderTester::mount(
        box_node(RenderWrap::new().with_cross_axis_alignment(WrapCrossAlignment::Center))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("a"))
            .child(box_node(RenderColoredBox::green(40.0, 10.0)).label("b")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // a is tall, b is short → b gets a 15px cross-axis offset to centre it.
    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    assert_eq!(
        run.offset(run.id("b")),
        Offset::new(px(40.0), px(15.0)),
        "shorter child must be centred within the run's cross extent",
    );
}

#[test]
fn harness_render_wrap_hit_tests_last_child_first() {
    // Two overlapping children (both at origin when loose): last is on top.
    // Because wrap places each child sequentially, they don't overlap here,
    // but we verify hit_test descends through all children in reverse order.
    let run = RenderTester::mount(
        box_node(RenderWrap::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("first"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("second")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("first")));
    assert_eq!(run.hit_first(60.0, 20.0), Some(run.id("second")));
}

// ============================================================================
// RenderIntrinsicWidth
// ============================================================================

#[test]
fn harness_intrinsic_width_leaf_sizes_to_zero() {
    // Without a child, a leaf IntrinsicWidth should shrink-wrap to zero
    // (constraints.smallest() with min_w=0).
    let run = RenderTester::mount(box_node(RenderIntrinsicWidth::unconstrained()))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
    assert_descendant_properties(&run.diagnostics(), "RenderIntrinsicWidth", &[]);
}

#[test]
fn harness_intrinsic_width_with_child_passes_size_through() {
    // Without step snapping the child's natural size is propagated unchanged
    // through constrain().
    let run = RenderTester::mount(
        box_node(RenderIntrinsicWidth::unconstrained())
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // IntrinsicWidth forwards unconstrained → child size = 60×40 → constrain
    // under 0..200 → stays 60×40.
    assert_eq!(run.box_geometry(run.root()), Size::new(px(60.0), px(40.0)));
}

#[test]
fn harness_intrinsic_width_self_describes_step_knobs() {
    let run = RenderTester::mount(box_node(RenderIntrinsicWidth::new(Some(20.0), Some(10.0))))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderIntrinsicWidth",
        &["step_width", "step_height"],
    );
}

// ---- Slice-2 milestone: dry == committed for filling child ----------------

/// `RenderIntrinsicWidth::unconstrained()` over a `RenderFlex` row (`MainAxisSize::Max`)
/// of two `50×30` children forces the child's width to its max intrinsic width (100)
/// in both `perform_layout` (real pass) and `compute_dry_layout` (dry pass).
///
/// Oracle cross-check:
/// - `RenderFlex::computeMaxIntrinsicWidth` (flex.dart) = sum of children = 100.
/// - `RenderIntrinsicWidth._childConstraints` (proxy_box.dart:712-720): not tight →
///   force to `_applyStep(child.getMaxIntrinsicWidth(maxHeight), null) = 100`; no
///   step_height so height unchanged; tighten → tight(100, ...).
/// - `_computeSize(dryLayoutChild, tight(100, [0..300]))` → flex at 100px →
///   width=100, height=30.
///
/// RED before Slice 2 (`compute_dry_layout` used a `child_dry_layout` approximation
/// that returned the loose max 500 instead of the intrinsic 100).
/// GREEN after Slice 2 (channel routes through `intrinsic_query`).
#[test]
fn harness_intrinsic_width_forces_filling_child() {
    let constraints = BoxConstraints::new(px(0.0), px(500.0), px(0.0), px(300.0));
    let mut run = RenderTester::mount(
        box_node(RenderIntrinsicWidth::unconstrained()).child(
            box_node(RenderFlex::row())
                .label("flex")
                .child(box_node(RenderColoredBox::red(50.0, 30.0)))
                .child(box_node(RenderColoredBox::red(50.0, 30.0))),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    let committed = run.box_geometry(run.root());
    let dry = run.dry_layout(run.root(), constraints);

    assert_eq!(
        committed,
        Size::new(px(100.0), px(30.0)),
        "perform_layout must force child to intrinsic width 100, not loose 500"
    );
    assert_eq!(
        dry,
        Size::new(px(100.0), px(30.0)),
        "compute_dry_layout must equal perform_layout (dry==committed invariant)"
    );
    assert_eq!(
        dry, committed,
        "dry and committed must agree: dry={dry:?}, committed={committed:?}"
    );
}

/// `RenderIntrinsicHeight::new()` over a `RenderFlex` row (`MainAxisSize::Max`)
/// of two `50×30` children forces the child's height to its max intrinsic height (30)
/// in both `perform_layout` and `compute_dry_layout`.
///
/// Oracle cross-check:
/// - `RenderIntrinsicHeight._childConstraints` (proxy_box.dart:816-819): not tight →
///   force to `child.getMaxIntrinsicHeight(constraints.maxWidth)` = 30;
///   tighten → tight(h=30).
/// - Flex at tight(h=30): cross=30, flex fills main up to 500 → `500×30`.
/// - `constraints.constrain(500×30)` = `500×30`.
/// - Dry path with intrinsic channel: same math, same result.
///
/// RED before Slice 2 (approximation via `child_dry_layout` at tight width,
/// which for a flex row diverges from the intrinsic for width-filling children).
/// GREEN after Slice 2.
#[test]
fn harness_intrinsic_height_forces_filling_child() {
    let constraints = BoxConstraints::new(px(0.0), px(500.0), px(0.0), px(300.0));
    let mut run = RenderTester::mount(
        box_node(RenderIntrinsicHeight::new()).child(
            box_node(RenderFlex::row())
                .label("flex")
                .child(box_node(RenderColoredBox::red(50.0, 30.0)))
                .child(box_node(RenderColoredBox::red(50.0, 30.0))),
        ),
    )
    .with_constraints(constraints)
    .run_layout();

    let committed = run.box_geometry(run.root());
    let dry = run.dry_layout(run.root(), constraints);

    // The child (flex row) has max intrinsic height = 30 (the max child height).
    // IntrinsicHeight tightens to 30, flex at tight(h=30) fills main → 500×30,
    // constrain to [0..500, 0..300] → 500×30.
    assert_eq!(
        committed,
        Size::new(px(500.0), px(30.0)),
        "perform_layout must force child to intrinsic height 30"
    );
    assert_eq!(
        dry,
        Size::new(px(500.0), px(30.0)),
        "compute_dry_layout must equal perform_layout (dry==committed invariant)"
    );
    assert_eq!(
        dry, committed,
        "dry and committed must agree: dry={dry:?}, committed={committed:?}"
    );
}

// ---- Slice-1 channel proof ------------------------------------------------

/// Verify that `BoxDryLayoutCtx::child_max_intrinsic_width` (the new intrinsic
/// channel added by ADR-0011 Slice 1) routes through the real memoized
/// `intrinsic_query` and returns the same value as a standalone
/// `max_intrinsic_width` call on the child.
///
/// This test uses a thin proxy whose `compute_dry_layout` records the
/// intrinsic it receives so the harness can assert equality.  It is
/// GREEN after Slice 1 (channel wired) and would be RED before it
/// (the accessor did not exist).
#[test]
fn harness_dry_layout_child_intrinsic_channel_matches_standalone_query() {
    use std::sync::{Arc, Mutex};

    use flui_rendering::{
        constraints::BoxConstraints,
        context::{BoxDryLayoutCtx, BoxIntrinsicsCtx},
        parent_data::BoxParentData,
        traits::RenderBox,
    };
    use flui_tree::Single;

    // Shared cell: `compute_dry_layout` writes the child intrinsic it observed.
    let captured: Arc<Mutex<f32>> = Arc::new(Mutex::new(f32::NAN));

    // Inline proxy whose only job is to expose the child's max-intrinsic-width
    // during a dry-layout pass.
    #[derive(Debug)]
    struct IntrinsicCapture {
        captured: Arc<Mutex<f32>>,
    }

    impl flui_foundation::Diagnosticable for IntrinsicCapture {
        fn debug_fill_properties(&self, _b: &mut flui_foundation::DiagnosticsBuilder) {}
    }

    impl RenderBox for IntrinsicCapture {
        type Arity = Single;
        type ParentData = BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Single, BoxParentData>,
        ) -> Size {
            // Pass constraints through to the child and forward the child size.
            let child_size = ctx.layout_child(0, *ctx.constraints());
            ctx.position_child(0, Offset::ZERO);
            child_size
        }

        fn compute_max_intrinsic_width(
            &self,
            _height: f32,
            _ctx: &mut BoxIntrinsicsCtx<'_>,
        ) -> f32 {
            0.0
        }

        fn compute_dry_layout(
            &self,
            constraints: BoxConstraints,
            ctx: &mut BoxDryLayoutCtx<'_>,
        ) -> Size {
            // Read the child's max intrinsic width through the new channel.
            let via_channel = ctx.child_max_intrinsic_width(0, f32::INFINITY);
            *self.captured.lock().unwrap() = via_channel;
            // Return the child dry size so the tree is structurally valid.
            ctx.child_dry_layout(0, constraints)
        }
    }

    // Build: IntrinsicCapture → RenderFlex row [ColoredBox(50x30), ColoredBox(50x30)]
    // Flex row max-intrinsic-width = sum of children = 100.
    let mut run = RenderTester::mount(
        box_node(IntrinsicCapture {
            captured: Arc::clone(&captured),
        })
        .child(
            box_node(RenderFlex::row())
                .label("flex")
                .child(box_node(RenderColoredBox::red(50.0, 30.0)))
                .child(box_node(RenderColoredBox::red(50.0, 30.0))),
        ),
    )
    .with_constraints(loose(500.0))
    .run_layout();

    let flex_id = run.id("flex");

    // Trigger dry-layout on the root (which will call compute_dry_layout on the
    // capture proxy, which in turn calls child_max_intrinsic_width).
    let constraints = BoxConstraints::new(px(0.0), px(500.0), px(0.0), px(300.0));
    run.dry_layout(run.root(), constraints);

    let via_channel = *captured.lock().unwrap();
    assert!(
        !via_channel.is_nan(),
        "compute_dry_layout was not called — channel not exercised"
    );

    // The standalone query must agree with what the channel reported.
    let standalone = run.max_intrinsic_width(flex_id, f32::INFINITY);
    assert_eq!(
        via_channel, standalone,
        "dry-layout child_max_intrinsic_width ({via_channel}) != \
         standalone max_intrinsic_width ({standalone})"
    );
    // Concretely: flex row of two 50-wide children → 100.
    assert_eq!(
        via_channel, 100.0,
        "flex intrinsic width should be 100 (2 × 50)"
    );
}

// ============================================================================
// RenderIntrinsicHeight
// ============================================================================

#[test]
fn harness_intrinsic_height_leaf_sizes_to_zero() {
    let run = RenderTester::mount(box_node(RenderIntrinsicHeight::new()))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
    assert_descendant_properties(&run.diagnostics(), "RenderIntrinsicHeight", &[]);
}

#[test]
fn harness_intrinsic_height_with_child_passes_size_through() {
    // `perform_layout` calls `ctx.child_max_intrinsic_height(0, max_width)` through
    // the live `box_intrinsic_query_borrowed` pipeline callback.  The child
    // (ColoredBox 60×40) reports max intrinsic height = 40px, so the child is
    // laid out at height tight to 40 and the result is 60×40.
    //
    // Oracle: proxy_box.dart:816-819 — `_childConstraints` forces height to the
    // child's `getMaxIntrinsicHeight(constraints.maxWidth)`.  For a fixed-size
    // child that is 40px tall, this gives tight(h=40).
    let run = RenderTester::mount(
        box_node(RenderIntrinsicHeight::new())
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(px(60.0), px(40.0)));
}

// ============================================================================
// RenderConstrainedOverflowBox
// ============================================================================

#[test]
fn harness_constrained_overflow_box_max_fit_claims_full_parent() {
    // Max fit (default): OverflowBox claims all of its loose parent space.
    let run = RenderTester::mount(
        box_node(RenderConstrainedOverflowBox::centered())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // Max fit: claimed size = constraints.biggest() = 200×200.
    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(200.0), px(200.0)),
        "OverflowBoxFit::Max must claim all available space",
    );
}

#[test]
fn harness_constrained_overflow_box_defer_to_child_shrink_wraps() {
    // DeferToChild: reported size follows constrain(child_size).
    let run = RenderTester::mount(
        box_node(RenderConstrainedOverflowBox::new(
            Alignment::CENTER,
            None,
            None,
            None,
            None,
            OverflowBoxFit::DeferToChild,
        ))
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(40.0), px(40.0)),
        "OverflowBoxFit::DeferToChild must constrain child size back to parent",
    );
}

#[test]
fn harness_constrained_overflow_box_self_describes_fit() {
    let run = RenderTester::mount(box_node(RenderConstrainedOverflowBox::new(
        Alignment::CENTER,
        None,
        Some(px(300.0)),
        None,
        None,
        OverflowBoxFit::Max,
    )))
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderConstrainedOverflowBox", &["fit"]);
}

// ============================================================================
// RenderSizedOverflowBox
// ============================================================================

#[test]
fn harness_sized_overflow_box_reports_requested_size() {
    // The box claims requested_size (clamped to constraints) regardless of child.
    let run = RenderTester::mount(
        box_node(RenderSizedOverflowBox::centered(80.0, 60.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(80.0), px(60.0)),
        "SizedOverflowBox must report the requested size, not the child size",
    );
}

#[test]
fn harness_sized_overflow_box_child_lays_out_under_incoming_constraints() {
    // Key contract: child sees the PARENT constraints, not the requested size.
    // Under loose(200) the child (fixed 40×40 ColoredBox) stays at 40×40,
    // even though the box claims 80×60.
    let run = RenderTester::mount(
        box_node(RenderSizedOverflowBox::centered(80.0, 60.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(px(40.0), px(40.0)),
        "child must be laid out under incoming constraints, not the requested size",
    );
}

#[test]
fn harness_sized_overflow_box_intrinsics_report_requested_size() {
    // RenderSizedOverflowBox OVERRIDES all four intrinsics to its requested
    // size (Flutter shifted_box.dart). The child's larger intrinsic (200×100)
    // must NOT leak through — the box reports 80×60. (Before the fix the
    // intrinsics delegated to the child and returned 200/100.)
    let mut run = RenderTester::mount(
        box_node(RenderSizedOverflowBox::centered(80.0, 60.0))
            .child(box_node(RenderColoredBox::red(200.0, 100.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let node = run.root();
    assert_eq!(run.min_intrinsic_width(node, 100.0), 80.0);
    assert_eq!(run.max_intrinsic_width(node, 100.0), 80.0);
    assert_eq!(run.min_intrinsic_height(node, 100.0), 60.0);
    assert_eq!(run.max_intrinsic_height(node, 100.0), 60.0);
}

#[test]
fn harness_sized_overflow_box_self_describes() {
    let run = RenderTester::mount(box_node(RenderSizedOverflowBox::centered(80.0, 60.0)))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderSizedOverflowBox",
        &["requested_width", "requested_height"],
    );
}

// ============================================================================
// RenderRotatedBox
// ============================================================================

#[test]
fn harness_rotated_box_even_turns_preserves_size() {
    // 0 quarter turns: child size is unchanged.
    let run = RenderTester::mount(
        box_node(RenderRotatedBox::new(0))
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(60.0), px(40.0)),
        "0 quarter turns must preserve child dimensions",
    );
}

#[test]
fn harness_rotated_box_odd_turns_swaps_axes() {
    // 1 quarter turn: child is constrained under flipped constraints (200h×200w),
    // then size is swapped: child 60×40 → parent reports 40×60.
    let run = RenderTester::mount(
        box_node(RenderRotatedBox::new(1))
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // After 90°: width becomes height and vice versa.
    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(40.0), px(60.0)),
        "1 quarter turn must swap child width↔height for the parent-reported size",
    );
}

#[test]
fn harness_rotated_box_two_turns_is_same_size_as_zero() {
    // 2 quarter turns = 180°: axes are not swapped (even).
    let run = RenderTester::mount(
        box_node(RenderRotatedBox::new(2))
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(60.0), px(40.0)),
        "2 quarter turns must not swap dimensions",
    );
}

#[test]
fn harness_rotated_box_leaf_sizes_to_zero_even_turns() {
    let run = RenderTester::mount(box_node(RenderRotatedBox::new(0)))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
    assert_descendant_properties(&run.diagnostics(), "RenderRotatedBox", &["quarter_turns"]);
}

#[test]
fn harness_rotated_box_hit_test_90_degree_within_child() {
    // After a 90° rotation (1 quarter turn) the child occupies a rotated
    // region.  The paint matrix maps child (0,0)..(60,40) into the parent frame
    // as a rotated rectangle centered in the parent slot (40×60).
    //
    // Parent size: 40×60 (swapped child).
    // Paint matrix: translate(20,30) * rotate(90°) * translate(-30,-20).
    // Child center in parent coords: (20, 30).
    //
    // A pointer at parent (20, 30) should hit the child (it maps to child center
    // (30, 20) which is inside the 60×40 child).
    let run = RenderTester::mount(
        box_node(RenderRotatedBox::new(1))
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    // The center of the parent slot — should always hit the child's center.
    let center_x = run.box_geometry(run.root()).width.get() / 2.0;
    let center_y = run.box_geometry(run.root()).height.get() / 2.0;
    assert!(
        run.hit(center_x, center_y).contains(&run.root()),
        "pointer at parent center must hit the rotated child",
    );
}

#[test]
fn harness_rotated_box_negative_quarter_turn_swaps_axes() {
    // -1 quarter turn (counter-clockwise 90°) is still odd → axes swapped.
    let run = RenderTester::mount(
        box_node(RenderRotatedBox::new(-1))
            .child(box_node(RenderColoredBox::red(60.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(40.0), px(60.0)),
        "-1 quarter turn (odd) must swap child width↔height",
    );
}

#[test]
fn harness_render_wrap_diagnostics_reports_all_properties() {
    let run = RenderTester::mount(box_node(RenderWrap::new()))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderWrap",
        &[
            "direction",
            "alignment",
            "run_alignment",
            "cross_axis_alignment",
        ],
    );
}

// ============================================================================
// RenderFlow — paint-time transform layout
// ============================================================================

/// Translates child `i` by `i * step` along x. Mirrors
/// `flow_delegate.rs`'s `LinearFlowDelegate` test fixture.
#[derive(Debug)]
struct StepFlowDelegate {
    step: f32,
}

impl FlowDelegate for StepFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        BoxConstraints::loose(constraints.biggest())
    }

    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
        for i in 0..context.child_count() {
            context.paint_child(i, Matrix4::translation(i as f32 * self.step, 0.0, 0.0));
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Paints every child at the SAME transform (fully overlapping) — isolates
/// paint-order effects from position effects for the reverse-hit-test case.
#[derive(Debug)]
struct OverlappingFlowDelegate;

impl FlowDelegate for OverlappingFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        BoxConstraints::loose(constraints.biggest())
    }

    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
        for i in 0..context.child_count() {
            context.paint_child(i, Matrix4::IDENTITY);
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Paints child 0 with a degenerate (zero-scale, non-invertible) transform;
/// every other child gets an ordinary translation.
#[derive(Debug)]
struct DegenerateFlowDelegate;

impl FlowDelegate for DegenerateFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        BoxConstraints::loose(constraints.biggest())
    }

    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
        for i in 0..context.child_count() {
            let transform = if i == 0 {
                Matrix4::scaling(0.0, 0.0, 1.0)
            } else {
                Matrix4::translation(i as f32 * 50.0, 0.0, 0.0)
            };
            context.paint_child(i, transform);
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn harness_flow_paints_children_in_delegate_order_under_per_child_transform_layers() {
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(StepFlowDelegate { step: 30.0 })))
            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(20.0, 20.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(20.0, 20.0)).label("c")),
    )
    .with_size(Size::new(px(200.0), px(50.0)))
    .run_frame();

    let painted = run
        .display_commands()
        .into_iter()
        .map(|cmd| cmd.line)
        .collect::<Vec<_>>();
    let rects = painted
        .iter()
        .filter(|line| line.contains("DrawRect"))
        .collect::<Vec<_>>();
    assert_eq!(
        rects.len(),
        3,
        "expected exactly 3 child DrawRects; commands:\n{}",
        painted.join("\n"),
    );
    assert!(
        rects[0].contains("#FF0000FF")
            && rects[1].contains("#00FF00FF")
            && rects[2].contains("#0000FFFF"),
        "paint order must follow the delegate's paint_child call order (red, green, blue); commands:\n{}",
        painted.join("\n"),
    );

    // Each child must be wrapped in its OWN Transform layer — proof that
    // paint emits a per-child transform, not one shared node-level
    // transform (which `RenderObject::paint_transform` already supports
    // and would show up as a single Transform layer regardless of child
    // count).
    let transform_layers = run
        .structure()
        .iter()
        .filter(|kind| **kind == "Transform")
        .count();
    assert_eq!(
        transform_layers,
        3,
        "expected one Transform layer per child (3), got structure: {:?}",
        run.structure(),
    );
}

#[test]
fn harness_flow_hit_test_uses_the_real_per_child_transform_not_layout_offset() {
    // Layout always positions every Flow child at Offset::ZERO (paint-time
    // transform is the ONLY thing that moves them) — so a naive hit-test
    // that used the layout offset instead of the delegate's real transform
    // would see every child at the SAME [0,40)x[0,40) box. x=70 is outside
    // that shared box entirely; it only resolves to child "b" by inverting
    // b's real +50px translation.
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(StepFlowDelegate { step: 50.0 })))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("a"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("c")),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("a")));
    assert_eq!(
        run.hit_first(70.0, 20.0),
        Some(run.id("b")),
        "x=70 lies outside every child's shared zero-offset box [0,40) — only \
         inverting child b's real +50px transform correctly resolves the hit",
    );
    assert_eq!(run.hit_first(120.0, 20.0), Some(run.id("c")));
    assert!(
        run.hit(250.0, 20.0).is_empty(),
        "outside every child's translated box must be a genuine miss",
    );
}

#[test]
fn harness_flow_hit_test_walks_paint_order_in_reverse_topmost_first() {
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(OverlappingFlowDelegate)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("bottom"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("top")),
    )
    .with_size(Size::new(px(40.0), px(40.0)))
    .run_frame();

    assert_eq!(
        run.hit_first(20.0, 20.0),
        Some(run.id("top")),
        "the child painted LAST (index 1, visually on top) must win an overlapping \
         hit — RenderFlow.hitTestChildren walks paint order in reverse (oracle L430)",
    );
}

#[test]
fn harness_flow_degenerate_transform_is_never_hit_but_siblings_still_are() {
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(DegenerateFlowDelegate)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("zeroed"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("normal")),
    )
    .with_size(Size::new(px(200.0), px(100.0)))
    .run_frame();

    // The zero-scale child collapses to a single point; no finite position
    // can hit it, and its inverse doesn't exist so `RenderFlow::hit_test`
    // must skip it outright rather than panicking or matching everything.
    assert!(run.hit(0.0, 0.0).is_empty());
    assert!(run.hit(10.0, 10.0).is_empty());
    // The sibling at a real translation is unaffected by child 0's
    // degenerate transform.
    assert_eq!(run.hit_first(70.0, 20.0), Some(run.id("normal")));
}

#[test]
fn harness_flow_clip_behavior_gates_the_clip_layer() {
    let clipped = RenderTester::mount(
        box_node(
            RenderFlow::new(Arc::new(StepFlowDelegate { step: 10.0 }))
                .with_clip_behavior(Clip::HardEdge),
        )
        .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();
    assert!(
        clipped.structure().contains(&"ClipRect"),
        "Clip::HardEdge must emit a ClipRect layer; structure: {:?}",
        clipped.structure(),
    );

    let unclipped = RenderTester::mount(
        box_node(
            RenderFlow::new(Arc::new(StepFlowDelegate { step: 10.0 }))
                .with_clip_behavior(Clip::None),
        )
        .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
    )
    .with_size(Size::new(px(100.0), px(100.0)))
    .run_frame();
    assert!(
        !unclipped.structure().contains(&"ClipRect"),
        "Clip::None must NOT emit a ClipRect layer; structure: {:?}",
        unclipped.structure(),
    );
}

#[test]
fn harness_flow_set_delegate_reports_relayout_and_diagnostics() {
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(StepFlowDelegate { step: 5.0 })))
            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_descendant_properties(&run.diagnostics(), "RenderFlow", &["clip_behavior"]);
}

// ============================================================================
// RenderTable
// ============================================================================

/// Tight width (forces `_computeColumnWidths`' pass 2 to grow the Flex column
/// to fill the remainder), loose height (so the table's own height comes from
/// content, not the incoming constraints).
fn table_tight_width_loose_height(width: f32, max_height: f32) -> BoxConstraints {
    BoxConstraints::new(px(width), px(width), px(0.0), px(max_height))
}

#[test]
fn harness_table_grid_lays_out_each_cell_at_its_exact_offset_and_size() {
    // 2 columns: Fixed(50) + Flex(1.0, the default) under a tight 200px
    // width -> column widths resolve to [50, 150] (pass 2 grows the flex
    // column to fill the 150px remainder). Row heights are each row's
    // tallest cell: row 0 = max(20, 30) = 30; row 1 = max(15, 10) = 15.
    let run = RenderTester::mount(
        box_node(
            RenderTable::new(2)
                .with_column_widths(HashMap::from([(0, TableColumnWidth::Fixed(50.0))])),
        )
        .child(box_node(RenderColoredBox::red(50.0, 20.0)).label("a"))
        .child(box_node(RenderColoredBox::green(150.0, 30.0)).label("b"))
        .child(box_node(RenderColoredBox::blue(50.0, 15.0)).label("c"))
        .child(box_node(RenderColoredBox::red(150.0, 10.0)).label("d")),
    )
    .with_constraints(table_tight_width_loose_height(200.0, 800.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(200.0), px(45.0)),
        "table size must be the sum of resolved column widths (200) and row heights (30+15)",
    );

    assert_eq!(run.offset(run.id("a")), Offset::new(px(0.0), px(0.0)));
    assert_eq!(run.box_geometry(run.id("a")), Size::new(px(50.0), px(20.0)));

    assert_eq!(run.offset(run.id("b")), Offset::new(px(50.0), px(0.0)));
    assert_eq!(
        run.box_geometry(run.id("b")),
        Size::new(px(150.0), px(30.0))
    );

    assert_eq!(run.offset(run.id("c")), Offset::new(px(0.0), px(30.0)));
    assert_eq!(run.box_geometry(run.id("c")), Size::new(px(50.0), px(15.0)));

    assert_eq!(run.offset(run.id("d")), Offset::new(px(50.0), px(30.0)));
    assert_eq!(
        run.box_geometry(run.id("d")),
        Size::new(px(150.0), px(10.0))
    );

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderTable",
        &["column_count", "default_vertical_alignment"],
    );
}

#[test]
fn harness_table_paints_row_decoration_then_children_then_border_in_order() {
    // 1 row x 2 columns, uniform border (so the outer edge is one DrawDRRect)
    // plus a solid `vertical_inside` (so there's exactly one interior line —
    // no `horizontal_inside` line since there's only 1 row).
    let border = TableBorder::all(BorderSide::new(Color::BLUE, px(2.0), BorderStyle::Solid));
    let run = RenderTester::mount(
        box_node(
            RenderTable::new(2)
                .with_row_decorations(vec![Some(BoxDecoration::with_color(Color::RED))])
                .with_border(Some(border)),
        )
        .child(box_node(RenderColoredBox::green(20.0, 10.0)).label("a"))
        .child(box_node(RenderColoredBox::green(20.0, 10.0)).label("b")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    let commands = run.display_commands();
    let kinds: Vec<_> = commands.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![
            DrawKind::Rect,   // row decoration
            DrawKind::Rect,   // cell "a"
            DrawKind::Rect,   // cell "b"
            DrawKind::Path,   // vertical_inside interior line
            DrawKind::DRRect, // uniform outer border
        ],
        "paint order must be decoration -> children -> border; commands:\n{}",
        commands
            .iter()
            .map(|c| c.line.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    assert!(commands[0].line.contains("#FF0000FF"), "{:?}", commands[0]);
    assert!(commands[1].line.contains("#00FF00FF"), "{:?}", commands[1]);
    assert!(commands[2].line.contains("#00FF00FF"), "{:?}", commands[2]);
    assert!(commands[4].line.contains("#0000FFFF"), "{:?}", commands[4]);
}

#[test]
fn harness_table_border_interior_lines_sit_exactly_on_the_column_and_row_boundaries() {
    // 2x2 grid of 20x10 cells (all Flex(1.0) columns share the 40px width
    // equally -> column boundary at x=20; row boundary at y=10).
    let border = TableBorder::all(BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid));
    let run = RenderTester::mount(
        box_node(RenderTable::new(2).with_border(Some(border)))
            .child(box_node(RenderColoredBox::red(20.0, 10.0)))
            .child(box_node(RenderColoredBox::red(20.0, 10.0)))
            .child(box_node(RenderColoredBox::red(20.0, 10.0)))
            .child(box_node(RenderColoredBox::red(20.0, 10.0))),
    )
    .with_constraints(table_tight_width_loose_height(40.0, 800.0))
    .run_frame();

    let commands = run.display_commands();
    let vertical_line = commands
        .iter()
        .find(|c| c.kind == DrawKind::Path)
        .expect("the interior vertical line must be a DrawPath command");
    assert!(
        vertical_line
            .line
            .contains("bounds=(20.00,0.00 0.00x20.00)"),
        "interior vertical line must run the full table height (20) at the \
         column boundary x=20; got: {}",
        vertical_line.line,
    );

    let horizontal_line = commands
        .iter()
        .filter(|c| c.kind == DrawKind::Path)
        .nth(1)
        .expect("the interior horizontal line must be a second DrawPath command");
    assert!(
        horizontal_line
            .line
            .contains("bounds=(0.00,10.00 40.00x0.00)"),
        "interior horizontal line must run the full table width (40) at the \
         row boundary y=10; got: {}",
        horizontal_line.line,
    );
}

#[test]
fn harness_table_hit_test_per_cell_and_miss_outside_bounds() {
    let run = RenderTester::mount(
        box_node(
            RenderTable::new(2)
                .with_column_widths(HashMap::from([(0, TableColumnWidth::Fixed(50.0))])),
        )
        .child(box_node(RenderColoredBox::red(50.0, 20.0)).label("a"))
        .child(box_node(RenderColoredBox::green(150.0, 30.0)).label("b"))
        .child(box_node(RenderColoredBox::blue(50.0, 15.0)).label("c"))
        .child(box_node(RenderColoredBox::red(150.0, 10.0)).label("d")),
    )
    .with_constraints(table_tight_width_loose_height(200.0, 800.0))
    .run_frame();

    assert_eq!(run.hit_first(10.0, 10.0), Some(run.id("a")));
    assert_eq!(run.hit_first(100.0, 10.0), Some(run.id("b")));
    assert_eq!(run.hit_first(10.0, 35.0), Some(run.id("c")));
    assert_eq!(run.hit_first(100.0, 35.0), Some(run.id("d")));
    assert!(
        run.hit(10.0, 999.0).is_empty(),
        "a point below the table's own bounds must be a genuine miss",
    );
}

#[test]
fn harness_table_baseline_alignment_lines_up_cells_on_their_shared_baseline() {
    // Both cells opt into `Baseline` alignment; the table-wide baseline
    // (`before_baseline`) is the max reported baseline in the row (30, from
    // "tall"). "short" (baseline 10) must be pushed down by 30 - 10 = 20 so
    // its own baseline coincides with "tall"'s at y=30 from the row top.
    let run = RenderTester::mount(
        box_node(RenderTable::new(2).with_text_baseline(Some(TextBaseline::Alphabetic)))
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(30.0)))
                    .child(box_node(RenderColoredBox::red(20.0, 10.0)))
                    .with_table_parent_data(
                        TableCellParentData::zero()
                            .with_alignment(TableCellVerticalAlignment::Baseline),
                    )
                    .label("tall"),
            )
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(10.0)))
                    .child(box_node(RenderColoredBox::green(20.0, 5.0)))
                    .with_table_parent_data(
                        TableCellParentData::zero()
                            .with_alignment(TableCellVerticalAlignment::Baseline),
                    )
                    .label("short"),
            ),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.offset(run.id("tall")).dy, px(0.0));
    assert_eq!(run.offset(run.id("short")).dy, px(20.0));
    assert_eq!(
        run.box_geometry(run.root()).height,
        px(30.0),
        "row height must be the table-wide baseline distance (30) since \
         after_baseline is 0 for both cells",
    );
}

#[test]
fn harness_table_dry_baseline_matches_the_committed_first_row_baseline() {
    // Two row-0 `Baseline` cells whose baselines are 30 and 10, so the table's
    // first-row baseline (`before_baseline`) is max(30, 10) = 30 — the value
    // the committed layout stores in `baseline_distance` (cf. the sibling test).
    // Each cell wraps a `RenderParagraph` (not a baseline-less `ColoredBox`) so
    // `RenderBaseline`'s dry path — which probes its child's dry baseline — has
    // a real child baseline and returns its configured offset.
    let constraints = loose(200.0);
    let paragraph = || {
        box_node(RenderParagraph::new(
            TextSpan::new("Ag"),
            TextDirection::Ltr,
        ))
    };
    let mut run = RenderTester::mount(
        box_node(RenderTable::new(2).with_text_baseline(Some(TextBaseline::Alphabetic)))
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(30.0)))
                    .child(paragraph())
                    .with_table_parent_data(
                        TableCellParentData::zero()
                            .with_alignment(TableCellVerticalAlignment::Baseline),
                    ),
            )
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, px(10.0)))
                    .child(paragraph())
                    .with_table_parent_data(
                        TableCellParentData::zero()
                            .with_alignment(TableCellVerticalAlignment::Baseline),
                    ),
            ),
    )
    .with_constraints(constraints)
    .run_layout();

    let dry = run.dry_baseline(run.root(), constraints, TextBaseline::Alphabetic);
    assert_eq!(
        dry,
        Some(30.0),
        "table dry baseline must equal the committed first-row baseline \
         (max of the row's cell baselines 30 and 10)",
    );
}

#[test]
fn harness_table_unset_cell_alignment_follows_a_later_default_change_but_an_explicit_cell_does_not()
{
    // 3 columns: "unset" has no parent-data override (defers to the table's
    // default); "explicit_top" pins `Top` directly; "spacer" is tall (50px)
    // so the row's height (50) leaves visible room for Top/Bottom to differ.
    let mut run = RenderTester::mount(
        box_node(RenderTable::new(3))
            .child(box_node(RenderColoredBox::red(20.0, 10.0)).label("unset"))
            .child(
                box_node(RenderColoredBox::green(20.0, 10.0))
                    .with_table_parent_data(
                        TableCellParentData::zero().with_alignment(TableCellVerticalAlignment::Top),
                    )
                    .label("explicit_top"),
            )
            .child(box_node(RenderColoredBox::blue(20.0, 50.0)).label("spacer")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    // Before the default changes, both cells sit at the row top.
    assert_eq!(run.offset(run.id("unset")).dy, px(0.0));
    assert_eq!(run.offset(run.id("explicit_top")).dy, px(0.0));

    run.update::<RenderTable>(run.root(), |table| {
        table.set_default_vertical_alignment(TableCellVerticalAlignment::Bottom);
    });
    run.pump();

    // Row height is 50 (the spacer); a Bottom-aligned 10px-tall cell sits at
    // dy = 50 - 10 = 40.
    assert_eq!(
        run.offset(run.id("unset")).dy,
        px(40.0),
        "an unset cell must follow the table's default_vertical_alignment \
         after it changes",
    );
    assert_eq!(
        run.offset(run.id("explicit_top")).dy,
        px(0.0),
        "a cell with an explicit vertical_alignment must NOT follow a later \
         default_vertical_alignment change",
    );
}

// ============================================================================
// RenderAnimatedSize
// ============================================================================
//
// Every test below constructs its own `AnimationController` (a fresh,
// never-pumped `Scheduler`, per ADR-0013 D2) and, where the test needs to
// drive the retarget animation across frames, keeps a `Clone` of it (`driver`)
// to call `tick_at(seconds_since_the_current_run_started)` directly —
// mirroring how `flui-animation`'s own controller tests and `Vsync::tick_all`
// drive deterministic virtual time, with no `thread::sleep`. `run.pump()`
// only re-runs the render pipeline; it does not itself advance the
// controller, so a value-listener-driven `mark_needs_layout` (buffered by
// `attach`) is drained on the very next `pump()`/`run_frame()` after a tick.

fn animated_size_controller(ms: u64) -> (AnimationController, AnimationController) {
    let controller =
        AnimationController::new(Duration::from_millis(ms), Arc::new(Scheduler::new()));
    let driver = controller.clone();
    (controller, driver)
}

fn assert_size_approx(actual: Size, expected: Size, eps: f32, what: &str) {
    assert!(
        (actual.width.get() - expected.width.get()).abs() < eps
            && (actual.height.get() - expected.height.get()).abs() < eps,
        "{what}: expected ~{expected:?} (±{eps}), got {actual:?}",
    );
}

#[test]
fn harness_render_animated_size_start_state_snaps_to_child_size_with_no_animation() {
    let (controller, _driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
        None,
    );

    let run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("child")),
    )
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(30.0), px(30.0)),
        "the very first layout must snap to the child's size, no animation",
    );
    assert!(
        !run.structure().contains(&"ClipRect"),
        "a settled first layout must not clip"
    );
}

#[test]
fn harness_render_animated_size_interpolates_over_several_frames_not_snap() {
    let (controller, driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
        None,
    );

    let mut run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .run_frame();
    assert_eq!(run.box_geometry(run.root()), Size::new(px(10.0), px(10.0)));

    // Grow the child: Stable -> Changed (begin = last committed size = 10,
    // end = 50), controller restarts at t = 0.
    run.update::<RenderColoredBox>(run.id("child"), |b| {
        b.set_preferred_size(Size::new(px(50.0), px(50.0)));
    });
    run.pump();
    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(10.0), px(10.0)),
        "the retarget frame itself reports t=0 (still the begin size) — no snap to end",
    );

    // Tick the controller to known fractions of the 100ms run and confirm the
    // reported size actually interpolates (hand-computed against
    // Tween::transform), not just holds or jumps straight to the target.
    driver.tick_at(0.025); // 25ms of 100ms => t=0.25
    run.pump();
    assert_size_approx(
        run.box_geometry(run.root()),
        Size::new(px(20.0), px(20.0)), // 10 + 0.25 * (50-10)
        0.5,
        "t=0.25",
    );

    driver.tick_at(0.05); // t=0.5
    run.pump();
    assert_size_approx(
        run.box_geometry(run.root()),
        Size::new(px(30.0), px(30.0)),
        0.5,
        "t=0.5",
    );

    driver.tick_at(0.075); // t=0.75
    run.pump();
    assert_size_approx(
        run.box_geometry(run.root()),
        Size::new(px(40.0), px(40.0)),
        0.5,
        "t=0.75",
    );

    driver.tick_at(0.1); // t=1.0, run completes
    run.pump();
    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(50.0), px(50.0)),
        "a completed run must land exactly on the target size",
    );
}

#[test]
fn harness_render_animated_size_clip_appears_mid_animation_and_disappears_once_settled() {
    let (controller, driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
        None,
    );

    let mut run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .run_frame();
    assert!(
        !run.structure().contains(&"ClipRect"),
        "a settled 10x10 box must not clip"
    );

    run.update::<RenderColoredBox>(run.id("child"), |b| {
        b.set_preferred_size(Size::new(px(50.0), px(50.0)));
    });
    run.pump();
    assert!(
        run.structure().contains(&"ClipRect"),
        "mid-animation, the reported size (10) is smaller than the tween's \
         target (50) — the child's full-size paint must be clipped; structure: {:?}",
        run.structure(),
    );

    driver.tick_at(0.1); // settle fully at the target
    run.pump();
    assert!(
        !run.structure().contains(&"ClipRect"),
        "once settled at the target size there is no overflow to clip; structure: {:?}",
        run.structure(),
    );
}

#[test]
fn harness_render_animated_size_respects_alignment_for_the_oversized_child_mid_animation() {
    let (controller, _driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::BOTTOM_RIGHT,
        Clip::HardEdge,
        None,
    );

    let mut run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .run_frame();

    run.update::<RenderColoredBox>(run.id("child"), |b| {
        b.set_preferred_size(Size::new(px(50.0), px(50.0)));
    });
    run.pump();

    // Retarget frame: reported size is still 10x10 (t=0) while the child is
    // laid out at its full 50x50 — BOTTOM_RIGHT must offset the (oversized)
    // child by exactly `size - child_size = (10-50, 10-50) = (-40, -40)`.
    assert_eq!(
        run.offset(run.id("child")),
        Offset::new(px(-40.0), px(-40.0)),
        "BOTTOM_RIGHT alignment must reach the child even while it is larger \
         than the still-animating parent box",
    );
}

#[test]
fn harness_render_animated_size_retarget_mid_flight_has_no_discontinuous_jump() {
    let (controller, driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
        None,
    );

    let mut run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .run_frame();

    // First retarget: 10 -> 50, let it run to t=0.5 and settle into `Stable`
    // (the child's size holds steady for one frame, so the ORIGINAL
    // interpolation span is left running rather than being touched).
    run.update::<RenderColoredBox>(run.id("child"), |b| {
        b.set_preferred_size(Size::new(px(50.0), px(50.0)));
    });
    run.pump();
    driver.tick_at(0.05); // t=0.5 of the 10->50 span
    run.pump();
    let mid_flight_size = run.box_geometry(run.root());
    assert_size_approx(
        mid_flight_size,
        Size::new(px(30.0), px(30.0)),
        0.5,
        "midpoint of the first span",
    );

    // Second retarget while STILL mid-flight (the 10->50 run has not reached
    // t=1.0): begin must be the CURRENT committed value (continuous), not a
    // degenerate collapse — this is the Stable->Changed formula, distinct
    // from the Changed->Unstable degenerate-collapse case tested at the unit
    // level.
    run.update::<RenderColoredBox>(run.id("child"), |b| {
        b.set_preferred_size(Size::new(px(90.0), px(90.0)));
    });
    run.pump();
    let retarget_frame_size = run.box_geometry(run.root());

    assert_eq!(
        retarget_frame_size, mid_flight_size,
        "retargeting mid-flight must begin exactly at the last committed \
         size — no discontinuous jump on the retarget frame itself",
    );
}

#[test]
fn harness_render_animated_size_baseline_matches_child_baseline_plus_recorded_offset() {
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    const PROBE_OFFSET_PX: f32 = 100.0;

    let (controller, _driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
        None,
    );

    let mut run = RenderTester::mount(
        box_node(RenderBaseline::new(
            TextBaseline::Alphabetic,
            px(PROBE_OFFSET_PX),
        ))
        .label("probe")
        .child(box_node(ro).label("animated_size").child(
            box_node(RenderParagraph::new(TextSpan::new("A"), TextDirection::Ltr)).label("text"),
        )),
    )
    .with_constraints(constraints)
    .run_layout();

    // The first (Start-state) layout has no active animation, so the live
    // baseline must equal the dry baseline computed against the same
    // (already-loosened-by-the-probe) constraints — the same "live == dry
    // for a statically laid out tree" argument `align_live_baseline_adds_child_offset_dy`
    // relies on for `RenderAlign`.
    let animated_size_constraints = constraints.loosen();
    let animated_size_bl_dry = run
        .dry_baseline(
            run.id("animated_size"),
            animated_size_constraints,
            TextBaseline::Alphabetic,
        )
        .expect("RenderAnimatedSize with a paragraph child must report a dry baseline");

    let animated_size_offset_dy = run.offset(run.id("animated_size")).dy.get();
    let expected_dy = PROBE_OFFSET_PX - animated_size_bl_dry;

    assert!(
        (animated_size_offset_dy - expected_dy).abs() < 0.5,
        "RenderAnimatedSize must forward the child's live baseline (+ its own \
         recorded child offset) through AligningShiftedBox, so the probe \
         positions it at probe_offset - (child_bl + align_dy) (got dy={animated_size_offset_dy}, \
         expected {expected_dy})",
    );
}

#[test]
fn harness_render_animated_size_fast_path_tight_constraints_snaps_and_leaves_offset_stale() {
    let (controller, _driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::BOTTOM_RIGHT,
        Clip::HardEdge,
        None,
    );

    let mut run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .run_frame();

    // Grow the child under loose constraints (the general path): BOTTOM_RIGHT
    // records a real, non-zero offset for the (temporarily oversized) child.
    run.update::<RenderColoredBox>(run.id("child"), |b| {
        b.set_preferred_size(Size::new(px(50.0), px(50.0)));
    });
    run.pump();
    assert_eq!(
        run.offset(run.id("child")),
        Offset::new(px(-40.0), px(-40.0)),
    );

    // Now force TIGHT root constraints — the fast path. The child is still
    // laid out (and, under a tight incoming constraint, its own
    // `RenderColoredBox::perform_layout` reports the constrained size), but
    // `align_child` must NOT run: the child's offset stays exactly what it
    // was, matching the oracle's stale-offset quirk (animated_size.dart
    // fast-path branch has no `alignChild()` call).
    run.owner_mut()
        .set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
    let root = run.root();
    run.owner_mut().mark_needs_layout(root);
    run.pump();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(px(100.0), px(100.0)),
        "the fast path must snap to the incoming tight size",
    );
    assert_eq!(
        run.offset(run.id("child")),
        Offset::new(px(-40.0), px(-40.0)),
        "the fast path must NOT call align_child — the child's offset must \
         stay exactly what it was before the tight constraints landed",
    );
}

// ============================================================================
// RenderSliverPersistentHeader family
// ============================================================================
//
// A note on `constraints.overlap`: while building these tests, driving a
// *real* `RenderViewport` to a nonzero scroll offset and inspecting the first
// sliver's `constraints.overlap` revealed that `RenderViewport::attempt_layout`
// (`crates/flui-objects/src/sliver/viewport.rs`, the `overlap: center_offset
// .min(0.0)` line) computed the wrong sign relative to both the oracle
// (`rendering/viewport.dart:1834`: `overlap: ... math.min(0.0, -centerOffset)`)
// and FLUI's own `RenderShrinkWrappingViewport::attempt_layout` sibling
// (already correct: `overlap: corrected_offset.min(0.0)`) — confirmed
// empirically (a Pinned header at scroll_offset=300 reported
// `paint_origin == -300.0`, i.e. `overlap == -300.0`, where a correct
// top-anchored forward viewport must report `overlap == 0.0` for its first
// sliver). **Fixed** (see `harness_viewport_forward_overlap_is_zero_without_
// leading_reverse_group` / `harness_viewport_reverse_group_overlap_is_always_
// zero` above, near the other `RenderViewport` harness tests): the formula now
// matches the oracle for both the no-reverse-group case and the
// leading-negative-child case (which forces `overlap` to `0.0` for both
// sequences). It was a pre-existing defect, not something introduced by this
// family's pass — no existing sliver in the catalog read `constraints.overlap`
// in a way any prior test asserted on, so it had zero coverage until these
// headers exercised it and it also would have affected
// `RenderSliverFillRemainingAndOverscroll`/`RenderSliverFillRemainingWithScrollable`,
// which already read `constraints.overlap`. The tests below still avoid
// depending on `overlap`-derived quantities through a real viewport (they
// assert `paint_extent`/`effective_scroll_offset`/`max_scroll_obstruction_extent`,
// none of which round-trip through `overlap` at scroll_offset > 0 in these
// specific scenarios); the stretch-configuration formulas that DO need a
// specific `overlap` are covered by the pure unit test in
// `sliver_persistent_header.rs` using a directly-constructed
// `SliverConstraints`, sidestepping the viewport entirely.
//
// A second, separate finding (since fixed): `RenderTester::mount` used to
// never call `RenderObject::attach` for a Sliver child. Box children went
// through `PipelineOwner::insert_child_render_object`, which calls
// `attach_inserted_node` — but Sliver children were inserted via the
// low-level `render_tree_mut().insert_sliver_child(...)`
// (`crates/flui-rendering/src/storage/tree.rs`), which did not, and
// `apply_deferred_mutation` (`crates/flui-rendering/src/pipeline/owner/
// layout.rs`, used by lazy-sliver child building) had the same gap for both
// protocols. `crate::testing::tree::mount_child` now inserts Sliver children
// via the new `PipelineOwner::insert_sliver_child_render_object` (the
// Sliver-protocol counterpart of `insert_child_render_object`), and
// `apply_deferred_mutation`'s `Insert` arm now calls `attach_inserted_node`
// for both `DeferredRenderObject` variants (see
// `crates/flui-rendering/tests/attach_detach_lifecycle.rs` for the
// regression coverage). The snap-animation test below no longer forces its
// own dirty mark — the real `attach()`-registered controller listener
// drives it end-to-end.

fn viewport_multi_with_scroll(
    offset: f32,
    slivers: impl IntoIterator<Item = TreeNode>,
) -> TreeNode {
    let mut node = box_node(RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::new(offset),
    ))
    .label("viewport");
    for sliver in slivers {
        node = node.child(sliver);
    }
    node
}

/// A tall filler sliver giving the viewport enough total scroll extent that
/// scrolling the header through its full shrink/reveal range never gets
/// clamped back down by `apply_content_dimensions`.
fn filler_sliver() -> TreeNode {
    sliver_node(RenderSliverToBoxAdapter::new())
        .label("filler")
        .child(box_node(RenderColoredBox::red(300.0, 2000.0)).label("filler_child"))
}

#[test]
fn harness_sliver_persistent_header_scrolling_shrinks_then_scrolls_off() {
    let header = RenderSliverScrollingPersistentHeader::new(40.0, 120.0);
    let mut run = RenderTester::mount(viewport_multi_with_scroll(
        0.0,
        [
            sliver_node(header)
                .label("header")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("child")),
            filler_sliver(),
        ],
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");

    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        120.0,
        "scroll_offset=0: fully expanded at max_extent",
    );
    assert!(run.sliver_geometry(header_id).has_visual_overflow);
    assert_eq!(
        run.offset(run.id("child")).dy,
        px(0.0),
        "fully expanded: child sits at the sliver's own origin",
    );

    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(60.0);
    });
    run.relayout();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        60.0,
        "mid-shrink: paint_extent = max_extent - scroll_offset",
    );

    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(80.0);
    });
    run.relayout();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        40.0,
        "at scroll_offset = max_extent - min_extent: shrunk to exactly min_extent",
    );

    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(200.0);
    });
    run.relayout();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        0.0,
        "past max_extent: fully scrolled off, paint_extent clamps to 0",
    );
}

#[test]
fn harness_sliver_persistent_header_pinned_stays_at_zero_and_reports_max_scroll_obstruction_extent()
{
    let header = RenderSliverPinnedPersistentHeader::new(40.0, 120.0);
    let mut run = RenderTester::mount(viewport_multi_with_scroll(
        0.0,
        [
            sliver_node(header)
                .label("header")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("child")),
            filler_sliver(),
        ],
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");

    assert_eq!(
        run.sliver_geometry(header_id).max_scroll_obstruction_extent,
        40.0,
        "max_scroll_obstruction_extent must report min_extent",
    );
    assert_eq!(run.offset(run.id("child")).dy, px(0.0));

    // Scroll well past full shrink — pinned headers never scroll off.
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(300.0);
    });
    run.relayout();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        40.0,
        "pinned at min_extent even scrolled far past max_extent",
    );
    assert_eq!(
        run.offset(run.id("child")).dy,
        px(0.0),
        "the defining pinned behavior: child_main_axis_position stays 0.0",
    );

    // Trap #5 regression: the pinned header's `max_scroll_obstruction_extent`
    // (min_extent) must be visible to the viewport's own accounting for the
    // FOLLOWING sliver via `max_scroll_obstruction_extent_before` — this is
    // the mechanism `max_scroll_obstruction_extent` actually feeds (see the
    // module-level note above the correction to the source plan's citation).
    let mut obstruction_before_filler = None;
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        obstruction_before_filler = vp.max_scroll_obstruction_extent_before(1);
    });
    assert_eq!(
        obstruction_before_filler,
        Some(40.0),
        "the pinned header's max_scroll_obstruction_extent must accumulate into \
         the viewport's max_scroll_obstruction_extent_before for slivers after it",
    );
}

#[test]
fn harness_sliver_persistent_header_floating_reveals_on_reverse_scroll_and_pointer_scroll_start_direction_permits_reveal()
 {
    let header: RenderSliverFloatingPersistentHeader =
        RenderSliverFloatingPersistentHeader::new(40.0, 120.0, None);
    let mut run = RenderTester::mount(viewport_multi_with_scroll(
        0.0,
        [
            sliver_node(header)
                .label("header")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("child")),
            filler_sliver(),
        ],
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");

    // Step 1: scroll forward past max_extent — header fully shrunk/hidden.
    // Shrinking (delta < 0) is unconditional regardless of user_scroll_direction,
    // so the exact direction here doesn't matter for this step.
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(300.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Reverse);
    });
    run.relayout();
    let mut effective = None;
    run.update::<RenderSliverFloatingPersistentHeader>(header_id, |h| {
        effective = h.effective_scroll_offset();
    });
    assert_eq!(
        effective,
        Some(300.0),
        "effective_scroll_offset == actual scroll_offset once fully shrunk"
    );
    assert_eq!(run.sliver_geometry(header_id).paint_extent, 0.0);

    // Step 2 (trap #3 + basic trap #4): scroll BACKWARD to 280 with
    // user_scroll_direction = Forward (FLUI's `ScrollDirection::Forward` is
    // the *reveal* direction — scroll offset decreasing, per its own doc
    // comment). The re-reveal branch engages (scroll_offset < last_actual)
    // and `allow_floating_expansion` is satisfied via its first disjunct,
    // clamping the stale effective_scroll_offset (300, past max_extent) down
    // to max_extent BEFORE applying the real 20px delta.
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(280.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Forward);
    });
    run.relayout();
    run.update::<RenderSliverFloatingPersistentHeader>(header_id, |h| {
        effective = h.effective_scroll_offset();
    });
    assert_eq!(
        effective,
        Some(100.0),
        "effective clamps to max_extent (120) first, then the 20px delta \
         applies: 120 - 20 = 100"
    );
    assert_eq!(run.sliver_geometry(header_id).paint_extent, 20.0);

    // Step 3 (trap #4's SECOND disjunct): continue scrolling backward to 250
    // with user_scroll_direction = Idle, but with `last_started_scroll_direction`
    // pre-seeded to Forward via `update_scroll_start_direction` (no caller
    // wires this in production yet — see the module docs — so a test drives
    // it directly). Without this disjunct, `allow_floating_expansion` would
    // be false, `delta` would be zeroed (only shrinking allowed), and
    // effective/paint_extent would stay at 100/20 (unchanged) instead of
    // continuing to reveal.
    run.update::<RenderSliverFloatingPersistentHeader>(header_id, |h| {
        h.update_scroll_start_direction(ScrollDirection::Forward);
    });
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(250.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Idle);
    });
    run.relayout();
    run.update::<RenderSliverFloatingPersistentHeader>(header_id, |h| {
        effective = h.effective_scroll_offset();
    });
    assert_eq!(
        effective,
        Some(70.0),
        "the second allow_floating_expansion disjunct (pointer/wheel scroll \
         bookkeeping) must still permit the reveal to continue: 100 - 30 = 70"
    );
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        50.0,
        "trap #4 regression: dropping the second disjunct would leave this \
         at 20.0 (unchanged from step 2) instead of continuing to 50.0"
    );
}

#[test]
fn harness_sliver_persistent_header_floating_allow_expansion_clamps_effective_to_max_extent_not_overshooting()
 {
    // Mount directly at scroll_offset=300: the FIRST-EVER layout takes the
    // `else` branch (no history), so effective_scroll_offset == 300.0 —
    // already past max_extent(120) — without needing a prior scroll step.
    let header: RenderSliverFloatingPersistentHeader =
        RenderSliverFloatingPersistentHeader::new(40.0, 120.0, None);
    let mut run = RenderTester::mount(viewport_multi_with_scroll(
        300.0,
        [
            sliver_node(header)
                .label("header")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("child")),
            filler_sliver(),
        ],
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");
    assert_eq!(run.sliver_geometry(header_id).paint_extent, 0.0);

    // Scroll backward to 200 with user_scroll_direction = Forward (reveal
    // direction): allow_floating_expansion is true, so the oracle's
    // `if (_effectiveScrollOffset! > maxExtent) { _effectiveScrollOffset =
    // maxExtent; }` (`sliver_persistent_header.dart:666-669`) must fire
    // BEFORE the 100px delta is applied.
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(200.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Forward);
    });
    run.relayout();

    let mut effective = None;
    run.update::<RenderSliverFloatingPersistentHeader>(header_id, |h| {
        effective = h.effective_scroll_offset();
    });
    assert_eq!(
        effective,
        Some(20.0),
        "clamp-to-max_extent-first must give 120 - 100 = 20; without it, the \
         naive clamp(300 - 100, 0, 200) = 200 would leave the header fully \
         hidden (paint_extent = 0) instead of 100px revealed",
    );
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        100.0,
        "not overshooting: paint_extent = max_extent - effective = 120 - 20 = 100",
    );
}

#[test]
fn harness_sliver_persistent_header_floating_pinned_shares_reveal_sequence_but_clamps_paint_extent_and_stays_pinned()
 {
    // Same re-reveal state machine as plain Floating (shared, not
    // duplicated — see the module docs) — reusing steps 1+2 of the Floating
    // reveal test, but asserting FloatingPinned's DISTINCT contract: paint
    // extent never drops below min_extent, and child_main_axis_position is
    // always 0.0 (unlike plain Floating, which can be negative).
    let header: RenderSliverFloatingPinnedPersistentHeader =
        RenderSliverFloatingPinnedPersistentHeader::new(40.0, 120.0, None);
    let mut run = RenderTester::mount(viewport_multi_with_scroll(
        0.0,
        [
            sliver_node(header)
                .label("header")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("child")),
            filler_sliver(),
        ],
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_layout();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");

    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(300.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Reverse);
    });
    run.relayout();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        40.0,
        "always at least min_extent visible, pinned — plain Floating would \
         report 0.0 here (fully hidden)",
    );
    assert_eq!(
        run.offset(run.id("child")).dy,
        px(0.0),
        "child_main_axis_position is always 0.0, even fully shrunk past max_extent",
    );

    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(280.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Forward);
    });
    run.relayout();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        40.0,
        "still clamped to min_extent while mid-reveal (raw formula gives 20, \
         below the pinned floor of 40)",
    );
    assert_eq!(
        run.offset(run.id("child")).dy,
        px(0.0),
        "child_main_axis_position stays 0.0 mid-reveal too, unlike plain Floating",
    );
}

#[test]
fn harness_sliver_persistent_header_floating_snap_animation_drives_effective_scroll_offset_across_ticks()
 {
    let ctl = AnimationController::new(Duration::from_millis(100), Arc::new(Scheduler::new()));
    let driver = ctl.clone();
    let header: RenderSliverFloatingPersistentHeader =
        RenderSliverFloatingPersistentHeader::new(40.0, 120.0, Some(ctl)).with_snap_configuration(
            flui_objects::FloatingHeaderSnapConfiguration::new(
                ArcCurve::new(Curves::Linear),
                Duration::from_millis(100),
            ),
        );

    let mut run = RenderTester::mount(viewport_multi_with_scroll(
        0.0,
        [
            sliver_node(header)
                .label("header")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("child")),
            filler_sliver(),
        ],
    ))
    .with_size(Size::new(px(300.0), px(400.0)))
    .run_frame();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");

    // Establish effective_scroll_offset = 100.0 via the same two real-scroll
    // steps as the reveal test (0 -> 300 -> 280), landing partially revealed
    // (paint_extent = 20) before kicking off the snap animation.
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(300.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Reverse);
    });
    run.pump();
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(280.0);
        vp.offset_mut()
            .set_user_scroll_direction(ScrollDirection::Forward);
    });
    run.pump();
    assert_eq!(run.sliver_geometry(header_id).paint_extent, 20.0);

    // Kick off a reveal snap toward 0.0 (fully expanded). No further real
    // scrolling happens for the rest of this test (scroll_offset stays at
    // 280), so the state machine's own delta stays 0 and passes the
    // animated value through unchanged each tick.
    run.update::<RenderSliverFloatingPersistentHeader>(header_id, |h| {
        h.maybe_start_snap_animation(ScrollDirection::Forward);
    });

    // The controller's own `Listenable` subscription (registered in
    // `RenderSliverFloatingPersistentHeader::attach`, now genuinely called
    // by `mount_child` for this Sliver header — see the module note above)
    // drives the relayout on tick: no manual dirty mark needed here.
    driver.tick_at(0.05); // t = 0.5 of the 100ms run (Linear curve)
    run.pump();
    let paint_extent_mid = run.sliver_geometry(header_id).paint_extent;
    assert!(
        (paint_extent_mid - 70.0).abs() < 1.0,
        "at t=0.5, effective_scroll_offset should have interpolated from 100 \
         toward 0 (currently ~50), giving paint_extent ~= 120 - 50 = 70; got {paint_extent_mid}",
    );

    driver.tick_at(0.1); // t = 1.0, run completes
    run.pump();
    assert_eq!(
        run.sliver_geometry(header_id).paint_extent,
        120.0,
        "a completed snap must land exactly on the target (fully revealed)",
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
    let objects_mod = include_str!("../src/lib.rs");
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
